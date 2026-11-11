//! A dataset side held in memory as columns of logical values.

use std::collections::BTreeMap;

use arrow::record_batch::RecordBatch;

use crate::value::{Value, column_values};

/// The columns needed for a comparison, converted to [`Value`]s and stored
/// under their logical names.
///
/// Every column has exactly `row_count` values; row `i` of the table is the
/// `i`-th value of each column.
#[derive(Debug, Clone)]
pub struct Table {
    row_count: usize,
    columns: BTreeMap<String, Vec<Value>>,
}

/// Which column to load from a file and what to call it in the table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnToLoad {
    /// The column's name in the Parquet file.
    pub physical_name: String,
    /// The name used for it in the comparison.
    pub logical_name: String,
}

impl Table {
    /// Converts the requested columns of `batches` into a table.
    ///
    /// # Panics
    ///
    /// Panics if a requested column is missing or has an unsupported type.
    /// The comparison plan rules both out before any data is loaded, so a
    /// panic here is a bug in this crate, not a problem with the data.
    pub fn load(batches: &[RecordBatch], columns_to_load: &[ColumnToLoad]) -> Self {
        let row_count = batches.iter().map(RecordBatch::num_rows).sum();

        let mut columns = BTreeMap::new();
        for column in columns_to_load {
            let mut values = Vec::with_capacity(row_count);
            for batch in batches {
                let array = batch
                    .column_by_name(&column.physical_name)
                    .expect("the plan only loads columns that exist");
                let batch_values = column_values(array.as_ref())
                    .expect("the plan only loads columns with a supported type");
                values.extend(batch_values);
            }
            columns.insert(column.logical_name.clone(), values);
        }

        Self { row_count, columns }
    }

    /// Builds a table directly from values. Mainly useful in tests.
    ///
    /// # Panics
    ///
    /// Panics if the columns do not all have the same length.
    pub fn from_columns(columns: Vec<(&str, Vec<Value>)>) -> Self {
        let row_count = columns.first().map_or(0, |(_, values)| values.len());
        assert!(
            columns.iter().all(|(_, values)| values.len() == row_count),
            "every column of a table must have the same number of rows"
        );

        let columns = columns
            .into_iter()
            .map(|(name, values)| (name.to_string(), values))
            .collect();
        Self { row_count, columns }
    }

    pub fn row_count(&self) -> usize {
        self.row_count
    }

    /// All values of one column, in row order.
    ///
    /// # Panics
    ///
    /// Panics if the table has no column with that name.
    pub fn column(&self, name: &str) -> &[Value] {
        self.columns
            .get(name)
            .unwrap_or_else(|| panic!("the table has no column named `{name}`"))
    }
}
