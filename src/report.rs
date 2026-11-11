//! Reports: the facts and verdicts of a comparison in one deterministic
//! document.
//!
//! A report contains only things derived from the data and the policy. It
//! never contains timestamps, file paths or durations, so the same inputs
//! always produce byte-identical JSON.

use std::fmt::Write as _;
use std::path::Path;

use serde::Serialize;

use crate::canonical::CANONICAL_VERSION;
use crate::error::Error;
use crate::fingerprint::{self, Fingerprint};
use crate::normalize::Normalization;
use crate::plan::ComparisonPlan;
use crate::policy::{self, RowFinding, RowPolicy, SchemaFinding, SchemaPolicy, Verdict};
use crate::rowdiff::{RowCounts, RowDiff};
use crate::schema::{self, Column, LogicalSchema, SchemaDifference};

/// Bumped whenever the JSON layout of a report changes.
pub const REPORT_VERSION: u32 = 1;

/// The result of a reconciliation.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub report_version: u32,
    /// The worst verdict of all sections.
    pub verdict: Verdict,
    pub schema: SchemaSection,
    /// Absent when only the schemas were compared.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<RowSection>,
    /// Absent when only the schemas were compared.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprints: Option<FingerprintSection>,
}

/// How the two schemas differ and what the schema policy says about it.
#[derive(Debug, Clone, Serialize)]
pub struct SchemaSection {
    pub policy: SchemaPolicy,
    pub verdict: Verdict,
    pub findings: Vec<SchemaFinding>,
    pub left_columns: Vec<Column>,
    pub right_columns: Vec<Column>,
}

/// How the rows differ and what the row policy says about it.
#[derive(Debug, Clone, Serialize)]
pub struct RowSection {
    pub verdict: Verdict,
    pub keys: Vec<String>,
    pub counts: RowCounts,
    /// One finding per row check, including the checks that passed.
    pub findings: Vec<RowFinding>,
    /// Every non-key column that exists on both sides, sorted by name.
    pub columns: Vec<ColumnResult>,
}

/// Digests that tie the report to its exact inputs and settings.
///
/// Fingerprints are a screening signal and never decide the verdict. They
/// are exact, so values that differ within a tolerance still give different
/// content fingerprints.
#[derive(Debug, Clone, Serialize)]
pub struct FingerprintSection {
    pub algorithm: &'static str,
    pub canonical_version: u32,
    /// The normalisation rules applied before fingerprinting.
    pub normalization: Normalization,
    /// The fingerprint of the whole configuration.
    pub configuration: Fingerprint,
    pub left: SideFingerprints,
    pub right: SideFingerprints,
    /// Whether the two content fingerprints are equal.
    pub content_matches: bool,
}

/// The fingerprints of one dataset.
///
/// Both are independent of row order and file layout, so rewriting a file
/// with its rows in another order does not change the report. Fingerprints
/// of the raw file bytes are deliberately not included here for the same
/// reason; they are available on [`crate::parquet_input::Dataset`].
#[derive(Debug, Clone, Serialize)]
pub struct SideFingerprints {
    /// The logical schema, after column mappings.
    pub schema: Fingerprint,
    /// Every row over the key and compared columns, independent of row
    /// order. Columns that exist on one side only, or that could not be
    /// compared, are not included.
    pub content: Fingerprint,
}

/// What happened to one column during the row comparison.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ColumnResult {
    /// The column was compared on every compared row.
    Compared {
        column: String,
        /// Rows where the values differ beyond any tolerance.
        mismatches: u64,
        /// The allowed numeric difference, if one is configured.
        #[serde(skip_serializing_if = "Option::is_none")]
        tolerance: Option<String>,
        /// Rows where the values differ but are within the tolerance.
        within_tolerance: u64,
    },
    /// The column could not be compared; `reason` says why.
    Inconclusive { column: String, reason: String },
}

impl ColumnResult {
    fn column(&self) -> &str {
        match self {
            Self::Compared { column, .. } | Self::Inconclusive { column, .. } => column,
        }
    }
}

impl SchemaSection {
    /// Diffs the two schemas and judges every difference under `policy`.
    pub fn new(left: &LogicalSchema, right: &LogicalSchema, policy: SchemaPolicy) -> Self {
        let differences = schema::diff_schemas(left, right);
        let findings = policy::evaluate_schema(differences, policy);
        let verdict = policy::overall_schema_verdict(&findings);

        Self {
            policy,
            verdict,
            findings,
            left_columns: left.columns().to_vec(),
            right_columns: right.columns().to_vec(),
        }
    }
}

impl RowSection {
    /// Judges the observed row differences under `policy`.
    pub fn new(plan: &ComparisonPlan, diff: &RowDiff, policy: &RowPolicy) -> Self {
        let counts = diff.counts();
        let findings =
            policy::evaluate_rows(&counts, plan.inconclusive_columns.len() as u64, policy);
        let verdict = policy::worst_verdict(findings.iter().map(|finding| finding.verdict));

        let compared =
            diff.column_mismatches
                .iter()
                .map(|(column, mismatches)| ColumnResult::Compared {
                    column: column.clone(),
                    mismatches: *mismatches,
                    tolerance: plan
                        .tolerances
                        .get(column)
                        .map(|tolerance| tolerance.max_difference().to_string()),
                    within_tolerance: diff.column_within_tolerance[column],
                });
        let inconclusive =
            plan.inconclusive_columns
                .iter()
                .map(|inconclusive| ColumnResult::Inconclusive {
                    column: inconclusive.column.clone(),
                    reason: inconclusive.reason.to_string(),
                });
        let mut columns: Vec<ColumnResult> = compared.chain(inconclusive).collect();
        columns.sort_by(|a, b| a.column().cmp(b.column()));

        Self {
            verdict,
            keys: plan.keys.clone(),
            counts,
            findings,
            columns,
        }
    }
}

