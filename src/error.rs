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
    #[error("cannot open `{location}`")]
    ReadFile {
        location: String,
        source: std::io::Error,
    },

    #[error("`{location}` is not a valid URL")]
    InvalidUrl {
        location: String,
        source: url::ParseError,
    },

    #[error("cannot read `{location}` from object storage")]
    ReadObjectStorage {
        location: String,
        source: object_store::Error,
    },

    #[error("`{location}` contains no .parquet or .csv files")]
    NoDataFilesInFolder { location: String },

    #[error("cannot read Parquet metadata from `{location}`")]
    ReadParquet {
        location: String,
        source: parquet::errors::ParquetError,
    },

    #[error("cannot read rows from `{location}`")]
    ReadRows {
        location: String,
        source: arrow::error::ArrowError,
    },

    #[error("cannot read `{location}` as CSV")]
    ReadCsv {
        location: String,
        source: arrow::error::ArrowError,
    },

    #[error(
        "`{location}` has more than one column named `{column}`, so columns cannot be matched by name"
    )]
    DuplicateColumn { location: String, column: String },

    #[error(
        "the table has more than one column named `{column}`, so columns cannot be matched by name"
    )]
    DuplicateColumnInTable { column: String },

    #[error("record batch {batch} does not have the columns of the table's schema")]
    BatchDisagreesWithSchema { batch: usize },

    #[error("a dataset needs at least one file")]
    NoInputFiles,

    #[error(
        "`{other}` does not have the same schema as `{first}`, so they cannot form one dataset"
    )]
    FilesDisagreeOnSchema { first: String, other: String },

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

    #[error("the configuration is not valid JSON for ReconcileRS")]
    ParseConfigJson { source: serde_json::Error },

    #[error("invalid configuration: {0}")]
    Config(#[from] ConfigError),
}
