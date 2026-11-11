# ADR 0001: Logical schemas and schema compatibility policies

Status: accepted
Date: 2026-11-11

## Context

Before comparing any rows, ReconcileRS must say whether two datasets have
schemas that can be compared at all. Two questions are easy to mix up:

1. What is different between the schemas? This is a fact about the files.
2. Is that difference acceptable? This depends on who is asking.

A migration team may accept `Int32 -> Int64`. A consumer with a fixed-width
export format may not. If the engine answers both questions in one step, the
second answer is baked into the first and cannot be audited or changed.

## Decision

### Comparison is directional

The **left** dataset is the baseline and the **right** dataset is the
candidate. Differences are named from that point of view: a column that
exists only on the left was *removed*, one that exists only on the right was
*added*. Swapping the inputs gives a different, mirrored report.

### A logical schema is a set of named top-level columns

`schema::LogicalSchema` holds, for each top-level column, its name, its Arrow
data type and whether it is nullable. Columns are sorted by name, because
column order in a file has no meaning for reconciliation.

- Columns are matched by exact, case-sensitive name.
- Renames are never guessed. A rename appears as one removed and one added
  column until the user supplies an explicit mapping.
- A file with two columns of the same name is rejected with an error, since
  matching by name would be ambiguous.
- Nested types (struct, list, map) are compared as a whole: any change
  inside them is one type change of the top-level column.

### Facts and verdicts live in different modules

`schema::diff_schemas` returns a list of `SchemaDifference` values and
nothing else. There are four kinds: `ColumnRemoved`, `ColumnAdded`,
`TypeChanged` and `NullabilityChanged`.

`policy::evaluate_schema` attaches to each difference exactly one
`SchemaRule`. The rule determines the verdict and the explanation, and its
name appears in the report as the reason code. There is no verdict without a
rule.

### Two policies to start with

| Rule | `strict` | `compatible` |
|---|---|---|
| any difference | fail (`strict_requires_identical_schema`) | see below |
| `column_removed` | | fail |
| `column_added` | | warn |
| `lossless_integer_widening` | | warn |
| `incompatible_type_change` | | fail |
| `nullability_relaxed` (not nullable to nullable) | | warn |
| `nullability_tightened` (nullable to not nullable) | | pass |

`strict` is the default. Under `compatible`, the only type change allowed is
an integer widening where every baseline value fits in the candidate type.
Everything else, including `Decimal -> Float`, fails until a later release
adds explicit per-column rules.

The overall schema verdict is the worst verdict among the findings, in the
order pass < warn < inconclusive < fail.

## Consequences

- A report can always explain a verdict by naming one rule.
- New policies and per-column overrides can be added in `policy` without
  touching the diff.
- `compatible` is deliberately narrow. Users will hit `fail` on changes they
  consider harmless, and must wait for (or ask for) an explicit rule. This is
  preferred to a silent pass.
- Type names in reports are Arrow's display names, such as
  `Decimal128(18, 2)`. They could change when Arrow is upgraded; the snapshot
  tests will show it, and `report_version` must then be bumped.
- Column names that differ only by case are different columns. Sources that
  fold case (for example Snowflake) will need an explicit mapping.
