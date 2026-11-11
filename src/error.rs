//! The error type for everything that can go wrong while *running* a
//! reconciliation: unreadable files, invalid Parquet, unwritable reports.
//!
//! A dataset difference is never an `Error`. Differences are findings in a
//! report; an `Error` means no trustworthy report could be produced at all.

use std::path::PathBuf;

/// An execution failure. The CLI maps every variant to exit code 2.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot open `{path}`")]
    OpenFile {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("cannot read Parquet metadata from `{path}`")]
    ReadParquet {
        path: PathBuf,
        source: parquet::errors::ParquetError,
    },

    #[error(
        "`{path}` has more than one column named `{column}`, so columns cannot be matched by name"
    )]
    DuplicateColumn { path: PathBuf, column: String },

    #[error("cannot write report to `{path}`")]
    WriteReport {
        path: PathBuf,
        source: std::io::Error,
    },
}
