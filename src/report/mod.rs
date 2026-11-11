//! Reports: the facts and verdicts of a comparison in one deterministic
//! document.
//!
//! A report contains only things derived from the data and the policy. It
//! never contains timestamps, file paths or durations, so the same inputs
//! always produce byte-identical JSON. Those machine-specific details live
//! in [`crate::run::RunMetadata`] instead.
//!
//! - This file defines the report and builds it.
//! - [`samples`] picks the example keys and decides what may be shown.
//! - [`text`] and [`markdown`] render a report for people.

pub mod markdown;
pub mod samples;
pub mod text;

use serde::Serialize;

use crate::canonical::CANONICAL_VERSION;
use crate::config::ReportConfig;
use crate::fingerprint::{self, Fingerprint};
use crate::normalize::Normalization;
use crate::plan::ComparisonPlan;
use crate::policy::{self, RowFinding, RowPolicy, SchemaFinding, SchemaPolicy, Verdict};
use crate::rowdiff::{RowCounts, RowDiff};
use crate::schema::{self, Column, LogicalSchema};
use samples::Samples;

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
    /// A bounded number of example keys for each kind of difference.
    pub samples: Samples,
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
/// reason; they are recorded in [`crate::run::RunMetadata`].
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
    pub fn column(&self) -> &str {
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
    pub fn new(
        plan: &ComparisonPlan,
        diff: &RowDiff,
        policy: &RowPolicy,
        report_config: &ReportConfig,
    ) -> Self {
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
            samples: Samples::new(diff, &plan.keys, report_config),
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

impl FingerprintSection {
    /// `MATCH` or `DIFFERENT`, for text and Markdown reports.
    pub fn outcome(&self) -> &'static str {
        if self.content_matches {
            "MATCH"
        } else {
            "DIFFERENT"
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

    /// A plain-text summary for the terminal.
    pub fn to_text(&self) -> String {
        text::render(self)
    }

    /// The report as a Markdown document.
    pub fn to_markdown(&self) -> String {
        markdown::render(self)
    }
}
