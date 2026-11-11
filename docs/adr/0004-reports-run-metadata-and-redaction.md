# ADR 0004: Reports, run metadata and redaction

Status: accepted
Date: 2026-11-11

## Context

A reconciliation result is used in three ways: a machine reads it in CI, a
person reads it to find out what went wrong, and an auditor keeps it as
evidence. These pull in different directions. Evidence should be
reproducible bit for bit. People want examples of the rows that differ. And
the rows that differ are, by definition, real data that may be sensitive.

## Decision

### Three output files with different promises

`--out-dir` writes:

| File | Content | Promise |
|---|---|---|
| `findings.json` | Facts, verdicts, fingerprints, samples | Byte-identical for the same data and configuration |
| `report.md` | The same report for people | Derived only from `findings.json` content |
| `run.json` | Tool version, start time, duration, file paths, sizes and digests | Describes one run; differs every time |

Anything that varies between runs or machines goes in `run.json`: clocks,
paths, and digests of raw file bytes (which change when a file is rewritten
with the same rows in another order). Nothing of that kind may be added to
the report types in `src/report/`. Every list in the report is sorted and
every map is a `BTreeMap`, so no ordering depends on a hash seed.

The terminal summary is the same report rendered as text. All three
renderings come from one `Report` value, so they cannot disagree.

### One report shape

`reconcilers schema`, `reconcilers rowdiff` and `reconcilers compare` all
produce a `Report`. Each section is present only when the command covers
it: `schema` writes the `schema` section, `rowdiff` writes the `rows`
section, and `compare` writes both plus `fingerprints`.
`report_version` is bumped whenever the JSON layout changes.

### Verdicts and exit codes

The overall verdict is the worst section verdict, in the order
pass < warn < inconclusive < fail.

| Exit code | Meaning |
|---|---|
| 0 | pass or warn |
| 1 | fail: the policy is violated |
| 2 | the comparison could not run |
| 3 | inconclusive: nothing failed, but something could not be compared |

A data difference is never an error and an error is never a finding. An
`Error` means no trustworthy report exists, so nothing is printed to
standard output and no files are written.

### Samples are bounded and redacted by default

The report lists up to `sample_limit` (default 10) examples of each kind of
difference: keys only on one side, duplicated keys, and changed rows with
the names of the columns that changed. The examples are the smallest keys,
so the selection is deterministic.

By default no key or cell value is written anywhere: not to the terminal,
not to JSON, not to Markdown. Each example is identified by the BLAKE3
fingerprint of its key. That is enough to tell examples apart, to see
whether the same key shows up in two runs, and for someone with access to
the data to find the row.

`[report] show_values = true` adds the real key values and both sides of
each changed cell. The key fingerprint is still present, so a redacted
report and a full one can be matched up.

A key fingerprint is not encryption. A low-entropy key such as a small
integer can be recovered by hashing candidates. Redaction keeps values out
of logs and shared reports; it does not make a report safe to publish if the
keys themselves are secret and guessable.

### No network, no telemetry

The tool reads the files it is given and writes the files it is asked to
write. It makes no network calls.

## Consequences

- CI can diff `findings.json` between runs and treat any change as real.
- Debugging a failure without `show_values` needs access to the data to
  resolve fingerprints back to keys. That friction is deliberate.
- Changed values are held in memory for every changed row, although only
  the first few are reported. For v0.1 datasets fit in memory anyway.
- There is no command to re-render a saved `findings.json`. All renderings
  are written together by `--out-dir`. A `report` subcommand would need the
  report types to be readable back from JSON and is left for later.
