"""Comparing dataframes from different libraries with each other and with
files. The same data must compare equal wherever it comes from."""

import pyarrow as pa
import pyarrow.parquet
import pytest

import reconcilers


def test_pyarrow_tables(accounts, accounts_with_one_change):
    result = reconcilers.compare(accounts, accounts_with_one_change, keys=["id"])

    assert result.verdict == "fail"
    assert result.report["rows"]["counts"]["changed_rows"] == 1
    assert result.report["rows"]["counts"]["unchanged_rows"] == 2


def test_a_record_batch_reader(accounts):
    reader = pa.RecordBatchReader.from_batches(accounts.schema, accounts.to_batches())

    assert reconcilers.compare(reader, accounts, keys=["id"]).verdict == "pass"


def test_pandas(accounts, accounts_with_one_change):
    left = accounts.to_pandas()
    right = accounts_with_one_change.to_pandas()

    assert reconcilers.compare(left, left, keys=["id"]).verdict == "pass"
    assert reconcilers.compare(left, right, keys=["id"]).verdict == "fail"


def test_the_pandas_index_is_not_compared(accounts):
    reindexed = accounts.to_pandas().set_index(pa.array([7, 8, 9]).to_pandas())

    result = reconcilers.compare(accounts.to_pandas(), reindexed, keys=["id"])

    assert result.verdict == "pass"


def test_polars(accounts, accounts_with_one_change):
    polars = pytest.importorskip("polars")
    left = polars.from_arrow(accounts)
    right = polars.from_arrow(accounts_with_one_change)

    assert reconcilers.compare(left, left, keys=["id"]).verdict == "pass"
    assert reconcilers.compare(left, right, keys=["id"]).verdict == "fail"


def test_a_polars_lazyframe_is_collected(accounts):
    polars = pytest.importorskip("polars")
    lazy = polars.from_arrow(accounts).lazy()

    assert reconcilers.compare(lazy, accounts, keys=["id"]).verdict == "pass"


def test_pandas_against_polars_against_a_file(accounts, tmp_path):
    polars = pytest.importorskip("polars")
    file = tmp_path / "accounts.parquet"
    pyarrow.parquet.write_table(accounts, file)

    from_pandas = accounts.to_pandas()
    from_polars = polars.from_arrow(accounts)

    # Each library stores text differently in memory. That must not show up
    # as a schema difference or as different fingerprints.
    for left, right in [(file, from_pandas), (from_pandas, from_polars), (from_polars, file)]:
        result = reconcilers.compare(left, right, keys=["id"])
        assert result.verdict == "pass", str(result)
        assert result.report["fingerprints"]["content_matches"] is True


def test_schema_of_dataframes(accounts):
    without_balance = accounts.drop_columns(["balance"])

    result = reconcilers.schema(accounts, without_balance, policy="compatible")

    assert result.verdict == "fail"
    assert "`balance` is missing from the right side" in str(result)


def test_something_that_is_not_a_dataset_is_rejected(accounts):
    with pytest.raises(TypeError, match="cannot read a dataset from int"):
        reconcilers.compare(42, accounts, keys=["id"])

    with pytest.raises(TypeError, match="file paths or URLs only"):
        reconcilers.compare([accounts], accounts, keys=["id"])
