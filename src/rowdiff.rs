//! Keyed row comparison.
//!
//! Rows on the left and right are paired by key, then the paired rows are
//! compared column by column. This module only produces *facts*: which keys
//! are missing, extra or duplicated, and which values changed. Whether those
//! facts are acceptable is decided in [`crate::policy`].
//!
//! The result does not depend on the order of rows in either table.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::table::Table;
use crate::tolerance::Tolerance;
use crate::value::Value;

/// The key of a row: one value per key column, in the configured order.
pub type Key = Vec<Value>;

/// Everything the row comparison observed. Lists are sorted by key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowDiff {
    pub left_rows: u64,
    pub right_rows: u64,
    /// Rows with NULL in any key column. They have no usable identity and
    /// take no further part in the comparison.
    pub null_key_rows_left: u64,
    pub null_key_rows_right: u64,
    /// Keys that appear on more than one row of that side.
    pub duplicate_keys_left: Vec<Key>,
    pub duplicate_keys_right: Vec<Key>,
    /// Keys present on the left and absent on the right.
    pub left_only_keys: Vec<Key>,
    /// Keys present on the right and absent on the left.
    pub right_only_keys: Vec<Key>,
    /// Keys present on both sides, including duplicated ones.
    pub matched_keys: u64,
    /// Compared rows where every compared column is equal.
    pub unchanged_rows: u64,
    /// Compared rows where at least one compared column differs.
    pub changed_rows: Vec<ChangedRow>,
    /// For every compared column, the number of rows where it differs.
    /// Columns with no differences are present with a count of zero.
    pub column_mismatches: BTreeMap<String, u64>,
    /// For every compared column, the number of rows where the two values
    /// are not equal but are within the column's tolerance. These do not
    /// count as mismatches and do not make a row changed.
    pub column_within_tolerance: BTreeMap<String, u64>,
}

/// A matched row whose values differ.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedRow {
    pub key: Key,
    /// The differing columns, sorted by column name.
    pub changes: Vec<ValueChange>,
}

/// One differing cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueChange {
    pub column: String,
    pub left: Value,
    pub right: Value,
}

/// The numbers of a [`RowDiff`], without the lists of keys.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RowCounts {
    pub left_rows: u64,
    pub right_rows: u64,
    pub null_key_rows_left: u64,
    pub null_key_rows_right: u64,
    pub duplicate_keys_left: u64,
    pub duplicate_keys_right: u64,
    pub left_only_keys: u64,
    pub right_only_keys: u64,
    pub matched_keys: u64,
    /// Rows that were compared value by value: matched keys that are unique
    /// on both sides. Always `changed_rows + unchanged_rows`.
    pub compared_rows: u64,
    pub changed_rows: u64,
    pub unchanged_rows: u64,
}

impl RowDiff {
    pub fn counts(&self) -> RowCounts {
        let changed_rows = self.changed_rows.len() as u64;
        RowCounts {
            left_rows: self.left_rows,
            right_rows: self.right_rows,
            null_key_rows_left: self.null_key_rows_left,
            null_key_rows_right: self.null_key_rows_right,
            duplicate_keys_left: self.duplicate_keys_left.len() as u64,
            duplicate_keys_right: self.duplicate_keys_right.len() as u64,
            left_only_keys: self.left_only_keys.len() as u64,
            right_only_keys: self.right_only_keys.len() as u64,
            matched_keys: self.matched_keys,
            compared_rows: changed_rows + self.unchanged_rows,
            changed_rows,
            unchanged_rows: self.unchanged_rows,
        }
    }
}

/// Compares two tables row by row.
///
/// `key_columns` identify a row. `compared_columns` are the other columns to
/// check on rows that match. Both tables must contain all of these columns.
///
/// `tolerances` holds the allowed numeric difference for the columns that
/// have one; all other columns must match exactly.
///
/// A key that is duplicated on either side is reported and then left out of
/// the value comparison: with two candidate rows there is no way to know
/// which one the other side should be compared with.
pub fn diff_rows(
    left: &Table,
    right: &Table,
    key_columns: &[String],
    compared_columns: &[String],
    tolerances: &BTreeMap<String, Tolerance>,
) -> RowDiff {
    let left_index = KeyIndex::build(left, key_columns);
    let right_index = KeyIndex::build(right, key_columns);

    let mut diff = RowDiff {
        left_rows: left.row_count() as u64,
        right_rows: right.row_count() as u64,
        null_key_rows_left: left_index.null_key_rows,
        null_key_rows_right: right_index.null_key_rows,
        duplicate_keys_left: left_index.duplicate_keys(),
        duplicate_keys_right: right_index.duplicate_keys(),
        left_only_keys: Vec::new(),
        right_only_keys: Vec::new(),
        matched_keys: 0,
        unchanged_rows: 0,
        changed_rows: Vec::new(),
        column_mismatches: zero_for_each(compared_columns),
        column_within_tolerance: zero_for_each(compared_columns),
    };

    // BTreeMap iterates in key order, so every list below ends up sorted.
    for (key, left_row_numbers) in &left_index.rows_by_key {
        let Some(right_row_numbers) = right_index.rows_by_key.get(key) else {
            diff.left_only_keys.push(key.clone());
            continue;
        };
        diff.matched_keys += 1;

        // Only a one-to-one match can be compared value by value.
        let (&[left_row], &[right_row]) = (&left_row_numbers[..], &right_row_numbers[..]) else {
            continue;
        };

        let mut changes = Vec::new();
        for column in compared_columns {
            let left_value = &left.column(column)[left_row];
            let right_value = &right.column(column)[right_row];
            if left_value == right_value {
                continue;
            }

            let tolerated = tolerances
                .get(column)
                .is_some_and(|tolerance| tolerance.allows(left_value, right_value));
            if tolerated {
                add_one(&mut diff.column_within_tolerance, column);
            } else {
                add_one(&mut diff.column_mismatches, column);
                changes.push(ValueChange {
                    column: column.clone(),
                    left: left_value.clone(),
                    right: right_value.clone(),
                });
            }
        }

        if changes.is_empty() {
            diff.unchanged_rows += 1;
        } else {
            diff.changed_rows.push(ChangedRow {
                key: key.clone(),
                changes,
            });
        }
    }

    for key in right_index.rows_by_key.keys() {
        if !left_index.rows_by_key.contains_key(key) {
            diff.right_only_keys.push(key.clone());
        }
    }

    diff
}

