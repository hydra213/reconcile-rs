//! The hand-labelled correctness corpus.
//!
//! Each test is one numbered case with a known right answer. Together they
//! describe what the engine promises. Every case goes through real Parquet
//! files and the public `compare_parquet` function.
//!
//! Finer-grained tests of each rule live next to the code in `src/`.

mod common;

use std::sync::Arc;

use arrow::array::{
    ArrayRef, Decimal128Array, Float64Array, Int32Array, Int64Array, ListArray, StringArray,
    TimestampMicrosecondArray, TimestampMillisecondArray,
};
use arrow::datatypes::Int32Type;
use arrow::record_batch::RecordBatch;
use common::{batch, write_parquet, write_parquet_in_row_groups};
use reconcilers::fingerprint::Fingerprint;
use reconcilers::parquet_input::read_dataset;
use reconcilers::policy::{RowCheck, SchemaRule, Verdict};
use reconcilers::report::{ColumnResult, Report, RowSection};
use reconcilers::{Config, compare_parquet};

const KEYED_ON_ID: &str = "[comparison]\nkeys = [\"id\"]\n";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ints(values: &[i64]) -> ArrayRef {
    Arc::new(Int64Array::from(values.to_vec()))
}

fn optional_ints(values: &[Option<i64>]) -> ArrayRef {
    Arc::new(Int64Array::from(values.to_vec()))
}

fn strings(values: &[&str]) -> ArrayRef {
    Arc::new(StringArray::from(values.to_vec()))
}

fn optional_strings(values: &[Option<&str>]) -> ArrayRef {
    Arc::new(StringArray::from(values.to_vec()))
}

fn floats(values: &[f64]) -> ArrayRef {
    Arc::new(Float64Array::from(values.to_vec()))
}

/// A decimal column from unscaled integers: `decimals(&[150], 18, 2)` is 1.50.
fn decimals(unscaled: &[i128], precision: u8, scale: i8) -> ArrayRef {
    Arc::new(
        Decimal128Array::from(unscaled.to_vec())
            .with_precision_and_scale(precision, scale)
            .expect("the precision should hold the values"),
    )
}

/// Writes both batches to files named after `case` and compares them.
fn compare(case: &str, config: &str, left: &RecordBatch, right: &RecordBatch) -> Report {
    let left = write_parquet(&format!("corpus_{case}_left.parquet"), left);
    let right = write_parquet(&format!("corpus_{case}_right.parquet"), right);
    let config = Config::from_toml_str(config).expect("the case's configuration should be valid");

    compare_parquet(&config, &[left], &[right])
        .expect("the comparison should run")
        .report
}

fn rows(report: &Report) -> &RowSection {
    report.rows.as_ref().expect("rows were compared")
}

/// The number of mismatches in a compared column.
fn mismatches(report: &Report, name: &str) -> u64 {
    match column(report, name) {
        ColumnResult::Compared { mismatches, .. } => *mismatches,
        other => panic!("`{name}` should have been compared, but was {other:?}"),
    }
}

fn is_inconclusive(report: &Report, name: &str) -> bool {
    matches!(column(report, name), ColumnResult::Inconclusive { .. })
}

fn column<'a>(report: &'a Report, name: &str) -> &'a ColumnResult {
    rows(report)
        .columns
        .iter()
        .find(|column| column.column() == name)
        .unwrap_or_else(|| panic!("the report should mention column `{name}`"))
}

fn check_verdict(report: &Report, check: RowCheck) -> Verdict {
    rows(report)
        .findings
        .iter()
        .find(|finding| finding.check == check)
        .expect("every check has a finding")
        .verdict
}

fn schema_rules(report: &Report) -> Vec<(&str, SchemaRule)> {
    report
        .schema
        .findings
        .iter()
        .map(|finding| (finding.difference.column(), finding.rule))
        .collect()
}

// ---------------------------------------------------------------------------
// The cases
// ---------------------------------------------------------------------------

/// Case 1: equal data with permuted rows and columns.
#[test]
fn case_01_permuted_rows_and_columns_are_equal() {
    let left = batch(vec![
        ("id", ints(&[1, 2, 3])),
        ("name", strings(&["a", "b", "c"])),
        ("qty", ints(&[10, 20, 30])),
    ]);
    let right = batch(vec![
        ("qty", ints(&[30, 10, 20])),
        ("name", strings(&["c", "a", "b"])),
        ("id", ints(&[3, 1, 2])),
    ]);

    let permuted = compare("01_permuted", KEYED_ON_ID, &left, &right);
    let unpermuted = compare("01_same", KEYED_ON_ID, &left, &left);

    assert_eq!(permuted.verdict, Verdict::Pass);
    assert!(permuted.fingerprints.as_ref().unwrap().content_matches);
    assert_eq!(permuted.to_json(), unpermuted.to_json());
}

