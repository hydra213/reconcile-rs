//! Schema rules: judging schema differences.
//!
//! To see how a schema difference is judged, read `choose_schema_rule` and
//! [`SchemaRule::verdict`]; there is no other place where it is decided.

use std::fmt;

use arrow::datatypes::DataType;
use serde::{Deserialize, Serialize};

use super::Verdict;
use crate::schema::SchemaDifference;

/// How strictly the right schema must match the left schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchemaPolicy {
    /// Any difference fails. This is the default.
    #[default]
    Strict,
    /// Changes that cannot lose or corrupt existing data are allowed with a
    /// warning. Everything else fails.
    Compatible,
}

impl fmt::Display for SchemaPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::Strict => "strict",
            Self::Compatible => "compatible",
        };
        f.write_str(text)
    }
}

/// The rule that judged a schema difference. Appears in reports as the
/// reason code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SchemaRule {
    StrictRequiresIdenticalSchema,
    ColumnRemoved,
    ColumnAdded,
    LosslessIntegerWidening,
    IncompatibleTypeChange,
    NullabilityRelaxed,
    NullabilityTightened,
}

impl SchemaRule {
    /// The verdict this rule always gives.
    pub fn verdict(self) -> Verdict {
        match self {
            Self::StrictRequiresIdenticalSchema => Verdict::Fail,
            Self::ColumnRemoved => Verdict::Fail,
            Self::ColumnAdded => Verdict::Warn,
            Self::LosslessIntegerWidening => Verdict::Warn,
            Self::IncompatibleTypeChange => Verdict::Fail,
            Self::NullabilityRelaxed => Verdict::Warn,
            Self::NullabilityTightened => Verdict::Pass,
        }
    }

    /// A one-sentence explanation for humans.
    pub fn explanation(self) -> &'static str {
        match self {
            Self::StrictRequiresIdenticalSchema => {
                "The strict policy requires both schemas to be identical."
            }
            Self::ColumnRemoved => {
                "A baseline column is missing from the candidate, so its data cannot be compared."
            }
            Self::ColumnAdded => {
                "A new column does not affect existing data, but it has no baseline to compare against."
            }
            Self::LosslessIntegerWidening => {
                "Every value of the baseline integer type fits in the candidate type."
            }
            Self::IncompatibleTypeChange => {
                "The type change is not on the list of lossless changes and needs an explicit decision."
            }
            Self::NullabilityRelaxed => {
                "The candidate column may now contain NULLs where the baseline could not."
            }
            Self::NullabilityTightened => {
                "The candidate column forbids NULLs, which is stricter than the baseline."
            }
        }
    }
}

/// A schema difference together with the rule that judged it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SchemaFinding {
    #[serde(flatten)]
    pub difference: SchemaDifference,
    pub rule: SchemaRule,
    pub verdict: Verdict,
    pub explanation: &'static str,
}

/// Judges every difference under the given policy. Keeps the input order.
pub fn evaluate_schema(
    differences: Vec<SchemaDifference>,
    policy: SchemaPolicy,
) -> Vec<SchemaFinding> {
    differences
        .into_iter()
        .map(|difference| {
            let rule = choose_schema_rule(&difference, policy);
            SchemaFinding {
                difference,
                rule,
                verdict: rule.verdict(),
                explanation: rule.explanation(),
            }
        })
        .collect()
}

/// The worst verdict among the findings, or `Pass` when there are none.
pub fn overall_schema_verdict(findings: &[SchemaFinding]) -> Verdict {
    super::worst_verdict(findings.iter().map(|finding| finding.verdict))
}

/// Picks the single rule that applies to a difference.
fn choose_schema_rule(difference: &SchemaDifference, policy: SchemaPolicy) -> SchemaRule {
    if policy == SchemaPolicy::Strict {
        return SchemaRule::StrictRequiresIdenticalSchema;
    }

    match difference {
        SchemaDifference::ColumnRemoved { .. } => SchemaRule::ColumnRemoved,
        SchemaDifference::ColumnAdded { .. } => SchemaRule::ColumnAdded,
        SchemaDifference::TypeChanged { left, right, .. } => {
            if is_lossless_integer_widening(left, right) {
                SchemaRule::LosslessIntegerWidening
            } else {
                SchemaRule::IncompatibleTypeChange
            }
        }
        SchemaDifference::NullabilityChanged { right_nullable, .. } => {
            if *right_nullable {
                SchemaRule::NullabilityRelaxed
            } else {
                SchemaRule::NullabilityTightened
            }
        }
    }
}

