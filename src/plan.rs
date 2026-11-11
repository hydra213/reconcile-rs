//! The comparison plan: what exactly will be compared.
//!
//! A plan is built from the configuration and the two schemas *before* any
//! row is read. Building it is where the configuration is checked against
//! the data: every key must exist and be usable, every mapping must point at
//! real columns. After that, the plan is the single place that says which
//! columns are keys, which are compared, and which cannot be compared.

use std::collections::BTreeMap;
use std::fmt;

use crate::config::{Config, ConfigError, Side};
use crate::schema::{Column, LogicalSchema};
use crate::table::ColumnToLoad;
use crate::value::ValueKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComparisonPlan {
    /// The left schema with column mappings applied, so both schemas use the
    /// same (right-side) names for the same logical column.
    pub left_schema: LogicalSchema,
    pub right_schema: LogicalSchema,
    /// The key columns, in the configured order.
    pub keys: Vec<String>,
    /// Non-key columns present on both sides with comparable types, sorted.
    pub compared_columns: Vec<String>,
    /// Non-key columns present on both sides that cannot be compared, sorted.
    pub inconclusive_columns: Vec<InconclusiveColumn>,
    /// The columns to read from each side: the keys and the compared columns.
    pub left_columns_to_load: Vec<ColumnToLoad>,
    pub right_columns_to_load: Vec<ColumnToLoad>,
}

/// A column that exists on both sides but cannot be compared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InconclusiveColumn {
    pub column: String,
    pub reason: InconclusiveReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InconclusiveReason {
    /// The engine has no comparison rule for this type.
    UnsupportedType { side: Side, data_type: String },
    /// Both types are supported, but they are different kinds of value.
    KindsDiffer { left: ValueKind, right: ValueKind },
}

impl fmt::Display for InconclusiveReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedType { side, data_type } => {
                write!(f, "the {side} type {data_type} is not supported yet")
            }
            Self::KindsDiffer { left, right } => write!(
                f,
                "the left side is a {left} and the right side is a {right}; \
                 comparing them needs an explicit rule"
            ),
        }
    }
}

impl ComparisonPlan {
    /// Builds the plan, or explains why the configuration cannot be used
    /// with these two schemas.
    pub fn new(
        config: &Config,
        left_schema: &LogicalSchema,
        right_schema: &LogicalSchema,
    ) -> Result<Self, ConfigError> {
        config.validate()?;

        let left_schema = apply_column_mappings(config, left_schema, right_schema)?;
        let right_schema = right_schema.clone();

        for key in &config.comparison.keys {
            check_key(key, &left_schema, &right_schema)?;
        }
        let keys = config.comparison.keys.clone();

        let mut compared_columns = Vec::new();
        let mut inconclusive_columns = Vec::new();
        for left_column in left_schema.columns() {
            if keys.contains(&left_column.name) {
                continue;
            }
            // A column on one side only is a schema finding, not a row one.
            let Some(right_column) = right_schema.column(&left_column.name) else {
                continue;
            };

            match why_inconclusive(left_column, right_column) {
                None => compared_columns.push(left_column.name.clone()),
                Some(reason) => inconclusive_columns.push(InconclusiveColumn {
                    column: left_column.name.clone(),
                    reason,
                }),
            }
        }

        // On the left, a mapped column is stored under its original name.
        let left_physical_names: BTreeMap<&str, &str> = config
            .column_mappings
            .iter()
            .map(|mapping| (mapping.right.as_str(), mapping.left.as_str()))
            .collect();

        let loaded_columns = keys.iter().chain(&compared_columns);
        let left_columns_to_load = loaded_columns
            .clone()
            .map(|logical_name| ColumnToLoad {
                physical_name: left_physical_names
                    .get(logical_name.as_str())
                    .unwrap_or(&logical_name.as_str())
                    .to_string(),
                logical_name: logical_name.clone(),
            })
            .collect();
        let right_columns_to_load = loaded_columns
            .map(|logical_name| ColumnToLoad {
                physical_name: logical_name.clone(),
                logical_name: logical_name.clone(),
            })
            .collect();

        Ok(Self {
            left_schema,
            right_schema,
            keys,
            compared_columns,
            inconclusive_columns,
            left_columns_to_load,
            right_columns_to_load,
        })
    }
}

