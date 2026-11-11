"""ReconcileRS: deterministic data reconciliation for files and dataframes.

Three functions, from narrow to complete:

- :func:`schema` compares the columns and their types only.
- :func:`rowdiff` compares the rows by key only.
- :func:`compare` does both and adds content fingerprints.

Each side can be a Parquet or CSV file, a folder, an object storage URL, a
list of those, or a dataframe (pandas, Polars, PySpark, pyarrow, or
anything else that exports Arrow)::

    import reconcilers

    result = reconcilers.compare("legacy.parquet", new_df, keys=["trade_id"])
    print(result)             # the report as text
    result.raise_if_failed()  # stop a pipeline on a failed reconciliation

The left side is the baseline and the right side is the candidate.
"""

from ._api import compare, rowdiff, schema
from ._native import ReconcileError, __version__
from ._result import ReconciliationFailed, Result

__all__ = [
    "compare",
    "rowdiff",
    "schema",
    "Result",
    "ReconcileError",
    "ReconciliationFailed",
    "__version__",
]