/// Case 2: missing and extra keys.
#[test]
fn case_02_missing_and_extra_keys() {
    let left = batch(vec![("id", ints(&[1, 2, 3])), ("v", ints(&[1, 1, 1]))]);
    let right = batch(vec![
        ("id", ints(&[2, 3, 4, 5])),
        ("v", ints(&[1, 1, 1, 1])),
    ]);

    let report = compare("02", KEYED_ON_ID, &left, &right);
    let counts = &rows(&report).counts;

    assert_eq!(counts.left_only_keys, 1);
    assert_eq!(counts.right_only_keys, 2);
    assert_eq!(counts.matched_keys, 2);
    assert_eq!(check_verdict(&report, RowCheck::MissingKeys), Verdict::Fail);
    assert_eq!(check_verdict(&report, RowCheck::ExtraKeys), Verdict::Fail);
}

/// Case 3: duplicate keys on either side. They are reported, and the
/// duplicated keys are not compared value by value.
#[test]
fn case_03_duplicate_keys_on_either_side() {
    let left = batch(vec![
        ("id", ints(&[1, 1, 2, 3])),
        ("v", ints(&[10, 11, 20, 30])),
    ]);
    let right = batch(vec![
        ("id", ints(&[1, 2, 3, 3])),
        ("v", ints(&[10, 20, 30, 31])),
    ]);

    let report = compare("03", KEYED_ON_ID, &left, &right);
    let counts = &rows(&report).counts;

    assert_eq!(counts.duplicate_keys_left, 1);
    assert_eq!(counts.duplicate_keys_right, 1);
    assert_eq!(counts.matched_keys, 3);
    // Only key 2 is unique on both sides.
    assert_eq!(counts.compared_rows, 1);
    assert_eq!(counts.changed_rows, 0);
    assert_eq!(
        check_verdict(&report, RowCheck::DuplicateKeys),
        Verdict::Fail
    );
}

/// Case 4: NULL keys, and NULL versus the empty string.
#[test]
fn case_04_null_keys_and_null_versus_empty_string() {
    let left = batch(vec![
        ("id", optional_ints(&[Some(1), Some(2), None])),
        ("note", optional_strings(&[None, Some(""), Some("x")])),
    ]);
    let right = batch(vec![
        ("id", optional_ints(&[Some(1), Some(2), None])),
        ("note", optional_strings(&[Some(""), Some(""), Some("x")])),
    ]);

    let report = compare("04", KEYED_ON_ID, &left, &right);
    let counts = &rows(&report).counts;

    // The NULL-key rows are counted and never matched to each other.
    assert_eq!(counts.null_key_rows_left, 1);
    assert_eq!(counts.null_key_rows_right, 1);
    assert_eq!(counts.matched_keys, 2);
    assert_eq!(check_verdict(&report, RowCheck::NullKeys), Verdict::Fail);
    // Key 1: NULL on the left, "" on the right. That is a change.
    assert_eq!(mismatches(&report, "note"), 1);
}

/// Case 5: extra and missing columns, and an explicitly mapped rename.
#[test]
fn case_05_extra_and_missing_columns_and_a_mapped_rename() {
    let left = batch(vec![
        ("id", ints(&[1])),
        ("amt", ints(&[100])),
        ("legacy_flag", strings(&["y"])),
    ]);
    let right = batch(vec![
        ("id", ints(&[1])),
        ("amount", ints(&[100])),
        ("desk", strings(&["fx"])),
    ]);
    let config = r#"
        [comparison]
        keys = ["id"]
        schema_policy = "compatible"

        [[column_mappings]]
        left = "amt"
        right = "amount"
    "#;

    let report = compare("05", config, &left, &right);

    // The mapped rename is not a schema finding; the other two are.
    assert_eq!(
        schema_rules(&report),
        vec![
            ("desk", SchemaRule::ColumnAdded),
            ("legacy_flag", SchemaRule::ColumnRemoved),
        ]
    );
    assert_eq!(report.schema.verdict, Verdict::Fail);
    // The renamed column is compared under its right-side name.
    assert_eq!(mismatches(&report, "amount"), 0);
    assert_eq!(rows(&report).verdict, Verdict::Pass);
}

