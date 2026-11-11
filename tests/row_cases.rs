//! Row comparison tests that go through real Parquet files.
//!
//! Each test writes small files to a scratch directory, runs the library on
//! them and checks the report. The comparison logic itself has finer-grained
//! unit tests in `src/rowdiff.rs`.

mod common;

use common::{accounts, accounts_with_float_balance, write_parquet};
use reconcilers::policy::Verdict;
use reconcilers::report::{ColumnResult, Report};
use reconcilers::{Config, Error, compare_parquet};

const KEYED_ON_ID: &str = "[comparison]\nkeys = [\"id\"]\n";

fn config(text: &str) -> Config {
    Config::from_toml_str(text).expect("the test configuration should be valid")
}

fn row_counts(report: &Report) -> &reconcilers::rowdiff::RowCounts {
    &report.rows.as_ref().expect("rows were compared").counts
}

#[test]
fn identical_files_pass() {
    let rows = [
        (Some(1), Some("ann"), Some(100)),
        (Some(2), Some("bob"), None),
    ];
    let left = write_parquet("identical_left.parquet", &accounts(&rows));
    let right = write_parquet("identical_right.parquet", &accounts(&rows));

    let report = compare_parquet(&config(KEYED_ON_ID), &[left], &[right]).unwrap();

    assert_eq!(report.verdict, Verdict::Pass);
    assert_eq!(row_counts(&report).unchanged_rows, 2);
}

#[test]
fn missing_extra_and_changed_rows_are_counted() {
    let left = write_parquet(
        "counted_left.parquet",
        &accounts(&[
            (Some(1), Some("ann"), Some(100)),
            (Some(2), Some("bob"), Some(200)),
            (Some(3), Some("cat"), Some(300)),
        ]),
    );
    let right = write_parquet(
        "counted_right.parquet",
        &accounts(&[
            (Some(1), Some("ann"), Some(100)),
            (Some(2), Some("bob"), Some(250)),
            (Some(4), Some("dan"), Some(400)),
        ]),
    );

    let report = compare_parquet(&config(KEYED_ON_ID), &[left], &[right]).unwrap();
    let counts = row_counts(&report);

    assert_eq!(report.verdict, Verdict::Fail);
    assert_eq!(counts.left_only_keys, 1);
    assert_eq!(counts.right_only_keys, 1);
    assert_eq!(counts.changed_rows, 1);
    assert_eq!(counts.unchanged_rows, 1);
    assert!(
        report
            .rows
            .unwrap()
            .columns
            .contains(&ColumnResult::Compared {
                column: "balance".to_string(),
                mismatches: 1,
                tolerance: None,
                within_tolerance: 0,
            })
    );
}

#[test]
fn reordering_rows_leaves_the_report_byte_identical() {
    let left_rows = [
        (Some(1), Some("ann"), Some(100)),
        (Some(2), Some("bob"), Some(200)),
        (Some(3), Some("cat"), Some(300)),
    ];
    let right_rows = [
        (Some(1), Some("ann"), Some(100)),
        (Some(2), Some("bob"), Some(250)),
        (Some(4), Some("dan"), Some(400)),
    ];
    let mut left_reversed = left_rows;
    left_reversed.reverse();
    let mut right_reversed = right_rows;
    right_reversed.reverse();

    let left = write_parquet("order_left.parquet", &accounts(&left_rows));
    let right = write_parquet("order_right.parquet", &accounts(&right_rows));
    let left_reversed = write_parquet("order_left_reversed.parquet", &accounts(&left_reversed));
    let right_reversed = write_parquet("order_right_reversed.parquet", &accounts(&right_reversed));

    let config = config(KEYED_ON_ID);
    let original = compare_parquet(&config, &[left], &[right]).unwrap();
    let reordered = compare_parquet(&config, &[left_reversed], &[right_reversed]).unwrap();

    assert_eq!(original.to_json(), reordered.to_json());
}

#[test]
fn a_dataset_split_over_two_files_equals_the_same_rows_in_one_file() {
    let first_half = [(Some(1), Some("ann"), Some(100))];
    let second_half = [(Some(2), Some("bob"), Some(200))];
    let all_rows = [first_half[0], second_half[0]];

    let part_1 = write_parquet("split_part_1.parquet", &accounts(&first_half));
    let part_2 = write_parquet("split_part_2.parquet", &accounts(&second_half));
    let whole = write_parquet("split_whole.parquet", &accounts(&all_rows));

    let report = compare_parquet(&config(KEYED_ON_ID), &[part_1, part_2], &[whole]).unwrap();

    assert_eq!(report.verdict, Verdict::Pass);
    assert_eq!(row_counts(&report).left_rows, 2);
}

#[test]
fn duplicate_and_null_keys_fail_and_are_not_compared() {
    let left = write_parquet(
        "dupes_left.parquet",
        &accounts(&[
            (Some(1), Some("ann"), Some(100)),
            (Some(1), Some("ann"), Some(999)),
            (None, Some("nobody"), Some(0)),
            (Some(2), Some("bob"), Some(200)),
        ]),
    );
    let right = write_parquet(
        "dupes_right.parquet",
        &accounts(&[
            (Some(1), Some("ann"), Some(100)),
            (Some(2), Some("bob"), Some(200)),
        ]),
    );

    let report = compare_parquet(&config(KEYED_ON_ID), &[left], &[right]).unwrap();
    let counts = row_counts(&report);

    assert_eq!(report.verdict, Verdict::Fail);
    assert_eq!(counts.duplicate_keys_left, 1);
    assert_eq!(counts.null_key_rows_left, 1);
    assert_eq!(counts.compared_rows, 1);
    assert_eq!(counts.changed_rows, 0);
}

