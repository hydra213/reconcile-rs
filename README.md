# ReconcileRS

> Trust the migration, not the row count.

ReconcileRS is a Rust library and command-line tool that compares two
Parquet datasets and explains, with evidence, how they differ. It answers
three questions separately:

1. **Structure.** Did the schema change, and is the change acceptable?
2. **Completeness.** Which keys are missing, extra, duplicated or NULL?
3. **Correctness.** For rows that match, which values changed?

Every answer comes with the rule that produced it and one of four verdicts:
**pass**, **warn**, **inconclusive** or **fail**. The same data and
configuration always produce the same report, byte for byte.

**Status:** v0.1. Local Parquet files, compared in memory.

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
Each side can be several Parquet files with the same schema:
`--left part-0.parquet part-1.parquet`.

What the sample files contain, and why this is the right answer for them, is
written down in [`tests/fixtures/README.md`](tests/fixtures/README.md).

## Commands

| Command | What it does |
|---|---|
| `reconcilers schema` | Compares schemas only. Reads file footers, no rows. |
| `reconcilers rowdiff` | Compares rows only. Pairs rows by key and compares their values. Schemas are read to find the comparable columns, but not judged. |
| `reconcilers compare` | The full reconciliation: the schema check, the row check and fingerprints, with one overall verdict. |

Each command maps to one library function: `compare_parquet_schemas`,
`compare_parquet_rows` and `compare_parquet`. Run any of them with `--help`
for its options.

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

`rowdiff` and `compare` read a TOML file. Only `keys` is required. Every other setting
defaults to the strict choice, and unknown settings are rejected.
[`examples/reconcile.toml`](examples/reconcile.toml) lists every setting.

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

## Use it as a library

```rust
use std::path::{Path, PathBuf};
use reconcilers::{Config, compare_parquet};

let config = Config::from_toml_file(Path::new("reconcile.toml"))?;
let reconciliation = compare_parquet(
    &config,
    &[PathBuf::from("legacy.parquet")],
    &[PathBuf::from("new.parquet")],
)?;

println!("{}", reconciliation.report.verdict);
std::fs::write("findings.json", reconciliation.report.to_json())?;
```

`compare_parquet` runs everything. To run one check on its own, call
`compare_parquet_schemas` or `compare_parquet_rows` instead.

## How the code is organised

Each module has one job. `compare_parquet` in `src/lib.rs` runs them in
this order, so that file is the place to start reading.

| Module | Job |
|---|---|
| `config` | Load and check `reconcile.toml`. |
| `parquet_input` | Read Parquet files from disk. |
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
cargo run --example generate_fixtures  # regenerate tests/fixtures/*.parquet
```

| Tests | What they cover |
|---|---|
| `src/**` unit tests | Each rule next to the code that implements it. |
| `tests/corpus.rs` | Fifteen hand-labelled cases with a known right answer, through real Parquet files. |
| `tests/properties.rs` | Rules that must hold for any data, checked on random tables. |
| `tests/row_cases.rs`, `tests/schema_cases.rs` | Library behaviour and report snapshots. |
| `tests/cli_cases.rs` | The real binary: output, files and exit codes. |

Snapshot tests use [insta](https://insta.rs). When a report changes on
purpose, run `INSTA_UPDATE=always cargo test`, read the diff in
`tests/snapshots/`, and commit it.

## Limitations

- Both datasets are loaded into memory. There is no streaming or spilling
  to disk yet, and no benchmarks have been run.
- Local Parquet files only. No database or cloud storage adapters.
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