/// Case 6: integer widening is allowed, an incompatible type change is not.
#[test]
fn case_06_integer_widening_and_incompatible_type_change() {
    let left = batch(vec![
        ("id", ints(&[1, 2])),
        ("qty", Arc::new(Int32Array::from(vec![5, 6])) as ArrayRef),
        ("code", ints(&[7, 8])),
    ]);
    let right = batch(vec![
        ("id", ints(&[1, 2])),
        ("qty", ints(&[5, 6])),
        ("code", strings(&["7", "8"])),
    ]);
    let config = "[comparison]\nkeys = [\"id\"]\nschema_policy = \"compatible\"\n";

    let report = compare("06", config, &left, &right);

    assert_eq!(
        schema_rules(&report),
        vec![
            ("code", SchemaRule::IncompatibleTypeChange),
            ("qty", SchemaRule::LosslessIntegerWidening),
        ]
    );
    // Int32 5 equals Int64 5.
    assert_eq!(mismatches(&report, "qty"), 0);
    // An integer is never silently compared with a string.
    assert!(is_inconclusive(&report, "code"));
}

/// Case 7: decimal scale and exact money comparison.
#[test]
fn case_07_decimal_scale_and_exact_money() {
    // 10.50, 0.10 and 99.99 at scale 2, then the same amounts at scale 4,
    // except the last, which is 99.9901.
    let left = batch(vec![
        ("id", ints(&[1, 2, 3])),
        ("amount", decimals(&[1050, 10, 9999], 18, 2)),
    ]);
    let right = batch(vec![
        ("id", ints(&[1, 2, 3])),
        ("amount", decimals(&[105_000, 1_000, 999_901], 20, 4)),
    ]);

    let report = compare("07", KEYED_ON_ID, &left, &right);

    // Only the row that differs by 0.0001 is a mismatch. No float rounding.
    assert_eq!(mismatches(&report, "amount"), 1);
    assert_eq!(rows(&report).counts.unchanged_rows, 2);
}

/// Case 8: timestamp precision and time zone representation.
#[test]
fn case_08_timestamp_precision_and_time_zones() {
    let millis_utc: ArrayRef =
        Arc::new(TimestampMillisecondArray::from(vec![1_000, 2_000]).with_timezone("UTC"));
    let micros_plus_ten: ArrayRef = Arc::new(
        TimestampMicrosecondArray::from(vec![1_000_000, 2_000_001]).with_timezone("+10:00"),
    );
    let micros_naive: ArrayRef =
        Arc::new(TimestampMicrosecondArray::from(vec![1_000_000, 2_000_000]));

    let zoned_left = batch(vec![("id", ints(&[1, 2])), ("at", millis_utc.clone())]);
    let zoned_right = batch(vec![("id", ints(&[1, 2])), ("at", micros_plus_ten)]);
    let naive_right = batch(vec![("id", ints(&[1, 2])), ("at", micros_naive)]);

    // Same instants in different units and zones are equal; the second row
    // is one microsecond later on the right.
    let zoned = compare("08_zoned", KEYED_ON_ID, &zoned_left, &zoned_right);
    assert_eq!(mismatches(&zoned, "at"), 1);

    // A timestamp without a zone is not assumed to be UTC.
    let naive = compare("08_naive", KEYED_ON_ID, &zoned_left, &naive_right);
    assert!(is_inconclusive(&naive, "at"));

    // Unless the configuration says so explicitly.
    let assume_utc =
        "[comparison]\nkeys = [\"id\"]\n[normalization]\nnaive_timestamps = \"assume_utc\"\n";
    let assumed = compare("08_assumed", assume_utc, &zoned_left, &naive_right);
    assert_eq!(mismatches(&assumed, "at"), 0);
}

/// Case 9: floating-point NaN, infinities and negative zero.
#[test]
fn case_09_nan_infinities_and_negative_zero() {
    let left = batch(vec![
        ("id", ints(&[1, 2, 3, 4])),
        ("x", floats(&[f64::NAN, f64::INFINITY, -0.0, f64::INFINITY])),
    ]);
    let right = batch(vec![
        ("id", ints(&[1, 2, 3, 4])),
        (
            "x",
            floats(&[f64::NAN, f64::INFINITY, 0.0, f64::NEG_INFINITY]),
        ),
    ]);

    let report = compare("09", KEYED_ON_ID, &left, &right);

    // NaN equals NaN, +inf equals +inf, -0.0 equals 0.0. Only row 4, +inf
    // against -inf, differs.
    assert_eq!(mismatches(&report, "x"), 1);
    assert_eq!(rows(&report).counts.unchanged_rows, 3);
}

/// Case 10: Unicode and non-ASCII strings, in keys and in values.
#[test]
fn case_10_unicode_and_non_ascii_strings() {
    let left = batch(vec![
        ("id", strings(&["東京", "Zürich", "🙂"])),
        ("name", strings(&["日本語", "Grüße", "naïve"])),
    ]);
    let right = batch(vec![
        ("id", strings(&["🙂", "東京", "Zürich"])),
        ("name", strings(&["naive", "日本語", "Grüße"])),
    ]);

    let report = compare("10", KEYED_ON_ID, &left, &right);

    assert_eq!(rows(&report).counts.matched_keys, 3);
    // "naïve" and "naive" are different strings.
    assert_eq!(mismatches(&report, "name"), 1);
}

