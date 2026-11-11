//! Numeric tolerances: how far apart two numbers may be and still count as
//! the same.
//!
//! A tolerance is always explicit, per column, and absolute. It is checked
//! only after two values have been found unequal, so a column without a
//! tolerance is never affected.

use crate::value::{Decimal, Value};

/// The largest absolute difference allowed between two values of a column.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tolerance {
    /// The limit as an exact decimal, used for integers and decimals.
    max_difference: Decimal,
    /// The same limit as a float, used for float columns.
    max_difference_float: f64,
}

impl Tolerance {
    /// Reads a limit such as `"0.01"`. Returns `None` unless the text is a
    /// plain, non-negative decimal number.
    pub fn parse(text: &str) -> Option<Self> {
        let max_difference = Decimal::parse(text)?;
        if max_difference.unscaled() < 0 {
            return None;
        }
        Some(Self {
            max_difference,
            max_difference_float: text.parse().ok()?,
        })
    }

    pub fn max_difference(&self) -> Decimal {
        self.max_difference
    }

    /// Whether two unequal values are close enough to count as the same.
    ///
    /// Only pairs of numbers of the same kind can be within tolerance. NULL
    /// against a number, NaN and infinities never are. Decimal and integer
    /// differences are computed exactly; if that would overflow 128 bits the
    /// answer is `false`, which reports a difference instead of hiding one.
    pub fn allows(&self, left: &Value, right: &Value) -> bool {
        match (left, right) {
            (Value::Decimal(left), Value::Decimal(right)) => self.allows_decimals(*left, *right),
            (Value::Integer(left), Value::Integer(right)) => {
                self.allows_decimals(Decimal::new(*left, 0), Decimal::new(*right, 0))
            }
            (Value::Float(left), Value::Float(right)) => {
                let (left, right) = (left.as_f64(), right.as_f64());
                left.is_finite()
                    && right.is_finite()
                    && (left - right).abs() <= self.max_difference_float
            }
            _ => false,
        }
    }

    fn allows_decimals(&self, left: Decimal, right: Decimal) -> bool {
        left.checked_sub(right)
            .and_then(Decimal::checked_abs)
            .and_then(|difference| difference.is_at_most(self.max_difference))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::NormalizedFloat;

    fn decimal(text: &str) -> Value {
        Value::Decimal(Decimal::parse(text).unwrap())
    }

    fn float(number: f64) -> Value {
        Value::Float(NormalizedFloat::new(number))
    }

    fn one_cent() -> Tolerance {
        Tolerance::parse("0.01").unwrap()
    }

    #[test]
    fn a_negative_or_malformed_limit_is_rejected() {
        assert!(Tolerance::parse("-0.01").is_none());
        assert!(Tolerance::parse("one cent").is_none());
        assert!(Tolerance::parse("1e-2").is_none());
        assert!(Tolerance::parse("0").is_some());
    }

    #[test]
    fn decimals_are_compared_exactly_at_the_boundary() {
        // Exactly one cent apart is allowed; a hair more is not.
        assert!(one_cent().allows(&decimal("10.00"), &decimal("10.01")));
        assert!(one_cent().allows(&decimal("10.01"), &decimal("10.00")));
        assert!(!one_cent().allows(&decimal("10.00"), &decimal("10.0101")));
    }

    #[test]
    fn integers_use_the_same_exact_rule() {
        let within_two = Tolerance::parse("2").unwrap();
        assert!(within_two.allows(&Value::Integer(10), &Value::Integer(12)));
        assert!(!within_two.allows(&Value::Integer(10), &Value::Integer(13)));
    }

    #[test]
    fn floats_within_the_limit_are_allowed() {
        assert!(one_cent().allows(&float(1.000), &float(1.005)));
        assert!(!one_cent().allows(&float(1.00), &float(1.02)));
    }

    #[test]
    fn nan_and_infinity_are_never_within_tolerance() {
        assert!(!one_cent().allows(&float(f64::NAN), &float(1.0)));
        assert!(!one_cent().allows(&float(f64::INFINITY), &float(1.0)));
        assert!(!one_cent().allows(&float(f64::INFINITY), &float(f64::NEG_INFINITY)));
    }

    #[test]
    fn null_against_a_number_is_never_within_tolerance() {
        assert!(!one_cent().allows(&Value::Null, &decimal("0.00")));
    }

    #[test]
    fn different_kinds_are_never_within_tolerance() {
        assert!(!one_cent().allows(&Value::Integer(1), &decimal("1.00")));
    }

    #[test]
    fn overflow_is_treated_as_a_difference() {
        let huge = Value::Integer(i128::MAX);
        let negative = Value::Integer(-1);
        assert!(!one_cent().allows(&huge, &negative));
    }
}
