//! Reading from real object storage.
//!
//! These tests need a bucket of your own, so they do nothing unless an
//! environment variable points at one. CI never sets them.
//!
//! | Variable | Example |
//! |---|---|
//! | `RECONCILERS_TEST_S3` | `s3://my-bucket/reconcilers-test/` |
//! | `RECONCILERS_TEST_AZURE` | `abfss://container@account.dfs.core.windows.net/reconcilers-test/` |
//! | `RECONCILERS_TEST_GCS` | `gs://my-bucket/reconcilers-test/` |
//!
//! Before the first run, copy the whole `tests/fixtures/` folder to that
//! place, so it contains `legacy_trades.parquet`, `legacy_trades.csv` and
//! the `legacy_trades_parts/` folder. Credentials come from the usual
//! environment variables of each cloud. Then run:
//!
//! ```text
//! cargo test --test cloud_cases -- --nocapture
//! ```
//!
//! The same checks also run against the local `tests/fixtures/` folder on
//! every `cargo test`, so a failure in the cloud points at the storage, not
//! at the checks.

use reconcilers::input::read_dataset;
use reconcilers::policy::Verdict;
use reconcilers::{Config, Dataset, Error};

const LOCAL_FIXTURES: &str = "tests/fixtures/";

/// Runs every check against the copy of `tests/fixtures/` at `base`, which
/// is a local folder or a URL ending in `/`.
fn check_fixture_copies(base: &str) {
    let parquet_config = config("[comparison]\nkeys = [\"trade_id\"]\n");
    let csv_config = config(include_str!("fixtures/legacy_trades_csv.toml"));
    let local_legacy = read(
        &parquet_config,
        &format!("{LOCAL_FIXTURES}legacy_trades.parquet"),
    );

    // One object, read byte for byte.
    let legacy = read(&parquet_config, &format!("{base}legacy_trades.parquet"));
    assert_eq!(
        legacy.files[0].fingerprint, local_legacy.files[0].fingerprint,
        "the object's bytes differ from tests/fixtures/legacy_trades.parquet; was the copy complete?"
    );
    assert_same_rows(&parquet_config, &local_legacy, &legacy);

    // A folder: both part files, and nothing else.
    let parts = read(&parquet_config, &format!("{base}legacy_trades_parts/"));
    assert_eq!(
        parts.files.len(),
        2,
        "the folder should hold two part files"
    );
    assert_same_rows(&parquet_config, &local_legacy, &parts);

    // A CSV file with declared types.
    let csv = read(&csv_config, &format!("{base}legacy_trades.csv"));
    assert_same_rows(&csv_config, &local_legacy, &csv);

    // Something that is not there is an error, not an empty dataset.
    let missing = format!("{base}no_such_file.parquet");
    assert!(
        read_dataset(std::slice::from_ref(&missing), &parquet_config.csv).is_err(),
        "reading `{missing}` should fail"
    );
}

fn config(text: &str) -> Config {
    Config::from_toml_str(text).expect("the test configuration should be valid")
}

fn read(config: &Config, location: &str) -> Dataset {
    match read_dataset(&[location.to_string()], &config.csv) {
        Ok(dataset) => dataset,
        // The chain of causes usually names the missing credential or the
        // HTTP status that object storage answered with.
        Err(error) => panic!("cannot read `{location}`: {error}\n\n{}", describe(&error)),
    }
}

fn describe(error: &Error) -> String {
    let mut text = String::new();
    let mut cause: Option<&dyn std::error::Error> = Some(error);
    while let Some(inner) = cause {
        text.push_str(&format!("caused by: {inner}\n"));
        cause = inner.source();
    }
    text
}

/// The rows are the same: every key matches and no value differs.
/// Schemas are not judged, because a CSV file cannot say that a column
/// never holds NULL, so its nullability differs from the Parquet file.
fn assert_same_rows(config: &Config, expected: &Dataset, actual: &Dataset) {
    let report = reconcilers::compare(config, expected, actual).expect("the comparison should run");
    let rows = report.rows.as_ref().expect("rows were compared");

    assert_eq!(rows.verdict, Verdict::Pass, "{}", report.to_text());
    assert!(
        report.fingerprints.as_ref().unwrap().content_matches,
        "{}",
        report.to_text()
    );
}

/// Runs the checks against the location in `variable`, or explains how to
/// turn the test on.
fn check_cloud(variable: &str) {
    let Ok(base) = std::env::var(variable) else {
        eprintln!("skipped: set {variable} to run this test (see tests/cloud_cases.rs)");
        return;
    };
    assert!(
        base.ends_with('/'),
        "{variable} must end with `/`, because it names a folder: `{base}`"
    );
    check_fixture_copies(&base);
    eprintln!("ok: read the fixtures from {base}");
}

#[test]
fn the_checks_hold_for_the_local_fixtures() {
    check_fixture_copies(LOCAL_FIXTURES);
}

#[test]
fn amazon_s3() {
    check_cloud("RECONCILERS_TEST_S3");
}

#[test]
fn azure_data_lake_storage() {
    check_cloud("RECONCILERS_TEST_AZURE");
}

#[test]
fn google_cloud_storage() {
    check_cloud("RECONCILERS_TEST_GCS");
}
