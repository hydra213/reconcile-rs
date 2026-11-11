"""The `Result` object."""

import json

import pytest

import reconcilers


def test_a_passing_result(accounts):
    result = reconcilers.compare(accounts, accounts, keys=["id"])

    assert result.passed
    assert repr(result) == "<reconcilers.Result verdict='pass'>"
    assert str(result).startswith("Verdict: PASS")
    assert result.to_markdown().startswith("# Reconciliation report")
    assert json.loads(result.to_json()) == result.report
    result.raise_if_failed()  # does nothing


def test_a_failing_result_can_stop_a_pipeline(accounts, accounts_with_one_change):
    result = reconcilers.compare(accounts, accounts_with_one_change, keys=["id"])

    assert not result.passed
    with pytest.raises(reconcilers.ReconciliationFailed) as raised:
        result.raise_if_failed()
    assert raised.value.result is result
    assert "reconciliation verdict: fail" in str(raised.value)


def test_the_report_is_identical_for_the_same_inputs(accounts, accounts_with_one_change):
    first = reconcilers.compare(accounts, accounts_with_one_change, keys=["id"])
    second = reconcilers.compare(accounts, accounts_with_one_change, keys=["id"])

    assert first.to_json() == second.to_json()


def test_notebooks_display_the_markdown_report(accounts):
    result = reconcilers.compare(accounts, accounts, keys=["id"])

    assert result._repr_markdown_() == result.to_markdown()
