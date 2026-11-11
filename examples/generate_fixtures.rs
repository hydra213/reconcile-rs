//! Regenerates the sample files in `tests/fixtures/`.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run --example generate_fixtures
//! ```
//!
//! The data is synthetic. The two Parquet files model a legacy trade export
//! and its migrated replacement, with deliberate differences that are listed
//! in `tests/fixtures/README.md`. Keep that file in step with this one.
//!
//! The legacy trades are also written in two other shapes that hold exactly
//! the same rows: as a CSV file, and as a folder of two Parquet part files.
//! They test the CSV reader and folder reading, locally and in the cloud.

use std::fs::File;
use std::sync::Arc;

use arrow::array::{
    ArrayRef, Date32Array, Decimal128Array, Float64Array, Int32Array, Int64Array, StringArray,
    TimestampMicrosecondArray,
};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;

const FIXTURE_DIR: &str = "tests/fixtures";

/// 2026-01-05 as days since 1970-01-01, the unit of Arrow's `Date32`.
const BUSINESS_DATE: i32 = 20_458;
/// 2026-01-05 09:30:00 UTC as microseconds since 1970-01-01.
const BOOKED_AT: i64 = 1_767_605_400_000_000;

fn main() -> anyhow::Result<()> {
    let legacy = legacy_trades()?;
    write_parquet("legacy_trades.parquet", &legacy)?;
    write_parquet("new_trades.parquet", &new_trades()?)?;

    // The same legacy rows, split over two files in one folder.
    std::fs::create_dir_all(format!("{FIXTURE_DIR}/legacy_trades_parts"))?;
    write_parquet("legacy_trades_parts/part-0.parquet", &legacy.slice(0, 3))?;
    write_parquet("legacy_trades_parts/part-1.parquet", &legacy.slice(3, 2))?;

    write_legacy_trades_csv()?;
    Ok(())
}

/// The baseline: five trades as exported by the legacy system.
fn legacy_trades() -> anyhow::Result<RecordBatch> {
    let schema = Schema::new(vec![
        Field::new("trade_id", DataType::Int64, false),
        Field::new("business_date", DataType::Date32, false),
        Field::new("trade_amt", DataType::Decimal128(18, 2), true),
        Field::new("quantity", DataType::Int32, true),
        Field::new("price", DataType::Decimal128(18, 4), true),
        Field::new("currency", DataType::Utf8, true),
        Field::new("booked_at", utc_timestamp(), true),
    ]);

    // Decimal128 stores unscaled integers: 150000 at scale 2 means 1500.00.
    let columns: Vec<ArrayRef> = vec![
        Arc::new(Int64Array::from(vec![1001, 1002, 1003, 1004, 1005])),
        Arc::new(Date32Array::from(vec![BUSINESS_DATE; 5])),
        Arc::new(
            Decimal128Array::from(vec![150_000, 275_050, 9_999, 1_000_000, 42_000])
                .with_precision_and_scale(18, 2)?,
        ),
        Arc::new(Int32Array::from(vec![10, 25, 1, 100, 4])),
        Arc::new(
            Decimal128Array::from(vec![1_500_000, 1_100_200, 999_900, 1_000_000, 1_050_000])
                .with_precision_and_scale(18, 4)?,
        ),
        Arc::new(StringArray::from(vec!["USD", "EUR", "USD", "AUD", "USD"])),
        Arc::new(TimestampMicrosecondArray::from(vec![BOOKED_AT; 5]).with_timezone("UTC")),
    ];

    Ok(RecordBatch::try_new(Arc::new(schema), columns)?)
}

/// The candidate: the same trades after migration, with deliberate changes.
///
/// Schema changes: `trade_amt` is renamed to `trade_amount`, `quantity` is
/// widened to Int64, `price` becomes a float, and `desk` is new.
/// Row changes: trade 1002 has a different amount, trade 1004 is missing and
/// trade 1006 is new.
fn new_trades() -> anyhow::Result<RecordBatch> {
    let schema = Schema::new(vec![
        Field::new("trade_id", DataType::Int64, false),
        Field::new("business_date", DataType::Date32, false),
        Field::new("trade_amount", DataType::Decimal128(18, 2), true),
        Field::new("quantity", DataType::Int64, true),
        Field::new("price", DataType::Float64, true),
        Field::new("currency", DataType::Utf8, true),
        Field::new("booked_at", utc_timestamp(), true),
        Field::new("desk", DataType::Utf8, true),
    ]);

    let columns: Vec<ArrayRef> = vec![
        Arc::new(Int64Array::from(vec![1001, 1002, 1003, 1005, 1006])),
        Arc::new(Date32Array::from(vec![BUSINESS_DATE; 5])),
        Arc::new(
            Decimal128Array::from(vec![150_000, 275_060, 9_999, 42_000, 88_000])
                .with_precision_and_scale(18, 2)?,
        ),
        Arc::new(Int64Array::from(vec![10, 25, 1, 4, 8])),
        Arc::new(Float64Array::from(vec![150.0, 110.02, 99.99, 105.0, 110.0])),
        Arc::new(StringArray::from(vec!["USD", "EUR", "USD", "USD", "GBP"])),
        Arc::new(TimestampMicrosecondArray::from(vec![BOOKED_AT; 5]).with_timezone("UTC")),
        Arc::new(StringArray::from(vec!["RATES", "FX", "RATES", "EQ", "FX"])),
    ];

    Ok(RecordBatch::try_new(Arc::new(schema), columns)?)
}

fn utc_timestamp() -> DataType {
    DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into()))
}

/// The legacy trades as CSV, read with `legacy_trades_csv.toml`.
///
/// Written as text on purpose, so the file shows exactly what a CSV export
/// from another system would contain.
fn write_legacy_trades_csv() -> anyhow::Result<()> {
    let csv = "\
trade_id,business_date,trade_amt,quantity,price,currency,booked_at
1001,2026-01-05,1500.00,10,150.0000,USD,2026-01-05T09:30:00Z
1002,2026-01-05,2750.50,25,110.0200,EUR,2026-01-05T09:30:00Z
1003,2026-01-05,99.99,1,99.9900,USD,2026-01-05T09:30:00Z
1004,2026-01-05,10000.00,100,100.0000,AUD,2026-01-05T09:30:00Z
1005,2026-01-05,420.00,4,105.0000,USD,2026-01-05T09:30:00Z
";
    let path = format!("{FIXTURE_DIR}/legacy_trades.csv");
    std::fs::write(&path, csv)?;
    println!("wrote {path} (5 rows)");
    Ok(())
}

fn write_parquet(file_name: &str, batch: &RecordBatch) -> anyhow::Result<()> {
    let path = format!("{FIXTURE_DIR}/{file_name}");
    let file = File::create(&path)?;

    let mut writer = ArrowWriter::try_new(file, batch.schema(), None)?;
    writer.write(batch)?;
    writer.close()?;

    println!("wrote {path} ({} rows)", batch.num_rows());
    Ok(())
}
