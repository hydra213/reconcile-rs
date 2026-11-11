//! End-to-end tests: run the real `reconcilers` binary on the sample files
//! in `tests/fixtures/` and check its output and exit code.

mod common;

use std::path::PathBuf;
use std::process::{Command, Output};

use common::{shops_with_opening_time, write_parquet};

const LEGACY: &str = "tests/fixtures/legacy_trades.parquet";
const NEW: &str = "tests/fixtures/new_trades.parquet";
const CONFIG: &str = "examples/reconcile.toml";

/// Runs the binary from the repository root with the given arguments.
fn reconcilers(args: &[&str]) -> Output {
    // Cargo builds the binary before integration tests and puts its path in
    // this environment variable.
    Command::new(env!("CARGO_BIN_EXE_reconcilers"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(args)
        .output()
        .expect("the reconcilers binary should start")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout should be UTF-8")
}

/// A report path inside Cargo's per-test scratch directory.
fn report_path(file_name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(file_name)
}

#[test]
fn a_file_compared_with_itself_passes_with_exit_code_0() {
    let output = reconcilers(&["schema", "--left", LEGACY, "--right", LEGACY]);

    assert_eq!(output.status.code(), Some(0));
    insta::assert_snapshot!(stdout(&output));
}

#[test]
fn sample_files_fail_under_compatible_with_exit_code_1() {
    let output = reconcilers(&[
        "schema",
        "--left",
        LEGACY,
        "--right",
        NEW,
        "--policy",
        "compatible",
    ]);

    assert_eq!(output.status.code(), Some(1));
    insta::assert_snapshot!(stdout(&output));
}

#[test]
fn a_missing_file_is_an_execution_error_with_exit_code_2() {
    let output = reconcilers(&["schema", "--left", "does_not_exist.parquet", "--right", NEW]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty(), "no report is printed on error");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cannot open `does_not_exist.parquet`"));
}

#[test]
fn a_file_that_is_not_parquet_is_an_execution_error_with_exit_code_2() {
    let output = reconcilers(&["schema", "--left", "Cargo.toml", "--right", NEW]);

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cannot read Parquet metadata from `Cargo.toml`"));
}

#[test]
fn the_json_report_is_byte_identical_across_runs() {
    let first = report_path("schema_report_first.json");
    let second = report_path("schema_report_second.json");

    for path in [&first, &second] {
        let path = path.to_str().expect("the temp path should be UTF-8");
        reconcilers(&[
            "schema",
            "--left",
            LEGACY,
            "--right",
            NEW,
            "--policy",
            "compatible",
            "--report",
            path,
        ]);
    }

    let first_json = std::fs::read_to_string(&first).expect("first report should exist");
    let second_json = std::fs::read_to_string(&second).expect("second report should exist");
    assert_eq!(first_json, second_json);
    insta::assert_snapshot!(first_json);
}

#[test]
fn rowdiff_on_the_sample_files_fails_with_exit_code_1() {
    let output = reconcilers(&[
        "rowdiff", "--left", LEGACY, "--right", NEW, "--config", CONFIG,
    ]);

    assert_eq!(output.status.code(), Some(1));
    insta::assert_snapshot!(stdout(&output));
}

#[test]
fn rowdiff_json_report_is_byte_identical_across_runs() {
    let first = report_path("rowdiff_report_first.json");
    let second = report_path("rowdiff_report_second.json");

    for path in [&first, &second] {
        let path = path.to_str().expect("the temp path should be UTF-8");
        reconcilers(&[
            "rowdiff", "--left", LEGACY, "--right", NEW, "--config", CONFIG, "--report", path,
        ]);
    }

    let first_json = std::fs::read_to_string(&first).expect("first report should exist");
    let second_json = std::fs::read_to_string(&second).expect("second report should exist");
    assert_eq!(first_json, second_json);
    insta::assert_snapshot!(first_json);
}

#[test]
fn rowdiff_with_a_missing_config_is_an_execution_error_with_exit_code_2() {
    let output = reconcilers(&[
        "rowdiff",
        "--left",
        LEGACY,
        "--right",
        NEW,
        "--config",
        "no_such.toml",
    ]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty(), "no report is printed on error");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cannot read configuration file `no_such.toml`"));
}

#[test]
fn rowdiff_that_is_only_inconclusive_exits_with_code_3() {
    // A file compared with itself has no differences. But the engine has no
    // comparison rule for time-of-day columns yet, so it must not claim the
    // `opens_at` column is equal.
    let file = write_parquet("cli_inconclusive.parquet", &shops_with_opening_time());
    let file = file.to_str().expect("the temp path should be UTF-8");

    let output = reconcilers(&[
        "rowdiff",
        "--left",
        file,
        "--right",
        file,
        "--config",
        "tests/fixtures/keyed_on_id.toml",
    ]);

    assert_eq!(output.status.code(), Some(3));
    insta::assert_snapshot!(stdout(&output));
}
