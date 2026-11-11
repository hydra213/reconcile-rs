//! Canonical encoding: one exact byte sequence for each logical value.
//!
//! A fingerprint is only meaningful if equal values always produce equal
//! bytes and different values always produce different bytes. This module
//! defines those bytes. The layout is part of the public contract: any
//! change to it must bump [`CANONICAL_VERSION`], because fingerprints made
//! with different versions cannot be compared.
//!
//! # Layout, version 1
//!
//! All integers are big-endian.
//!
//! A **value** is one tag byte followed by a payload:
//!
//! | Tag  | Value            | Payload                                         |
//! |------|------------------|-------------------------------------------------|
//! | 0x00 | NULL             | nothing                                         |
//! | 0x01 | boolean          | 1 byte: 0 or 1                                  |
//! | 0x02 | integer          | 16 bytes: the number as `i128`                  |
//! | 0x03 | float            | 8 bytes: the normalised IEEE 754 bits           |
//! | 0x04 | decimal          | 4 bytes scale (`i32`), 16 bytes unscaled (`i128`) |
//! | 0x05 | string           | 8 bytes length (`u64`), then the UTF-8 bytes    |
//! | 0x06 | binary           | 8 bytes length (`u64`), then the bytes          |
//! | 0x07 | date             | 4 bytes: days since 1970-01-01 (`i32`)          |
//! | 0x08 | instant          | 16 bytes: nanoseconds since the epoch (`i128`)  |
//! | 0x09 | naive timestamp  | 16 bytes: nanoseconds since the epoch (`i128`)  |
//!
//! The tag keeps NULL, zero, the empty string and empty binary apart. The
//! length prefix keeps `("ab", "c")` apart from `("a", "bc")`.
//!
//! A **row** is: 4 bytes version (`u32`), 4 bytes column count (`u32`), then
//! for each column 4 bytes name length (`u32`), the UTF-8 name, and the
//! encoded value. Columns appear in the order the caller gives, which for
//! fingerprints is always sorted by logical column name.

use crate::value::Value;

/// The version of the byte layout described in the module documentation.
pub const CANONICAL_VERSION: u32 = 1;

const TAG_NULL: u8 = 0x00;
const TAG_BOOLEAN: u8 = 0x01;
const TAG_INTEGER: u8 = 0x02;
const TAG_FLOAT: u8 = 0x03;
const TAG_DECIMAL: u8 = 0x04;
const TAG_STRING: u8 = 0x05;
const TAG_BINARY: u8 = 0x06;
const TAG_DATE: u8 = 0x07;
const TAG_INSTANT: u8 = 0x08;
const TAG_NAIVE_TIMESTAMP: u8 = 0x09;

/// Appends the canonical bytes of one value to `out`.
pub fn encode_value(value: &Value, out: &mut Vec<u8>) {
    match value {
        Value::Null => out.push(TAG_NULL),
        Value::Boolean(flag) => {
            out.push(TAG_BOOLEAN);
            out.push(u8::from(*flag));
        }
        Value::Integer(number) => {
            out.push(TAG_INTEGER);
            out.extend_from_slice(&number.to_be_bytes());
        }
        Value::Float(float) => {
            out.push(TAG_FLOAT);
            out.extend_from_slice(&float.bits().to_be_bytes());
        }
        Value::Decimal(decimal) => {
            out.push(TAG_DECIMAL);
            out.extend_from_slice(&decimal.scale().to_be_bytes());
            out.extend_from_slice(&decimal.unscaled().to_be_bytes());
        }
        Value::Utf8(text) => {
            out.push(TAG_STRING);
            encode_length_prefixed(text.as_bytes(), out);
        }
        Value::Binary(bytes) => {
            out.push(TAG_BINARY);
            encode_length_prefixed(bytes, out);
        }
        Value::Date(days) => {
            out.push(TAG_DATE);
            out.extend_from_slice(&days.to_be_bytes());
        }
        Value::Instant(nanoseconds) => {
            out.push(TAG_INSTANT);
            out.extend_from_slice(&nanoseconds.to_be_bytes());
        }
        Value::NaiveTimestamp(nanoseconds) => {
            out.push(TAG_NAIVE_TIMESTAMP);
            out.extend_from_slice(&nanoseconds.to_be_bytes());
        }
    }
}

