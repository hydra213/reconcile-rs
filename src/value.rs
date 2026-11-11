//! Logical values: one representation for a cell, whatever its storage type.
//!
//! Arrow has many physical types for the same idea. `Int32` and `Int64` both
//! hold integers; `Decimal128(18, 2)` and `Decimal128(10, 4)` both hold exact
//! decimals. To compare two datasets fairly, every cell is first converted
//! to a [`Value`]. Two cells are equal exactly when their `Value`s are equal.
//!
//! The conversion rules are deliberately conservative. A type with no rule
//! here is *unsupported*: columns of that type are reported as inconclusive,
//! never silently compared.

use std::fmt;

use arrow::array::{Array, AsArray};
use arrow::datatypes::{
    ArrowPrimitiveType, DataType, Date32Type, Decimal128Type, Float32Type, Float64Type, Int8Type,
    Int16Type, Int32Type, Int64Type, TimeUnit, TimestampMicrosecondType, TimestampMillisecondType,
    TimestampNanosecondType, TimestampSecondType, UInt8Type, UInt16Type, UInt32Type, UInt64Type,
};
use serde::Serialize;

/// One cell of a dataset in its logical form.
///
/// The derived `Ord` exists so values can be used as sorted map keys, which
/// keeps results deterministic. It is not a meaningful business ordering.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Value {
    /// SQL NULL. Different from an empty string, zero or empty binary.
    Null,
    Boolean(bool),
    /// Any signed or unsigned integer up to 64 bits. `i128` holds them all.
    Integer(i128),
    Float(NormalizedFloat),
    Decimal(Decimal),
    Utf8(String),
    Binary(Vec<u8>),
    /// A calendar date as days since 1970-01-01.
    Date(i32),
    /// A point in time, as nanoseconds since 1970-01-01T00:00:00 UTC.
    /// Comes from timestamp columns that carry a time zone.
    Instant(i128),
    /// A wall-clock date and time with no time zone, as nanoseconds since
    /// 1970-01-01T00:00:00 on that wall clock. Never equal to an `Instant`,
    /// because the zone it was recorded in is unknown.
    NaiveTimestamp(i128),
}

impl Value {
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }
}

impl fmt::Display for Value {
    /// Writes the value the way it is shown in reports.
    ///
    /// Strings are quoted, so the text `"NULL"` cannot be mistaken for a
    /// missing value, which is written as a bare `NULL`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => f.write_str("NULL"),
            Self::Boolean(flag) => write!(f, "{flag}"),
            Self::Integer(number) => write!(f, "{number}"),
            Self::Float(float) => write!(f, "{}", float.as_f64()),
            Self::Decimal(decimal) => write!(f, "{decimal}"),
            Self::Utf8(text) => write!(f, "{text:?}"),
            Self::Binary(bytes) => {
                f.write_str("0x")?;
                bytes.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
            }
            Self::Date(days) => match date_from_epoch_days(*days) {
                Some(date) => write!(f, "{date}"),
                None => write!(f, "day {days} since 1970-01-01"),
            },
            Self::Instant(nanoseconds) => match date_time_from_epoch_nanoseconds(*nanoseconds) {
                Some(date_time) => write!(f, "{}Z", date_time.format("%Y-%m-%dT%H:%M:%S%.f")),
                None => write!(f, "{nanoseconds} ns since 1970-01-01T00:00:00Z"),
            },
            Self::NaiveTimestamp(nanoseconds) => {
                match date_time_from_epoch_nanoseconds(*nanoseconds) {
                    Some(date_time) => write!(f, "{}", date_time.format("%Y-%m-%dT%H:%M:%S%.f")),
                    None => write!(f, "{nanoseconds} ns since 1970-01-01T00:00:00"),
                }
            }
        }
    }
}

/// The calendar date for a day count, or `None` if it is out of range.
fn date_from_epoch_days(days: i32) -> Option<chrono::NaiveDate> {
    chrono::NaiveDate::from_ymd_opt(1970, 1, 1)?
        .checked_add_signed(chrono::Duration::days(days.into()))
}

