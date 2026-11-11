//! ReconcileRS: deterministic data reconciliation for Parquet datasets.
//!
//! The crate is organised as a pipeline. Each module has one job, and the
//! three functions at the bottom of this file run the modules in order:
//!
//! - [`compare_parquet_schemas`] checks the schemas only.
//! - [`compare_parquet_rows`] checks the rows only.
//! - [`compare_parquet`] runs the full reconciliation.
//!
//! | Module            | Job                                                          |
//! |-------------------|--------------------------------------------------------------|
//! | [`config`]        | Load and check `reconcile.toml`.                             |
//! | [`parquet_input`] | Read Parquet files from disk.                                |
//! | [`schema`]        | Describe schemas and list the *facts* that differ.           |
//! | [`plan`]          | Decide which columns are keys, compared or not comparable.   |
//! | [`value`]         | Convert Arrow cells to logical values that can be compared.  |
//! | [`normalize`]     | Apply opt-in rules (Unicode, naive timestamps) to values.    |
//! | [`table`]         | Hold one side's values in memory.                            |
//! | [`tolerance`]     | Decide whether two numbers are close enough.                 |
//! | [`rowdiff`]       | Pair rows by key and list the *facts* that differ.           |
//! | [`canonical`]     | Define the exact bytes of a value, for hashing.              |
//! | [`fingerprint`]   | Hash files, schemas and table content.                       |
//! | [`policy`]        | Turn facts into verdicts (pass / warn / inconclusive / fail).|
//! | [`report`]        | Assemble facts and verdicts into a deterministic report.     |
//! | [`run`]           | Record what is specific to one run: time, paths, file sizes. |
//!
//! Facts and verdicts are kept apart on purpose: `schema` and `rowdiff`
//! never decide whether a difference is acceptable, and `policy` never looks
//! at files.
//!
//! In every comparison the **left** dataset is the baseline (for example the
//! legacy system) and the **right** dataset is the candidate (for example the
//! migrated system).

pub mod canonical;
pub mod config;
pub mod error;
pub mod fingerprint;
pub mod normalize;
pub mod parquet_input;
pub mod plan;
pub mod policy;
pub mod report;
pub mod rowdiff;
pub mod run;
pub mod schema;
pub mod table;
pub mod tolerance;
pub mod value;

use std::path::PathBuf;

pub use config::Config;
pub use error::Error;
use fingerprint::Fingerprint;
use parquet_input::InputFile;
use plan::ComparisonPlan;
use policy::SchemaPolicy;
use report::{FingerprintSection, Report, RowSection, SchemaSection, SideFingerprints};
use table::Table;

/// Compares the schemas of two Parquet datasets.
///
/// Each side is one or more Parquet files. Only the file footers are read,
/// so this is fast even for large files.
pub fn compare_parquet_schemas(
    left: &[PathBuf],
    right: &[PathBuf],
    policy: SchemaPolicy,
) -> Result<Report, Error> {
    let left_schema = parquet_input::read_schema(left)?;
    let right_schema = parquet_input::read_schema(right)?;

    let schema_section = SchemaSection::new(&left_schema, &right_schema, policy);
    Ok(Report::schema_only(schema_section))
}

/// The result of [`compare_parquet_rows`] and [`compare_parquet`].
#[derive(Debug, Clone)]
pub struct Reconciliation {
    /// The deterministic report: facts, verdicts and fingerprints.
    pub report: Report,
    /// The files that were read, with their sizes and fingerprints. Kept
    /// outside the report because they depend on file layout and location.
    pub left_files: Vec<InputFile>,
    pub right_files: Vec<InputFile>,
}

/// Compares the rows of two Parquet datasets by key.
///
/// The schemas are read, because they decide which columns can be compared,
/// but they are not judged: `schema_policy` in the configuration is ignored
/// and the report has no schema section. Use [`compare_parquet`] to judge
/// both.
///
/// Each side is one or more Parquet files. Both datasets are loaded into
/// memory, so they must fit in RAM.
pub fn compare_parquet_rows(
    config: &Config,
    left: &[PathBuf],
    right: &[PathBuf],
) -> Result<Reconciliation, Error> {
    let compared = ComparedRows::load(config, left, right)?;

    Ok(Reconciliation {
        report: Report::rows_only(compared.row_section),
        left_files: compared.left_files,
        right_files: compared.right_files,
    })
}

/// Compares two Parquet datasets in full: their schemas, their rows and
/// their fingerprints, with one overall verdict.
///
/// Each side is one or more Parquet files. Both datasets are loaded into
/// memory, so they must fit in RAM.
pub fn compare_parquet(
    config: &Config,
    left: &[PathBuf],
    right: &[PathBuf],
) -> Result<Reconciliation, Error> {
    let compared = ComparedRows::load(config, left, right)?;
    let plan = &compared.plan;

    let schema_section = SchemaSection::new(
        &plan.left_schema,
        &plan.right_schema,
        config.comparison.schema_policy,
    );

    let fingerprint_section = FingerprintSection::new(
        plan.normalization,
        config.fingerprint(),
        SideFingerprints {
            schema: Fingerprint::of_schema(&plan.left_schema),
            content: Fingerprint::of_table(&compared.left_table),
        },
        SideFingerprints {
            schema: Fingerprint::of_schema(&plan.right_schema),
            content: Fingerprint::of_table(&compared.right_table),
        },
    );

    Ok(Reconciliation {
        report: Report::new(schema_section, compared.row_section, fingerprint_section),
        left_files: compared.left_files,
        right_files: compared.right_files,
    })
}

/// Everything the row comparison produces. [`compare_parquet_rows`] reports
/// the row section alone; [`compare_parquet`] adds the schema verdict and
/// the fingerprints, which need the plan and the tables.
struct ComparedRows {
    plan: ComparisonPlan,
    left_table: Table,
    right_table: Table,
    row_section: RowSection,
    left_files: Vec<InputFile>,
    right_files: Vec<InputFile>,
}

impl ComparedRows {
    /// Reads both datasets, checks the configuration against their schemas,
    /// and compares the rows.
    fn load(config: &Config, left: &[PathBuf], right: &[PathBuf]) -> Result<Self, Error> {
        let left_dataset = parquet_input::read_dataset(left)?;
        let right_dataset = parquet_input::read_dataset(right)?;

        // Checks the configuration against both schemas before touching rows.
        let plan = ComparisonPlan::new(config, &left_dataset.schema, &right_dataset.schema)?;

        let left_table = Table::load(
            &left_dataset.batches,
            &plan.left_columns_to_load,
            &plan.normalization,
        );
        let right_table = Table::load(
            &right_dataset.batches,
            &plan.right_columns_to_load,
            &plan.normalization,
        );

        let diff = rowdiff::diff_rows(
            &left_table,
            &right_table,
            &plan.keys,
            &plan.compared_columns,
            &plan.tolerances,
        );
        let row_section = RowSection::new(&plan, &diff, &config.policy, &config.report);

        Ok(Self {
            plan,
            left_table,
            right_table,
            row_section,
            left_files: left_dataset.files,
            right_files: right_dataset.files,
        })
    }
}
