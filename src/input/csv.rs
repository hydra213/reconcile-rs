//! Turning the bytes of one CSV file into Arrow data.
//!
//! A CSV file has column names but no column types. ReconcileRS never
//! guesses a type from the data, because a guess can change when the data
//! changes. Instead:
//!
//! - a column whose type is declared in `[csv.types]` is read as that type;
//! - every other column is read as text.
//!
//! The first row must hold the column names. A value that does not fit its
//! declared type stops the run with an error that names the line.

use std::io::Cursor;
use std::sync::Arc;

use arrow::datatypes::{DataType, Field, Schema, SchemaRef, TimeUnit};
use arrow::record_batch::RecordBatch;
use arrow_csv::ReaderBuilder;
use arrow_csv::reader::Format;
use regex::Regex;

use super::fetch::FetchedFile;
use crate::config::CsvConfig;
use crate::error::Error;

/// The column names from the first row, with their declared types.
pub fn read_schema(file: &FetchedFile, config: &CsvConfig) -> Result<SchemaRef, Error> {
    let csv_error = |source| Error::ReadCsv {
        location: file.location.clone(),
        source,
    };

    // Asking for zero data rows reads the header only. Every column comes
    // back as text, and only the names are used.
    let (header, _) = format(config)
        .infer_schema(Cursor::new(text_bytes(file)), Some(0))
        .map_err(csv_error)?;

    let fields: Vec<Field> = header
        .fields()
        .iter()
        .map(|column| {
            let data_type = config
                .types
                .get(column.name())
                .and_then(|type_name| parse_type(type_name))
                .unwrap_or(DataType::Utf8);
            // A CSV file cannot promise that a column has no missing values.
            Field::new(column.name(), data_type, true)
        })
        .collect();
    Ok(Arc::new(Schema::new(fields)))
}

/// The schema and all rows of the file.
pub fn read(
    file: &FetchedFile,
    config: &CsvConfig,
) -> Result<(SchemaRef, Vec<RecordBatch>), Error> {
    let csv_error = |source| Error::ReadCsv {
        location: file.location.clone(),
        source,
    };
    let schema = read_schema(file, config)?;

    let reader = ReaderBuilder::new(schema.clone())
        .with_format(format(config))
        .build(Cursor::new(text_bytes(file)))
        .map_err(csv_error)?;

    let mut batches = Vec::new();
    for batch in reader {
        batches.push(batch.map_err(csv_error)?);
    }
    Ok((schema, batches))
}

/// The file's bytes without the invisible "byte order mark" that some
/// programs, such as Excel, write at the start of a UTF-8 file. Left in
/// place, it would become part of the first column's name.
fn text_bytes(file: &FetchedFile) -> &[u8] {
    const BYTE_ORDER_MARK: &[u8] = b"\xEF\xBB\xBF";
    file.bytes
        .strip_prefix(BYTE_ORDER_MARK)
        .unwrap_or(&file.bytes)
}

/// How the text is split into rows and values.
fn format(config: &CsvConfig) -> Format {
    Format::default()
        .with_header(true)
        .with_delimiter(config.delimiter_byte())
        .with_null_regex(null_pattern(&config.null_values))
}

/// A pattern that matches a value exactly when it is one of `null_values`.
fn null_pattern(null_values: &[String]) -> Regex {
    let alternatives: Vec<String> = null_values
        .iter()
        .map(|value| regex::escape(value))
        .collect();
    let pattern = format!("^(?:{})$", alternatives.join("|"));
    Regex::new(&pattern).expect("escaped text joined with `|` is a valid pattern")
}

/// Parses a type name from `[csv.types]`.
///
/// | Name | Meaning |
/// |---|---|
/// | `string` | text (the default for undeclared columns) |
/// | `bool` | `true` or `false` |
/// | `int8`, `int16`, `int32`, `int64` | signed whole numbers |
/// | `uint8`, `uint16`, `uint32`, `uint64` | unsigned whole numbers |
/// | `float32`, `float64` | floating point numbers |
/// | `decimal(18, 2)` | exact decimal with precision 18 and scale 2 |
/// | `date` | a calendar date, `2026-11-11` |
/// | `timestamp[us]` | date and time without a zone; unit `s`, `ms`, `us` or `ns` |
/// | `timestamp[us, UTC]` | an instant in the named time zone |
///
/// Returns `None` for any other name.
pub fn parse_type(name: &str) -> Option<DataType> {
    let name = name.trim();
    let simple = match name {
        "string" => Some(DataType::Utf8),
        "bool" => Some(DataType::Boolean),
        "int8" => Some(DataType::Int8),
        "int16" => Some(DataType::Int16),
        "int32" => Some(DataType::Int32),
        "int64" => Some(DataType::Int64),
        "uint8" => Some(DataType::UInt8),
        "uint16" => Some(DataType::UInt16),
        "uint32" => Some(DataType::UInt32),
        "uint64" => Some(DataType::UInt64),
        "float32" => Some(DataType::Float32),
        "float64" => Some(DataType::Float64),
        "date" => Some(DataType::Date32),
        _ => None,
    };
    if simple.is_some() {
        return simple;
    }

    if let Some(arguments) = arguments_of(name, "decimal(", ")") {
        let (precision, scale) = arguments.split_once(',')?;
        let precision: u8 = precision.trim().parse().ok()?;
        let scale: i8 = scale.trim().parse().ok()?;
        // 38 digits is the most a 128-bit decimal can hold.
        let valid = (1..=38).contains(&precision) && (0..=precision as i8).contains(&scale);
        return valid.then_some(DataType::Decimal128(precision, scale));
    }

    if let Some(arguments) = arguments_of(name, "timestamp[", "]") {
        let (unit, zone) = match arguments.split_once(',') {
            Some((unit, zone)) => (unit.trim(), Some(zone.trim())),
            None => (arguments.trim(), None),
        };
        let unit = match unit {
            "s" => TimeUnit::Second,
            "ms" => TimeUnit::Millisecond,
            "us" => TimeUnit::Microsecond,
            "ns" => TimeUnit::Nanosecond,
            _ => return None,
        };
        if zone == Some("") {
            return None;
        }
        return Some(DataType::Timestamp(unit, zone.map(Arc::from)));
    }

    None
}

