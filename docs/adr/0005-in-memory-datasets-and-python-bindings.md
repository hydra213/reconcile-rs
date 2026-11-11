# ADR 0005: In-memory datasets and the Python package

Status: accepted
Date: 2026-11-11

## Context

The people who reconcile data are data engineers. Their baseline is often a
file, but their candidate is a table in Snowflake or Databricks, or a
dataframe in a pipeline task. A tool that only reads local Parquet files
makes them export both sides first.

Writing one reader per warehouse would be a large amount of code to own.
But all of these systems can already return their data as Apache Arrow,
which is the format the engine uses internally.

## Decision

### Reading and comparing are separate steps

The library functions take data that is already in memory:

| Function | Input |
|---|---|
| `compare_schemas` | two `LogicalSchema`s |
| `compare_rows` | a `Config` and two `Dataset`s |
| `compare` | a `Config` and two `Dataset`s |

A `Dataset` is a schema plus Arrow record batches. The `input` module builds
one from Parquet files; `Dataset::from_batches` builds one from Arrow data
that came from anywhere else. Nothing after that point knows where the rows
came from.

This replaced `compare_parquet` and its two siblings, which read files and
compared them in one call. The command-line tool now does the same two
steps the library user does.

### Storage encodings are not type differences

Arrow can hold the same text as `Utf8`, `LargeUtf8` or `Utf8View`, and the
same bytes as `Binary`, `LargeBinary` or `BinaryView`. Which one a column
uses depends on the program that produced it: Parquet readers give `Utf8`,
Polars gives `LargeUtf8` or `Utf8View`. A logical schema therefore names
them all `Utf8` and `Binary`. Without this, comparing a file with a
dataframe would report a type change on every text column.

Differences that can change a value are still reported: integer widths,
decimal precision and scale, timestamp units and time zones.

### The Python package is a thin layer

The package in `python/` has two halves:

- `python/src/lib.rs` is compiled with PyO3. It only receives the inputs,
  calls the library and returns the report as JSON, text and Markdown.
- `python/reconcilers/*.py` is plain Python: the public functions, the
  conversion of inputs, the configuration and the `Result` class.

No comparison logic lives in either half, so the command-line tool and the
Python package cannot disagree.

Dataframes cross into Rust through the Arrow PyCapsule interface
(`__arrow_c_stream__`), using the `pyo3-arrow` crate. pyarrow and Polars
implement it directly. pandas and PySpark dataframes are converted to a
pyarrow table in `_inputs.py` first. Libraries are recognised by module
name, so the package imports none of them and depends on none of them.

The configuration crosses as JSON with the same sections as the TOML file,
parsed by `Config::from_json_str`, so both spellings go through the same
validation.

The Python crate is a Cargo workspace member. That gives it the same
`Cargo.lock` as the library, which guarantees that `pyo3-arrow` and the
library use the same Arrow version. Plain `cargo build` and `cargo test`
still build the library only.

## Consequences

- Any source that can produce Arrow can be reconciled without new Rust
  code: Snowflake (`fetch_arrow_all`), Databricks, Spark, DuckDB, Polars,
  pandas.
- Both sides are still loaded into memory. A warehouse table must be
  filtered to the rows being reconciled before it is passed in.
- In-memory datasets have no files, so `run.json` style metadata (paths,
  sizes, digests) does not exist for them. The Python `Result` carries the
  report only.
- PySpark needs version 4.0 or later for `DataFrame.toArrow()`.
- A `Result` treats `inconclusive` as not passed, like exit code 3.
