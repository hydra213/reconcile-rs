# reconcilers (Python)

> Trust the migration, not the row count.

Compare two datasets and get an auditable answer: did the schema change,
which keys are missing or extra, and which values changed. Each side can be
Parquet or CSV files, on disk or in object storage, or a dataframe. The
same call checks an incoming file against what landed in Snowflake,
Databricks, a lake or a local folder.

The comparison runs in Rust. This package is a thin Python layer over the
[ReconcileRS](https://github.com/hydra213/reconcile-rs) library.

## Install

```bash
pip install reconcilers
```

Wheels are published for Linux, Windows and macOS, for Python 3.9 and
later. pandas dataframes also need `pyarrow` (`pip install "reconcilers[pandas]"`).

To build from a checkout of the repository instead (needs a
[Rust toolchain](https://rustup.rs)):

```bash
pip install maturin
cd python
maturin develop --release      # builds and installs into the active environment
```

## Use

```python
import reconcilers

result = reconcilers.compare("legacy.parquet", "new.parquet", keys=["trade_id"])

print(result)              # the report as text
result.verdict             # "pass", "warn", "inconclusive" or "fail"
result.passed              # True for pass and warn
result.report              # the full report as a dict
result.to_markdown()       # the report as Markdown
result.raise_if_failed()   # raises ReconciliationFailed unless it passed
```

The left side is the baseline and the right side is the candidate.

There are three functions, from narrow to complete:

| Function | What it does |
|---|---|
| `reconcilers.schema(left, right, policy="strict")` | Compares columns and types only. |
| `reconcilers.rowdiff(left, right, keys=[...])` | Compares rows by key only. Schemas are not judged. |
| `reconcilers.compare(left, right, keys=[...])` | Both, plus content fingerprints and one overall verdict. |

### What each side can be

| Input | How it is read |
|---|---|
| A path or URL, or a list of them | Parquet or CSV files, read by the Rust library. A folder means every data file directly inside it. URLs can be `s3://`, `abfss://`, `az://` or `gs://`; a URL that ends with `/` is a folder. |
| `pyarrow.Table`, `RecordBatchReader` | Used directly. |
| Polars `DataFrame` or `LazyFrame` | Used directly (a `LazyFrame` is collected). |
| pandas `DataFrame` | Converted with `pyarrow.Table.from_pandas`, without the index. |
| PySpark `DataFrame` | Collected with `toArrow()` (PySpark 4.0 or later). |
| Anything with `__arrow_c_stream__` | Used directly, for example a DuckDB result. |

The two sides do not have to be the same kind.

### A file against a warehouse table

Both sides are loaded into memory, so compare the increment, not the whole
table: today's file against today's partition.

```python
# Snowflake
landed = cursor.execute(
    "select * from landing.trades where load_date = %s", (load_date,)
).fetch_arrow_all()

# Databricks or any Spark session
landed = spark.table("landing.trades").where(f"load_date = '{load_date}'")

result = reconcilers.compare("incoming/trades.parquet", landed, keys=["trade_id"])
result.raise_if_failed()
```

### CSV files and object storage

A CSV column is text unless its type is declared. Types are never guessed.

```python
result = reconcilers.compare(
    "incoming/trades.csv",
    "s3://lake/landing/trades/load_date=2026-11-11/",
    config={
        "comparison": {"keys": ["trade_id"]},
        "csv": {"types": {"trade_id": "int64", "amount": "decimal(18, 2)"}},
    },
)
```

Object storage credentials come from the usual environment variables of
each cloud, or from the role or identity attached to the machine. The type
names and the other `[csv]` settings are listed in the
[main README](https://github.com/hydra213/reconcile-rs#inputs).

Do not point a URL at a Delta table's folder: it also holds files the table
no longer uses. Query the table and pass the result instead.

### Configuration

`keys=[...]` is the shortcut for the smallest configuration. For anything
else pass `config=`, either a path to a `reconcile.toml` file or a dict with
the same sections:

```python
result = reconcilers.compare(
    legacy_df,
    new_df,
    config={
        "comparison": {"keys": ["trade_id"], "schema_policy": "compatible"},
        "column_mappings": [{"left": "trade_amt", "right": "trade_amount"}],
        "tolerances": [{"column": "price", "max_difference": "0.01"}],
        "report": {"show_values": True},
    },
)
```

Every setting is described in
[`examples/reconcile.toml`](https://github.com/hydra213/reconcile-rs/blob/main/examples/reconcile.toml).
Unknown settings are rejected. Reading a TOML file needs Python 3.11 or later.

### Errors

- `reconcilers.ReconcileError`: the comparison could not run, for example an
  unreadable file or an invalid configuration.
- `reconcilers.ReconciliationFailed`: raised by `raise_if_failed()`; its
  `result` attribute holds the `Result`.

A difference between the datasets is never an error. It is a finding in the
result.

## Things to know about dataframes

- **Types are not coerced.** An integer column on one side and a float column
  on the other is reported as inconclusive, not compared. pandas turns an
  integer column that contains missing values into floats unless it uses a
  nullable dtype such as `Int64`.
- **Timestamps keep their unit in the schema.** pandas uses nanoseconds and
  Spark uses microseconds, so `compare` reports a schema type change between
  them. The values themselves are still compared correctly, and `rowdiff`
  does not judge schemas at all.
- **Values are hidden by default.** Reports identify example rows by a
  fingerprint of their key. Set `{"report": {"show_values": True}}` to see
  real keys and values.

## How the package is organised

| File | Job |
|---|---|
| `reconcilers/_api.py` | The three public functions. |
| `reconcilers/_inputs.py` | Turns a path or a dataframe into something the library accepts. |
| `reconcilers/_config.py` | Turns `keys=` or `config=` into the library's configuration. |
| `reconcilers/_result.py` | The `Result` class. |
| `src/lib.rs` | The compiled glue: calls the Rust library and returns the report. |

## Develop

```bash
pip install maturin pytest pyarrow pandas polars
maturin develop
pytest
```