/// Renames the mapped columns of the left schema to their right-side names.
fn apply_column_mappings(
    config: &Config,
    left_schema: &LogicalSchema,
    right_schema: &LogicalSchema,
) -> Result<LogicalSchema, ConfigError> {
    let mut new_names = BTreeMap::new();
    for mapping in &config.column_mappings {
        if left_schema.column(&mapping.left).is_none() {
            return Err(ConfigError::MappedColumnNotFound {
                side: Side::Left,
                column: mapping.left.clone(),
            });
        }
        if right_schema.column(&mapping.right).is_none() {
            return Err(ConfigError::MappedColumnNotFound {
                side: Side::Right,
                column: mapping.right.clone(),
            });
        }
        new_names.insert(mapping.left.clone(), mapping.right.clone());
    }

    left_schema
        .with_renamed_columns(&new_names)
        .map_err(|duplicate| {
            let mapping = config
                .column_mappings
                .iter()
                .find(|mapping| mapping.right == duplicate.column)
                .expect("a collision can only be caused by one of the mappings");
            ConfigError::MappingCollides {
                left: mapping.left.clone(),
                right: mapping.right.clone(),
            }
        })
}

/// Checks that a key column exists on both sides with matching, usable types.
fn check_key(
    key: &str,
    left_schema: &LogicalSchema,
    right_schema: &LogicalSchema,
) -> Result<(), ConfigError> {
    let left_kind = key_kind(key, Side::Left, left_schema)?;
    let right_kind = key_kind(key, Side::Right, right_schema)?;

    if left_kind != right_kind {
        return Err(ConfigError::KeyKindsDiffer {
            column: key.to_string(),
            left_kind: left_kind.to_string(),
            right_kind: right_kind.to_string(),
        });
    }
    Ok(())
}

fn key_kind(key: &str, side: Side, schema: &LogicalSchema) -> Result<ValueKind, ConfigError> {
    let column = schema.column(key).ok_or_else(|| ConfigError::KeyNotFound {
        side,
        column: key.to_string(),
    })?;

    match ValueKind::of(&column.data_type) {
        Some(kind) if kind.can_be_key() => Ok(kind),
        _ => Err(ConfigError::KeyTypeNotUsable {
            side,
            column: key.to_string(),
            data_type: column.data_type.to_string(),
        }),
    }
}

