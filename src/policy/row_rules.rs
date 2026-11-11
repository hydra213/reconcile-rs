//! Row rules: judging the counts produced by the row comparison.
//!
//! Each [`RowCheck`] looks at one number, compares it with the number the
//! policy allows, and gives a verdict. All of that happens in
//! [`evaluate_rows`], one line per check.

use serde::{Deserialize, Serialize};

use super::Verdict;
use crate::rowdiff::RowCounts;

/// The `[policy]` section of the configuration. Every default is the strict
/// choice, so a missing setting can never loosen a comparison.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RowPolicy {
    /// Fail when a key exists only on the left side. Otherwise warn.
    pub fail_on_missing_keys: bool,
    /// Fail when a key exists only on the right side. Otherwise warn.
    pub fail_on_extra_keys: bool,
    /// Fail when a key appears on more than one row. Otherwise warn.
    pub fail_on_duplicate_keys: bool,
    /// Fail when a row has NULL in a key column. Otherwise warn.
    pub fail_on_null_keys: bool,
    /// Fail when more than this many matched rows have changed values.
    pub max_changed_rows: u64,
    /// Give an inconclusive verdict when a column could not be compared.
    /// Otherwise warn.
    pub strict_on_inconclusive: bool,
}

impl Default for RowPolicy {
    fn default() -> Self {
        Self {
            fail_on_missing_keys: true,
            fail_on_extra_keys: true,
            fail_on_duplicate_keys: true,
            fail_on_null_keys: true,
            max_changed_rows: 0,
            strict_on_inconclusive: true,
        }
    }
}

/// The things the row policy checks. Appears in reports as the check name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RowCheck {
    MissingKeys,
    ExtraKeys,
    DuplicateKeys,
    NullKeys,
    ChangedRows,
    InconclusiveColumns,
}

impl RowCheck {
    /// What the check counts, phrased to follow a number.
    fn counted_thing(self) -> &'static str {
        match self {
            Self::MissingKeys => "key(s) exist only on the left side",
            Self::ExtraKeys => "key(s) exist only on the right side",
            Self::DuplicateKeys => "key(s) appear on more than one row",
            Self::NullKeys => "row(s) have NULL in a key column",
            Self::ChangedRows => "matched row(s) have changed values",
            Self::InconclusiveColumns => "column(s) could not be compared",
        }
    }
}

/// The outcome of one check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RowFinding {
    pub check: RowCheck,
    /// The number that was observed in the data.
    pub observed: u64,
    /// The largest number the policy accepts as a pass.
    pub allowed: u64,
    pub verdict: Verdict,
    pub explanation: String,
}

/// Runs every row check. The result always has one finding per check, in a
/// fixed order, including the checks that passed.
pub fn evaluate_rows(
    counts: &RowCounts,
    inconclusive_columns: u64,
    policy: &RowPolicy,
) -> Vec<RowFinding> {
    vec![
        judge(
            RowCheck::MissingKeys,
            counts.left_only_keys,
            0,
            fail_or_warn(policy.fail_on_missing_keys),
            format!("fail_on_missing_keys = {}", policy.fail_on_missing_keys),
        ),
        judge(
            RowCheck::ExtraKeys,
            counts.right_only_keys,
            0,
            fail_or_warn(policy.fail_on_extra_keys),
            format!("fail_on_extra_keys = {}", policy.fail_on_extra_keys),
        ),
        judge(
            RowCheck::DuplicateKeys,
            counts.duplicate_keys_left + counts.duplicate_keys_right,
            0,
            fail_or_warn(policy.fail_on_duplicate_keys),
            format!("fail_on_duplicate_keys = {}", policy.fail_on_duplicate_keys),
        ),
        judge(
            RowCheck::NullKeys,
            counts.null_key_rows_left + counts.null_key_rows_right,
            0,
            fail_or_warn(policy.fail_on_null_keys),
            format!("fail_on_null_keys = {}", policy.fail_on_null_keys),
        ),
        judge(
            RowCheck::ChangedRows,
            counts.changed_rows,
            policy.max_changed_rows,
            Verdict::Fail,
            format!("max_changed_rows = {}", policy.max_changed_rows),
        ),
        judge(
            RowCheck::InconclusiveColumns,
            inconclusive_columns,
            0,
            if policy.strict_on_inconclusive {
                Verdict::Inconclusive
            } else {
                Verdict::Warn
            },
            format!("strict_on_inconclusive = {}", policy.strict_on_inconclusive),
        ),
    ]
}

