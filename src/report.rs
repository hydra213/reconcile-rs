//! Reports: the facts and verdicts of a comparison in one deterministic
//! document.
//!
//! A report contains only things derived from the data and the policy. It
//! never contains timestamps, file paths or durations, so the same inputs
//! always produce byte-identical JSON.

use std::fmt::Write as _;
use std::path::Path;

use serde::Serialize;

use crate::error::Error;
use crate::policy::{self, SchemaFinding, SchemaPolicy, Verdict};
use crate::schema::{self, Column, LogicalSchema, SchemaDifference};

/// Bumped whenever the JSON layout of a report changes.
pub const REPORT_VERSION: u32 = 1;

/// The result of comparing two schemas under a policy.
#[derive(Debug, Clone, Serialize)]
pub struct SchemaReport {
    pub report_version: u32,
    pub schema_policy: SchemaPolicy,
    pub verdict: Verdict,
    pub findings: Vec<SchemaFinding>,
    pub left_columns: Vec<Column>,
    pub right_columns: Vec<Column>,
}

impl SchemaReport {
    /// Diffs the two schemas and judges every difference under `policy`.
    pub fn new(left: LogicalSchema, right: LogicalSchema, policy: SchemaPolicy) -> Self {
        let differences = schema::diff_schemas(&left, &right);
        let findings = policy::evaluate_schema(differences, policy);
        let verdict = policy::overall_schema_verdict(&findings);

        Self {
            report_version: REPORT_VERSION,
            schema_policy: policy,
            verdict,
            findings,
            left_columns: left.columns().to_vec(),
            right_columns: right.columns().to_vec(),
        }
    }

    /// The report as pretty-printed JSON, ending with a newline.
    pub fn to_json(&self) -> String {
        let mut json = serde_json::to_string_pretty(self)
            .expect("a SchemaReport contains only strings, numbers and booleans");
        json.push('\n');
        json
    }

    /// Writes the JSON report to `path`, replacing any existing file.
    pub fn write_json(&self, path: &Path) -> Result<(), Error> {
        std::fs::write(path, self.to_json()).map_err(|source| Error::WriteReport {
            path: path.to_path_buf(),
            source,
        })
    }

    /// A short plain-text summary for the terminal.
    pub fn to_text(&self) -> String {
        let mut text = String::new();

        // Writing to a String cannot fail, so the results are ignored.
        let _ = writeln!(
            text,
            "Schema: {} (policy: {})",
            self.verdict, self.schema_policy
        );
        let _ = writeln!(
            text,
            "Columns: {} left, {} right",
            self.left_columns.len(),
            self.right_columns.len()
        );

        if self.findings.is_empty() {
            let _ = writeln!(text, "No schema differences.");
            return text;
        }

        for finding in &self.findings {
            let _ = writeln!(text);
            let _ = writeln!(
                text,
                "{}  {}",
                finding.verdict,
                describe_difference(&finding.difference)
            );
            let _ = writeln!(text, "      {}", finding.explanation);
        }
        text
    }
}

fn describe_difference(difference: &SchemaDifference) -> String {
    match difference {
        SchemaDifference::ColumnRemoved { column } => {
            format!("`{column}` is missing from the right side")
        }
        SchemaDifference::ColumnAdded { column } => {
            format!("`{column}` exists only on the right side")
        }
        SchemaDifference::TypeChanged {
            column,
            left,
            right,
        } => format!("`{column}` changed type from {left} to {right}"),
        SchemaDifference::NullabilityChanged {
            column,
            left_nullable,
            right_nullable,
        } => format!(
            "`{column}` changed from {} to {}",
            nullability_name(*left_nullable),
            nullability_name(*right_nullable)
        ),
    }
}

fn nullability_name(nullable: bool) -> &'static str {
    if nullable { "nullable" } else { "not nullable" }
}
