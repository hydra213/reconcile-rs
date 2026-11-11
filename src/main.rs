//! The `reconcilers` command-line tool.
//!
//! This file only parses arguments, calls the library and chooses an exit
//! code. All reconciliation logic lives in the library.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};

use reconcilers::Config;
use reconcilers::policy::{SchemaPolicy, Verdict};
use reconcilers::report::Report;

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
    /// The baseline: one or more Parquet files with the same schema.
    #[arg(long, required = true, num_args = 1..)]
    left: Vec<PathBuf>,

    /// The candidate: one or more Parquet files with the same schema.
    #[arg(long, required = true, num_args = 1..)]
    right: Vec<PathBuf>,

    /// How strictly the right schema must match the left schema.
    #[arg(long, value_enum, default_value = "strict")]
    policy: PolicyArg,

    /// Also write the full report as JSON to this path.
    #[arg(long)]
    report: Option<PathBuf>,
}

#[derive(clap::Args)]
struct RowdiffArgs {
    /// The baseline: one or more Parquet files with the same schema.
    #[arg(long, required = true, num_args = 1..)]
    left: Vec<PathBuf>,

    /// The candidate: one or more Parquet files with the same schema.
    #[arg(long, required = true, num_args = 1..)]
    right: Vec<PathBuf>,

    /// The TOML file that names the keys, column mappings and policy.
    #[arg(long)]
    config: PathBuf,

    /// Also write the full report as JSON to this path.
    #[arg(long)]
    report: Option<PathBuf>,
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

fn main() -> ExitCode {
    let cli = Cli::parse();

    let result = match cli.command {
        Command::Schema(args) => run_schema(args),
        Command::Rowdiff(args) => run_rowdiff(args),
    };

    match result {
        Ok(verdict) => ExitCode::from(exit_code_for(verdict)),
        Err(error) => {
            // `{:#}` prints the whole chain of causes on one line.
            eprintln!("error: {error:#}");
            ExitCode::from(EXIT_EXECUTION_ERROR)
        }
    }
}

fn run_schema(args: SchemaArgs) -> anyhow::Result<Verdict> {
    let report = reconcilers::compare_parquet_schemas(&args.left, &args.right, args.policy.into())
        .context("schema comparison could not run")?;
    finish(&report, args.report.as_deref())
}

fn run_rowdiff(args: RowdiffArgs) -> anyhow::Result<Verdict> {
    let config = Config::from_toml_file(&args.config).context("row comparison could not run")?;
    let report = reconcilers::compare_parquet(&config, &args.left, &args.right)
        .context("row comparison could not run")?;
    finish(&report, args.report.as_deref())
}

/// Prints the summary, writes the JSON report if asked, and returns the
/// verdict that decides the exit code.
fn finish(report: &Report, report_path: Option<&Path>) -> anyhow::Result<Verdict> {
    print!("{}", report.to_text());

    if let Some(path) = report_path {
        report.write_json(path)?;
    }
    Ok(report.verdict)
}

fn exit_code_for(verdict: Verdict) -> u8 {
    match verdict {
        Verdict::Pass | Verdict::Warn => EXIT_PASS,
        Verdict::Fail => EXIT_POLICY_VIOLATED,
        Verdict::Inconclusive => EXIT_INCONCLUSIVE,
    }
}
