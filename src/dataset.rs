//! A dataset held in memory, wherever it came from.
//!
//! Everything after this module works on a [`Dataset`] and does not care
//! whether the rows were read from Parquet files or handed over as Arrow
//! data by another program, for example a dataframe library.

use arrow::datatypes::Schema;
use arrow::record_batch::RecordBatch;

use crate::error::Error;
use crate::fingerprint::Fingerprint;
use crate::schema::LogicalSchema;

/// One side of a comparison, loaded into memory.
#[derive(Debug, Clone)]
pub struct Dataset {
    pub schema: LogicalSchema,
    /// The rows, in the chunks ("record batches") the source produced.
    pub batches: Vec<RecordBatch>,
    /// The files the dataset was read from, in the order they were named.
    /// Empty when the dataset did not come from files.
    pub files: Vec<InputFile>,
}

/// One file of a dataset, described by the bytes that were actually read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputFile {
    /// Where the file was read from: a local path or a URL.
    pub location: String,
    pub size_bytes: u64,
    /// The fingerprint of the file's bytes.
    pub fingerprint: Fingerprint,
}

impl Dataset {
    /// Builds a dataset from Arrow data that is already in memory.
    ///
    /// `schema` describes the columns even when there are no batches, so an
    /// empty table still has a schema. Fails if a column name repeats or if
    /// a batch does not have the columns that `schema` describes.
    pub fn from_batches(schema: &Schema, batches: Vec<RecordBatch>) -> Result<Self, Error> {
        let logical_schema = LogicalSchema::from_arrow(schema).map_err(|duplicate| {
            Error::DuplicateColumnInTable {
                column: duplicate.column,
            }
        })?;

        for (index, batch) in batches.iter().enumerate() {
            let batch_schema = LogicalSchema::from_arrow(&batch.schema()).ok();
            if batch_schema.as_ref() != Some(&logical_schema) {
                return Err(Error::BatchDisagreesWithSchema { batch: index });
            }
        }

        Ok(Self {
            schema: logical_schema,
            batches,
            files: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::array::{ArrayRef, Int64Array, StringArray};
    use arrow::datatypes::{DataType, Field};

    use super::*;

    fn batch(columns: Vec<(&str, ArrayRef)>) -> RecordBatch {
        RecordBatch::try_from_iter(columns).unwrap()
    }

    fn ids(values: &[i64]) -> ArrayRef {
        Arc::new(Int64Array::from(values.to_vec()))
    }

    #[test]
    fn batches_with_the_declared_schema_form_a_dataset() {
        let first = batch(vec![("id", ids(&[1, 2]))]);
        let second = batch(vec![("id", ids(&[3]))]);

        let dataset = Dataset::from_batches(&first.schema(), vec![first, second]).unwrap();

        assert_eq!(dataset.schema.columns().len(), 1);
        assert_eq!(dataset.batches.len(), 2);
        assert!(dataset.files.is_empty());
    }

    #[test]
    fn an_empty_table_keeps_its_schema() {
        let schema = Schema::new(vec![Field::new("id", DataType::Int64, false)]);

        let dataset = Dataset::from_batches(&schema, Vec::new()).unwrap();

        assert!(dataset.schema.column("id").is_some());
        assert!(dataset.batches.is_empty());
    }

    #[test]
    fn a_repeated_column_name_is_rejected() {
        let schema = Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("id", DataType::Int64, false),
        ]);

        let error = Dataset::from_batches(&schema, Vec::new()).unwrap_err();

        assert!(matches!(error, Error::DuplicateColumnInTable { column } if column == "id"));
    }

    #[test]
    fn a_batch_with_other_columns_is_rejected() {
        let declared = batch(vec![("id", ids(&[1]))]);
        let names: ArrayRef = Arc::new(StringArray::from(vec!["ann"]));
        let other = batch(vec![("name", names)]);

        let error = Dataset::from_batches(&declared.schema(), vec![declared, other]).unwrap_err();

        assert!(matches!(
            error,
            Error::BatchDisagreesWithSchema { batch: 1 }
        ));
    }
}
