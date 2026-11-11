# ADR 0002: Keys, logical values and row comparison

Status: accepted
Date: 2026-11-11

## Context

Equal row counts say little. To know *which* records differ, rows on the two
sides must be paired by a key and then compared value by value. That raises
three questions the engine must answer the same way every time:

1. When is a key usable, and what happens when it is not?
2. When are two cells equal, given that the same value can be stored as
   different physical types?
3. What does the engine do with a column it does not know how to compare?

## Decision

### Keyed comparison is the ground truth

Rows are paired by an explicit list of key columns from the configuration.
Row order and file layout never affect the result: keys are held in a sorted
map, and every list in the result is sorted by key.

- **NULL keys.** A row with NULL in any key column has no identity. It is
  counted (`null_key_rows_left` / `_right`) and takes no further part.
  NULLs are never grouped together as if they were equal.
- **Duplicate keys.** A key on more than one row of a side is reported
  (`duplicate_keys_left` / `_right`) and is **not** compared value by value,
  because there is no way to know which row is the partner. No row is picked
  as the winner.
- **Presence.** A key on the left only is `left_only`, on the right only is
  `right_only`, otherwise it is `matched`. Matched keys that are unique on
  both sides are the compared rows.
- **Key types.** A key column must exist on both sides with the same kind of
  value. Floats cannot be keys.

All of this is checked when the comparison plan is built, before any row is
read. A bad key is a configuration error (exit code 2), not a finding.

### Cells are compared as logical values

Every cell is converted to a `value::Value` and two cells are equal exactly
when their `Value`s are equal.

| Kind | Arrow types | Equality rule |
|---|---|---|
| boolean | Boolean | same truth value |
| integer | Int8 to Int64, UInt8 to UInt64 | same number, whatever the width |
| float | Float32, Float64 | same number; NaN equals NaN; -0.0 equals 0.0 |
| decimal | Decimal128 | same number, whatever the scale (1.50 = 1.5) |
| string | Utf8, LargeUtf8, Utf8View | same bytes; no Unicode normalisation |
| binary | Binary, LargeBinary, BinaryView | same bytes |
| date | Date32 | same day |
| instant | Timestamp with a time zone | same moment, whatever the unit or zone |
| naive timestamp | Timestamp without a time zone | same wall-clock reading |

NULL equals NULL and nothing else. NULL is not zero, not an empty string and
not empty binary.

Two columns are compared only when they are the same kind. Different kinds
are never coerced: an integer is not compared with a decimal, a decimal is
not compared with a float, and a naive timestamp is not assumed to be UTC.

### What cannot be compared is inconclusive

A column that exists on both sides but has an unsupported type (nested
types, time of day, Decimal256, dictionaries and others), or different kinds
on the two sides, is listed as `inconclusive` with the reason. It is not
loaded and not compared. Under `strict_on_inconclusive = true` (the default)
the row verdict is then at best `inconclusive`, and the CLI exits with 3 if
nothing else failed.

### Column mappings

A mapping says that a left column and a right column are the same logical
column. After mapping, the column is known everywhere by its right-side
name: in keys, in reports and in the schema diff. A mapped rename therefore
produces no schema finding.

### Facts first, verdicts second

`rowdiff::diff_rows` returns counts, the affected keys and the changed
values. `policy::evaluate_rows` then runs six checks, each comparing one
count with what the policy allows. The report lists all six, including the
ones that passed, with the setting that decided each.

## Consequences

- Both datasets are held in memory as `Value`s. This is simple to follow and
  to debug, and is enough for v0.1. Larger-than-memory comparison is future
  work and will need a different loading strategy, not a different result.
- Being conservative means some comparisons a human would call obvious
  (integer 5 against decimal 5.00) come back inconclusive. Explicit
  per-column rules can relax that later; a silent coercion could not be
  taken back.
- Float equality is exact. Tolerances are a separate, explicit setting and
  come with the canonical encoding work.
