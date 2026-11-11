//! Normalisation: explicit, opt-in rules applied to values before they are
//! compared or fingerprinted.
//!
//! Nothing is normalised by default. Each rule here changes what "equal"
//! means, so it must be switched on in the configuration and is recorded in
//! the report.

use arrow::datatypes::DataType;
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization as _;

use crate::value::{Value, ValueKind};

/// The `[normalization]` section of the configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Normalization {
    pub unicode: UnicodeRule,
    pub naive_timestamps: NaiveTimestampRule,
}

/// What to do with strings that look the same but use different code points,
/// such as `é` as one character or as `e` plus a combining accent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnicodeRule {
    /// Compare strings byte for byte. This is the default.
    #[default]
    None,
    /// Convert strings to Unicode Normalization Form C first.
    Nfc,
}

/// What to do with timestamps that carry no time zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NaiveTimestampRule {
    /// Keep them apart from timestamps with a zone. This is the default.
    #[default]
    Distinct,
    /// Treat them as UTC, which makes them comparable with timestamps that
    /// carry a zone. Only correct if the data really was written in UTC.
    AssumeUtc,
}

impl Normalization {
    /// The kind a column of this type has once these rules are applied, or
    /// `None` when the type is unsupported.
    pub fn kind_of(&self, data_type: &DataType) -> Option<ValueKind> {
        let kind = ValueKind::of(data_type)?;
        if kind == ValueKind::NaiveTimestamp
            && self.naive_timestamps == NaiveTimestampRule::AssumeUtc
        {
            return Some(ValueKind::Instant);
        }
        Some(kind)
    }

    /// Applies the rules to one value.
    pub fn apply(&self, value: Value) -> Value {
        match value {
            Value::Utf8(text) if self.unicode == UnicodeRule::Nfc => {
                Value::Utf8(text.nfc().collect())
            }
            Value::NaiveTimestamp(nanoseconds)
                if self.naive_timestamps == NaiveTimestampRule::AssumeUtc =>
            {
                Value::Instant(nanoseconds)
            }
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow::datatypes::TimeUnit;

    /// "é" as a single code point.
    const COMPOSED: &str = "caf\u{00e9}";
    /// "é" as "e" followed by a combining acute accent.
    const DECOMPOSED: &str = "cafe\u{0301}";

    fn text(text: &str) -> Value {
        Value::Utf8(text.to_string())
    }

    #[test]
    fn by_default_nothing_changes() {
        let rules = Normalization::default();

        assert_eq!(rules.apply(text(DECOMPOSED)), text(DECOMPOSED));
        assert_eq!(
            rules.apply(Value::NaiveTimestamp(5)),
            Value::NaiveTimestamp(5)
        );
        assert_ne!(rules.apply(text(COMPOSED)), rules.apply(text(DECOMPOSED)));
    }

    #[test]
    fn nfc_makes_composed_and_decomposed_strings_equal() {
        let rules = Normalization {
            unicode: UnicodeRule::Nfc,
            ..Normalization::default()
        };
        assert_eq!(rules.apply(text(COMPOSED)), rules.apply(text(DECOMPOSED)));
    }

    #[test]
    fn assume_utc_turns_naive_timestamps_into_instants() {
        let rules = Normalization {
            naive_timestamps: NaiveTimestampRule::AssumeUtc,
            ..Normalization::default()
        };
        let naive_type = DataType::Timestamp(TimeUnit::Microsecond, None);

        assert_eq!(rules.apply(Value::NaiveTimestamp(5)), Value::Instant(5));
        assert_eq!(rules.kind_of(&naive_type), Some(ValueKind::Instant));
        assert_eq!(
            Normalization::default().kind_of(&naive_type),
            Some(ValueKind::NaiveTimestamp)
        );
    }
}
