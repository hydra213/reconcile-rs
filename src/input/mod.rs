//! Reading datasets from files.
//!
//! One side of a comparison is given as one or more *locations*. A location
//! is a local path or an object storage URL (`s3://`, `abfss://`, `gs://`),
//! and names either one file or a folder of files. All files of one side
//! must have the same schema; their rows together form the [`Dataset`].
//!
//! Reading happens in three steps, one module each:
//!
//! - [`fetch`] finds the files a location names and reads their bytes.
//! - [`parquet`] and [`csv`] turn the bytes of one file into Arrow data.
//! - This file puts the files of one side together and checks that they
//!   agree on the schema.

pub mod csv;
pub mod fetch;
pub mod parquet;

use arrow::datatypes::SchemaRef;
use arrow::record_batch::RecordBatch;

use crate::config::CsvConfig;
use crate::dataset::{Dataset, InputFile};
use crate::error::Error;
use crate::fingerprint::Fingerprint;
use crate::schema::LogicalSchema;
use fetch::FetchedFile;

/// The file formats that can be read. The format is chosen by file name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Parquet,
    Csv,
}

impl Format {
    /// `.csv` files are CSV. Every other file is treated as Parquet, because
    /// Parquet files are often written without the `.parquet` ending.
    pub fn of(location: &str) -> Self {
        if location.to_ascii_lowercase().ends_with(".csv") {
            Self::Csv
        } else {
            Self::Parquet
        }
    }
}

/// Reads the schema of a dataset. No rows are decoded.
pub fn read_schema(locations: &[String], csv: &CsvConfig) -> Result<LogicalSchema, Error> {
    let files = fetch::fetch_all(locations)?;

    let mut schemas = FilesWithOneSchema::default();
    for file in &files {
        let arrow_schema = match Format::of(&file.location) {
            Format::Parquet => parquet::read_schema(file)?,
            Format::Csv => csv::read_schema(file, csv)?,
        };
        schemas.add(file, &arrow_schema)?;
    }
    schemas.into_schema()
}

/// Reads a whole dataset, schema and rows, into memory.
///
/// Each file is read exactly once. Its fingerprint and its rows both come
/// from those same bytes, so the fingerprint always describes exactly the
/// data that was compared, even if the file changes while the comparison
/// runs.
pub fn read_dataset(locations: &[String], csv: &CsvConfig) -> Result<Dataset, Error> {
    let files = fetch::fetch_all(locations)?;

    let mut schemas = FilesWithOneSchema::default();
    let mut batches: Vec<RecordBatch> = Vec::new();
    let mut input_files = Vec::new();
    for file in &files {
        let (arrow_schema, file_batches) = match Format::of(&file.location) {
            Format::Parquet => parquet::read(file)?,
            Format::Csv => csv::read(file, csv)?,
        };
        schemas.add(file, &arrow_schema)?;
        batches.extend(file_batches);
        input_files.push(InputFile {
            location: file.location.clone(),
            size_bytes: file.bytes.len() as u64,
            fingerprint: Fingerprint::of_bytes(&file.bytes),
        });
    }

    Ok(Dataset {
        schema: schemas.into_schema()?,
        batches,
        files: input_files,
    })
}

/// Checks, file by file, that every file of one side has the same schema.
#[derive(Default)]
struct FilesWithOneSchema {
    /// The location and schema of the first file.
    first: Option<(String, LogicalSchema)>,
}

impl FilesWithOneSchema {
    fn add(&mut self, file: &FetchedFile, arrow_schema: &SchemaRef) -> Result<(), Error> {
        let schema = LogicalSchema::from_arrow(arrow_schema).map_err(|duplicate| {
            Error::DuplicateColumn {
                location: file.location.clone(),
                column: duplicate.column,
            }
        })?;

        match &self.first {
            None => self.first = Some((file.location.clone(), schema)),
            Some((first_location, first_schema)) => {
                if &schema != first_schema {
                    return Err(Error::FilesDisagreeOnSchema {
                        first: first_location.clone(),
                        other: file.location.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    fn into_schema(self) -> Result<LogicalSchema, Error> {
        match self.first {
            Some((_, schema)) => Ok(schema),
            None => Err(Error::NoInputFiles),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_format_comes_from_the_file_name() {
        assert_eq!(Format::of("trades.csv"), Format::Csv);
        assert_eq!(Format::of("s3://bucket/landing/TRADES.CSV"), Format::Csv);
        assert_eq!(Format::of("trades.parquet"), Format::Parquet);
        assert_eq!(Format::of("part-00000"), Format::Parquet);
    }
}
