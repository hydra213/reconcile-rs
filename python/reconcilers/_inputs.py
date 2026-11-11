"""Turning what the caller passed as one side of a comparison into
something the compiled library accepts.

The library accepts two things:

- a list of locations: paths or URLs of Parquet or CSV files or folders,
  which it reads itself, or
- an object that exports Arrow data through ``__arrow_c_stream__``, the
  standard way Python libraries hand tables to each other without copying.

pyarrow tables and Polars dataframes already have that method. pandas and
PySpark dataframes are converted to a pyarrow table first.
"""

import os


def to_source(data):
    """Return ``data`` as a list of locations or as an Arrow-exporting object."""
    if isinstance(data, (str, os.PathLike)):
        return [os.fspath(data)]

    if isinstance(data, (list, tuple)):
        if not data:
            raise ValueError("a dataset needs at least one file")
        if all(isinstance(item, (str, os.PathLike)) for item in data):
            return [os.fspath(item) for item in data]
        raise TypeError("a list must contain file paths or URLs only")

    # The library a dataframe comes from is recognised by its module name,
    # so that none of these libraries has to be installed or imported here.
    library = type(data).__module__.split(".")[0]

    if library == "pandas":
        return _from_pandas(data)
    if library == "pyspark":
        return _from_pyspark(data)
    if library == "polars" and hasattr(data, "collect"):
        # A LazyFrame is a query that has not run yet.
        data = data.collect()

    if hasattr(data, "__arrow_c_stream__"):
        return data

    raise TypeError(
        f"cannot read a dataset from {type(data).__name__}: expected a path or URL, "
        "a list of them, or a pandas, Polars, PySpark or pyarrow table"
    )


def _from_pandas(dataframe):
    try:
        import pyarrow
    except ModuleNotFoundError as error:
        raise ModuleNotFoundError(
            "comparing a pandas dataframe needs pyarrow: pip install pyarrow"
        ) from error

    # The index is not data that came from the source, so it is left out.
    # Call `dataframe.reset_index()` first if the index holds a real column.
    return pyarrow.Table.from_pandas(dataframe, preserve_index=False)


def _from_pyspark(dataframe):
    if not hasattr(dataframe, "toArrow"):
        raise TypeError(
            "comparing a PySpark dataframe needs PySpark 4.0 or later, which has "
            "DataFrame.toArrow(); on older versions pass dataframe.toPandas()"
        )
    # This collects the whole dataframe to the driver, so filter it down to
    # the rows being reconciled (for example one load date) first.
    return dataframe.toArrow()
