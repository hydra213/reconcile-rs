//! The `reconcilers` command-line tool.
//!
//! This file only parses arguments, calls the library, writes the output
//! files and chooses an exit code. All reconciliation logic lives in the
//! library.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};

use reconcilers::Config;
use reconcilers::policy::{SchemaPolicy, Verdict};
use reconcilers::report::Report;
use reconcilers::run::{RunFile, RunMetadata, TOOL_VERSION};

/// Exit code when the comparison ran and the policy holds.
const EXIT_PASS: u8 = 0;
/// Exit code when the comparison ran and the policy is violated.
const EXIT_POLICY_VIOLATED: u8 = 1;
/// Exit code when the comparison could not run (bad input, IO error).
/// `clap` uses the same code for invalid arguments.
const EXIT_EXECUTION_ERROR: u8 = 2;
/// Exit code when the comparison ran but could not reach a decision.
const EXIT_INCONCLUSIVE: u8 = 3;

#[derive(Parser)]
#[command(
    name = "reconcilers",
    version,
    about = "Deterministic data reconciliation for Parquet datasets"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Compare the schemas of two Parquet datasets.
    Schema(SchemaArgs),
    /// Compare the schemas and rows of two Parquet datasets.
    Rowdiff(RowdiffArgs),
}

#[derive(clap::Args)]
struct SchemaArgs {
    #[command(flatten)]
    datasets: DatasetArgs,

    /// How strictly the right schema must match the left schema.
    #[arg(long, value_enum, default_value = "strict")]
    policy: PolicyArg,
}

#[derive(clap::Args)]
struct RowdiffArgs {
    #[command(flatten)]
    datasets: DatasetArgs,

    /// The TOML file that names the keys, column mappings and policy.
    #[arg(long)]
    config: PathBuf,
}

/// The arguments every comparison command shares.
#[derive(clap::Args)]
struct DatasetArgs {
    /// The baseline: one or more Parquet files with the same schema.
    #[arg(long, required = true, num_args = 1..)]
    left: Vec<PathBuf>,

    /// The candidate: one or more Parquet files with the same schema.
    #[arg(long, required = true, num_args = 1..)]
    right: Vec<PathBuf>,

    /// Write `findings.json`, `report.md` and `run.json` to this directory.
    /// The directory is created if it does not exist.
    #[arg(long)]
    out_dir: Option<PathBuf>,
}

/// The command-line spelling of [`SchemaPolicy`]. Kept separate so the
/// library does not depend on `clap`.
#[derive(Clone, Copy, ValueEnum)]
enum PolicyArg {
    Strict,
    Compatible,
}

impl From<PolicyArg> for SchemaPolicy {
    fn from(arg: PolicyArg) -> Self {
        match arg {
            PolicyArg::Strict => Self::Strict,
            PolicyArg::Compatible => Self::Compatible,
        }
    }
}

/// What a command produced: the report, and the parts of the run metadata
/// that only the command knows.
struct Outcome {
    report: Report,
    command: &'static str,
    config_file: Option<PathBuf>,
    left_files: Vec<RunFile>,
    right_files: Vec<RunFile>,
    out_dir: Option<PathBuf>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let started_at = chrono::Utc::now();
    let stopwatch = Instant::now();

    let result = match cli.command {
        Command::Schema(args) => run_schema(args),
        Command::Rowdiff(args) => run_rowdiff(args),
    }
    .and_then(|outcome| finish(outcome, started_at, stopwatch));

    match result {
        Ok(verdict) => ExitCode::from(exit_code_for(verdict)),
        Err(error) => {
            // `{:#}` prints the whole chain of causes on one line.
            eprintln!("error: {error:#}");
            ExitCode::from(EXIT_EXECUTION_ERROR)
        }
    }
}

/// Prints the summary, writes the output files if asked, and returns the
/// verdict that decides the exit code.
fn finish(
    outcome: Outcome,
    started_at: chrono::DateTime<chrono::Utc>,
    stopwatch: Instant,
) -> anyhow::Result<Verdict> {
    print!("{}", outcome.report.to_text());

    if let Some(out_dir) = &outcome.out_dir {
        let run = RunMetadata {
            tool_version: TOOL_VERSION,
            command: outcome.command.to_string(),
            started_at: started_at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            duration_ms: stopwatch.elapsed().as_millis() as u64,
            config_file: outcome
                .config_file
                .as_ref()
                .map(|path| path.display().to_string()),
            left_files: outcome.left_files,
            right_files: outcome.right_files,
        };
        write_outputs(out_dir, &outcome.report, &run)?;
    }
    Ok(outcome.report.verdict)
}

fn run_schema(args: SchemaArgs) -> anyhow::Result<Outcome> {
    let SchemaArgs { datasets, policy } = args;

    let report =
        reconcilers::compare_parquet_schemas(&datasets.left, &datasets.right, policy.into())
            .context("schema comparison could not run")?;

    // Only the file footers were read, so there are no sizes or fingerprints.
    let paths_only =
        |paths: &[PathBuf]| paths.iter().map(|path| RunFile::path_only(path)).collect();
    Ok(Outcome {
        report,
        command: "schema",
        config_file: None,
        left_files: paths_only(&datasets.left),
        right_files: paths_only(&datasets.right),
        out_dir: datasets.out_dir,
    })
}

fn run_rowdiff(args: RowdiffArgs) -> anyhow::Result<Outcome> {
    let RowdiffArgs { datasets, config } = args;

    let settings = Config::from_toml_file(&config).context("row comparison could not run")?;
    let reconciliation = reconcilers::compare_parquet(&settings, &datasets.left, &datasets.right)
        .context("row comparison could not run")?;

    Ok(Outcome {
        report: reconciliation.report,
        command: "rowdiff",
        config_file: Some(config),
        left_files: reconciliation
            .left_files
            .iter()
            .map(RunFile::from)
            .collect(),
        right_files: reconciliation
            .right_files
            .iter()
            .map(RunFile::from)
            .collect(),
        out_dir: datasets.out_dir,
    })
}

/// Writes the three output files into `out_dir`.
///
/// - `findings.json`: the deterministic report.
/// - `report.md`: the same report for people.
/// - `run.json`: when and where this run happened.
fn write_outputs(out_dir: &Path, report: &Report, run: &RunMetadata) -> anyhow::Result<()> {
    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("cannot create output directory `{}`", out_dir.display()))?;

    let files = [
        ("findings.json", report.to_json()),
        ("report.md", report.to_markdown()),
        ("run.json", run.to_json()),
    ];
    for (file_name, contents) in files {
        let path = out_dir.join(file_name);
        std::fs::write(&path, contents)
            .with_context(|| format!("cannot write `{}`", path.display()))?;
    }
    Ok(())
}

fn exit_code_for(verdict: Verdict) -> u8 {
    match verdict {
        Verdict::Pass | Verdict::Warn => EXIT_PASS,
        Verdict::Fail => EXIT_POLICY_VIOLATED,
        Verdict::Inconclusive => EXIT_INCONCLUSIVE,
    }
}
