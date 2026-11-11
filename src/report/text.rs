//! The plain-text rendering of a report, for the terminal.

use std::fmt::Write as _;

use super::samples::Samples;
use super::{ColumnResult, FingerprintSection, Report, RowSection, SchemaSection};

/// Renders the whole report. Called through [`Report::to_text`].
///
/// Writing to a `String` cannot fail, so the results of `writeln!` are
/// ignored throughout this file.
pub fn render(report: &Report) -> String {
    let mut text = String::new();
    let _ = writeln!(text, "Verdict: {}", report.verdict);
    let _ = writeln!(text);
    write_schema(&mut text, &report.schema);
    if let Some(rows) = &report.rows {
        let _ = writeln!(text);
        write_rows(&mut text, rows);
    }
    if let Some(fingerprints) = &report.fingerprints {
        let _ = writeln!(text);
        write_fingerprints(&mut text, fingerprints);
    }
    text
}

fn write_schema(text: &mut String, schema: &SchemaSection) {
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
        let _ = writeln!(text, "  {:<12}  {}", finding.verdict, finding.difference);
        let _ = writeln!(text, "  {:<12}  {}", "", finding.explanation);
    }
}

fn write_rows(text: &mut String, rows: &RowSection) {
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
        let _ = writeln!(text, "  {}", describe_column(column));
    }

    if !rows.samples.is_empty() {
        let _ = writeln!(text);
        write_samples(text, &rows.samples);
    }
}

/// One line about a column, shared with the Markdown renderer.
pub(super) fn describe_column(column: &ColumnResult) -> String {
    match column {
        ColumnResult::Compared {
            column,
            mismatches,
            tolerance: None,
            ..
        } => format!("`{column}`: {mismatches} mismatch(es)"),
        ColumnResult::Compared {
            column,
            mismatches,
            tolerance: Some(tolerance),
            within_tolerance,
        } => format!(
            "`{column}`: {mismatches} mismatch(es), \
             {within_tolerance} more within the tolerance of {tolerance}"
        ),
        ColumnResult::Inconclusive { column, reason } => {
            format!("`{column}`: not compared, {reason}")
        }
    }
}

fn write_samples(text: &mut String, samples: &Samples) {
    let _ = writeln!(text, "Examples ({}):", samples.describe_limits());

    let key_lists = [
        ("only on the left", &samples.left_only_keys),
        ("only on the right", &samples.right_only_keys),
        ("duplicated on the left", &samples.duplicate_keys_left),
        ("duplicated on the right", &samples.duplicate_keys_right),
    ];
    for (label, keys) in key_lists {
        for key in keys {
            let _ = writeln!(text, "  {label}: {}", key.label());
        }
    }
    for row in &samples.changed_rows {
        let _ = writeln!(text, "  changed: {}", row.label());
    }
}

fn write_fingerprints(text: &mut String, fingerprints: &FingerprintSection) {
    let _ = writeln!(
        text,
        "Fingerprints: {} ({}, canonical version {})",
        fingerprints.outcome(),
        fingerprints.algorithm,
        fingerprints.canonical_version
    );
    let _ = writeln!(
        text,
        "  A screening signal over the key and compared columns, not a verdict."
    );
    let _ = writeln!(text, "  left content   {}", fingerprints.left.content);
    let _ = writeln!(text, "  right content  {}", fingerprints.right.content);
}