/// The date and time for a nanosecond count, or `None` if it is out of
/// the range the calendar library supports.
fn date_time_from_epoch_nanoseconds(nanoseconds: i128) -> Option<chrono::NaiveDateTime> {
    const NANOSECONDS_PER_SECOND: i128 = 1_000_000_000;
    // `div_euclid` and `rem_euclid` keep the remainder positive for times
    // before 1970, which is what the calendar library expects.
    let seconds = i64::try_from(nanoseconds.div_euclid(NANOSECONDS_PER_SECOND)).ok()?;
    let remainder = u32::try_from(nanoseconds.rem_euclid(NANOSECONDS_PER_SECOND)).ok()?;
    Some(chrono::DateTime::from_timestamp(seconds, remainder)?.naive_utc())
}

/// The family a column's values belong to. Two columns can be compared only
/// when they belong to the same family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueKind {
    Boolean,
    Integer,
    Float,
    Decimal,
    Utf8,
    Binary,
    Date,
    Instant,
    NaiveTimestamp,
}

impl ValueKind {
    /// The family of an Arrow type, or `None` when the type is unsupported.
    ///
    /// Must stay in step with [`column_values`]: a type has a kind here
    /// exactly when it can be converted there. A unit test checks this.
    pub fn of(data_type: &DataType) -> Option<Self> {
        let kind = match data_type {
            DataType::Boolean => Self::Boolean,
            DataType::Int8
            | DataType::Int16
            | DataType::Int32
            | DataType::Int64
            | DataType::UInt8
            | DataType::UInt16
            | DataType::UInt32
            | DataType::UInt64 => Self::Integer,
            DataType::Float32 | DataType::Float64 => Self::Float,
            DataType::Decimal128(_, _) => Self::Decimal,
            DataType::Utf8 | DataType::LargeUtf8 | DataType::Utf8View => Self::Utf8,
            DataType::Binary | DataType::LargeBinary | DataType::BinaryView => Self::Binary,
            DataType::Date32 => Self::Date,
            DataType::Timestamp(_, Some(_)) => Self::Instant,
            DataType::Timestamp(_, None) => Self::NaiveTimestamp,
            _ => return None,
        };
        Some(kind)
    }

    /// Whether values of this kind are numbers, so a tolerance can apply.
    pub fn is_numeric(self) -> bool {
        matches!(self, Self::Integer | Self::Float | Self::Decimal)
    }

    /// Whether a column of this kind may be used as a key.
    ///
    /// Floats are excluded: rounding makes them unreliable identifiers.
    pub fn can_be_key(self) -> bool {
        self != Self::Float
    }
}

impl fmt::Display for ValueKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::Boolean => "boolean",
            Self::Integer => "integer",
            Self::Float => "float",
            Self::Decimal => "decimal",
            Self::Utf8 => "string",
            Self::Binary => "binary",
            Self::Date => "date",
            Self::Instant => "timestamp with time zone",
            Self::NaiveTimestamp => "timestamp without time zone",
        };
        f.write_str(text)
    }
}

/// A 64-bit float stored so that equal numbers have equal bits.
///
/// Plain floats break two rules that reconciliation needs: `NaN != NaN`, and
/// `0.0 == -0.0` although their bits differ. Normalising fixes both:
///
/// - every NaN becomes the same NaN, so NaN equals NaN;
/// - `-0.0` becomes `0.0`.
///
/// Infinities and all other numbers are kept exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NormalizedFloat {
    bits: u64,
}

impl NormalizedFloat {
    pub fn new(number: f64) -> Self {
        let normalized = if number.is_nan() {
            f64::NAN
        } else if number == 0.0 {
            0.0
        } else {
            number
        };
        Self {
            bits: normalized.to_bits(),
        }
    }

    pub fn as_f64(self) -> f64 {
        f64::from_bits(self.bits)
    }

