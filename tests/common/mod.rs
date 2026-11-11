//! Helpers shared by the integration tests.

// Each test file compiles this module separately and uses only part of it.
#![allow(dead_code)]

use std::fs::File;
use std::path::PathBuf;
use std::sync::Arc;

use arrow::array::{ArrayRef, Float64Array, Int64Array, StringArray, Time64MicrosecondArray};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;

/// A row of the small `accounts` table used in tests: `(id, owner, balance)`.
/// `None` is NULL.
pub type AccountRow = (Option<i64>, Option<&'static str>, Option<i64>);

/// Builds an `accounts` batch with columns `id` (Int64), `owner` (Utf8) and
/// `balance` (Int64).
pub fn accounts(rows: &[AccountRow]) -> RecordBatch {
    let schema = Schema::new(vec![
        Field::new("id", DataType::Int64, true),
        Field::new("owner", DataType::Utf8, true),
        Field::new("balance", DataType::Int64, true),
    ]);
    let columns: Vec<ArrayRef> = vec![
        Arc::new(Int64Array::from_iter(rows.iter().map(|row| row.0))),
        Arc::new(StringArray::from_iter(rows.iter().map(|row| row.1))),
        Arc::new(Int64Array::from_iter(rows.iter().map(|row| row.2))),
    ];
    RecordBatch::try_new(Arc::new(schema), columns).expect("the columns match the schema")
}

/// Like [`accounts`], but `balance` is a Float64 column.
pub fn accounts_with_float_balance(rows: &[(i64, &'static str, f64)]) -> RecordBatch {
    let schema = Schema::new(vec![
        Field::new("id", DataType::Int64, true),
        Field::new("owner", DataType::Utf8, true),
        Field::new("balance", DataType::Float64, true),
    ]);
    let columns: Vec<ArrayRef> = vec![
        Arc::new(Int64Array::from_iter_values(rows.iter().map(|row| row.0))),
        Arc::new(StringArray::from_iter_values(rows.iter().map(|row| row.1))),
        Arc::new(Float64Array::from_iter_values(rows.iter().map(|row| row.2))),
    ];
    RecordBatch::try_new(Arc::new(schema), columns).expect("the columns match the schema")
}

/// A one-row batch with an `id` (Int64) and an `opens_at` time-of-day column,
/// a type the engine does not compare yet.
pub fn shops_with_opening_time() -> RecordBatch {
    let schema = Schema::new(vec![
        Field::new("id", DataType::Int64, true),
        Field::new("opens_at", DataType::Time64(TimeUnit::Microsecond), true),
    ]);
    let nine_o_clock = 9 * 60 * 60 * 1_000_000;
    let columns: Vec<ArrayRef> = vec![
        Arc::new(Int64Array::from(vec![1])),
        Arc::new(Time64MicrosecondArray::from(vec![nine_o_clock])),
    ];
    RecordBatch::try_new(Arc::new(schema), columns).expect("the columns match the schema")
}

/// Writes a batch to a Parquet file in Cargo's scratch directory for
/// integration tests and returns its path. Use a unique `file_name` per test,
/// because tests run in parallel.
pub fn write_parquet(file_name: &str, batch: &RecordBatch) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(file_name);
    let file = File::create(&path).expect("the scratch directory should be writable");

    let mut writer =
        ArrowWriter::try_new(file, batch.schema(), None).expect("the schema is valid Parquet");
    writer.write(batch).expect("the batch should be written");
    writer.close().expect("the file should be finished");
    path
}
