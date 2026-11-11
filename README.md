# ReconcileRS

> Trust the migration, not the row count.

ReconcileRS is a Rust library, a command-line tool and a Python package
that compares two datasets and explains, with evidence, how they differ.
Each side can be Parquet or CSV files, on disk or in object storage (S3,
ADLS Gen2, GCS), or, from Python, a dataframe (pandas, Polars, PySpark,
pyarrow). It answers three questions separately:

1. **Structure.** Did the schema change, and is the change acceptable?
2. **Completeness.** Which keys are missing, extra, duplicated or NULL?
3. **Correctness.** For rows that match, which values changed?

Every answer comes with the rule that produced it and one of four verdicts:
**pass**, **warn**, **inconclusive** or **fail**. The same data and
configuration always produce the same report, byte for byte.

**Status:** v0.1. Files and dataframes, compared in memory.

## Try it

You need a [Rust toolchain](https://rustup.rs). Two sample files and a sample
configuration are included.

```bash
cargo run -- compare \
  --left tests/fixtures/legacy_trades.parquet \
  --right tests/fixtures/new_trades.parquet \
  --config examples/reconcile.toml \
  --out-dir artifacts
```

```text
Verdict: FAIL

Schema: FAIL (policy: compatible)
  7 left columns, 8 right columns
  WARN          `desk` exists only on the right side
                A new column does not affect existing data, but it has no baseline to compare against.
  FAIL          `price` changed type from Decimal128(18, 4) to Float64
                The type change is not on the list of lossless changes and needs an explicit decision.
  WARN          `quantity` changed type from Int32 to Int64
                Every value of the baseline integer type fits in the candidate type.

Rows: FAIL (keys: trade_id)
  5 left rows, 5 right rows, 4 matched keys, 4 rows compared
  FAIL          1 key(s) exist only on the left side; at most 0 allowed (fail_on_missing_keys = true).
  FAIL          1 key(s) exist only on the right side; at most 0 allowed (fail_on_extra_keys = true).
  PASS          0 key(s) appear on more than one row; at most 0 allowed (fail_on_duplicate_keys = true).
  PASS          0 row(s) have NULL in a key column; at most 0 allowed (fail_on_null_keys = true).
  FAIL          1 matched row(s) have changed values; at most 0 allowed (max_changed_rows = 0).
  INCONCLUSIVE  1 column(s) could not be compared; at most 0 allowed (strict_on_inconclusive = true).

Columns:
  `booked_at`: 0 mismatch(es)
  `business_date`: 0 mismatch(es)
  `currency`: 0 mismatch(es)
  `price`: not compared, the left side is a decimal and the right side is a float; comparing them needs an explicit rule
  `quantity`: 0 mismatch(es)
  `trade_amount`: 1 mismatch(es)

Examples (at most 10 of each kind; values are hidden, set `[report] show_values = true` to see them):
  only on the left: key fingerprint c46d15685335e23d
  only on the right: key fingerprint 08310d9533563235
  changed: key fingerprint fa1fd134d8ad2873: `trade_amount`

Fingerprints: DIFFERENT (blake3, canonical version 1)
  A screening signal over the key and compared columns, not a verdict.
  left content   a59bcf738a7f7315e1a1b666a8e919d4c31898f806a3de2c67432f8715b6f860
  right content  f89afc7092b98170418fe4ff13428ba2ced54cebac7b6cf61b5529ba11af4eab
```

The left dataset is the baseline and the right dataset is the candidate.

What the sample files contain, and why this is the right answer for them, is
written down in [`tests/fixtures/README.md`](tests/fixtures/README.md).

## Inputs

Each of `--left` and `--right` takes one or more locations. All files of one
side must have the same schema; their rows together form the dataset.

| Location | What is read |
|---|---|
| `data/trades.parquet` | One file. |
| `part-0.parquet part-1.parquet` | Several files as one dataset. |
| `data/trades/` | Every `.parquet` and `.csv` file directly inside the folder. Marker files such as `_SUCCESS` are skipped, and sub-folders are not searched. |
| `s3://bucket/trades.parquet` | One object in object storage. |
| `s3://bucket/trades/` | Every data file directly under the prefix. The URL must end with `/`. |

A file whose name ends in `.csv` is read as CSV. Every other file is read
as Parquet. The two sides can be of different formats.

**Object storage.** `s3://`, `abfss://`, `az://` and `gs://` URLs are read
with the Arrow project's `object_store` crate. Credentials come from the
environment variables each cloud uses, for example `AWS_ACCESS_KEY_ID`,
`AWS_SECRET_ACCESS_KEY` and `AWS_REGION`, `AZURE_STORAGE_ACCOUNT_NAME` and
`AZURE_STORAGE_ACCOUNT_KEY`, or `GOOGLE_SERVICE_ACCOUNT`. On a cloud machine
with an attached role or managed identity nothing needs to be set.

```bash
reconcilers compare \
  --left incoming/trades.csv \
  --right s3://lake/landing/trades/load_date=2026-11-11/ \
  --config reconcile.toml
```

A Delta table must not be read this way. Its folder also holds files that
the table no longer uses, so query the table and compare the result from
Python instead.

**CSV.** A CSV file has column names but no types, and ReconcileRS never
guesses one. A column is text unless its type is declared in the
configuration, by the column's name in the file:

```toml
[csv]
delimiter = ","          # default
null_values = [""]       # default: an empty value is NULL

[csv.types]
trade_id = "int64"
trade_amount = "decimal(18, 2)"
business_date = "date"
booked_at = "timestamp[us, UTC]"
```

The type names are `string`, `bool`, `int8` to `int64`, `uint8` to `uint64`,
`float32`, `float64`, `decimal(precision, scale)`, `date`,
`timestamp[s|ms|us|ns]` and `timestamp[unit, zone]`. The first row must hold
the column names. A value that does not fit its declared type stops the run
with exit code 2 and an error that names the line. Text is not silently
compared with numbers: an undeclared CSV column compared with a typed
column is reported as inconclusive.

The `schema` command takes no configuration, so it sees every CSV column as
text. Use `compare` to judge a CSV schema with its declared types.

## Commands

| Command | What it does |
|---|---|
| `reconcilers schema` | Compares schemas only. No rows are decoded. |
| `reconcilers rowdiff` | Compares rows only. Pairs rows by key and compares their values. Schemas are read to find the comparable columns, but not judged. |
| `reconcilers compare` | The full reconciliation: the schema check, the row check and fingerprints, with one overall verdict. |

Each command maps to one library function: `compare_schemas`,
`compare_rows` and `compare`. Run any of them with `--help` for its options.

### Output files

Every command prints a summary. With `--out-dir <directory>` they also write:

| File | Content |
|---|---|
| `findings.json` | The report, with the sections that command covers. Byte-identical for the same data and configuration. |
| `report.md` | The same report as Markdown. |
| `run.json` | What is specific to this run: tool version, start time, duration, file paths, sizes and digests. |

### Exit codes

| Code | Meaning |
|---|---|
| 0 | Pass, possibly with warnings |
| 1 | The policy is violated |
| 2 | The comparison could not run (bad arguments, unreadable file, invalid configuration) |
| 3 | Nothing failed, but something could not be compared |

## Configuration

`rowdiff` and `compare` read a TOML file. Only `keys` is required. Every
other setting defaults to the strict choice, and unknown settings are
rejected. [`examples/reconcile.toml`](examples/reconcile.toml) lists every
setting. The `[csv]` section is described under [Inputs](#inputs).

```toml
[comparison]
keys = ["trade_id", "business_date"]
schema_policy = "compatible"       # strict (default) | compatible

# Renames are never guessed. A mapped column goes by its right-side name.
[[column_mappings]]
left = "trade_amt"
right = "trade_amount"

# Numbers in this column may differ by up to 0.01.
[[tolerances]]
column = "trade_amount"
max_difference = "0.01"

[normalization]
unicode = "nfc"                    # none (default) | nfc
naive_timestamps = "assume_utc"    # distinct (default) | assume_utc

[policy]
fail_on_missing_keys = true
fail_on_extra_keys = false         # warn only
fail_on_duplicate_keys = true
fail_on_null_keys = true
max_changed_rows = 0
strict_on_inconclusive = true

[report]
sample_limit = 10
show_values = false                # true writes real keys and values
```

## How it decides

**Schema policies.** `strict` fails on any schema difference. `compatible`
warns on added columns, lossless integer widening and relaxed nullability,
and fails on everything else.

**Keys.** Rows are paired by the configured key. A key on more than one row
is reported and not compared, because there is no way to know which row is
the partner. A row with NULL in a key column is counted and set aside. Row
order and file layout never affect the result.

**Values.** Cells are compared as logical values, not stored bytes: `Int32`
7 equals `Int64` 7, decimal `1.50` equals `1.5`, and the same instant in
milliseconds and microseconds is equal. NULL equals only NULL. NaN equals
NaN and -0.0 equals 0.0.

**No silent coercion.** A column that is a decimal on one side and a float
on the other, or has a type the engine does not handle yet (nested types,
time of day), is reported as **inconclusive** with the reason. It is never
guessed at and never counted as a pass.

**Tolerances and normalisation.** Everything is exact unless the
configuration says otherwise. Tolerated differences are still counted in
the report.

**Fingerprints.** Each report carries BLAKE3 fingerprints of both sides'
content and schema, and of the configuration. They are a quick signal that
two datasets agree, never a verdict: they are exact, so two values inside a
tolerance still give different fingerprints.

**Redaction.** Reports contain no key or cell values by default. Examples
are identified by a fingerprint of their key. Set `show_values = true` to
see real values.

The reasoning behind each of these is in [`docs/adr/`](docs/adr/):

- [0001](docs/adr/0001-logical-schemas-and-compatibility-policies.md): logical schemas and compatibility policies
- [0002](docs/adr/0002-keys-logical-values-and-row-comparison.md): keys, logical values and row comparison
- [0003](docs/adr/0003-canonical-encoding-fingerprints-and-tolerances.md): canonical encoding, fingerprints and tolerances
- [0004](docs/adr/0004-reports-run-metadata-and-redaction.md): reports, run metadata and redaction
- [0005](docs/adr/0005-in-memory-datasets-and-python-bindings.md): in-memory datasets and the Python package
- [0006](docs/adr/0006-file-inputs-object-storage-and-csv.md): folders, object storage and CSV

## Use it from Python

The Python package compares files and dataframes with the same engine. It
lives in [`python/`](python/), and [`python/README.md`](python/README.md)
is its full guide.

```python
import reconcilers

result = reconcilers.compare("legacy.parquet", new_dataframe, keys=["trade_id"])

print(result)              # the report as text
result.raise_if_failed()   # stop a pipeline on a failed reconciliation
```

`reconcilers.schema`, `reconcilers.rowdiff` and `reconcilers.compare` match
the three commands. Each side can be a location (a file, a folder or an
object storage URL), a list of locations, or a pandas, Polars, PySpark or
pyarrow table, and the two sides can be of different kinds. That is how an incoming file is checked against a table in
Snowflake or Databricks: query the rows to check, and pass the result.

## Use it as a Rust library

Reading and comparing are separate steps. Read each side into a `Dataset`,
then compare the datasets.

```rust
use std::path::Path;
use reconcilers::{Config, input};

let config = Config::from_toml_file(Path::new("reconcile.toml"))?;
let left = input::read_dataset(&["legacy.parquet".to_string()], &config.csv)?;
let right = input::read_dataset(&["s3://lake/new/".to_string()], &config.csv)?;

let report = reconcilers::compare(&config, &left, &right)?;

println!("{}", report.verdict);
std::fs::write("findings.json", report.to_json())?;
```

`compare` runs everything. To run one check on its own, call
`compare_schemas` or `compare_rows` instead. Arrow data that is already in
memory becomes a dataset with `Dataset::from_batches`, so any source that
can produce Arrow record batches can be compared.

## How the code is organised

Each module has one job. `compare` in `src/lib.rs` runs them in this order,
so that file is the place to start reading.

| Module | Job |
|---|---|
| `config` | Load and check `reconcile.toml`. |
| `dataset` | Hold one side of a comparison in memory, from any source. |
| `input` | Read files into a dataset. `input::fetch` finds the files and reads their bytes from disk or object storage; `input::parquet` and `input::csv` decode them. |
| `schema` | Describe schemas and list the facts that differ. |
| `plan` | Check the configuration against the schemas and decide which columns are keys, compared or not comparable. |
| `value` | Convert Arrow cells to logical values that can be compared. |
| `normalize` | Apply opt-in rules (Unicode, naive timestamps) to values. |
| `table` | Hold one side's values in memory. |
| `tolerance` | Decide whether two numbers are close enough. |
| `rowdiff` | Pair rows by key and list the facts that differ. |
| `canonical` | Define the exact bytes of a value, for hashing. |
| `fingerprint` | Hash rows, table content, schemas and files. |
| `policy` | Turn facts into verdicts. Every verdict names the rule or check that produced it. |
| `report` | Assemble facts and verdicts into deterministic JSON, text and Markdown. |
| `run` | Record what is specific to one run. |
| `main.rs` | The CLI: parse arguments, call the library, write files, pick an exit code. |
| `python/` | The Python package: a thin layer that passes files and dataframes to the library. |

`schema` and `rowdiff` only report facts. `policy` is the only place where
something is judged acceptable or not.

### Debugging a result

- **A verdict looks wrong.** Find the rule or check name in the report, then
  read `src/policy/schema_rules.rs` or `src/policy/row_rules.rs`. Each
  verdict is decided in exactly one place.
- **Two values should be equal but are not (or the reverse).** Equality is
  defined by `Value` in `src/value.rs`. Run with `show_values = true` to see
  both sides as the engine sees them.
- **A column was not compared.** `src/plan.rs` decides that, and the report
  gives the reason.
- **A configuration is rejected.** Every message comes from `ConfigError` in
  `src/config.rs`.

## Development

```bash
cargo test                             # all tests
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo run --example generate_fixtures  # regenerate the files in tests/fixtures/
```

| Tests | What they cover |
|---|---|
| `src/**` unit tests | Each rule next to the code that implements it. |
| `tests/corpus.rs` | Fifteen hand-labelled cases with a known right answer, through real Parquet files. |
| `tests/properties.rs` | Rules that must hold for any data, checked on random tables. |
| `tests/row_cases.rs`, `tests/schema_cases.rs` | Library behaviour and report snapshots. |
| `tests/input_cases.rs` | Reading CSV files, folders and object storage URLs. |
| `tests/cloud_cases.rs` | Reading from a real bucket. Skipped unless you point it at one, see below. |
| `tests/cli_cases.rs` | The real binary: output, files and exit codes. |
| `python/tests/` | The Python package, with files and with pandas, Polars, PySpark and pyarrow dataframes. |

Snapshot tests use [insta](https://insta.rs). When a report changes on
purpose, run `INSTA_UPDATE=always cargo test`, read the diff in
`tests/snapshots/`, and commit it.

### Testing against a real bucket

`tests/cloud_cases.rs` reads the sample files from object storage and checks
that they give exactly the same rows as the local copies. It does nothing
unless you point it at a location of your own:

1. Copy the whole `tests/fixtures/` folder to the bucket, for example
   `aws s3 cp --recursive tests/fixtures s3://my-bucket/reconcilers-test/`.
2. Set the usual credentials of that cloud in your environment.
3. Run it, with the location ending in `/`:

```bash
RECONCILERS_TEST_S3=s3://my-bucket/reconcilers-test/ cargo test --test cloud_cases -- --nocapture
```

Use `RECONCILERS_TEST_AZURE` for an `abfss://` location and
`RECONCILERS_TEST_GCS` for a `gs://` one. Credentials stay in your
environment; nothing is written to the bucket.

### Releasing the Python package

Releases are built and uploaded by
[`.github/workflows/release.yml`](.github/workflows/release.yml), with wheels
for Linux (x86-64 and ARM), Windows and macOS (Apple Silicon and Intel).
Every wheel that can run on its build machine is installed there and tested
before anything is uploaded. PyPI trusts this workflow directly, so no token
is stored anywhere.

1. Set the version in `[workspace.package]` in the root `Cargo.toml` and
   commit it. The crate, the CLI and the Python package all use it.
2. Dry run: on GitHub, open **Actions → Release → Run workflow**. This
   uploads to [TestPyPI](https://test.pypi.org/project/reconcilers/). Check
   it with
   `pip install --index-url https://test.pypi.org/simple/ --extra-index-url https://pypi.org/simple/ reconcilers`.
3. Release: tag the commit and push the tag.

```bash
git tag v0.1.0
git push origin v0.1.0
```

The workflow stops if the tag does not match the version in `Cargo.toml`.
The upload to PyPI waits for approval in the `pypi` environment.

## Limitations

- Both datasets are loaded into memory. There is no streaming or spilling
  to disk yet, and no benchmarks have been run.
- Files are Parquet or CSV. There are no built-in database readers; a
  warehouse table reaches the engine as Arrow, for example a query result
  passed to the Python package.
- Object storage is tested on every run through local `file://` URLs, which
  use the same code path. Tests against a real S3, Azure or GCS account
  are run by hand (see "Testing against a real bucket").
- A file is always read whole, also by the `schema` command and also from
  object storage.
- CSV files need a header row, and every CSV file in one run shares the
  same `[csv]` settings. Compressed CSV files are not read.
- The Python package is built from source with maturin. No wheels are
  published yet.
- Nested types, time of day, `Date64`, `Decimal256`, `Float16`, dictionary
  and fixed-size binary columns are reported as inconclusive, not compared.
- Columns are matched at the top level only.
- Tolerances are absolute. There is no relative tolerance.
- `assume_utc` is the only time zone rule for timestamps without a zone.
- Aggregate and control-total checks are not implemented.
- A saved `findings.json` cannot be re-rendered; text, JSON and Markdown are
  written together.
- Results have not been cross-checked against an independent SQL
  implementation.

## Licence

MIT. See [LICENSE](LICENSE).
