//! End-to-end tests: run the real `reconcilers` binary on the sample files
//! in `tests/fixtures/` and check its output and exit code.

mod common;

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::Arc;

use arrow::array::{Int64Array, StringArray};
use common::{batch, shops_with_opening_time, write_parquet};

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

/// An output directory inside Cargo's per-test scratch directory. Use a
/// unique name per test, because tests run in parallel.
fn out_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name)
}

fn read(path: PathBuf) -> String {
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} should be readable: {error}", path.display()))
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
fn compare_on_the_sample_files_fails_with_exit_code_1() {
    let output = reconcilers(&[
        "compare", "--left", LEGACY, "--right", NEW, "--config", CONFIG,
    ]);

    assert_eq!(output.status.code(), Some(1));
    insta::assert_snapshot!(stdout(&output));
}

#[test]
fn findings_are_byte_identical_across_runs_and_run_metadata_is_separate() {
    let first = out_dir("compare_first");
    let second = out_dir("compare_second");

    for dir in [&first, &second] {
        let dir = dir.to_str().expect("the temp path should be UTF-8");
        reconcilers(&[
            "compare",
            "--left",
            LEGACY,
            "--right",
            NEW,
            "--config",
            CONFIG,
            "--out-dir",
            dir,
        ]);
    }

    let findings = read(first.join("findings.json"));
    assert_eq!(findings, read(second.join("findings.json")));
    assert_eq!(
        read(first.join("report.md")),
        read(second.join("report.md"))
    );
    insta::assert_snapshot!("findings_json", findings);
    insta::assert_snapshot!("report_md", read(first.join("report.md")));

    // The run metadata holds what the findings must not: time and paths.
    let run = read(first.join("run.json"));
    assert!(run.contains("\"started_at\""));
    assert!(run.contains(LEGACY));
    assert!(!findings.contains("started_at"));
    assert!(!findings.contains(LEGACY));
}

#[test]
fn schema_command_also_writes_the_output_files() {
    let dir = out_dir("schema_outputs");
    let output = reconcilers(&[
        "schema",
        "--left",
        LEGACY,
        "--right",
        NEW,
        "--policy",
        "compatible",
        "--out-dir",
        dir.to_str().expect("the temp path should be UTF-8"),
    ]);

    assert_eq!(output.status.code(), Some(1));
    insta::assert_snapshot!("schema_findings_json", read(dir.join("findings.json")));
    assert!(read(dir.join("report.md")).starts_with("# Reconciliation report"));
    assert!(read(dir.join("run.json")).contains("\"command\": \"schema\""));
}

#[test]
fn showing_values_puts_real_keys_in_the_report() {
    let output = reconcilers(&[
        "compare",
        "--left",
        LEGACY,
        "--right",
        NEW,
        "--config",
        "tests/fixtures/show_values.toml",
    ]);
    let text = stdout(&output);

    assert!(text.contains("only on the left: key (1004)"));
    assert!(text.contains("changed: key (1002): `trade_amount` 2750.5 -> 2750.6"));
}

#[test]
fn values_are_hidden_from_the_report_by_default() {
    let output = reconcilers(&[
        "compare", "--left", LEGACY, "--right", NEW, "--config", CONFIG,
    ]);
    let text = stdout(&output);

    assert!(text.contains("only on the left: key fingerprint "));
    assert!(!text.contains("1004"));
    assert!(!text.contains("2750"));
}

