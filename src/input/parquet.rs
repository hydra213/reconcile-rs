//! Turning the bytes of one Parquet file into Arrow data.

use arrow::datatypes::SchemaRef;
use arrow::record_batch::RecordBatch;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

use super::fetch::FetchedFile;
use crate::error::Error;

/// The schema of the file, from its footer. No rows are decoded.
pub fn read_schema(file: &FetchedFile) -> Result<SchemaRef, Error> {
    Ok(open(file)?.schema().clone())
}

/// The schema and all rows of the file.
pub fn read(file: &FetchedFile) -> Result<(SchemaRef, Vec<RecordBatch>), Error> {
    let builder = open(file)?;
    let schema = builder.schema().clone();

    let reader = builder.build().map_err(|source| Error::ReadParquet {
        location: file.location.clone(),
        source,
    })?;

    let mut batches = Vec::new();
    for batch in reader {
        let batch = batch.map_err(|source| Error::ReadRows {
            location: file.location.clone(),
            source,
        })?;
        batches.push(batch);
    }
    Ok((schema, batches))
}

/// Reads the footer, which holds the schema and where the rows are.
///
/// `Bytes` is a cheap handle to the file's bytes, so cloning it does not
/// copy the file.
fn open(file: &FetchedFile) -> Result<ParquetRecordBatchReaderBuilder<bytes::Bytes>, Error> {
    ParquetRecordBatchReaderBuilder::try_new(file.bytes.clone()).map_err(|source| {
        Error::ReadParquet {
            location: file.location.clone(),
            source,
        }
    })
}