/// Where each key can be found in a table.
struct KeyIndex {
    /// For every key without NULLs, the numbers of the rows that carry it.
    rows_by_key: BTreeMap<Key, Vec<usize>>,
    null_key_rows: u64,
}

impl KeyIndex {
    fn build(table: &Table, key_columns: &[String]) -> Self {
        let key_values: Vec<&[Value]> = key_columns
            .iter()
            .map(|column| table.column(column))
            .collect();

        let mut rows_by_key: BTreeMap<Key, Vec<usize>> = BTreeMap::new();
        let mut null_key_rows = 0;

        for row in 0..table.row_count() {
            let key: Key = key_values
                .iter()
                .map(|values| values[row].clone())
                .collect();

            if key.iter().any(Value::is_null) {
                null_key_rows += 1;
            } else {
                rows_by_key.entry(key).or_default().push(row);
            }
        }

        Self {
            rows_by_key,
            null_key_rows,
        }
    }

    /// The keys that appear on more than one row, sorted.
    fn duplicate_keys(&self) -> Vec<Key> {
        self.rows_by_key
            .iter()
            .filter(|(_, row_numbers)| row_numbers.len() > 1)
            .map(|(key, _)| key.clone())
            .collect()
    }
}

/// A counter per column, all starting at zero.
fn zero_for_each(columns: &[String]) -> BTreeMap<String, u64> {
    columns.iter().map(|column| (column.clone(), 0)).collect()
}

