//! ReconcileRS: deterministic data reconciliation for Parquet datasets.
//!
//! The crate is organised as a pipeline. Each module has one job:
//!
//! | Module            | Job                                                        |
//! |-------------------|------------------------------------------------------------|
//! | [`parquet_input`] | Read Parquet files from disk.                              |
//! | [`schema`]        | Describe schemas and list the *facts* that differ.         |
//! | [`policy`]        | Turn facts into verdicts (pass / warn / fail).             |
//! | [`report`]        | Assemble facts and verdicts into a deterministic report.   |
//!
//! Facts and verdicts are kept apart on purpose: `schema` never decides
//! whether a difference is acceptable, and `policy` never looks at files.
//!
//! In every comparison the **left** dataset is the baseline (for example the
//! legacy system) and the **right** dataset is the candidate (for example the
//! migrated system).

pub mod error;
pub mod parquet_input;
pub mod policy;
pub mod report;
pub mod schema;

use std::path::Path;

pub use error::Error;
use policy::SchemaPolicy;
use report::SchemaReport;

/// Reads the schemas of two Parquet files and reports how they differ.
///
/// Only the file footers are read, so this is fast even for large files.
pub fn compare_parquet_schemas(
    left: &Path,
    right: &Path,
    policy: SchemaPolicy,
) -> Result<SchemaReport, Error> {
    let left_schema = parquet_input::read_schema(left)?;
    let right_schema = parquet_input::read_schema(right)?;
    Ok(SchemaReport::new(left_schema, right_schema, policy))
}