    /// The normalised IEEE 754 bits, used by the canonical encoding.
    pub fn bits(self) -> u64 {
        self.bits
    }
}

/// An exact decimal number: `unscaled * 10^(-scale)`.
///
/// Stored without trailing zeros so that equal numbers have one
/// representation: `1.50` and `1.5` are both `unscaled = 15, scale = 1`.
/// Zero is always `unscaled = 0, scale = 0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Decimal {
    unscaled: i128,
    scale: i32,
}

impl Decimal {
    pub fn new(unscaled: i128, scale: i32) -> Self {
        if unscaled == 0 {
            return Self {
                unscaled: 0,
                scale: 0,
            };
        }

        let mut unscaled = unscaled;
        let mut scale = scale;
        while unscaled % 10 == 0 {
            unscaled /= 10;
            scale -= 1;
        }
        Self { unscaled, scale }
    }

    pub fn unscaled(self) -> i128 {
        self.unscaled
    }

    pub fn scale(self) -> i32 {
        self.scale
    }

    /// Reads a plain decimal number such as `12`, `-0.5` or `0.010`.
    ///
    /// Returns `None` for anything else, including exponents (`1e-3`), a
    /// missing digit before or after the point (`.5`, `5.`) and numbers too
    /// large for 128 bits.
    pub fn parse(text: &str) -> Option<Self> {
        let (negative, digits) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let (whole, fraction) = match digits.split_once('.') {
            Some((whole, fraction)) => (whole, fraction),
            None => (digits, ""),
        };

        let all_digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
        if !all_digits(whole) || (digits.contains('.') && !all_digits(fraction)) {
            return None;
        }

        // "12.34" is the integer 1234 with two digits after the point.
        let unscaled: i128 = format!("{whole}{fraction}").parse().ok()?;
        let scale = i32::try_from(fraction.len()).ok()?;
        Some(Self::new(
            if negative { -unscaled } else { unscaled },
            scale,
        ))
    }

    /// `self - other`, or `None` if the result does not fit in 128 bits.
    pub fn checked_sub(self, other: Self) -> Option<Self> {
        // Bring both numbers to the larger scale, then subtract the integers.
        let scale = self.scale.max(other.scale);
        let left = self.unscaled_at_scale(scale)?;
        let right = other.unscaled_at_scale(scale)?;
        Some(Self::new(left.checked_sub(right)?, scale))
    }

    /// The absolute value, or `None` if it does not fit in 128 bits.
    pub fn checked_abs(self) -> Option<Self> {
        Some(Self {
            unscaled: self.unscaled.checked_abs()?,
            scale: self.scale,
        })
    }

    /// Whether `self <= other`. `None` if the comparison overflows 128 bits.
    pub fn is_at_most(self, other: Self) -> Option<bool> {
        Some(self.checked_sub(other)?.unscaled <= 0)
    }

    /// The unscaled integer this number would have at a larger `scale`.
    /// For example 1.5 (15 at scale 1) is 1500 at scale 3.
    fn unscaled_at_scale(self, scale: i32) -> Option<i128> {
        let extra_digits = u32::try_from(scale - self.scale).ok()?;
        let factor = 10_i128.checked_pow(extra_digits)?;
        self.unscaled.checked_mul(factor)
    }
}

impl fmt::Display for Decimal {
    /// Writes the number in plain notation, for example `-0.05` or `1500`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.unscaled < 0 { "-" } else { "" };
        let digits = self.unscaled.unsigned_abs().to_string();

        if self.scale <= 0 {
            // A negative scale means trailing zeros: 15 at scale -2 is 1500.
            let zeros = "0".repeat(self.scale.unsigned_abs() as usize);
            return write!(f, "{sign}{digits}{zeros}");
        }

        let scale = self.scale as usize;
        // Pad with leading zeros so there is at least one digit before the
        // point: 5 at scale 2 becomes "005", shown as 0.05.
        let digits = format!("{digits:0>width$}", width = scale + 1);
        let (whole, fraction) = digits.split_at(digits.len() - scale);
        write!(f, "{sign}{whole}.{fraction}")
    }
}