#[test]
fn a_column_with_different_kinds_is_inconclusive_not_passed() {
    let left = write_parquet(
        "kinds_left.parquet",
        &accounts(&[(Some(1), Some("ann"), Some(100))]),
    );
    let right = write_parquet(
        "kinds_right.parquet",
        &accounts_with_float_balance(&[(1, "ann", 100.0)]),
    );
    let report = compare_parquet(&config(KEYED_ON_ID), &[left], &[right]).unwrap();
    let rows = report.rows.unwrap();

    assert_eq!(rows.verdict, Verdict::Inconclusive);
    assert!(matches!(
        rows.columns.as_slice(),
        [
            ColumnResult::Inconclusive { column, .. },
            ColumnResult::Compared { mismatches: 0, .. },
        ] if column == "balance"
    ));
}

#[test]
fn a_key_that_is_not_in_the_data_is_a_configuration_error() {
    let rows = [(Some(1), Some("ann"), Some(100))];
    let left = write_parquet("badkey_left.parquet", &accounts(&rows));
    let right = write_parquet("badkey_right.parquet", &accounts(&rows));

    let result = compare_parquet(
        &config("[comparison]\nkeys = [\"account_id\"]\n"),
        &[left],
        &[right],
    );

    assert!(matches!(result, Err(Error::Config(_))));
}

#[test]
fn files_with_different_schemas_cannot_form_one_dataset() {
    let ints = write_parquet(
        "mixed_ints.parquet",
        &accounts(&[(Some(1), Some("ann"), Some(100))]),
    );
    let floats = write_parquet(
        "mixed_floats.parquet",
        &accounts_with_float_balance(&[(2, "bob", 200.0)]),
    );

    let result = compare_parquet(&config(KEYED_ON_ID), &[ints.clone(), floats], &[ints]);

    assert!(matches!(result, Err(Error::FilesDisagreeOnSchema { .. })));
}

#[test]
fn a_difference_within_tolerance_passes_but_fingerprints_still_differ() {
    let left = write_parquet(
        "tolerance_left.parquet",
        &accounts(&[
            (Some(1), Some("ann"), Some(100)),
            (Some(2), Some("bob"), Some(200)),
        ]),
    );
    let right = write_parquet(
        "tolerance_right.parquet",
        &accounts(&[
            (Some(1), Some("ann"), Some(100)),
            (Some(2), Some("bob"), Some(203)),
        ]),
    );
    let within_five = config(
        r#"
        [comparison]
        keys = ["id"]

        [[tolerances]]
        column = "balance"
        max_difference = "5"
        "#,
    );

    let report = compare_parquet(&within_five, &[left], &[right]).unwrap();

    assert_eq!(report.verdict, Verdict::Pass);
    assert_eq!(row_counts(&report).changed_rows, 0);
    assert!(
        report
            .rows
            .as_ref()
            .unwrap()
            .columns
            .contains(&ColumnResult::Compared {
                column: "balance".to_string(),
                mismatches: 0,
                tolerance: Some("5".to_string()),
                within_tolerance: 1,
            })
    );
    // Fingerprints are exact: 200 and 203 are different numbers.
    assert!(!report.fingerprints.unwrap().content_matches);
}

#[test]
fn a_difference_beyond_tolerance_fails() {
    let left = write_parquet(
        "beyond_left.parquet",
        &accounts(&[(Some(1), Some("ann"), Some(100))]),
    );
    let right = write_parquet(
        "beyond_right.parquet",
        &accounts(&[(Some(1), Some("ann"), Some(106))]),
    );
    let within_five = config(
        "[comparison]\nkeys = [\"id\"]\n[[tolerances]]\ncolumn = \"balance\"\nmax_difference = \"5\"\n",
    );

    let report = compare_parquet(&within_five, &[left], &[right]).unwrap();

    assert_eq!(report.verdict, Verdict::Fail);
    assert_eq!(row_counts(&report).changed_rows, 1);
}

#[test]
fn equal_content_in_a_different_row_order_has_matching_fingerprints() {
    let rows = [
        (Some(1), Some("ann"), Some(100)),
        (Some(2), Some("bob"), None),
    ];
    let mut reversed = rows;
    reversed.reverse();
    let left = write_parquet("fingerprint_left.parquet", &accounts(&rows));
    let right = write_parquet("fingerprint_right.parquet", &accounts(&reversed));

    let report = compare_parquet(&config(KEYED_ON_ID), &[left], &[right]).unwrap();
    let fingerprints = report.fingerprints.unwrap();

    assert!(fingerprints.content_matches);
    assert_eq!(fingerprints.left.schema, fingerprints.right.schema);
}

#[test]
fn composed_and_decomposed_accents_differ_unless_nfc_is_switched_on() {
    // The same visible name, "zoé", stored with two different code point
    // sequences.
    let left = write_parquet(
        "unicode_left.parquet",
        &accounts(&[(Some(1), Some("zo\u{00e9}"), Some(100))]),
    );
    let right = write_parquet(
        "unicode_right.parquet",
        &accounts(&[(Some(1), Some("zoe\u{0301}"), Some(100))]),
    );
    let with_nfc = config("[comparison]\nkeys = [\"id\"]\n[normalization]\nunicode = \"nfc\"\n");

    let exact = compare_parquet(
        &config(KEYED_ON_ID),
        std::slice::from_ref(&left),
        std::slice::from_ref(&right),
    )
    .unwrap();
    let normalized = compare_parquet(&with_nfc, &[left], &[right]).unwrap();

    assert_eq!(row_counts(&exact).changed_rows, 1);
    assert_eq!(normalized.verdict, Verdict::Pass);
    assert!(normalized.fingerprints.unwrap().content_matches);
}
