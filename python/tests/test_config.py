"""The `keys=` shortcut and the `config=` argument."""

import pytest

import reconcilers


def test_a_single_key_can_be_a_plain_string(accounts):
    assert reconcilers.compare(accounts, accounts, keys="id").verdict == "pass"


def test_keys_and_config_cannot_be_combined(accounts):
    with pytest.raises(TypeError, match="either keys"):
        reconcilers.compare(accounts, accounts, keys=["id"], config={})

    with pytest.raises(TypeError, match="either keys"):
        reconcilers.compare(accounts, accounts)


def test_a_dict_has_the_sections_of_the_toml_file(accounts, accounts_with_one_change):
    config = {
        "comparison": {"keys": ["id"]},
        "tolerances": [{"column": "balance", "max_difference": "50"}],
        "report": {"show_values": True},
    }

    result = reconcilers.compare(accounts, accounts_with_one_change, config=config)

    # 200 and 250 differ by 50, which the tolerance allows.
    assert result.verdict == "pass"
    balance = next(c for c in result.report["rows"]["columns"] if c["column"] == "balance")
    assert balance["within_tolerance"] == 1


def test_show_values_puts_real_values_in_the_report(accounts, accounts_with_one_change):
    hidden = reconcilers.compare(accounts, accounts_with_one_change, keys=["id"])
    shown = reconcilers.compare(
        accounts,
        accounts_with_one_change,
        config={"comparison": {"keys": ["id"]}, "report": {"show_values": True}},
    )

    assert "250" not in str(hidden)
    assert "`balance` 200 -> 250" in str(shown)


def test_an_unknown_setting_is_rejected(accounts):
    with pytest.raises(reconcilers.ReconcileError, match="unknown field `kyes`"):
        reconcilers.compare(accounts, accounts, config={"comparison": {"kyes": ["id"]}})


def test_a_key_that_is_not_a_column_is_rejected(accounts):
    with pytest.raises(reconcilers.ReconcileError, match="account_id"):
        reconcilers.compare(accounts, accounts, keys=["account_id"])


def test_config_of_the_wrong_type_is_rejected(accounts):
    with pytest.raises(TypeError, match="path to a TOML file or a dict"):
        reconcilers.compare(accounts, accounts, config=42)


def test_an_unknown_schema_policy_is_rejected(accounts):
    with pytest.raises(ValueError, match="policy must be one of"):
        reconcilers.schema(accounts, accounts, policy="lenient")