#[test]
fn compare_with_a_missing_config_is_an_execution_error_with_exit_code_2() {
    let output = reconcilers(&[
        "compare",
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
fn compare_that_is_only_inconclusive_exits_with_code_3() {
    // A file compared with itself has no differences. But the engine has no
    // comparison rule for time-of-day columns yet, so it must not claim the
    // `opens_at` column is equal.
    let file = write_parquet("cli_inconclusive.parquet", &shops_with_opening_time());
    let file = file.to_str().expect("the temp path should be UTF-8");

    let output = reconcilers(&[
        "compare",
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

#[test]
fn rowdiff_on_the_sample_files_reports_the_rows_only() {
    let output = reconcilers(&[
        "rowdiff", "--left", LEGACY, "--right", NEW, "--config", CONFIG,
    ]);
    let text = stdout(&output);

    assert_eq!(output.status.code(), Some(1));
    assert!(!text.contains("Schema:"), "rowdiff does not judge schemas");
    assert!(!text.contains("Fingerprints:"));
    insta::assert_snapshot!(text);
}

#[test]
fn rowdiff_passes_where_compare_fails_on_a_schema_difference() {
    // The rows agree on every shared column, but the right file has one
    // more column. Only `compare` judges that.
    let left = batch(vec![
        ("id", Arc::new(Int64Array::from(vec![1, 2]))),
        ("owner", Arc::new(StringArray::from(vec!["ann", "bob"]))),
    ]);
    let right = batch(vec![
        ("id", Arc::new(Int64Array::from(vec![1, 2]))),
        ("owner", Arc::new(StringArray::from(vec!["ann", "bob"]))),
        (
            "region",
            Arc::new(StringArray::from(vec!["north", "south"])),
        ),
    ]);
    let left = write_parquet("cli_added_column_left.parquet", &left);
    let right = write_parquet("cli_added_column_right.parquet", &right);
    let left = left.to_str().expect("the temp path should be UTF-8");
    let right = right.to_str().expect("the temp path should be UTF-8");
    let config = "tests/fixtures/keyed_on_id.toml";

    let rowdiff = reconcilers(&[
        "rowdiff", "--left", left, "--right", right, "--config", config,
    ]);
    let compare = reconcilers(&[
        "compare", "--left", left, "--right", right, "--config", config,
    ]);

    assert_eq!(rowdiff.status.code(), Some(0));
    assert!(stdout(&rowdiff).starts_with("Verdict: PASS"));
    assert_eq!(compare.status.code(), Some(1));
    assert!(stdout(&compare).contains("`region` exists only on the right side"));
}

#[test]
fn rowdiff_command_writes_findings_without_a_schema_section() {
    let dir = out_dir("rowdiff_outputs");
    reconcilers(&[
        "rowdiff",
        "--left",
        LEGACY,
        "--right",
        NEW,
        "--config",
        CONFIG,
        "--out-dir",
        dir.to_str().expect("the temp path should be UTF-8"),
    ]);

    let findings = read(dir.join("findings.json"));
    assert!(findings.contains("\"rows\""));
    assert!(!findings.contains("\"schema\""));
    assert!(!findings.contains("\"fingerprints\""));
    assert!(read(dir.join("run.json")).contains("\"command\": \"rowdiff\""));
}

#[test]
fn compare_reads_csv_files_with_the_types_from_the_config() {
    let folder = out_dir("cli_csv");
    std::fs::create_dir_all(&folder).expect("the scratch folder should be creatable");
    let write = |name: &str, text: &str| {
        let path = folder.join(name);
        std::fs::write(&path, text).expect("the file should be writable");
        path.to_str()
            .expect("the temp path should be UTF-8")
            .to_string()
    };
    let left = write("left.csv", "id,amount\n1,10.50\n2,20.00\n");
    let right = write("right.csv", "id,amount\n1,10.50\n2,20.01\n");
    let config = write(
        "csv.toml",
        "[comparison]\nkeys = [\"id\"]\n[csv.types]\nid = \"int64\"\namount = \"decimal(18, 2)\"\n\
         [[tolerances]]\ncolumn = \"amount\"\nmax_difference = \"0.01\"\n",
    );

    let output = reconcilers(&[
        "compare", "--left", &left, "--right", &right, "--config", &config,
    ]);
    let text = stdout(&output);

    // 20.00 and 20.01 are within the tolerance, which only works because
    // `amount` was read as a decimal and not as text.
    assert_eq!(output.status.code(), Some(0), "{text}");
    assert!(text.contains("`amount`: 0 mismatch(es), 1 more within the tolerance of 0.01"));
}