/// Returns the reason two columns cannot be compared, or `None` if they can.
fn why_inconclusive(left: &Column, right: &Column) -> Option<InconclusiveReason> {
    let Some(left_kind) = ValueKind::of(&left.data_type) else {
        return Some(InconclusiveReason::UnsupportedType {
            side: Side::Left,
            data_type: left.data_type.to_string(),
        });
    };
    let Some(right_kind) = ValueKind::of(&right.data_type) else {
        return Some(InconclusiveReason::UnsupportedType {
            side: Side::Right,
            data_type: right.data_type.to_string(),
        });
    };

    if left_kind != right_kind {
        return Some(InconclusiveReason::KindsDiffer {
            left: left_kind,
            right: right_kind,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ColumnMapping, ComparisonConfig};
    use crate::policy::{RowPolicy, SchemaPolicy};
    use arrow::datatypes::{DataType, Field, Schema};

    fn schema(fields: Vec<(&str, DataType)>) -> LogicalSchema {
        let fields: Vec<Field> = fields
            .into_iter()
            .map(|(name, data_type)| Field::new(name, data_type, true))
            .collect();
        LogicalSchema::from_arrow(&Schema::new(fields)).unwrap()
    }

    fn config(keys: &[&str], mappings: &[(&str, &str)]) -> Config {
        Config {
            comparison: ComparisonConfig {
                keys: keys.iter().map(|key| key.to_string()).collect(),
                schema_policy: SchemaPolicy::Strict,
            },
            column_mappings: mappings
                .iter()
                .map(|(left, right)| ColumnMapping {
                    left: left.to_string(),
                    right: right.to_string(),
                })
                .collect(),
            policy: RowPolicy::default(),
        }
    }

    #[test]
    fn columns_are_sorted_into_keys_compared_and_inconclusive() {
        let left = schema(vec![
            ("id", DataType::Int32),
            ("amount", DataType::Decimal128(18, 2)),
            ("price", DataType::Decimal128(18, 4)),
            ("tags", DataType::new_list(DataType::Utf8, true)),
            ("left_only", DataType::Utf8),
        ]);
        let right = schema(vec![
            ("id", DataType::Int64),
            ("amount", DataType::Decimal128(20, 4)),
            ("price", DataType::Float64),
            ("tags", DataType::new_list(DataType::Utf8, true)),
            ("right_only", DataType::Utf8),
        ]);

        let plan = ComparisonPlan::new(&config(&["id"], &[]), &left, &right).unwrap();

        assert_eq!(plan.keys, vec!["id"]);
        assert_eq!(plan.compared_columns, vec!["amount"]);
        assert_eq!(
            plan.inconclusive_columns,
            vec![
                InconclusiveColumn {
                    column: "price".to_string(),
                    reason: InconclusiveReason::KindsDiffer {
                        left: ValueKind::Decimal,
                        right: ValueKind::Float,
                    },
                },
                InconclusiveColumn {
                    column: "tags".to_string(),
                    reason: InconclusiveReason::UnsupportedType {
                        side: Side::Left,
                        data_type: DataType::new_list(DataType::Utf8, true).to_string(),
                    },
                },
            ]
        );
    }

    #[test]
    fn a_mapped_column_is_loaded_by_its_left_name_and_known_by_its_right_name() {
        let left = schema(vec![
            ("id", DataType::Int64),
            ("trade_amt", DataType::Int64),
        ]);
        let right = schema(vec![
            ("id", DataType::Int64),
            ("trade_amount", DataType::Int64),
        ]);

        let plan = ComparisonPlan::new(
            &config(&["id"], &[("trade_amt", "trade_amount")]),
            &left,
            &right,
        )
        .unwrap();

        assert_eq!(plan.compared_columns, vec!["trade_amount"]);
        assert!(plan.left_schema.column("trade_amount").is_some());
        assert!(plan.left_columns_to_load.contains(&ColumnToLoad {
            physical_name: "trade_amt".to_string(),
            logical_name: "trade_amount".to_string(),
        }));
    }

    #[test]
    fn a_missing_key_is_rejected() {
        let left = schema(vec![("id", DataType::Int64)]);
        let right = schema(vec![("other", DataType::Int64)]);

        assert_eq!(
            ComparisonPlan::new(&config(&["id"], &[]), &left, &right),
            Err(ConfigError::KeyNotFound {
                side: Side::Right,
                column: "id".to_string()
            })
        );
    }

    #[test]
    fn a_float_key_is_rejected() {
        let both = schema(vec![("id", DataType::Float64)]);

        assert_eq!(
            ComparisonPlan::new(&config(&["id"], &[]), &both, &both),
            Err(ConfigError::KeyTypeNotUsable {
                side: Side::Left,
                column: "id".to_string(),
                data_type: "Float64".to_string(),
            })
        );
    }

    #[test]
    fn a_key_with_different_kinds_on_each_side_is_rejected() {
        let left = schema(vec![("id", DataType::Int64)]);
        let right = schema(vec![("id", DataType::Utf8)]);

        assert_eq!(
            ComparisonPlan::new(&config(&["id"], &[]), &left, &right),
            Err(ConfigError::KeyKindsDiffer {
                column: "id".to_string(),
                left_kind: "integer".to_string(),
                right_kind: "string".to_string(),
            })
        );
    }

    #[test]
    fn a_mapping_to_a_missing_column_is_rejected() {
        let both = schema(vec![("id", DataType::Int64)]);

        assert_eq!(
            ComparisonPlan::new(&config(&["id"], &[("nope", "id")]), &both, &both),
            Err(ConfigError::MappedColumnNotFound {
                side: Side::Left,
                column: "nope".to_string()
            })
        );
    }

    #[test]
    fn a_mapping_onto_an_existing_left_column_is_rejected() {
        let left = schema(vec![
            ("id", DataType::Int64),
            ("old", DataType::Int64),
            ("new", DataType::Int64),
        ]);
        let right = schema(vec![("id", DataType::Int64), ("new", DataType::Int64)]);

        assert_eq!(
            ComparisonPlan::new(&config(&["id"], &[("old", "new")]), &left, &right),
            Err(ConfigError::MappingCollides {
                left: "old".to_string(),
                right: "new".to_string()
            })
        );
    }
}
