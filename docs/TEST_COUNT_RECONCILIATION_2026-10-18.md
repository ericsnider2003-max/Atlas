# Test-count reconciliation — 2026-10-18

## Result

The apparent drop from 8,915 to 7,184 was a reporting mistake.

The current direct Cargo discovery command was:

```powershell
cargo test --manifest-path atlas\Cargo.toml --all-targets -- --list
```

It discovered **9,030 tests across 45 Cargo targets**:

- 1,343 library unit tests;
- 1 binary unit test;
- 7,184 tests in `tests/all.rs`;
- 501 tests across the remaining integration-test binaries;
- one example target with zero tests.

There are 44 test binaries plus one zero-test example target. The earlier
8,915 figure was a historical explicit-target aggregate recorded on 2026-10-15
without a preserved per-target inventory. It is 115 lower than the current
discovery and cannot be used as evidence that tests were removed.

## Why 7,184 appeared in the latest report

The full `cargo test --all-targets` execution began running targets in order.
The large `tests/all.rs` target contains 7,184 tests. That target produced the
failure summary, so the reported result stopped at that target and did not
represent the complete discovered target inventory. The 7,184 figure was
therefore incorrectly described as the all-target total.

## Current authoritative counts

| Measurement | Targets | Tests | Meaning |
|---|---:|---:|---|
| Historical audit, 2026-10-15 | 44 | 8,915 | stale; no per-target inventory preserved |
| Current `--list` discovery | 45 | 9,030 | authoritative discovered total |
| `tests/all.rs` binary | 1 | 7,184 | one large integration target only |
| Latest executed aggregate before failure summary | partial | 7,184 reported | incomplete execution report, not discovery |

## Correction

No evidence shows that 1,731 tests were deleted. The prior response conflated
the `tests/all.rs` target result with the complete Cargo target inventory. The
current baseline for Goal 2 is **9,030 discovered tests**, with execution still
requiring a target-by-target campaign because the large aggregate target has
known failures and can prevent later targets from being reported.

## Next verification step

Run the remaining 44 test binaries independently with one worker, record each
target’s pass/fail/ignored counts, and then separately rerun the failing
`tests/all.rs` cases. This produces a complete execution matrix without one
large target hiding results from later targets.
