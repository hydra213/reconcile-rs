//! Policies: turning observed facts into verdicts.
//!
//! Facts come from [`crate::schema`] and [`crate::rowdiff`]. This module
//! decides what they mean. Every verdict is tied to one named rule or check,
//! so a report can always say *why* something passed, warned or failed.
//!
//! - [`schema_rules`] judges schema differences.
//! - [`row_rules`] judges row and key counts.

pub mod row_rules;
pub mod schema_rules;

use std::fmt;

use serde::Serialize;

pub use row_rules::{RowCheck, RowFinding, RowPolicy, evaluate_rows};
pub use schema_rules::{
    SchemaFinding, SchemaPolicy, SchemaRule, evaluate_schema, overall_schema_verdict,
};

/// The outcome of a check, ordered from best to worst.
///
/// The order matters: the overall verdict of a report is the worst verdict
/// among its findings, found with `max()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// The check holds.
    Pass,
    /// A difference exists and the policy allows it. Worth a human look.
    Warn,
    /// The engine could not decide. Never treated as a pass.
    Inconclusive,
    /// The policy is violated.
    Fail,
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::Pass => "PASS",
            Self::Warn => "WARN",
            Self::Inconclusive => "INCONCLUSIVE",
            Self::Fail => "FAIL",
        };
        // `pad` honours width requests such as `{:<12}`, which keeps the
        // text report aligned.
        f.pad(text)
    }
}

/// The worst verdict in a list, or `Pass` when the list is empty.
pub fn worst_verdict(verdicts: impl IntoIterator<Item = Verdict>) -> Verdict {
    verdicts.into_iter().max().unwrap_or(Verdict::Pass)
}
