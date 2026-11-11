//! The error type for everything that can go wrong while *running* a
//! reconciliation: unreadable files, invalid Parquet, unusable configuration.
//!
//! A dataset difference is never an `Error`. Differences are findings in a
//! report; an `Error` means no trustworthy report could be produced at all.

use std::path::PathBuf;

use crate::config::ConfigError;

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

    #[error("cannot read rows from `{path}`")]
    ReadRows {
        path: PathBuf,
        source: arrow::error::ArrowError,
    },

    #[error("a dataset needs at least one file")]
    NoInputFiles,

    #[error(
        "`{other}` does not have the same schema as `{first}`, so they cannot form one dataset"
    )]
    FilesDisagreeOnSchema { first: PathBuf, other: PathBuf },

    #[error("cannot read configuration file `{path}`")]
    ReadConfig {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("the configuration is not valid TOML for ReconcileRS")]
    ParseConfig {
        // Boxed because the TOML error is large and would bloat every `Result`.
        source: Box<toml::de::Error>,
    },

    #[error("invalid configuration: {0}")]
    Config(#[from] ConfigError),
}
