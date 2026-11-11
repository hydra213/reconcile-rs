//! ReconcileRS: deterministic data reconciliation for Parquet datasets.
//!
//! The crate is organised as a pipeline. Each module has one job, and the
//! two functions at the bottom of this file run the modules in order.
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
pub mod schema;
pub mod table;
pub mod tolerance;
pub mod value;

use std::path::PathBuf;

pub use config::Config;
pub use error::Error;
use fingerprint::Fingerprint;
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

/// Compares two Parquet datasets: first their schemas, then their rows.
///
/// Each side is one or more Parquet files. Both datasets are loaded into
/// memory, so they must fit in RAM.
pub fn compare_parquet(
    config: &Config,
    left: &[PathBuf],
    right: &[PathBuf],
) -> Result<Report, Error> {
    let left_dataset = parquet_input::read_dataset(left)?;
    let right_dataset = parquet_input::read_dataset(right)?;

    // Checks the configuration against both schemas before touching rows.
    let plan = ComparisonPlan::new(config, &left_dataset.schema, &right_dataset.schema)?;

    let schema_section = SchemaSection::new(
        &plan.left_schema,
        &plan.right_schema,
        config.comparison.schema_policy,
    );

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
    let row_section = RowSection::new(&plan, &diff, &config.policy);

    let fingerprint_section = FingerprintSection::new(
        plan.normalization,
        config.fingerprint(),
        SideFingerprints {
            schema: Fingerprint::of_schema(&plan.left_schema),
            content: Fingerprint::of_table(&left_table),
        },
        SideFingerprints {
            schema: Fingerprint::of_schema(&plan.right_schema),
            content: Fingerprint::of_table(&right_table),
        },
    );

    Ok(Report::new(
        schema_section,
        row_section,
        fingerprint_section,
    ))
}
