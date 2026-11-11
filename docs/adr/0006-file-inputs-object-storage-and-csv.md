# ADR 0006: Folders, object storage and CSV

Status: accepted
Date: 2026-11-11

## Context

Incoming data rarely arrives as one local Parquet file. It is a folder of
part files, it sits in S3 or ADLS, and it is often CSV. Each of these used
to need a manual step before ReconcileRS could be run.

## Decision

### A location is a path or a URL, and may be a folder

The `input` module reads a dataset from *locations*. A location that
contains `://` is an object storage URL; anything else is a local path. A
local folder, or a URL ending in `/`, means every `.parquet` and `.csv`
file directly inside it, in name order. Other files are skipped, because
engines write markers such as `_SUCCESS` next to the data. Sub-folders are
not searched, so nothing is read that the user did not point at.

`input::fetch` is the only module that knows about object storage. It
returns each file's bytes, and everything after it is identical for local
and remote files.

### Object storage through `object_store`

`s3://`, `abfss://`, `az://` and `gs://` URLs are read with the Arrow
project's `object_store` crate, which is one interface for all of them.
Credentials are taken from the environment variables each cloud defines,
so ReconcileRS has no credential settings of its own and none can end up
in a configuration file or a report.

`object_store` is asynchronous. ReconcileRS does one thing at a time, so
`fetch` runs each request to its end on a small single-threaded Tokio
runtime. No other module is asynchronous.

### Files are always read whole

Every file is fetched into memory once, and its digest and its rows come
from those bytes (ADR 0003). The `schema` command now does the same and
only skips decoding the rows. Reading just the Parquet footer would be
faster for large files, but it would need a second code path per storage
kind. Both datasets must fit in memory anyway, so one simple path was
chosen.

### CSV types are declared, never inferred

A CSV file carries no types. Inferring them from the data would make the
result depend on which rows happen to be in the file: a column that is all
digits today is an integer, and text tomorrow. That is the opposite of a
deterministic reconciliation.

So a CSV column is text unless `[csv.types]` declares its type by name.
The names are short Arrow-style names (`int64`, `decimal(18, 2)`,
`timestamp[us, UTC]`), so a column can be declared to match the other side
exactly. The usual rule then applies: values of different kinds are never
coerced, so an undeclared CSV column compared with a typed column is
inconclusive, not a silent pass or a wall of false mismatches.

A value that does not fit its declared type is an execution error, not a
finding, because the file could not be read as the user described it.

By default an empty value is NULL in a column of any type, which matches
how warehouses load CSV. `null_values` replaces that list.

The format is chosen by file name: `.csv` is CSV, everything else is
Parquet, because Parquet files are often written without an extension.

## Consequences

- An incoming CSV can be compared directly with the Parquet files of a
  landing folder in a lake.
- The crate now depends on `object_store`, `tokio`, `url`, `arrow-csv` and
  `regex`, and a clean build takes longer.
- Object storage is covered by tests through `file://` URLs, which use the
  same code path, but not against a real cloud account.
- A Delta table's folder must not be read as a folder of Parquet files,
  because it contains files the table no longer references.
- The `schema` command has no configuration file, so it sees CSV columns
  as text.
- Adding the `[csv]` section changed the configuration fingerprint of
  every configuration, because the fingerprint covers all settings.