fn add_one(counters: &mut BTreeMap<String, u64>, column: &str) {
    *counters
        .get_mut(column)
        .expect("every compared column has a counter") += 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int(number: i128) -> Value {
        Value::Integer(number)
    }

    fn text(text: &str) -> Value {
        Value::Utf8(text.to_string())
    }

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    /// A two-column table: an integer `id` and a string `name`.
    fn table(rows: &[(Value, Value)]) -> Table {
        let ids = rows.iter().map(|(id, _)| id.clone()).collect();
        let row_names = rows.iter().map(|(_, name)| name.clone()).collect();
        Table::from_columns(vec![("id", ids), ("name", row_names)])
    }

    fn diff(left: &Table, right: &Table) -> RowDiff {
        diff_rows(
            left,
            right,
            &names(&["id"]),
            &names(&["name"]),
            &BTreeMap::new(),
        )
    }

    #[test]
    fn identical_tables_have_only_unchanged_rows() {
        let left = table(&[(int(1), text("a")), (int(2), text("b"))]);
        let result = diff(&left, &left.clone());

        assert_eq!(result.matched_keys, 2);
        assert_eq!(result.unchanged_rows, 2);
        assert!(result.changed_rows.is_empty());
        assert!(result.left_only_keys.is_empty());
        assert!(result.right_only_keys.is_empty());
    }

    #[test]
    fn missing_and_extra_keys_are_listed() {
        let left = table(&[(int(1), text("a")), (int(2), text("b"))]);
        let right = table(&[(int(2), text("b")), (int(3), text("c"))]);
        let result = diff(&left, &right);

        assert_eq!(result.left_only_keys, vec![vec![int(1)]]);
        assert_eq!(result.right_only_keys, vec![vec![int(3)]]);
        assert_eq!(result.matched_keys, 1);
    }

    #[test]
    fn a_changed_value_is_reported_with_both_sides() {
        let left = table(&[(int(1), text("a")), (int(2), text("b"))]);
        let right = table(&[(int(1), text("a")), (int(2), text("B"))]);
        let result = diff(&left, &right);

        assert_eq!(result.unchanged_rows, 1);
        assert_eq!(
            result.changed_rows,
            vec![ChangedRow {
                key: vec![int(2)],
                changes: vec![ValueChange {
                    column: "name".to_string(),
                    left: text("b"),
                    right: text("B"),
                }],
            }]
        );
        assert_eq!(result.column_mismatches["name"], 1);
    }

    #[test]
    fn row_order_does_not_change_the_result() {
        let left = table(&[
            (int(1), text("a")),
            (int(2), text("b")),
            (int(3), text("c")),
        ]);
        let right = table(&[
            (int(1), text("a")),
            (int(2), text("x")),
            (int(4), text("d")),
        ]);
        let left_reversed = table(&[
            (int(3), text("c")),
            (int(2), text("b")),
            (int(1), text("a")),
        ]);
        let right_reversed = table(&[
            (int(4), text("d")),
            (int(2), text("x")),
            (int(1), text("a")),
        ]);

        assert_eq!(diff(&left, &right), diff(&left_reversed, &right_reversed));
    }

    #[test]
    fn a_duplicated_key_is_reported_and_not_compared() {
        let left = table(&[
            (int(1), text("a")),
            (int(1), text("b")),
            (int(2), text("c")),
        ]);
        let right = table(&[(int(1), text("a")), (int(2), text("c"))]);
        let result = diff(&left, &right);

        assert_eq!(result.duplicate_keys_left, vec![vec![int(1)]]);
        assert!(result.duplicate_keys_right.is_empty());
        assert_eq!(result.matched_keys, 2);
        // Only key 2 is compared; key 1 has no one-to-one partner.
        assert_eq!(result.counts().compared_rows, 1);
        assert!(result.changed_rows.is_empty());
    }

    #[test]
    fn rows_with_a_null_key_are_counted_and_set_aside() {
        let left = table(&[(Value::Null, text("a")), (int(1), text("b"))]);
        let right = table(&[
            (Value::Null, text("a")),
            (Value::Null, text("z")),
            (int(1), text("b")),
        ]);
        let result = diff(&left, &right);

        assert_eq!(result.null_key_rows_left, 1);
        assert_eq!(result.null_key_rows_right, 2);
        assert_eq!(result.matched_keys, 1);
        assert!(result.duplicate_keys_right.is_empty());
    }

    #[test]
    fn null_and_empty_string_are_different_values() {
        let left = table(&[(int(1), Value::Null)]);
        let right = table(&[(int(1), text(""))]);
        assert_eq!(diff(&left, &right).changed_rows.len(), 1);
    }

    #[test]
    fn composite_keys_match_on_every_part() {
        let build = |rows: &[(i128, &str, i128)]| {
            Table::from_columns(vec![
                ("id", rows.iter().map(|row| int(row.0)).collect()),
                ("day", rows.iter().map(|row| text(row.1)).collect()),
                ("qty", rows.iter().map(|row| int(row.2)).collect()),
            ])
        };
        let left = build(&[(1, "mon", 10), (1, "tue", 20)]);
        let right = build(&[(1, "mon", 10), (1, "wed", 20)]);

        let result = diff_rows(
            &left,
            &right,
            &names(&["id", "day"]),
            &names(&["qty"]),
            &BTreeMap::new(),
        );

        assert_eq!(result.matched_keys, 1);
        assert_eq!(result.left_only_keys, vec![vec![int(1), text("tue")]]);
        assert_eq!(result.right_only_keys, vec![vec![int(1), text("wed")]]);
    }

    #[test]
    fn a_difference_within_tolerance_is_counted_but_is_not_a_change() {
        let amounts = |values: &[i128]| {
            Table::from_columns(vec![
                ("id", (1..=values.len() as i128).map(int).collect()),
                ("qty", values.iter().map(|value| int(*value)).collect()),
            ])
        };
        let left = amounts(&[100, 200, 300]);
        let right = amounts(&[100, 201, 305]);
        let tolerances = BTreeMap::from([("qty".to_string(), Tolerance::parse("1").unwrap())]);

        let result = diff_rows(
            &left,
            &right,
            &names(&["id"]),
            &names(&["qty"]),
            &tolerances,
        );

        // Row 2 differs by 1 (allowed), row 3 by 5 (not allowed).
        assert_eq!(result.column_within_tolerance["qty"], 1);
        assert_eq!(result.column_mismatches["qty"], 1);
        assert_eq!(result.unchanged_rows, 2);
        assert_eq!(result.changed_rows.len(), 1);
        assert_eq!(result.changed_rows[0].key, vec![int(3)]);
    }

    #[test]
    fn counts_add_up() {
        let left = table(&[
            (int(1), text("a")),
            (int(2), text("b")),
            (int(3), text("c")),
        ]);
        let right = table(&[
            (int(1), text("a")),
            (int(2), text("x")),
            (int(4), text("d")),
        ]);
        let counts = diff(&left, &right).counts();

        assert_eq!(counts.left_rows, 3);
        assert_eq!(counts.right_rows, 3);
        assert_eq!(counts.matched_keys, 2);
        assert_eq!(counts.compared_rows, 2);
        assert_eq!(counts.changed_rows, 1);
        assert_eq!(counts.unchanged_rows, 1);
        assert_eq!(counts.left_only_keys, 1);
        assert_eq!(counts.right_only_keys, 1);
    }
}
