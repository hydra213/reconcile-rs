"""Shared test data: the sample Parquet files of the Rust test suite, and a
small `accounts` table built by hand."""

from pathlib import Path

import pyarrow as pa
import pytest

REPOSITORY = Path(__file__).resolve().parents[2]
LEGACY = REPOSITORY / "tests" / "fixtures" / "legacy_trades.parquet"
NEW = REPOSITORY / "tests" / "fixtures" / "new_trades.parquet"
EXAMPLE_CONFIG = REPOSITORY / "examples" / "reconcile.toml"


@pytest.fixture
def accounts():
    """Three accounts as a pyarrow table."""
    return pa.table(
        {
            "id": [1, 2, 3],
            "owner": ["ann", "bob", "cat"],
            "balance": [100, 200, 300],
        }
    )


@pytest.fixture
def accounts_with_one_change():
    """The same accounts, but bob's balance is 250 instead of 200."""
    return pa.table(
        {
            "id": [1, 2, 3],
            "owner": ["ann", "bob", "cat"],
            "balance": [100, 250, 300],
        }
    )