/// The canonical bytes of one row: its column names and values.
///
/// `columns` must already be in the order that defines the encoding. Callers
/// that fingerprint rows pass columns sorted by logical name.
pub fn encode_row(columns: &[(&str, &Value)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&CANONICAL_VERSION.to_be_bytes());
    out.extend_from_slice(&(columns.len() as u32).to_be_bytes());

    for (name, value) in columns {
        out.extend_from_slice(&(name.len() as u32).to_be_bytes());
        out.extend_from_slice(name.as_bytes());
        encode_value(value, &mut out);
    }
    out
}

fn encode_length_prefixed(bytes: &[u8], out: &mut Vec<u8>) {
    out.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
    out.extend_from_slice(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::{Decimal, NormalizedFloat};

    fn encoded(value: &Value) -> Vec<u8> {
        let mut out = Vec::new();
        encode_value(value, &mut out);
        out
    }

    fn text(text: &str) -> Value {
        Value::Utf8(text.to_string())
    }

    #[test]
    fn null_zero_empty_string_and_empty_binary_are_all_different() {
        let encodings = [
            encoded(&Value::Null),
            encoded(&Value::Integer(0)),
            encoded(&Value::Float(NormalizedFloat::new(0.0))),
            encoded(&Value::Decimal(Decimal::new(0, 0))),
            encoded(&text("")),
            encoded(&Value::Binary(Vec::new())),
            encoded(&Value::Boolean(false)),
            encoded(&Value::Date(0)),
            encoded(&Value::Instant(0)),
            encoded(&Value::NaiveTimestamp(0)),
        ];

        for (i, first) in encodings.iter().enumerate() {
            for second in &encodings[i + 1..] {
                assert_ne!(first, second);
            }
        }
    }

    #[test]
    fn a_string_and_binary_with_the_same_bytes_are_different() {
        assert_ne!(
            encoded(&text("abc")),
            encoded(&Value::Binary(b"abc".to_vec()))
        );
    }

    #[test]
    fn moving_a_character_between_columns_changes_the_row() {
        let first = encode_row(&[("a", &text("ab")), ("b", &text("c"))]);
        let second = encode_row(&[("a", &text("a")), ("b", &text("bc"))]);
        assert_ne!(first, second);
    }

    #[test]
    fn the_column_name_is_part_of_the_row() {
        let first = encode_row(&[("a", &Value::Integer(1))]);
        let second = encode_row(&[("b", &Value::Integer(1))]);
        assert_ne!(first, second);
    }

    #[test]
    fn equal_numbers_encode_equally_whatever_their_storage() {
        // 1.50 and 1.5 are the same decimal; -0.0 and 0.0 the same float.
        assert_eq!(
            encoded(&Value::Decimal(Decimal::new(150, 2))),
            encoded(&Value::Decimal(Decimal::new(15, 1)))
        );
        assert_eq!(
            encoded(&Value::Float(NormalizedFloat::new(-0.0))),
            encoded(&Value::Float(NormalizedFloat::new(0.0)))
        );
    }

    /// Pins the exact bytes of version 1. If this test fails, the layout has
    /// changed: bump `CANONICAL_VERSION` and update the module documentation
    /// instead of editing the expected bytes.
    #[test]
    fn version_1_layout_is_pinned() {
        let row = encode_row(&[("id", &Value::Integer(7)), ("name", &text("ann"))]);

        let mut expected = Vec::new();
        expected.extend_from_slice(&[0, 0, 0, 1]); // version
        expected.extend_from_slice(&[0, 0, 0, 2]); // column count
        expected.extend_from_slice(&[0, 0, 0, 2]); // length of "id"
        expected.extend_from_slice(b"id");
        expected.push(0x02); // integer tag
        expected.extend_from_slice(&[0; 15]);
        expected.push(7);
        expected.extend_from_slice(&[0, 0, 0, 4]); // length of "name"
        expected.extend_from_slice(b"name");
        expected.push(0x05); // string tag
        expected.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 3]); // length of "ann"
        expected.extend_from_slice(b"ann");

        assert_eq!(row, expected);
    }
}