/// Converts every cell of an Arrow array to a [`Value`].
///
/// Returns `None` when the array's type is unsupported.
pub fn column_values(array: &dyn Array) -> Option<Vec<Value>> {
    let values = match array.data_type() {
        DataType::Boolean => array
            .as_boolean()
            .iter()
            .map(|cell| cell.map_or(Value::Null, Value::Boolean))
            .collect(),

        DataType::Int8 => primitive_values::<Int8Type>(array, |n| Value::Integer(n.into())),
        DataType::Int16 => primitive_values::<Int16Type>(array, |n| Value::Integer(n.into())),
        DataType::Int32 => primitive_values::<Int32Type>(array, |n| Value::Integer(n.into())),
        DataType::Int64 => primitive_values::<Int64Type>(array, |n| Value::Integer(n.into())),
        DataType::UInt8 => primitive_values::<UInt8Type>(array, |n| Value::Integer(n.into())),
        DataType::UInt16 => primitive_values::<UInt16Type>(array, |n| Value::Integer(n.into())),
        DataType::UInt32 => primitive_values::<UInt32Type>(array, |n| Value::Integer(n.into())),
        DataType::UInt64 => primitive_values::<UInt64Type>(array, |n| Value::Integer(n.into())),

        // Every f32 is exactly representable as an f64, so widening is safe.
        DataType::Float32 => {
            primitive_values::<Float32Type>(array, |n| Value::Float(NormalizedFloat::new(n.into())))
        }
        DataType::Float64 => {
            primitive_values::<Float64Type>(array, |n| Value::Float(NormalizedFloat::new(n)))
        }

        DataType::Decimal128(_, scale) => {
            let scale = i32::from(*scale);
            primitive_values::<Decimal128Type>(array, |unscaled| {
                Value::Decimal(Decimal::new(unscaled, scale))
            })
        }

        DataType::Utf8 => string_values(array.as_string::<i32>().iter()),
        DataType::LargeUtf8 => string_values(array.as_string::<i64>().iter()),
        DataType::Utf8View => string_values(array.as_string_view().iter()),

        DataType::Binary => binary_values(array.as_binary::<i32>().iter()),
        DataType::LargeBinary => binary_values(array.as_binary::<i64>().iter()),
        DataType::BinaryView => binary_values(array.as_binary_view().iter()),

        DataType::Date32 => primitive_values::<Date32Type>(array, Value::Date),

        DataType::Timestamp(unit, zone) => {
            // A zone means the stored number is an instant in UTC. No zone
            // means a wall-clock reading whose zone is unknown.
            let wrap: fn(i128) -> Value = if zone.is_some() {
                Value::Instant
            } else {
                Value::NaiveTimestamp
            };
            match unit {
                TimeUnit::Second => primitive_values::<TimestampSecondType>(array, |n| {
                    wrap(i128::from(n) * 1_000_000_000)
                }),
                TimeUnit::Millisecond => primitive_values::<TimestampMillisecondType>(array, |n| {
                    wrap(i128::from(n) * 1_000_000)
                }),
                TimeUnit::Microsecond => primitive_values::<TimestampMicrosecondType>(array, |n| {
                    wrap(i128::from(n) * 1_000)
                }),
                TimeUnit::Nanosecond => {
                    primitive_values::<TimestampNanosecondType>(array, |n| wrap(i128::from(n)))
                }
            }
        }

        _ => return None,
    };
    Some(values)
}

/// Converts a numeric Arrow array, turning NULL cells into [`Value::Null`]
/// and every other cell into a value with `convert`.
///
/// `T` names the Arrow type of the array, for example `Int32Type`.
fn primitive_values<T: ArrowPrimitiveType>(
    array: &dyn Array,
    convert: impl Fn(T::Native) -> Value,
) -> Vec<Value> {
    array
        .as_primitive::<T>()
        .iter()
        .map(|cell| cell.map_or(Value::Null, &convert))
        .collect()
}

