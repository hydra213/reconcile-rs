//! Samples: a bounded number of example keys for each kind of difference.
//!
//! Counts say how much differs; samples say where to start looking. Because
//! keys and cell values are real data, they are hidden unless the
//! configuration opts in with `[report] show_values = true`. A hidden
//! example is still identified by the fingerprint of its key, which is
//! stable across runs and can be matched against a report that shows values.

use serde::Serialize;

use crate::config::ReportConfig;
use crate::fingerprint::Fingerprint;
use crate::rowdiff::{ChangedRow, Key, RowDiff};
use crate::value::Value;

/// Example keys, taken from the start of each sorted list of differences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Samples {
    /// The largest number of examples listed per kind of difference.
    pub limit: u64,
    /// Whether real keys and cell values are included.
    pub values_shown: bool,
    pub left_only_keys: Vec<KeySample>,
    pub right_only_keys: Vec<KeySample>,
    pub duplicate_keys_left: Vec<KeySample>,
    pub duplicate_keys_right: Vec<KeySample>,
    pub changed_rows: Vec<ChangedRowSample>,
}

/// One example key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KeySample {
    /// The fingerprint of the key's column names and values.
    pub key_fingerprint: Fingerprint,
    /// The key's values, one per key column. Present only when values are
    /// shown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<Vec<String>>,
}

/// One example of a matched row with changed values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChangedRowSample {
    #[serde(flatten)]
    pub key: KeySample,
    pub changes: Vec<ChangeSample>,
}

/// One changed cell. The values are present only when values are shown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChangeSample {
    pub column: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub left: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub right: Option<String>,
}

impl Samples {
    /// Takes up to `config.sample_limit` examples of each kind from `diff`.
    pub fn new(diff: &RowDiff, key_columns: &[String], config: &ReportConfig) -> Self {
        // A limit larger than `usize` cannot be reached anyway.
        let limit = usize::try_from(config.sample_limit).unwrap_or(usize::MAX);
        let show_values = config.show_values;

        let key_samples = |keys: &[Key]| -> Vec<KeySample> {
            keys.iter()
                .take(limit)
                .map(|key| KeySample::new(key, key_columns, show_values))
                .collect()
        };

        Self {
            limit: config.sample_limit,
            values_shown: show_values,
            left_only_keys: key_samples(&diff.left_only_keys),
            right_only_keys: key_samples(&diff.right_only_keys),
            duplicate_keys_left: key_samples(&diff.duplicate_keys_left),
            duplicate_keys_right: key_samples(&diff.duplicate_keys_right),
            changed_rows: diff
                .changed_rows
                .iter()
                .take(limit)
                .map(|row| ChangedRowSample::new(row, key_columns, show_values))
                .collect(),
        }
    }
}

impl Samples {
    /// Whether there are no examples at all.
    pub fn is_empty(&self) -> bool {
        self.left_only_keys.is_empty()
            && self.right_only_keys.is_empty()
            && self.duplicate_keys_left.is_empty()
            && self.duplicate_keys_right.is_empty()
            && self.changed_rows.is_empty()
    }

    /// A short phrase about the limit and whether values are hidden.
    pub fn describe_limits(&self) -> String {
        if self.values_shown {
            format!("at most {} of each kind", self.limit)
        } else {
            format!(
                "at most {} of each kind; values are hidden, \
                 set `[report] show_values = true` to see them",
                self.limit
            )
        }
    }
}

impl KeySample {
    /// How the key is written in text and Markdown reports: its values when
    /// they are shown, otherwise the start of its fingerprint.
    pub fn label(&self) -> String {
        match &self.key {
            Some(values) => format!("key ({})", values.join(", ")),
            None => format!("key fingerprint {}", &self.key_fingerprint.to_hex()[..16]),
        }
    }

    fn new(key: &Key, key_columns: &[String], show_values: bool) -> Self {
        let named_values: Vec<(&str, &Value)> =
            key_columns.iter().map(String::as_str).zip(key).collect();

        Self {
            key_fingerprint: Fingerprint::of_row(&named_values),
            key: show_values.then(|| key.iter().map(Value::to_string).collect()),
        }
    }
}

