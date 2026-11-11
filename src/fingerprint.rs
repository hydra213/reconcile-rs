//! Fingerprints: short digests that identify content.
//!
//! A fingerprint is the BLAKE3 hash of canonical bytes (see
//! [`crate::canonical`]). Equal content always gives equal fingerprints.
//! Different content gives different fingerprints except for a hash
//! collision, which for a 256-bit hash is not a practical concern but is
//! also not a mathematical proof of equality.
//!
//! Fingerprints are a *screening signal*. They are exact: two values inside
//! a configured tolerance still have different fingerprints. The keyed row
//! comparison in [`crate::rowdiff`] remains the evidence for what differs.

use std::fmt;

use serde::{Serialize, Serializer};

use crate::canonical::{self, CANONICAL_VERSION};
use crate::schema::LogicalSchema;
use crate::table::Table;
use crate::value::Value;

/// The name of the hash algorithm, recorded in every report.
pub const ALGORITHM: &str = "blake3";

/// A 256-bit BLAKE3 digest. Shown and serialised as 64 hex characters.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Fingerprint([u8; 32]);

impl Fingerprint {
    /// The fingerprint of raw bytes, for example the contents of a file.
    pub fn of_bytes(bytes: &[u8]) -> Self {
        Self(*blake3::hash(bytes).as_bytes())
    }

    /// The fingerprint of one row: its column names and values.
    ///
    /// `columns` must be sorted by column name so that the same row always
    /// gives the same fingerprint.
    pub fn of_row(columns: &[(&str, &Value)]) -> Self {
        Self::of_bytes(&canonical::encode_row(columns))
    }

    /// The fingerprint of a schema: every column's name, type and
    /// nullability, in name order.
    pub fn of_schema(schema: &LogicalSchema) -> Self {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"schema");
        bytes.extend_from_slice(&CANONICAL_VERSION.to_be_bytes());
        bytes.extend_from_slice(&(schema.columns().len() as u32).to_be_bytes());

        for column in schema.columns() {
            let type_name = column.data_type.to_string();
            bytes.extend_from_slice(&(column.name.len() as u32).to_be_bytes());
            bytes.extend_from_slice(column.name.as_bytes());
            bytes.extend_from_slice(&(type_name.len() as u32).to_be_bytes());
            bytes.extend_from_slice(type_name.as_bytes());
            bytes.push(u8::from(column.nullable));
        }
        Self::of_bytes(&bytes)
    }

    /// The fingerprint of a table's content: every row, over all columns.
    ///
    /// The result does not depend on row order, and a repeated row changes
    /// it. This is done by fingerprinting each row, sorting the row
    /// fingerprints and hashing the sorted list. (Combining row hashes with
    /// XOR would be order-independent too, but two identical rows would
    /// cancel out.)
    pub fn of_table(table: &Table) -> Self {
        let column_names = table.column_names();
        let columns: Vec<&[Value]> = column_names.iter().map(|name| table.column(name)).collect();

        let mut row_fingerprints: Vec<Fingerprint> = (0..table.row_count())
            .map(|row| {
                let cells: Vec<(&str, &Value)> = column_names
                    .iter()
                    .zip(&columns)
                    .map(|(name, values)| (*name, &values[row]))
                    .collect();
                Self::of_row(&cells)
            })
            .collect();
        row_fingerprints.sort_unstable();

        let mut bytes = Vec::with_capacity(16 + 32 * row_fingerprints.len());
        bytes.extend_from_slice(b"table");
        bytes.extend_from_slice(&CANONICAL_VERSION.to_be_bytes());
        bytes.extend_from_slice(&(row_fingerprints.len() as u64).to_be_bytes());
        for fingerprint in &row_fingerprints {
            bytes.extend_from_slice(&fingerprint.0);
        }
        Self::of_bytes(&bytes)
    }

    /// The fingerprint as 64 lowercase hex characters.
    pub fn to_hex(self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fingerprint({})", self.to_hex())
    }
}

impl Serialize for Fingerprint {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int(number: i128) -> Value {
        Value::Integer(number)
    }

    fn table(ids: &[i128], names: &[&str]) -> Table {
        Table::from_columns(vec![
            ("id", ids.iter().map(|id| int(*id)).collect()),
            (
                "name",
                names
                    .iter()
                    .map(|name| Value::Utf8(name.to_string()))
                    .collect(),
            ),
        ])
    }

    #[test]
    fn a_fingerprint_is_64_hex_characters() {
        let hex = Fingerprint::of_bytes(b"hello").to_hex();
        assert_eq!(hex.len(), 64);
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn row_order_does_not_change_a_table_fingerprint() {
        let original = table(&[1, 2, 3], &["a", "b", "c"]);
        let reordered = table(&[3, 1, 2], &["c", "a", "b"]);
        assert_eq!(
            Fingerprint::of_table(&original),
            Fingerprint::of_table(&reordered)
        );
    }

    #[test]
    fn a_changed_value_changes_a_table_fingerprint() {
        let original = table(&[1, 2], &["a", "b"]);
        let changed = table(&[1, 2], &["a", "B"]);
        assert_ne!(
            Fingerprint::of_table(&original),
            Fingerprint::of_table(&changed)
        );
    }

    #[test]
    fn a_repeated_row_changes_a_table_fingerprint() {
        // With XOR, the two extra identical rows would cancel out.
        let original = table(&[1], &["a"]);
        let with_duplicates = table(&[1, 2, 2], &["a", "b", "b"]);
        let with_one_copy = table(&[1, 2], &["a", "b"]);

        assert_ne!(
            Fingerprint::of_table(&original),
            Fingerprint::of_table(&with_duplicates)
        );
        assert_ne!(
            Fingerprint::of_table(&with_one_copy),
            Fingerprint::of_table(&with_duplicates)
        );
    }

    #[test]
    fn swapping_values_between_rows_changes_a_table_fingerprint() {
        let original = table(&[1, 2], &["a", "b"]);
        let swapped = table(&[1, 2], &["b", "a"]);
        assert_ne!(
            Fingerprint::of_table(&original),
            Fingerprint::of_table(&swapped)
        );
    }

    #[test]
    fn an_empty_table_has_a_fingerprint() {
        let empty = table(&[], &[]);
        assert_eq!(Fingerprint::of_table(&empty), Fingerprint::of_table(&empty));
        assert_ne!(
            Fingerprint::of_table(&empty),
            Fingerprint::of_table(&table(&[1], &["a"]))
        );
    }
}
