# ReconcileRS

> Trust the migration, not the row count.

ReconcileRS is a Rust library and command-line tool that compares two
datasets and explains, with evidence, how they differ. It is deterministic:
the same inputs and policy always produce the same report, byte for byte.

**Status:** early development. Schema comparison works today. Keyed row
comparison, fingerprints and tolerances are next.

## Try it

You need a [Rust toolchain](https://rustup.rs). Two sample files are included.

```bash
cargo run -- schema \
  --left tests/fixtures/legacy_trades.parquet \
  --right tests/fixtures/new_trades.parquet \
  --policy compatible \
  --report schema_report.json
```

```text
Schema: FAIL (policy: compatible)
Columns: 7 left, 8 right

WARN  `desk` exists only on the right side
      A new column does not affect existing data, but it has no baseline to compare against.

FAIL  `price` changed type from Decimal128(18, 4) to Float64
      The type change is not on the list of lossless changes and needs an explicit decision.
...
```

The left file is the baseline and the right file is the candidate.

### Schema policies

| Policy | Meaning |
|---|---|
| `strict` (default) | Any schema difference fails. |
| `compatible` | Added columns, lossless integer widening and relaxed nullability warn. Everything else fails. |

### Exit codes

| Code | Meaning |
|---|---|
| 0 | Pass, possibly with warnings |
| 1 | The policy is violated |
| 2 | The comparison could not run (bad arguments, unreadable file) |
| 3 | The comparison ran but was inconclusive |

## Use it as a library

```rust
use std::path::Path;
use reconcilers::policy::SchemaPolicy;

let report = reconcilers::compare_parquet_schemas(
    Path::new("legacy.parquet"),
    Path::new("new.parquet"),
    SchemaPolicy::Compatible,
)?;
println!("{}", report.verdict);
```

## How the code is organised

Each module has one job, and data flows through them in this order:

| Module | Job |
|---|---|
| `parquet_input` | Read Parquet files from disk. |
| `schema` | Describe schemas and list the facts that differ. |
| `policy` | Turn facts into verdicts. Every verdict names the rule that produced it. |
| `report` | Assemble facts and verdicts into deterministic JSON and text. |
| `main.rs` | The CLI: parse arguments, call the library, pick an exit code. |

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
