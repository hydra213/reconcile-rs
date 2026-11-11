//! The reconciliation configuration, usually loaded from `reconcile.toml`.
//!
//! Loading checks everything that can be checked without looking at the
//! data, such as an empty key list. Checks that need the schemas, such as
//! "does the key column exist", happen in [`crate::plan`].

use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use serde::Deserialize;

use crate::error::Error;
use crate::policy::{RowPolicy, SchemaPolicy};

/// The whole configuration file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub comparison: ComparisonConfig,
    /// Columns that have different names on the two sides.
    #[serde(default)]
    pub column_mappings: Vec<ColumnMapping>,
    #[serde(default)]
    pub policy: RowPolicy,
}

/// The `[comparison]` section.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComparisonConfig {
    /// The columns that identify a row, in order. A mapped column is named
    /// by its right-side name.
    pub keys: Vec<String>,
    #[serde(default)]
    pub schema_policy: SchemaPolicy,
}

/// One `[[column_mappings]]` entry: the left column `left` is the same
/// logical column as the right column `right`.
///
/// After mapping, the column is known everywhere by its right-side name.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColumnMapping {
    pub left: String,
    pub right: String,
}

impl Config {
    /// Reads and validates a TOML configuration file.
    pub fn from_toml_file(path: &Path) -> Result<Self, Error> {
        let text = std::fs::read_to_string(path).map_err(|source| Error::ReadConfig {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_toml_str(&text)
    }

    /// Parses and validates TOML configuration text.
    pub fn from_toml_str(text: &str) -> Result<Self, Error> {
        let config: Self = toml::from_str(text).map_err(|source| Error::ParseConfig {
            source: Box::new(source),
        })?;
        config.validate()?;
        Ok(config)
    }

    /// Checks the rules that do not depend on the data.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.comparison.keys.is_empty() {
            return Err(ConfigError::NoKeys);
        }
        if let Some(column) = first_repeated(self.comparison.keys.iter()) {
            return Err(ConfigError::RepeatedKey { column });
        }

        let left_names = self.column_mappings.iter().map(|mapping| &mapping.left);
        if let Some(column) = first_repeated(left_names) {
            return Err(ConfigError::RepeatedMapping {
                side: Side::Left,
                column,
            });
        }
        let right_names = self.column_mappings.iter().map(|mapping| &mapping.right);
        if let Some(column) = first_repeated(right_names) {
            return Err(ConfigError::RepeatedMapping {
                side: Side::Right,
                column,
            });
        }
        Ok(())
    }
}

/// The first name that appears twice, if any.
fn first_repeated<'a>(names: impl Iterator<Item = &'a String>) -> Option<String> {
    let mut seen = BTreeSet::new();
    for name in names {
        if !seen.insert(name) {
            return Some(name.clone());
        }
    }
    None
}

/// Which of the two datasets something refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The baseline.
    Left,
    /// The candidate.
    Right,
}

impl fmt::Display for Side {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Left => "left",
            Self::Right => "right",
        })
    }
}

/// A configuration that cannot be used, either on its own or with the given
/// datasets. Raised before any row is compared.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("`comparison.keys` is empty; at least one key column is required")]
    NoKeys,

    #[error("key column `{column}` is listed more than once")]
    RepeatedKey { column: String },

    #[error("{side} column `{column}` appears in more than one column mapping")]
    RepeatedMapping { side: Side, column: String },

    #[error("a column mapping names `{column}`, which is not a column of the {side} dataset")]
    MappedColumnNotFound { side: Side, column: String },

    #[error(
        "mapping `{left}` to `{right}` is ambiguous: the left dataset already has a column named `{right}`"
    )]
    MappingCollides { left: String, right: String },

    #[error("key column `{column}` does not exist in the {side} dataset")]
    KeyNotFound { side: Side, column: String },

    #[error(
        "key column `{column}` has type {data_type} in the {side} dataset, which cannot be used as a key"
    )]
    KeyTypeNotUsable {
        side: Side,
        column: String,
        data_type: String,
    },

    #[error(
        "key column `{column}` is a {left_kind} on the left and a {right_kind} on the right, so keys cannot be matched"
    )]
    KeyKindsDiffer {
        column: String,
        left_kind: String,
        right_kind: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_error(text: &str) -> ConfigError {
        match Config::from_toml_str(text) {
            Err(Error::Config(error)) => error,
            other => panic!("expected a configuration error, got {other:?}"),
        }
    }

    #[test]
    fn a_minimal_config_gets_strict_defaults() {
        let config = Config::from_toml_str("[comparison]\nkeys = [\"id\"]\n").unwrap();

        assert_eq!(config.comparison.keys, vec!["id".to_string()]);
        assert_eq!(config.comparison.schema_policy, SchemaPolicy::Strict);
        assert!(config.column_mappings.is_empty());
        assert_eq!(config.policy, RowPolicy::default());
    }

    #[test]
    fn a_full_config_is_read() {
        let config = Config::from_toml_str(
            r#"
            [comparison]
            keys = ["trade_id", "business_date"]
            schema_policy = "compatible"

            [[column_mappings]]
            left = "trade_amt"
            right = "trade_amount"

            [policy]
            fail_on_extra_keys = false
            max_changed_rows = 5
            "#,
        )
        .unwrap();

        assert_eq!(config.comparison.schema_policy, SchemaPolicy::Compatible);
        assert_eq!(
            config.column_mappings,
            vec![ColumnMapping {
                left: "trade_amt".to_string(),
                right: "trade_amount".to_string(),
            }]
        );
        assert!(!config.policy.fail_on_extra_keys);
        assert_eq!(config.policy.max_changed_rows, 5);
        // Settings that are not mentioned keep their strict default.
        assert!(config.policy.fail_on_missing_keys);
    }

    #[test]
    fn an_unknown_setting_is_rejected() {
        let result = Config::from_toml_str("[comparison]\nkeys = [\"id\"]\nkey = \"typo\"\n");
        assert!(matches!(result, Err(Error::ParseConfig { .. })));
    }

    #[test]
    fn an_empty_key_list_is_rejected() {
        assert_eq!(
            config_error("[comparison]\nkeys = []\n"),
            ConfigError::NoKeys
        );
    }

    #[test]
    fn a_repeated_key_is_rejected() {
        assert_eq!(
            config_error("[comparison]\nkeys = [\"id\", \"id\"]\n"),
            ConfigError::RepeatedKey {
                column: "id".to_string()
            }
        );
    }

    #[test]
    fn a_column_mapped_twice_is_rejected() {
        let error = config_error(
            r#"
            [comparison]
            keys = ["id"]

            [[column_mappings]]
            left = "a"
            right = "x"

            [[column_mappings]]
            left = "b"
            right = "x"
            "#,
        );
        assert_eq!(
            error,
            ConfigError::RepeatedMapping {
                side: Side::Right,
                column: "x".to_string()
            }
        );
    }
}