impl FingerprintSection {
    pub fn new(
        normalization: Normalization,
        configuration: Fingerprint,
        left: SideFingerprints,
        right: SideFingerprints,
    ) -> Self {
        Self {
            algorithm: fingerprint::ALGORITHM,
            canonical_version: CANONICAL_VERSION,
            normalization,
            configuration,
            content_matches: left.content == right.content,
            left,
            right,
        }
    }
}

impl Report {
    /// A report that covers the schemas only.
    pub fn schema_only(schema: SchemaSection) -> Self {
        Self {
            report_version: REPORT_VERSION,
            verdict: schema.verdict,
            schema,
            rows: None,
            fingerprints: None,
        }
    }

    /// A report that covers the schemas and the rows.
    pub fn new(schema: SchemaSection, rows: RowSection, fingerprints: FingerprintSection) -> Self {
        Self {
            report_version: REPORT_VERSION,
            verdict: schema.verdict.max(rows.verdict),
            schema,
            rows: Some(rows),
            fingerprints: Some(fingerprints),
        }
    }

    /// The report as pretty-printed JSON, ending with a newline.
    pub fn to_json(&self) -> String {
        let mut json = serde_json::to_string_pretty(self)
            .expect("a Report contains only strings, numbers and booleans");
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

    /// A plain-text summary for the terminal.
    pub fn to_text(&self) -> String {
        let mut text = String::new();
        // Writing to a String cannot fail, so the results are ignored here
        // and in the helper functions below.
        let _ = writeln!(text, "Verdict: {}", self.verdict);
        let _ = writeln!(text);
        write_schema_text(&mut text, &self.schema);
        if let Some(rows) = &self.rows {
            let _ = writeln!(text);
            write_rows_text(&mut text, rows);
        }
        if let Some(fingerprints) = &self.fingerprints {
            let _ = writeln!(text);
            write_fingerprints_text(&mut text, fingerprints);
        }
        text
    }
}

fn write_schema_text(text: &mut String, schema: &SchemaSection) {
    let _ = writeln!(
        text,
        "Schema: {} (policy: {})",
        schema.verdict, schema.policy
    );
    let _ = writeln!(
        text,
        "  {} left columns, {} right columns",
        schema.left_columns.len(),
        schema.right_columns.len()
    );

    if schema.findings.is_empty() {
        let _ = writeln!(text, "  No schema differences.");
    }
    for finding in &schema.findings {
        let _ = writeln!(
            text,
            "  {:<12}  {}",
            finding.verdict,
            describe_difference(&finding.difference)
        );
        let _ = writeln!(text, "  {:<12}  {}", "", finding.explanation);
    }
}

fn write_rows_text(text: &mut String, rows: &RowSection) {
    let counts = &rows.counts;
    let _ = writeln!(
        text,
        "Rows: {} (keys: {})",
        rows.verdict,
        rows.keys.join(", ")
    );
    let _ = writeln!(
        text,
        "  {} left rows, {} right rows, {} matched keys, {} rows compared",
        counts.left_rows, counts.right_rows, counts.matched_keys, counts.compared_rows
    );
    for finding in &rows.findings {
        let _ = writeln!(text, "  {:<12}  {}", finding.verdict, finding.explanation);
    }

    let _ = writeln!(text);
    let _ = writeln!(text, "Columns:");
    if rows.columns.is_empty() {
        let _ = writeln!(text, "  No columns besides the keys exist on both sides.");
    }
    for column in &rows.columns {
        match column {
            ColumnResult::Compared {
                column,
                mismatches,
                tolerance,
                within_tolerance,
            } => {
                let _ = write!(text, "  `{column}`: {mismatches} mismatch(es)");
                if let Some(tolerance) = tolerance {
                    let _ = write!(
                        text,
                        ", {within_tolerance} more within the tolerance of {tolerance}"
                    );
                }
                let _ = writeln!(text);
            }
            ColumnResult::Inconclusive { column, reason } => {
                let _ = writeln!(text, "  `{column}`: not compared, {reason}");
            }
        }
    }
}

fn write_fingerprints_text(text: &mut String, fingerprints: &FingerprintSection) {
    let outcome = if fingerprints.content_matches {
        "MATCH"
    } else {
        "DIFFERENT"
    };
    let _ = writeln!(
        text,
        "Fingerprints: {outcome} ({}, canonical version {})",
        fingerprints.algorithm, fingerprints.canonical_version
    );
    let _ = writeln!(
        text,
        "  A screening signal over the key and compared columns, not a verdict."
    );
    let _ = writeln!(text, "  left content   {}", fingerprints.left.content);
    let _ = writeln!(text, "  right content  {}", fingerprints.right.content);
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