/// Case 11: nested Parquet structures are reported as not compared.
#[test]
fn case_11_nested_columns_are_inconclusive() {
    let tags = || -> ArrayRef {
        Arc::new(ListArray::from_iter_primitive::<Int32Type, _, _>(vec![
            Some(vec![Some(1), Some(2)]),
        ]))
    };
    let both = batch(vec![("id", ints(&[1])), ("tags", tags())]);

    let report = compare("11", KEYED_ON_ID, &both, &both);

    assert!(is_inconclusive(&report, "tags"));
    // Identical files, yet the engine does not claim the lists are equal.
    assert_eq!(report.verdict, Verdict::Inconclusive);
}

/// Case 12: same row counts but altered values.
#[test]
fn case_12_same_row_counts_but_altered_values() {
    let left = batch(vec![("id", ints(&[1, 2, 3])), ("v", ints(&[10, 20, 30]))]);
    let right = batch(vec![("id", ints(&[1, 2, 3])), ("v", ints(&[10, 99, 30]))]);

    let report = compare("12", KEYED_ON_ID, &left, &right);
    let counts = &rows(&report).counts;

    assert_eq!(counts.left_rows, counts.right_rows);
    assert_eq!(counts.changed_rows, 1);
    assert_eq!(report.verdict, Verdict::Fail);
}

/// Case 13: changed data within and beyond a declared tolerance.
#[test]
fn case_13_within_and_beyond_tolerance() {
    // 10.001 against 10.002 (within 0.01) and 10.00 against 10.02 (beyond).
    let left = batch(vec![
        ("id", ints(&[1, 2])),
        ("amount", decimals(&[10_001, 10_000], 18, 3)),
    ]);
    let right = batch(vec![
        ("id", ints(&[1, 2])),
        ("amount", decimals(&[10_002, 10_020], 18, 3)),
    ]);
    let config = r#"
        [comparison]
        keys = ["id"]

        [[tolerances]]
        column = "amount"
        max_difference = "0.01"
    "#;

    let report = compare("13", config, &left, &right);

    assert_eq!(
        column(&report, "amount"),
        &ColumnResult::Compared {
            column: "amount".to_string(),
            mismatches: 1,
            tolerance: Some("0.01".to_string()),
            within_tolerance: 1,
        }
    );
    // The tolerated row still makes the exact fingerprints differ.
    assert!(!report.fingerprints.unwrap().content_matches);
}

/// Case 14: a file changed on disk after it was read. The recorded
/// fingerprint describes the bytes that were compared, not the new bytes.
#[test]
fn case_14_file_mutated_after_it_was_read() {
    let original = batch(vec![("id", ints(&[1])), ("v", ints(&[10]))]);
    let replacement = batch(vec![("id", ints(&[1])), ("v", ints(&[99]))]);

    let path = write_parquet("corpus_14.parquet", &original);
    let bytes_before = std::fs::read(&path).unwrap();
    let dataset = read_dataset(std::slice::from_ref(&path)).unwrap();

    write_parquet("corpus_14.parquet", &replacement);
    let bytes_after = std::fs::read(&path).unwrap();

    assert_ne!(bytes_before, bytes_after);
    assert_eq!(
        dataset.files[0].fingerprint,
        Fingerprint::of_bytes(&bytes_before)
    );
    assert_eq!(dataset.files[0].size_bytes, bytes_before.len() as u64);
}

/// Case 15: large keys, variable-length strings and mixed row groups.
#[test]
fn case_15_large_keys_long_strings_and_many_row_groups() {
    const ROWS: usize = 1_000;
    // Keys of about 200 characters and values from empty to 500 characters.
    let keys: Vec<String> = (0..ROWS)
        .map(|i| format!("{}-{i:06}", "k".repeat(200)))
        .collect();
    let values: Vec<String> = (0..ROWS).map(|i| "v".repeat(i % 500)).collect();
    let table = batch(vec![
        (
            "id",
            Arc::new(StringArray::from_iter_values(&keys)) as ArrayRef,
        ),
        (
            "payload",
            Arc::new(StringArray::from_iter_values(&values)) as ArrayRef,
        ),
    ]);

    let one_group = write_parquet("corpus_15_one_group.parquet", &table);
    let many_groups = write_parquet_in_row_groups("corpus_15_many_groups.parquet", &table, 64);
    let config = Config::from_toml_str(KEYED_ON_ID).unwrap();

    let report = compare_parquet(&config, &[one_group], &[many_groups])
        .unwrap()
        .report;

    assert_eq!(report.verdict, Verdict::Pass);
    assert_eq!(rows(&report).counts.unchanged_rows, ROWS as u64);
    assert!(report.fingerprints.unwrap().content_matches);
}