fn string_values<'a>(cells: impl Iterator<Item = Option<&'a str>>) -> Vec<Value> {
    cells
        .map(|cell| cell.map_or(Value::Null, |text| Value::Utf8(text.to_string())))
        .collect()
}

fn binary_values<'a>(cells: impl Iterator<Item = Option<&'a [u8]>>) -> Vec<Value> {
    cells
        .map(|cell| cell.map_or(Value::Null, |bytes| Value::Binary(bytes.to_vec())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow::array::{
        ArrayRef, Decimal128Array, Float32Array, Float64Array, Int32Array, Int64Array, StringArray,
        TimestampMicrosecondArray, TimestampMillisecondArray, UInt64Array, new_null_array,
    };
    use std::sync::Arc;

    fn values(array: impl Array) -> Vec<Value> {
        column_values(&array).expect("the type should be supported")
    }

    #[test]
    fn integers_of_different_widths_are_equal() {
        assert_eq!(
            values(Int32Array::from(vec![7])),
            values(Int64Array::from(vec![7]))
        );
        assert_eq!(
            values(UInt64Array::from(vec![u64::MAX])),
            vec![Value::Integer(i128::from(u64::MAX))]
        );
    }

    #[test]
    fn null_is_not_zero_or_empty_string() {
        assert_eq!(
            values(Int32Array::from(vec![None, Some(0)])),
            vec![Value::Null, Value::Integer(0)]
        );
        assert_eq!(
            values(StringArray::from(vec![None, Some("")])),
            vec![Value::Null, Value::Utf8(String::new())]
        );
    }

    #[test]
    fn decimals_with_different_scales_are_equal_when_the_number_is_equal() {
        let two_places = Decimal128Array::from(vec![150, 0, -1200])
            .with_precision_and_scale(18, 2)
            .unwrap();
        let four_places = Decimal128Array::from(vec![15_000, 0, -120_000])
            .with_precision_and_scale(18, 4)
            .unwrap();
        assert_eq!(values(two_places), values(four_places));
    }

    #[test]
    fn decimal_is_stored_without_trailing_zeros() {
        let decimal = Decimal::new(1500, 3);
        assert_eq!((decimal.unscaled(), decimal.scale()), (15, 1));

        let zero = Decimal::new(0, 4);
        assert_eq!((zero.unscaled(), zero.scale()), (0, 0));
    }

    #[test]
    fn values_are_displayed_unambiguously() {
        let shown = |value: Value| value.to_string();

        assert_eq!(shown(Value::Null), "NULL");
        assert_eq!(shown(Value::Utf8("NULL".to_string())), "\"NULL\"");
        assert_eq!(shown(Value::Integer(-7)), "-7");
        assert_eq!(shown(Value::Decimal(Decimal::new(275_050, 2))), "2750.5");
        assert_eq!(shown(Value::Float(NormalizedFloat::new(f64::NAN))), "NaN");
        assert_eq!(shown(Value::Binary(vec![0, 255])), "0x00ff");
        assert_eq!(shown(Value::Date(20_458)), "2026-01-05");
        assert_eq!(
            shown(Value::Instant(1_767_605_400_000_000_000)),
            "2026-01-05T09:30:00Z"
        );
        assert_eq!(
            shown(Value::NaiveTimestamp(1_767_605_400_500_000_000)),
            "2026-01-05T09:30:00.500"
        );
        // One nanosecond before the epoch.
        assert_eq!(shown(Value::Instant(-1)), "1969-12-31T23:59:59.999999999Z");
    }

    #[test]
    fn decimal_parses_plain_numbers_only() {
        assert_eq!(Decimal::parse("0.01"), Some(Decimal::new(1, 2)));
        assert_eq!(Decimal::parse("-12.50"), Some(Decimal::new(-125, 1)));
        assert_eq!(Decimal::parse("7"), Some(Decimal::new(7, 0)));

        for invalid in ["", "-", ".5", "5.", "1e-3", "1,5", "abc", "+1", "1.2.3"] {
            assert_eq!(Decimal::parse(invalid), None, "{invalid:?}");
        }
    }

    #[test]
    fn decimal_displays_in_plain_notation() {
        for text in ["0", "7", "-0.05", "12.5", "1500", "-2750.6"] {
            assert_eq!(Decimal::parse(text).unwrap().to_string(), text);
        }
    }

    #[test]
    fn decimal_subtraction_aligns_scales() {
        let difference = Decimal::parse("2750.60")
            .unwrap()
            .checked_sub(Decimal::parse("2750.5").unwrap());
        assert_eq!(difference, Decimal::parse("0.1"));
    }

    #[test]
    fn decimal_arithmetic_reports_overflow_instead_of_wrapping() {
        let huge = Decimal::new(i128::MAX, 0);
        assert_eq!(huge.checked_sub(Decimal::new(-1, 0)), None);
        assert_eq!(huge.checked_sub(Decimal::new(1, 30)), None);
    }

    #[test]
    fn nan_equals_nan_and_negative_zero_equals_zero() {
        assert_eq!(
            NormalizedFloat::new(f64::NAN),
            NormalizedFloat::new(-f64::NAN)
        );
        assert_eq!(NormalizedFloat::new(-0.0), NormalizedFloat::new(0.0));
        assert_ne!(
            NormalizedFloat::new(f64::INFINITY),
            NormalizedFloat::new(f64::NEG_INFINITY)
        );
    }

    #[test]
    fn float32_and_float64_are_equal_when_the_number_is_equal() {
        assert_eq!(
            values(Float32Array::from(vec![0.5])),
            values(Float64Array::from(vec![0.5]))
        );
    }

    #[test]
    fn timestamps_with_a_zone_compare_as_instants_across_units() {
        let millis = TimestampMillisecondArray::from(vec![1_500]).with_timezone("UTC");
        let micros = TimestampMicrosecondArray::from(vec![1_500_000]).with_timezone("+10:00");
        assert_eq!(values(millis), values(micros));
    }

    #[test]
    fn a_naive_timestamp_never_equals_an_instant() {
        let naive = TimestampMicrosecondArray::from(vec![1_500_000]);
        let instant = TimestampMicrosecondArray::from(vec![1_500_000]).with_timezone("UTC");
        assert_ne!(values(naive), values(instant));
    }

    #[test]
    fn kind_and_conversion_agree_on_which_types_are_supported() {
        let data_types = [
            DataType::Boolean,
            DataType::Int8,
            DataType::Int16,
            DataType::Int32,
            DataType::Int64,
            DataType::UInt8,
            DataType::UInt16,
            DataType::UInt32,
            DataType::UInt64,
            DataType::Float16,
            DataType::Float32,
            DataType::Float64,
            DataType::Decimal128(18, 2),
            DataType::Decimal256(40, 2),
            DataType::Utf8,
            DataType::LargeUtf8,
            DataType::Utf8View,
            DataType::Binary,
            DataType::LargeBinary,
            DataType::BinaryView,
            DataType::FixedSizeBinary(4),
            DataType::Date32,
            DataType::Date64,
            DataType::Time64(TimeUnit::Microsecond),
            DataType::Timestamp(TimeUnit::Second, None),
            DataType::Timestamp(TimeUnit::Millisecond, None),
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            DataType::Timestamp(TimeUnit::Nanosecond, Some("UTC".into())),
            DataType::Duration(TimeUnit::Second),
            DataType::new_list(DataType::Int32, true),
        ];

        for data_type in data_types {
            let array: ArrayRef = new_null_array(&data_type, 1);
            let converted = column_values(&Arc::clone(&array));
            assert_eq!(
                ValueKind::of(&data_type).is_some(),
                converted.is_some(),
                "ValueKind::of and column_values disagree on {data_type}"
            );
        }
    }
}
