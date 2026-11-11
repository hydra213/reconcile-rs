# Reconciliation report

**Verdict: FAIL**

## Schema: FAIL

Policy `strict`. 7 left columns, 8 right columns.

| Verdict | Difference | Explanation |
|---|---|---|
| FAIL | `desk` exists only on the right side | The strict policy requires both schemas to be identical. |
| FAIL | `price` changed type from Decimal128(18, 4) to Float64 | The strict policy requires both schemas to be identical. |
| FAIL | `quantity` changed type from Int32 to Int64 | The strict policy requires both schemas to be identical. |
| FAIL | `trade_amount` exists only on the right side | The strict policy requires both schemas to be identical. |
| FAIL | `trade_amt` is missing from the right side | The strict policy requires both schemas to be identical. |

## Rows: FAIL

Keys: `trade_id`.

| Count | Left | Right |
|---|---:|---:|
| Rows | 5 | 5 |
| Rows with a NULL key | 0 | 0 |
| Duplicated keys | 0 | 0 |
| Keys only on this side | 1 | 1 |

4 matched keys, 4 rows compared: 4 unchanged, 0 changed.

### Checks

| Verdict | Explanation |
|---|---|
| FAIL | 1 key(s) exist only on the left side; at most 0 allowed (fail_on_missing_keys = true). |
| FAIL | 1 key(s) exist only on the right side; at most 0 allowed (fail_on_extra_keys = true). |
| PASS | 0 key(s) appear on more than one row; at most 0 allowed (fail_on_duplicate_keys = true). |
| PASS | 0 row(s) have NULL in a key column; at most 0 allowed (fail_on_null_keys = true). |
| PASS | 0 matched row(s) have changed values; at most 0 allowed (max_changed_rows = 0). |
| INCONCLUSIVE | 1 column(s) could not be compared; at most 0 allowed (strict_on_inconclusive = true). |

### Columns

- `booked_at`: 0 mismatch(es)
- `business_date`: 0 mismatch(es)
- `currency`: 0 mismatch(es)
- `price`: not compared, the left side is a decimal and the right side is a float; comparing them needs an explicit rule
- `quantity`: 0 mismatch(es)

### Examples

At most 10 of each kind.

| Difference | Example |
|---|---|
| Only on the left | key (1004) |
| Only on the right | key (1006) |

## Fingerprints: DIFFERENT

Algorithm `blake3`, canonical version 1. A screening signal over the key and compared columns, not a verdict: values inside a tolerance still have different fingerprints.

| | Left | Right |
|---|---|---|
| Content | `bf94de5fb7c4f49fa303b91fde7b62781920b92837c51b1dd27aa38603885355` | `3ccdc75cb9ec9ce90fba86b02453b08ae168154ff094f162dd46f6fd4e9a8e75` |
| Schema | `0c3e0be95382207a24ebea864fd11176b01692fb0237b74ccb9f529242772657` | `09ddcc88ed40362f846645351df357c4f8e568b8f3e710512bf65c7d32e137c1` |

Configuration: `5037a22164d671c723205db9e7e4a142be81e4d857638dac6d548fdf30f379ec`
