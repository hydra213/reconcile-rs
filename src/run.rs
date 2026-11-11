//! Run metadata: the facts about one execution that must *not* be in the
//! report.
//!
//! A report is deterministic: the same data and configuration always give
//! the same bytes. Timestamps, durations, file paths and digests of raw file
//! bytes change from run to run or machine to machine, so they are kept
//! here and written to a separate `run.json`.

use std::path::Path;

use serde::Serialize;

use crate::fingerprint::Fingerprint;
use crate::parquet_input::InputFile;

/// Everything worth recording about one run of the tool.
#[derive(Debug, Clone, Serialize)]
pub struct RunMetadata {
    /// The version of this crate that produced the report.
    pub tool_version: &'static str,
    /// The subcommand that ran, for example `rowdiff`.
    pub command: String,
    /// When the run started, in UTC, as an RFC 3339 timestamp.
    pub started_at: String,
    pub duration_ms: u64,
    /// The configuration file, if the command used one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_file: Option<String>,
    pub left_files: Vec<RunFile>,
    pub right_files: Vec<RunFile>,
}

/// One input file as seen by this run.
#[derive(Debug, Clone, Serialize)]
pub struct RunFile {
    pub path: String,
    /// Present only when the file's rows were read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    /// The fingerprint of the file's bytes. Present only when the file's
    /// rows were read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<Fingerprint>,
}

impl RunFile {
    /// A file whose footer was read but whose contents were not.
    pub fn path_only(path: &Path) -> Self {
        Self {
            path: path.display().to_string(),
            size_bytes: None,
            fingerprint: None,
        }
    }
}

impl From<&InputFile> for RunFile {
    fn from(file: &InputFile) -> Self {
        Self {
            path: file.path.display().to_string(),
            size_bytes: Some(file.size_bytes),
            fingerprint: Some(file.fingerprint),
        }
    }
}

impl RunMetadata {
    /// The metadata as pretty-printed JSON, ending with a newline.
    pub fn to_json(&self) -> String {
        let mut json = serde_json::to_string_pretty(self)
            .expect("RunMetadata contains only strings and numbers");
        json.push('\n');
        json
    }
}

/// The version of this crate, from `Cargo.toml`.
pub const TOOL_VERSION: &str = env!("CARGO_PKG_VERSION");