impl ChangedRowSample {
    /// The key followed by the changed columns, with both values when they
    /// are shown.
    pub fn label(&self) -> String {
        let changes: Vec<String> = self
            .changes
            .iter()
            .map(|change| match (&change.left, &change.right) {
                (Some(left), Some(right)) => format!("`{}` {left} -> {right}", change.column),
                _ => format!("`{}`", change.column),
            })
            .collect();
        format!("{}: {}", self.key.label(), changes.join(", "))
    }

    fn new(row: &ChangedRow, key_columns: &[String], show_values: bool) -> Self {
        let changes = row
            .changes
            .iter()
            .map(|change| ChangeSample {
                column: change.column.clone(),
                left: show_values.then(|| change.left.to_string()),
                right: show_values.then(|| change.right.to_string()),
            })
            .collect();

        Self {
            key: KeySample::new(&row.key, key_columns, show_values),
            changes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rowdiff::diff_rows;
    use crate::table::Table;
    use std::collections::BTreeMap;

    fn int(number: i128) -> Value {
        Value::Integer(number)
    }

    fn text(text: &str) -> Value {
        Value::Utf8(text.to_string())
    }

    /// Left has ids 1 to 5. Right drops ids 1 to 3 and changes id 4's name.
    fn diff_with_three_missing_and_one_changed() -> RowDiff {
        let names = ["a", "b", "c", "d", "e"];
        let left = Table::from_columns(vec![
            ("id", (1..=5).map(int).collect()),
            ("name", names.iter().map(|name| text(name)).collect()),
        ]);
        let right = Table::from_columns(vec![
            ("id", vec![int(4), int(5)]),
            ("name", vec![text("D"), text("e")]),
        ]);
        diff_rows(
            &left,
            &right,
            &["id".to_string()],
            &["name".to_string()],
            &BTreeMap::new(),
        )
    }

    fn samples(sample_limit: u64, show_values: bool) -> Samples {
        Samples::new(
            &diff_with_three_missing_and_one_changed(),
            &["id".to_string()],
            &ReportConfig {
                sample_limit,
                show_values,
            },
        )
    }

    #[test]
    fn values_are_hidden_by_default() {
        let samples = samples(10, false);

        assert_eq!(samples.left_only_keys.len(), 3);
        assert!(samples.left_only_keys.iter().all(|key| key.key.is_none()));
        let change = &samples.changed_rows[0].changes[0];
        assert_eq!(change.column, "name");
        assert_eq!((&change.left, &change.right), (&None, &None));

        let json = serde_json::to_string(&samples).unwrap();
        assert!(!json.contains("\"key\":"));
        assert!(!json.contains("\"left\":"));
    }

    #[test]
    fn values_are_shown_only_when_asked_for() {
        let samples = samples(10, true);

        assert_eq!(samples.left_only_keys[0].key, Some(vec!["1".to_string()]));
        let change = &samples.changed_rows[0].changes[0];
        assert_eq!(change.left.as_deref(), Some("\"d\""));
        assert_eq!(change.right.as_deref(), Some("\"D\""));
    }

    #[test]
    fn the_key_fingerprint_is_the_same_whether_values_are_shown_or_not() {
        assert_eq!(
            samples(10, false).left_only_keys[0].key_fingerprint,
            samples(10, true).left_only_keys[0].key_fingerprint
        );
    }

    #[test]
    fn the_limit_caps_each_list_and_keeps_the_smallest_keys() {
        let samples = samples(2, true);

        assert_eq!(samples.left_only_keys.len(), 2);
        assert_eq!(samples.left_only_keys[0].key, Some(vec!["1".to_string()]));
        assert_eq!(samples.left_only_keys[1].key, Some(vec!["2".to_string()]));
    }

    #[test]
    fn a_limit_of_zero_lists_nothing() {
        let samples = samples(0, true);

        assert!(samples.left_only_keys.is_empty());
        assert!(samples.changed_rows.is_empty());
    }
}