/// True when every value of integer type `from` fits in integer type `to`.
///
/// Examples: `Int32 -> Int64` and `UInt8 -> Int16` are lossless.
/// `Int64 -> Int32` (narrower), `Int32 -> UInt64` (loses negatives) and
/// `UInt32 -> Int32` (loses the top half) are not.
fn is_lossless_integer_widening(from: &DataType, to: &DataType) -> bool {
    let (Some(from), Some(to)) = (integer_shape(from), integer_shape(to)) else {
        return false;
    };

    match (from.signed, to.signed) {
        // Same signedness: the target only needs more bits.
        (true, true) | (false, false) => to.bits > from.bits,
        // Unsigned into signed: the sign bit costs one bit, so the target
        // must be strictly wider.
        (false, true) => to.bits > from.bits,
        // Signed into unsigned: negative values have nowhere to go.
        (true, false) => false,
    }
}

struct IntegerShape {
    signed: bool,
    bits: u8,
}

/// Describes an integer type, or returns `None` for any other type.
fn integer_shape(data_type: &DataType) -> Option<IntegerShape> {
    let (signed, bits) = match data_type {
        DataType::Int8 => (true, 8),
        DataType::Int16 => (true, 16),
        DataType::Int32 => (true, 32),
        DataType::Int64 => (true, 64),
        DataType::UInt8 => (false, 8),
        DataType::UInt16 => (false, 16),
        DataType::UInt32 => (false, 32),
        DataType::UInt64 => (false, 64),
        _ => return None,
    };
    Some(IntegerShape { signed, bits })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_change(left: DataType, right: DataType) -> SchemaDifference {
        SchemaDifference::TypeChanged {
            column: "c".to_string(),
            left,
            right,
        }
    }

    fn rule_for(difference: SchemaDifference, policy: SchemaPolicy) -> SchemaRule {
        evaluate_schema(vec![difference], policy)[0].rule
    }

    #[test]
    fn strict_fails_every_difference() {
        let added = SchemaDifference::ColumnAdded {
            column: "c".to_string(),
        };
        let finding = &evaluate_schema(vec![added], SchemaPolicy::Strict)[0];
        assert_eq!(finding.rule, SchemaRule::StrictRequiresIdenticalSchema);
        assert_eq!(finding.verdict, Verdict::Fail);
    }

    #[test]
    fn compatible_allows_lossless_integer_widening() {
        for (left, right) in [
            (DataType::Int32, DataType::Int64),
            (DataType::UInt8, DataType::UInt16),
            (DataType::UInt8, DataType::Int16),
        ] {
            assert_eq!(
                rule_for(type_change(left, right), SchemaPolicy::Compatible),
                SchemaRule::LosslessIntegerWidening
            );
        }
    }

    #[test]
    fn compatible_rejects_lossy_type_changes() {
        for (left, right) in [
            (DataType::Int64, DataType::Int32),
            (DataType::Int32, DataType::UInt64),
            (DataType::UInt32, DataType::Int32),
            (DataType::Decimal128(18, 4), DataType::Float64),
            (DataType::Int32, DataType::Utf8),
        ] {
            assert_eq!(
                rule_for(type_change(left, right), SchemaPolicy::Compatible),
                SchemaRule::IncompatibleTypeChange
            );
        }
    }

    #[test]
    fn compatible_distinguishes_relaxed_and_tightened_nullability() {
        let relaxed = SchemaDifference::NullabilityChanged {
            column: "c".to_string(),
            left_nullable: false,
            right_nullable: true,
        };
        let tightened = SchemaDifference::NullabilityChanged {
            column: "c".to_string(),
            left_nullable: true,
            right_nullable: false,
        };
        assert_eq!(
            rule_for(relaxed, SchemaPolicy::Compatible),
            SchemaRule::NullabilityRelaxed
        );
        assert_eq!(
            rule_for(tightened, SchemaPolicy::Compatible),
            SchemaRule::NullabilityTightened
        );
    }

    #[test]
    fn overall_verdict_is_the_worst_finding() {
        let findings = evaluate_schema(
            vec![
                SchemaDifference::ColumnAdded {
                    column: "a".to_string(),
                },
                SchemaDifference::ColumnRemoved {
                    column: "b".to_string(),
                },
            ],
            SchemaPolicy::Compatible,
        );
        assert_eq!(overall_schema_verdict(&findings), Verdict::Fail);
        assert_eq!(overall_schema_verdict(&[]), Verdict::Pass);
    }
}
