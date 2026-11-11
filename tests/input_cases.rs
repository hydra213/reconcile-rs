//! Tests for reading datasets: CSV files, folders of files, and object
//! storage URLs.
//!
//! No test here needs a network. Object storage is exercised through
//! `file://` URLs, which go through the same code path as `s3://`,
//! `abfss://` and `gs://` URLs, with the local disk as the "bucket".

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow::array::{Decimal128Array, Int64Array, StringArray};
use arrow::record_batch::RecordBatch;
use common::{batch, compare_files, read_files, write_parquet};
use reconcilers::policy::Verdict;
use reconcilers::report::{ColumnResult, Report};
use reconcilers::{Config, Error};

const KEYED_ON_ID: &str = "[comparison]\nkeys = [\"id\"]\n";

/// Configuration for the `payments` CSV: `id` and `amount` are typed, and
/// `payer` is left as text.
const PAYMENTS_CSV_CONFIG: &str = r#"
    [comparison]
    keys = ["id"]

    [csv.types]
    id = "int64"
    amount = "decimal(18, 2)"
"#;

fn config(text: &str) -> Config {
    Config::from_toml_str(text).expect("the test configuration should be valid")
}

/// A fresh, empty folder for one test inside Cargo's scratch directory.
fn scratch_folder(name: &str) -> PathBuf {
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    // The folder may be left over from an earlier run.
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).expect("the scratch folder should be creatable");
    folder
}

fn write_text(folder: &Path, file_name: &str, text: &str) -> PathBuf {
    let path = folder.join(file_name);
    std::fs::write(&path, text).expect("the file should be writable");
    path
}

/// Two payments as a typed table: `id` Int64, `payer` Utf8, `amount`
/// Decimal128(18, 2).
fn payments(amounts_in_cents: [i128; 2]) -> RecordBatch {
    let amounts = Decimal128Array::from(amounts_in_cents.to_vec())
        .with_precision_and_scale(18, 2)
        .expect("the precision should hold the values");
    batch(vec![
        ("id", Arc::new(Int64Array::from(vec![1, 2]))),
        ("payer", Arc::new(StringArray::from(vec!["ann", "bob"]))),
        ("amount", Arc::new(amounts)),
    ])
}

fn mismatches(report: &Report, name: &str) -> u64 {
    let rows = report.rows.as_ref().expect("rows were compared");
    match rows.columns.iter().find(|column| column.column() == name) {
        Some(ColumnResult::Compared { mismatches, .. }) => *mismatches,
        other => panic!("`{name}` should have been compared, but was {other:?}"),
    }
}

/// A `file://` URL for a path, with a trailing `/` for a folder.
fn file_url(path: &Path, is_folder: bool) -> String {
    let url = if is_folder {
        url::Url::from_directory_path(path)
    } else {
        url::Url::from_file_path(path)
    };
    url.expect("an absolute path converts to a URL").to_string()
}

// ---------------------------------------------------------------------------
// CSV
// ---------------------------------------------------------------------------

#[test]
fn a_csv_file_with_declared_types_equals_the_same_rows_in_parquet() {
    let folder = scratch_folder("csv_equals_parquet");
    let csv = write_text(
        &folder,
        "payments.csv",
        "id,payer,amount\n1,ann,10.50\n2,bob,20.00\n",
    );
    let parquet = write_parquet("csv_equals_parquet.parquet", &payments([1050, 2000]));

    let report = compare_files(&config(PAYMENTS_CSV_CONFIG), &[csv], &[parquet]).unwrap();

    assert_eq!(report.verdict, Verdict::Pass, "{}", report.to_text());
    assert!(report.fingerprints.unwrap().content_matches);
}

#[test]
fn a_changed_value_in_a_csv_file_is_found() {
    let folder = scratch_folder("csv_changed_value");
    let csv = write_text(
        &folder,
        "payments.csv",
        "id,payer,amount\n1,ann,10.50\n2,bob,20.01\n",
    );
    let parquet = write_parquet("csv_changed_value.parquet", &payments([1050, 2000]));

    let report = compare_files(&config(PAYMENTS_CSV_CONFIG), &[csv], &[parquet]).unwrap();

    assert_eq!(report.verdict, Verdict::Fail);
    assert_eq!(mismatches(&report, "amount"), 1);
    assert_eq!(mismatches(&report, "payer"), 0);
}

#[test]
fn two_csv_files_compare_as_text_without_any_declared_type() {
    let folder = scratch_folder("csv_as_text");
    let left = write_text(&folder, "left.csv", "id,payer\n1,ann\n2,bob\n");
    let right = write_text(&folder, "right.csv", "id,payer\n2,bob\n1,ann\n");

    let report = compare_files(&config(KEYED_ON_ID), &[left], &[right]).unwrap();

    assert_eq!(report.verdict, Verdict::Pass);
}

