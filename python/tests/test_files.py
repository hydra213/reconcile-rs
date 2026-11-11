"""Comparing Parquet files, using the sample files of the Rust test suite.
The expected numbers are documented in tests/fixtures/README.md."""

import sys

import pyarrow
import pytest

import reconcilers
from conftest import EXAMPLE_CONFIG, LEGACY, NEW

needs_tomllib = pytest.mark.skipif(
    sys.version_info < (3, 11), reason="reading TOML needs Python 3.11"
)


def test_a_file_compared_with_itself_passes():
    result = reconcilers.compare(LEGACY, LEGACY, keys=["trade_id"])

    assert result.verdict == "pass"
    assert result.report["fingerprints"]["content_matches"] is True


def test_schema_reports_the_sample_differences():
    result = reconcilers.schema(LEGACY, NEW, policy="compatible")

    assert result.verdict == "fail"
    assert "rows" not in result.report
    verdict_by_column = {
        finding["column"]: finding["verdict"]
        for finding in result.report["schema"]["findings"]
    }
    assert verdict_by_column == {
        "desk": "warn",
        "price": "fail",
        "quantity": "warn",
        "trade_amount": "warn",
        "trade_amt": "fail",
    }


@needs_tomllib
def test_compare_with_a_toml_file_matches_the_documented_counts():
    result = reconcilers.compare(LEGACY, NEW, config=EXAMPLE_CONFIG)

    counts = result.report["rows"]["counts"]
    assert result.verdict == "fail"
    assert counts["matched_keys"] == 4
    assert counts["changed_rows"] == 1
    assert counts["left_only_keys"] == 1
    assert counts["right_only_keys"] == 1


def test_rowdiff_has_no_schema_section():
    result = reconcilers.rowdiff(str(LEGACY), str(LEGACY), keys=["trade_id"])

    assert result.verdict == "pass"
    assert "schema" not in result.report
    assert "fingerprints" not in result.report


def test_several_files_form_one_dataset():
    result = reconcilers.rowdiff([LEGACY, LEGACY], LEGACY, keys=["trade_id"])

    # Every key now appears twice on the left.
    assert result.verdict == "fail"
    assert result.report["rows"]["counts"]["duplicate_keys_left"] == 5


def test_a_missing_file_is_an_error_not_a_finding():
    with pytest.raises(reconcilers.ReconcileError, match="cannot open"):
        reconcilers.compare("does_not_exist.parquet", NEW, keys=["trade_id"])


def test_csv_files_are_read_with_declared_types(tmp_path):
    left = tmp_path / "left.csv"
    right = tmp_path / "right.csv"
    left.write_text("id,amount\n1,10.50\n2,20.00\n")
    right.write_text("id,amount\n1,10.50\n2,20.01\n")
    config = {
        "comparison": {"keys": ["id"]},
        "csv": {"types": {"id": "int64", "amount": "decimal(18, 2)"}},
    }

    result = reconcilers.compare(left, right, config=config)

    assert result.verdict == "fail"
    assert result.report["rows"]["counts"]["changed_rows"] == 1


def test_a_csv_file_against_a_dataframe(tmp_path, accounts):
    file = tmp_path / "accounts.csv"
    file.write_text("id,owner,balance\n1,ann,100\n2,bob,200\n3,cat,300\n")
    config = {
        "comparison": {"keys": ["id"]},
        "csv": {"types": {"id": "int64", "balance": "int64"}},
    }

    result = reconcilers.compare(file, accounts, config=config)

    assert result.verdict == "pass", str(result)


def test_a_folder_is_read_as_all_its_data_files(tmp_path):
    (tmp_path / "part-0.csv").write_text("id,owner\n1,ann\n")
    (tmp_path / "part-1.csv").write_text("id,owner\n2,bob\n")
    (tmp_path / "_SUCCESS").write_text("")
    whole = pyarrow.table({"id": ["1", "2"], "owner": ["ann", "bob"]})

    result = reconcilers.compare(tmp_path, whole, keys=["id"])

    assert result.verdict == "pass", str(result)


def test_a_url_is_read_through_object_storage():
    result = reconcilers.rowdiff(LEGACY.as_uri(), LEGACY, keys=["trade_id"])

    assert result.verdict == "pass"