fn fail_or_warn(fail: bool) -> Verdict {
    if fail { Verdict::Fail } else { Verdict::Warn }
}

/// Builds one finding: a pass when `observed` is within `allowed`, otherwise
/// `verdict_when_exceeded`. `setting` names the configuration value that
/// decided it, so the explanation points at what to change.
fn judge(
    check: RowCheck,
    observed: u64,
    allowed: u64,
    verdict_when_exceeded: Verdict,
    setting: String,
) -> RowFinding {
    let verdict = if observed <= allowed {
        Verdict::Pass
    } else {
        verdict_when_exceeded
    };
    let explanation = format!(
        "{observed} {}; at most {allowed} allowed ({setting}).",
        check.counted_thing()
    );

    RowFinding {
        check,
        observed,
        allowed,
        verdict,
        explanation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verdict_of(findings: &[RowFinding], check: RowCheck) -> Verdict {
        findings
            .iter()
            .find(|finding| finding.check == check)
            .expect("every check has a finding")
            .verdict
    }

    #[test]
    fn clean_counts_pass_every_check() {
        let findings = evaluate_rows(&RowCounts::default(), 0, &RowPolicy::default());
        assert_eq!(findings.len(), 6);
        assert!(findings.iter().all(|f| f.verdict == Verdict::Pass));
    }

    #[test]
    fn default_policy_fails_each_kind_of_key_problem() {
        let counts = RowCounts {
            left_only_keys: 1,
            right_only_keys: 1,
            duplicate_keys_right: 1,
            null_key_rows_left: 1,
            changed_rows: 1,
            ..RowCounts::default()
        };
        let findings = evaluate_rows(&counts, 0, &RowPolicy::default());

        for check in [
            RowCheck::MissingKeys,
            RowCheck::ExtraKeys,
            RowCheck::DuplicateKeys,
            RowCheck::NullKeys,
            RowCheck::ChangedRows,
        ] {
            assert_eq!(verdict_of(&findings, check), Verdict::Fail, "{check:?}");
        }
    }

    #[test]
    fn a_relaxed_setting_turns_a_fail_into_a_warning() {
        let counts = RowCounts {
            right_only_keys: 3,
            ..RowCounts::default()
        };
        let policy = RowPolicy {
            fail_on_extra_keys: false,
            ..RowPolicy::default()
        };
        let findings = evaluate_rows(&counts, 0, &policy);
        assert_eq!(verdict_of(&findings, RowCheck::ExtraKeys), Verdict::Warn);
    }

    #[test]
    fn changed_rows_pass_up_to_the_allowed_number() {
        let counts = RowCounts {
            changed_rows: 2,
            ..RowCounts::default()
        };
        let allows_two = RowPolicy {
            max_changed_rows: 2,
            ..RowPolicy::default()
        };
        let allows_one = RowPolicy {
            max_changed_rows: 1,
            ..RowPolicy::default()
        };
        assert_eq!(
            verdict_of(
                &evaluate_rows(&counts, 0, &allows_two),
                RowCheck::ChangedRows
            ),
            Verdict::Pass
        );
        assert_eq!(
            verdict_of(
                &evaluate_rows(&counts, 0, &allows_one),
                RowCheck::ChangedRows
            ),
            Verdict::Fail
        );
    }

    #[test]
    fn an_uncomparable_column_is_inconclusive_when_strict_and_a_warning_otherwise() {
        let strict = RowPolicy::default();
        let relaxed = RowPolicy {
            strict_on_inconclusive: false,
            ..RowPolicy::default()
        };
        let counts = RowCounts::default();
        assert_eq!(
            verdict_of(
                &evaluate_rows(&counts, 1, &strict),
                RowCheck::InconclusiveColumns
            ),
            Verdict::Inconclusive
        );
        assert_eq!(
            verdict_of(
                &evaluate_rows(&counts, 1, &relaxed),
                RowCheck::InconclusiveColumns
            ),
            Verdict::Warn
        );
    }
}