#[test]
fn text_is_not_silently_compared_with_numbers() {
    // Without a declared type the CSV `amount` is text. Comparing it with a
    // decimal column needs a rule, so the column is inconclusive.
    let folder = scratch_folder("csv_undeclared_amount");
    let csv = write_text(
        &folder,
        "payments.csv",
        "id,payer,amount\n1,ann,10.50\n2,bob,20.00\n",
    );
    let parquet = write_parquet("csv_undeclared_amount.parquet", &payments([1050, 2000]));
    let only_id_declared = "[comparison]\nkeys = [\"id\"]\n[csv.types]\nid = \"int64\"\n";

    let report = compare_files(&config(only_id_declared), &[csv], &[parquet]).unwrap();

    let rows = report.rows.as_ref().unwrap();
    assert_eq!(rows.verdict, Verdict::Inconclusive);
    assert!(rows.columns.iter().any(|column| matches!(
        column,
        ColumnResult::Inconclusive { column, .. } if column == "amount"
    )));
}

#[test]
fn a_csv_value_that_does_not_fit_its_declared_type_stops_the_run() {
    let folder = scratch_folder("csv_bad_value");
    let csv = write_text(&folder, "payments.csv", "id,payer,amount\nx,ann,10.50\n");

    let result = read_files(&config(PAYMENTS_CSV_CONFIG), &[csv]);

    assert!(matches!(result, Err(Error::ReadCsv { .. })));
}

// ---------------------------------------------------------------------------
// Folders
// ---------------------------------------------------------------------------

#[test]
fn a_folder_is_read_as_all_the_data_files_directly_inside_it() {
    let folder = scratch_folder("folder_of_parts");
    write_text(&folder, "part-0.csv", "id,payer\n1,ann\n");
    write_text(&folder, "part-1.csv", "id,payer\n2,bob\n");
    write_text(&folder, "_SUCCESS", "");
    let whole = write_text(
        &scratch_folder("folder_of_parts_whole"),
        "whole.csv",
        "id,payer\n1,ann\n2,bob\n",
    );

    let config = config(KEYED_ON_ID);
    let dataset = read_files(&config, std::slice::from_ref(&folder)).unwrap();
    let report = compare_files(&config, &[folder], &[whole]).unwrap();

    assert_eq!(dataset.files.len(), 2, "the marker file is not data");
    assert!(dataset.files[0].location.ends_with("part-0.csv"));
    assert_eq!(report.verdict, Verdict::Pass);
}

#[test]
fn a_folder_without_data_files_is_an_error() {
    let folder = scratch_folder("folder_without_data");
    write_text(&folder, "notes.txt", "nothing to compare");

    let result = read_files(&config(KEYED_ON_ID), &[folder]);

    assert!(matches!(result, Err(Error::NoDataFilesInFolder { .. })));
}

// ---------------------------------------------------------------------------
// Object storage URLs
// ---------------------------------------------------------------------------

#[test]
fn a_url_of_one_object_is_read_through_object_storage() {
    let parquet = write_parquet("url_one_object.parquet", &payments([1050, 2000]));
    let url = file_url(&parquet, false);
    let config = config(KEYED_ON_ID);

    let from_url =
        reconcilers::input::read_dataset(std::slice::from_ref(&url), &config.csv).unwrap();
    let from_path = read_files(&config, &[parquet]).unwrap();
    let report = reconcilers::compare(&config, &from_path, &from_url).unwrap();

    assert_eq!(from_url.files[0].location, url);
    assert_eq!(
        from_url.files[0].fingerprint,
        from_path.files[0].fingerprint
    );
    assert_eq!(report.verdict, Verdict::Pass);
}

#[test]
fn a_url_ending_in_a_slash_is_read_as_a_folder() {
    let folder = scratch_folder("url_folder");
    write_text(&folder, "part-0.csv", "id,payer\n1,ann\n");
    write_text(&folder, "part-1.csv", "id,payer\n2,bob\n");
    write_text(&folder, "_SUCCESS", "");
    let url = file_url(&folder, true);

    let dataset =
        reconcilers::input::read_dataset(std::slice::from_ref(&url), &config(KEYED_ON_ID).csv)
            .unwrap();

    let locations: Vec<&str> = dataset
        .files
        .iter()
        .map(|file| file.location.as_str())
        .collect();
    assert_eq!(
        locations,
        vec![format!("{url}part-0.csv"), format!("{url}part-1.csv")]
    );
}

#[test]
fn an_object_that_does_not_exist_is_an_error() {
    let missing = file_url(
        &scratch_folder("url_missing").join("nothing.parquet"),
        false,
    );

    let result = reconcilers::input::read_dataset(&[missing], &config(KEYED_ON_ID).csv);

    assert!(matches!(result, Err(Error::ReadObjectStorage { .. })));
}
