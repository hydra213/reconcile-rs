"""Comparing PySpark dataframes. Skipped when PySpark is not installed,
because it needs a Java runtime."""

import pytest

import reconcilers

pyspark = pytest.importorskip("pyspark")


@pytest.fixture(scope="module")
def spark():
    from pyspark.sql import SparkSession

    session = (
        SparkSession.builder.master("local[1]")
        .appName("reconcilers-tests")
        .config("spark.ui.enabled", "false")
        .getOrCreate()
    )
    yield session
    session.stop()


def test_pyspark_dataframes(spark):
    left = spark.createDataFrame([(1, "ann", 100), (2, "bob", 200)], "id long, owner string, balance long")
    right = spark.createDataFrame([(1, "ann", 100), (2, "bob", 250)], "id long, owner string, balance long")

    assert reconcilers.compare(left, left, keys=["id"]).verdict == "pass"
    result = reconcilers.compare(left, right, keys=["id"])
    assert result.verdict == "fail"
    assert result.report["rows"]["counts"]["changed_rows"] == 1


def test_pyspark_against_pyarrow(spark, accounts):
    dataframe = spark.createDataFrame(accounts.to_pandas())

    result = reconcilers.rowdiff(dataframe, accounts, keys=["id"])

    assert result.verdict == "pass", str(result)
