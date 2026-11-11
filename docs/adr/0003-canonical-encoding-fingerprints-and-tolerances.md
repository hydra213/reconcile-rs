# ADR 0003: Canonical encoding, fingerprints and tolerances

Status: accepted
Date: 2026-11-11

## Context

A report should be tied to exactly what was compared, and two datasets with
the same logical content should be recognisable as such without a full diff.
Hashes can do both, but only if "the same content" always means "the same
bytes". Separately, some numeric differences are acceptable to the business
and must not fail a reconciliation. These two ideas pull in opposite
directions and are easy to confuse.

## Decision

### A versioned canonical encoding

`canonical` defines one byte sequence per logical `Value`: a type tag, then
a fixed-width or length-prefixed payload, all big-endian. The full layout is
in the module documentation. The properties that matter:

- **Type tags** keep NULL, zero, the empty string and empty binary apart.
- **Length prefixes** keep `("ab", "c")` apart from `("a", "bc")`.
- **Logical, not physical.** The encoding works on `Value`s, so `Int32` 7
  and `Int64` 7, or decimal `1.50` and `1.5`, encode identically.
- **Column names are part of a row**, in sorted order.
- **`CANONICAL_VERSION`** is written into every encoded row and recorded in
  every report. A unit test pins the exact bytes of version 1. Changing the
  layout means bumping the version; fingerprints from different versions
  must never be compared.

### Fingerprints are BLAKE3 over canonical bytes

| Fingerprint | Covers |
|---|---|
| row | one row's column names and values |
| content | all rows of a side, over the key and compared columns |
| schema | every column's name, Arrow type name and nullability |
| configuration | every setting, as canonical JSON |

The content fingerprint is the hash of the **sorted list of row
fingerprints** plus the row count. That makes it independent of row order
and file layout, and sensitive to duplicates. XOR of row hashes was rejected
because two identical rows cancel out.

### Fingerprints are a screening signal, not evidence

- Equal content fingerprints mean the two sides agree on the key and
  compared columns, up to a hash collision. BLAKE3 has a 256-bit output, so
  an accidental collision is not a practical concern, but it is not a proof.
- Different fingerprints say *that* something differs, not *what*. The keyed
  row comparison is the evidence.
- Fingerprints never affect the verdict.

### Fingerprints of raw files stay out of the report

A report must be byte-identical when a file is rewritten with its rows in a
different order. A digest of the file bytes would break that, so the report
carries only order-independent fingerprints. `dataset::Dataset` still
exposes the per-file digests for run metadata.

Each input file is read from disk exactly once; its digest and its rows come
from the same bytes in memory. A file changed on disk during a run therefore
cannot make the digest describe different data from what was compared.

### Tolerances are explicit, absolute and per column

```toml
[[tolerances]]
column = "trade_amount"
max_difference = "0.01"
```

- A tolerance applies only to a compared, numeric column. Anything else is a
  configuration error before any row is read.
- The limit is a string so it is parsed as an exact decimal. Integers and
  decimals are compared with exact 128-bit arithmetic: a difference of
  exactly `0.01` is allowed, `0.0101` is not. Floats use `f64` arithmetic.
- NULL against a number, NaN and infinities are never within tolerance.
- If the exact arithmetic would overflow, the values are reported as
  different. A false alarm is preferred to a hidden difference.
- A tolerated difference is counted per column (`within_tolerance`) so it
  remains visible, but it is not a mismatch and does not make a row changed.

### Tolerant comparison and exact hashes are not interchangeable

Values inside a tolerance still have different fingerprints. A report can
therefore say `Verdict: PASS` and `Fingerprints: DIFFERENT` at the same
time, and both are correct. The text report says so next to the
fingerprints.

### Normalisation is opt-in

```toml
[normalization]
unicode = "nfc"                 # default: none
naive_timestamps = "assume_utc" # default: distinct
```

Normalisation is applied to every value before comparison and before
fingerprinting, and the active rules are recorded in the report. By default
strings are compared byte for byte and timestamps without a zone are never
compared with timestamps that have one.

## Consequences

- The schema fingerprint uses Arrow's type display names, so it can change
  with an Arrow upgrade. Such an upgrade must bump `CANONICAL_VERSION`.
- Only absolute tolerances exist. Relative tolerances can be added as a
  second kind without changing the meaning of existing configurations.
- `assume_utc` is the only zone rule. Other zones need a time zone database
  and are left for later.
- Fingerprinting hashes every row once per side. This is cheap next to
  reading Parquet, and is not used to skip the row comparison in v0.1.
