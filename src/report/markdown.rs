//! The Markdown rendering of a report, for sharing and for pull requests.

use std::fmt::Write as _;

use super::samples::Samples;
use super::text::describe_column;
use super::{FingerprintSection, Report, RowSection, SchemaSection};

/// Renders the whole report. Called through [`Report::to_markdown`].
///
/// Writing to a `String` cannot fail, so the results of `writeln!` are
/// ignored throughout this file.
pub fn render(report: &Report) -> String {
    let mut md = String::new();
    let _ = writeln!(md, "# Reconciliation report");
    let _ = writeln!(md);
    let _ = writeln!(md, "**Verdict: {}**", report.verdict);
    if let Some(schema) = &report.schema {
        let _ = writeln!(md);
        write_schema(&mut md, schema);
    }
    if let Some(rows) = &report.rows {
        let _ = writeln!(md);
        write_rows(&mut md, rows);
    }
    if let Some(fingerprints) = &report.fingerprints {
        let _ = writeln!(md);
        write_fingerprints(&mut md, fingerprints);
    }
    md
}

fn write_schema(md: &mut String, schema: &SchemaSection) {
    let _ = writeln!(md, "## Schema: {}", schema.verdict);
    let _ = writeln!(md);
    let _ = writeln!(
        md,
        "Policy `{}`. {} left columns, {} right columns.",
        schema.policy,
        schema.left_columns.len(),
        schema.right_columns.len()
    );
    let _ = writeln!(md);

    if schema.findings.is_empty() {
        let _ = writeln!(md, "No schema differences.");
        return;
    }
    let _ = writeln!(md, "| Verdict | Difference | Explanation |");
    let _ = writeln!(md, "|---|---|---|");
    for finding in &schema.findings {
        let _ = writeln!(
            md,
            "| {} | {} | {} |",
            finding.verdict,
            cell(&finding.difference.to_string()),
            cell(finding.explanation)
        );
    }
}

fn write_rows(md: &mut String, rows: &RowSection) {
    let counts = &rows.counts;
    let _ = writeln!(md, "## Rows: {}", rows.verdict);
    let _ = writeln!(md);
    let _ = writeln!(md, "Keys: `{}`.", rows.keys.join("`, `"));
    let _ = writeln!(md);

    let _ = writeln!(md, "| Count | Left | Right |");
    let _ = writeln!(md, "|---|---:|---:|");
    let side_counts = [
        ("Rows", counts.left_rows, counts.right_rows),
        (
            "Rows with a NULL key",
            counts.null_key_rows_left,
            counts.null_key_rows_right,
        ),
        (
            "Duplicated keys",
            counts.duplicate_keys_left,
            counts.duplicate_keys_right,
        ),
        (
            "Keys only on this side",
            counts.left_only_keys,
            counts.right_only_keys,
        ),
    ];
    for (label, left, right) in side_counts {
        let _ = writeln!(md, "| {label} | {left} | {right} |");
    }
    let _ = writeln!(md);
    let _ = writeln!(
        md,
        "{} matched keys, {} rows compared: {} unchanged, {} changed.",
        counts.matched_keys, counts.compared_rows, counts.unchanged_rows, counts.changed_rows
    );
    let _ = writeln!(md);

    let _ = writeln!(md, "### Checks");
    let _ = writeln!(md);
    let _ = writeln!(md, "| Verdict | Explanation |");
    let _ = writeln!(md, "|---|---|");
    for finding in &rows.findings {
        let _ = writeln!(
            md,
            "| {} | {} |",
            finding.verdict,
            cell(&finding.explanation)
        );
    }
    let _ = writeln!(md);

    let _ = writeln!(md, "### Columns");
    let _ = writeln!(md);
    if rows.columns.is_empty() {
        let _ = writeln!(md, "No columns besides the keys exist on both sides.");
    }
    for column in &rows.columns {
        let _ = writeln!(md, "- {}", describe_column(column));
    }

    if !rows.samples.is_empty() {
        let _ = writeln!(md);
        write_samples(md, &rows.samples);
    }
}

fn write_samples(md: &mut String, samples: &Samples) {
    let _ = writeln!(md, "### Examples");
    let _ = writeln!(md);
    let _ = writeln!(md, "{}.", capitalised(&samples.describe_limits()));
    let _ = writeln!(md);
    let _ = writeln!(md, "| Difference | Example |");
    let _ = writeln!(md, "|---|---|");

    let key_lists = [
        ("Only on the left", &samples.left_only_keys),
        ("Only on the right", &samples.right_only_keys),
        ("Duplicated on the left", &samples.duplicate_keys_left),
        ("Duplicated on the right", &samples.duplicate_keys_right),
    ];
    for (label, keys) in key_lists {
        for key in keys {
            let _ = writeln!(md, "| {label} | {} |", cell(&key.label()));
        }
    }
    for row in &samples.changed_rows {
        let _ = writeln!(md, "| Changed | {} |", cell(&row.label()));
    }
}

fn write_fingerprints(md: &mut String, fingerprints: &FingerprintSection) {
    let _ = writeln!(md, "## Fingerprints: {}", fingerprints.outcome());
    let _ = writeln!(md);
    let _ = writeln!(
        md,
        "Algorithm `{}`, canonical version {}. A screening signal over the key \
         and compared columns, not a verdict: values inside a tolerance still \
         have different fingerprints.",
        fingerprints.algorithm, fingerprints.canonical_version
    );
    let _ = writeln!(md);
    let _ = writeln!(md, "| | Left | Right |");
    let _ = writeln!(md, "|---|---|---|");
    let _ = writeln!(
        md,
        "| Content | `{}` | `{}` |",
        fingerprints.left.content, fingerprints.right.content
    );
    let _ = writeln!(
        md,
        "| Schema | `{}` | `{}` |",
        fingerprints.left.schema, fingerprints.right.schema
    );
    let _ = writeln!(md);
    let _ = writeln!(md, "Configuration: `{}`", fingerprints.configuration);
}

/// Makes text safe inside a Markdown table cell: a `|` would end the cell
/// and a line break would end the row.
fn cell(text: &str) -> String {
    text.replace('|', "\\|").replace(['\r', '\n'], " ")
}

fn capitalised(text: &str) -> String {
    let mut characters = text.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().chain(characters).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_cells_cannot_be_broken_by_data() {
        assert_eq!(cell("a|b"), "a\\|b");
        assert_eq!(cell("line one\nline two"), "line one line two");
    }
}