/// The text between `prefix` and `suffix`, if `name` has both.
fn arguments_of<'a>(name: &'a str, prefix: &str, suffix: &str) -> Option<&'a str> {
    name.strip_prefix(prefix)?.strip_suffix(suffix)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use arrow::array::{Array, AsArray};
    use arrow::datatypes::{Decimal128Type, Int64Type};
    use bytes::Bytes;

    use super::*;

    fn file(text: &'static str) -> FetchedFile {
        FetchedFile {
            location: "test.csv".to_string(),
            bytes: Bytes::from_static(text.as_bytes()),
        }
    }

    fn config_with_types(types: &[(&str, &str)]) -> CsvConfig {
        CsvConfig {
            types: types
                .iter()
                .map(|(column, type_name)| (column.to_string(), type_name.to_string()))
                .collect::<BTreeMap<_, _>>(),
            ..CsvConfig::default()
        }
    }

    #[test]
    fn undeclared_columns_are_text() {
        let (schema, batches) =
            read(&file("id,name\n1,ann\n2,bob\n"), &CsvConfig::default()).unwrap();

        assert_eq!(schema.field(0).data_type(), &DataType::Utf8);
        assert_eq!(schema.field(1).data_type(), &DataType::Utf8);
        assert_eq!(batches[0].num_rows(), 2);
        assert_eq!(batches[0].column(0).as_string::<i32>().value(0), "1");
    }

    #[test]
    fn declared_columns_are_read_as_their_type() {
        let config = config_with_types(&[("id", "int64"), ("amount", "decimal(18, 2)")]);

        let (schema, batches) = read(&file("id,amount,name\n7,12.50,ann\n"), &config).unwrap();

        assert_eq!(schema.field(0).data_type(), &DataType::Int64);
        assert_eq!(schema.field(1).data_type(), &DataType::Decimal128(18, 2));
        assert_eq!(schema.field(2).data_type(), &DataType::Utf8);
        let batch = &batches[0];
        assert_eq!(batch.column(0).as_primitive::<Int64Type>().value(0), 7);
        // 12.50 with scale 2 is stored as the whole number 1250.
        assert_eq!(
            batch.column(1).as_primitive::<Decimal128Type>().value(0),
            1250
        );
    }

    #[test]
    fn an_empty_value_is_null_by_default_also_in_text_columns() {
        let config = config_with_types(&[("id", "int64")]);

        let (_, batches) = read(&file("id,name\n1,\n,bob\n"), &config).unwrap();

        let batch = &batches[0];
        assert!(batch.column(1).is_null(0));
        assert!(batch.column(0).is_null(1));
        assert!(batch.column(1).is_valid(1));
    }

    #[test]
    fn configured_null_values_replace_the_default() {
        let config = CsvConfig {
            null_values: vec!["NULL".to_string(), r"\N".to_string()],
            ..CsvConfig::default()
        };

        let (_, batches) = read(&file("a,b,c\nNULL,\\N,\n"), &config).unwrap();

        let batch = &batches[0];
        assert!(batch.column(0).is_null(0));
        assert!(batch.column(1).is_null(0));
        // The empty value is no longer on the list, so it is an empty text.
        assert_eq!(batch.column(2).as_string::<i32>().value(0), "");
    }

    #[test]
    fn another_delimiter_can_be_configured() {
        let config = CsvConfig {
            delimiter: ";".to_string(),
            ..CsvConfig::default()
        };

        let (schema, _) = read(&file("id;name\n1;ann\n"), &config).unwrap();

        assert_eq!(schema.fields().len(), 2);
    }

    #[test]
    fn a_byte_order_mark_does_not_become_part_of_a_column_name() {
        let schema = read_schema(&file("\u{feff}id,name\n1,ann\n"), &CsvConfig::default()).unwrap();

        assert_eq!(schema.field(0).name(), "id");
    }

    #[test]
    fn a_value_that_does_not_fit_its_type_is_an_error() {
        let config = config_with_types(&[("id", "int64")]);

        let error = read(&file("id\n1\nabc\n"), &config).unwrap_err();

        assert!(matches!(error, Error::ReadCsv { .. }));
    }

    #[test]
    fn type_names_are_parsed() {
        assert_eq!(parse_type("string"), Some(DataType::Utf8));
        assert_eq!(parse_type(" int32 "), Some(DataType::Int32));
        assert_eq!(parse_type("date"), Some(DataType::Date32));
        assert_eq!(
            parse_type("decimal(38,10)"),
            Some(DataType::Decimal128(38, 10))
        );
        assert_eq!(
            parse_type("timestamp[us]"),
            Some(DataType::Timestamp(TimeUnit::Microsecond, None))
        );
        assert_eq!(
            parse_type("timestamp[ms, UTC]"),
            Some(DataType::Timestamp(
                TimeUnit::Millisecond,
                Some("UTC".into())
            ))
        );
    }

    #[test]
    fn unknown_type_names_are_rejected() {
        for name in [
            "integer",
            "varchar",
            "decimal",
            "decimal(18)",
            "decimal(39, 2)",
            "decimal(5, 6)",
            "timestamp",
            "timestamp[minutes]",
            "timestamp[us, ]",
        ] {
            assert_eq!(parse_type(name), None, "`{name}` should not be a type");
        }
    }
}
