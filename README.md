# ReconcileRS

> Trust the migration, not the row count.

ReconcileRS is a Rust library and command-line tool that compares two
datasets and explains, with evidence, how they differ. It is deterministic:
the same inputs and policy always produce the same report, byte for byte.

**Status:** early development. Schema comparison and keyed row comparison
work today. Fingerprints, tolerances and Markdown reports are next.

## Try it

You need a [Rust toolchain](https://rustup.rs). Two sample files and a sample
configuration are included.

```bash
cargo run -- rowdiff \
  --left tests/fixtures/legacy_trades.parquet \
  --right tests/fixtures/new_trades.parquet \
  --config examples/reconcile.toml \
  --report report.json
```

```text
Verdict: FAIL

Schema: FAIL (policy: compatible)
  7 left columns, 8 right columns
  WARN          `desk` exists only on the right side
                A new column does not affect existing data, but it has no baseline to compare against.
  FAIL          `price` changed type from Decimal128(18, 4) to Float64
                The type change is not on the list of lossless changes and needs an explicit decision.
  ...

Rows: FAIL (keys: trade_id)
  5 left rows, 5 right rows, 4 matched keys, 4 rows compared
  FAIL          1 key(s) exist only on the left side; at most 0 allowed (fail_on_missing_keys = true).
  ...

Columns:
  `price`: not compared, the left side is a decimal and the right side is a float; comparing them needs an explicit rule
  `trade_amount`: 1 mismatch(es)
  ...
```

The left dataset is the baseline and the right dataset is the candidate.
Each side can be several Parquet files with the same schema:
`--left part-0.parquet part-1.parquet`.

### Commands

| Command | What it does |
|---|---|
| `reconcilers schema` | Compares schemas only. Reads file footers, no rows. |
| `reconcilers rowdiff` | Compares schemas, then pairs rows by key and compares their values. |

Run either with `--help` for its options.

### Configuration

`rowdiff` reads a TOML file. Only `keys` is required; every other setting
defaults to the strict choice. See
[`examples/reconcile.toml`](examples/reconcile.toml) for all settings.

```toml
[comparison]
keys = ["trade_id"]
schema_policy = "compatible"

[[column_mappings]]
left = "trade_amt"
right = "trade_amount"

[policy]
fail_on_extra_keys = false
```

### Schema policies

| Policy | Meaning |
|---|---|
| `strict` (default) | Any schema difference fails. |
| `compatible` | Added columns, lossless integer widening and relaxed nullability warn. Everything else fails. |

### How values are compared

Cells are compared as logical values, not as stored bytes: `Int32` 7 equals
`Int64` 7, and decimal `1.50` equals `1.5`. Different kinds of value are
never coerced. A column that is a decimal on one side and a float on the
other is reported as **inconclusive**, not guessed at. The full rules are in
[ADR 0002](docs/adr/0002-keys-logical-values-and-row-comparison.md).

### Exit codes

| Code | Meaning |
|---|---|
| 0 | Pass, possibly with warnings |
| 1 | The policy is violated |
| 2 | The comparison could not run (bad arguments, unreadable file, invalid configuration) |
| 3 | Nothing failed, but something could not be compared |

## Use it as a library

```rust
use std::path::{Path, PathBuf};
use reconcilers::{Config, compare_parquet};

let config = Config::from_toml_file(Path::new("reconcile.toml"))?;
let report = compare_parquet(
    &config,
    &[PathBuf::from("legacy.parquet")],
    &[PathBuf::from("new.parquet")],
)?;
println!("{}", report.verdict);
```

## How the code is organised

Each module has one job. `src/lib.rs` runs them in this order:

| Module | Job |
|---|---|
| `config` | Load and check `reconcile.toml`. |
| `parquet_input` | Read Parquet files from disk. |
| `schema` | Describe schemas and list the facts that differ. |
| `plan` | Check the configuration against the schemas and decide which columns are keys, compared or not comparable. |
| `value` | Convert Arrow cells to logical values that can be compared. |
| `table` | Hold one side's values in memory. |
| `rowdiff` | Pair rows by key and list the facts that differ. |
| `policy` | Turn facts into verdicts. Every verdict names the rule or check that produced it. |
| `report` | Assemble facts and verdicts into deterministic JSON and text. |
| `main.rs` | The CLI: parse arguments, call the library, pick an exit code. |

`schema` and `rowdiff` only report facts. `policy` is the only place where
something is judged acceptable or not.

Design decisions are recorded in [`docs/adr/`](docs/adr/).

## Development

```bash
cargo test                             # unit, snapshot and CLI tests
cargo fmt --check && cargo clippy --all-targets -- -D warnings
cargo run --example generate_fixtures  # regenerate tests/fixtures/*.parquet
```

Snapshot tests use [insta](https://insta.rs). When a report changes on
purpose, run `INSTA_UPDATE=always cargo test`, read the diff in
`tests/snapshots/`, and commit it.
