//! Property tests: rules that must hold for *any* data, checked on many
//! randomly generated tables.
//!
//! Each property is tried on a few hundred small tables. Ids are drawn from
//! a small range so that duplicates, missing keys and matches all occur.

use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;
use reconcilers::fingerprint::Fingerprint;
use reconcilers::rowdiff::{RowDiff, diff_rows};
use reconcilers::table::Table;
use reconcilers::value::Value;

/// A row of the generated table: an `id` and a `name`.
type Row = (i64, String);

/// Up to 30 rows with ids from 0 to 14 and short names.
fn rows() -> impl Strategy<Value = Vec<Row>> {
    prop::collection::vec((0..15_i64, "[ab]{0,2}"), 0..30)
}

/// The same rows in a random order.
fn shuffled(rows: Vec<Row>) -> impl Strategy<Value = Vec<Row>> {
    Just(rows).prop_shuffle()
}

fn table(rows: &[Row]) -> Table {
    Table::from_columns(vec![
        (
            "id",
            rows.iter()
                .map(|(id, _)| Value::Integer((*id).into()))
                .collect(),
        ),
        (
            "name",
            rows.iter()
                .map(|(_, name)| Value::Utf8(name.clone()))
                .collect(),
        ),
    ])
}

fn diff(left: &[Row], right: &[Row]) -> RowDiff {
    diff_rows(
        &table(left),
        &table(right),
        &["id".to_string()],
        &["name".to_string()],
        &BTreeMap::new(),
    )
}

fn distinct_ids(rows: &[Row]) -> usize {
    rows.iter().map(|(id, _)| id).collect::<BTreeSet<_>>().len()
}

proptest! {
    /// A table compared with a reordering of itself has no differences.
    #[test]
    fn a_table_equals_any_reordering_of_itself(
        (original, reordered) in rows().prop_flat_map(|rows| (Just(rows.clone()), shuffled(rows)))
    ) {
        let result = diff(&original, &reordered);

        prop_assert!(result.left_only_keys.is_empty());
        prop_assert!(result.right_only_keys.is_empty());
        prop_assert!(result.changed_rows.is_empty());
    }

    /// Reordering the rows of either side never changes the result.
    #[test]
    fn row_order_never_changes_the_result(
        (left, left_reordered) in rows().prop_flat_map(|rows| (Just(rows.clone()), shuffled(rows))),
        (right, right_reordered) in rows().prop_flat_map(|rows| (Just(rows.clone()), shuffled(rows))),
    ) {
        prop_assert_eq!(diff(&left, &right), diff(&left_reordered, &right_reordered));
    }

    /// Swapping the two sides mirrors the result.
    #[test]
    fn swapping_sides_mirrors_the_result(left in rows(), right in rows()) {
        let forward = diff(&left, &right);
        let backward = diff(&right, &left);

        prop_assert_eq!(&forward.left_only_keys, &backward.right_only_keys);
        prop_assert_eq!(&forward.right_only_keys, &backward.left_only_keys);
        prop_assert_eq!(&forward.duplicate_keys_left, &backward.duplicate_keys_right);
        prop_assert_eq!(forward.matched_keys, backward.matched_keys);
        prop_assert_eq!(forward.changed_rows.len(), backward.changed_rows.len());
        prop_assert_eq!(forward.unchanged_rows, backward.unchanged_rows);
    }

    /// Every distinct key is either matched or only on its own side, and
    /// every compared row is either changed or unchanged.
    #[test]
    fn the_counts_always_add_up(left in rows(), right in rows()) {
        let counts = diff(&left, &right).counts();

        prop_assert_eq!(
            counts.left_only_keys + counts.matched_keys,
            distinct_ids(&left) as u64
        );
        prop_assert_eq!(
            counts.right_only_keys + counts.matched_keys,
            distinct_ids(&right) as u64
        );
        prop_assert_eq!(counts.changed_rows + counts.unchanged_rows, counts.compared_rows);
        prop_assert!(counts.compared_rows <= counts.matched_keys);
    }

    /// The content fingerprint ignores row order.
    #[test]
    fn the_content_fingerprint_ignores_row_order(
        (original, reordered) in rows().prop_flat_map(|rows| (Just(rows.clone()), shuffled(rows)))
    ) {
        prop_assert_eq!(
            Fingerprint::of_table(&table(&original)),
            Fingerprint::of_table(&table(&reordered))
        );
    }

    /// Adding a row always changes the content fingerprint, even when the
    /// new row is an exact copy of an existing one.
    #[test]
    fn adding_a_row_always_changes_the_content_fingerprint(
        original in rows(),
        extra in (0..15_i64, "[ab]{0,2}"),
    ) {
        let mut extended = original.clone();
        extended.push(extra);

        prop_assert_ne!(
            Fingerprint::of_table(&table(&original)),
            Fingerprint::of_table(&table(&extended))
        );
    }
}
