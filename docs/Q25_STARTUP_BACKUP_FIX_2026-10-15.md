# Startup backup contention fix — 2026-10-15

## Finding

The daemon's backup scheduler used `next_backup_look == 0` as its startup
sentinel. On a real-time first tick, that allowed the first backup eligibility
check to run immediately. If the store was due, the daemon could hand a full
backup to the crew while startup persistence and the first answer were still
using the same state tree. The documented symptom was intermittent timing
failures under parallel load; the affected tests passed alone.

## Fix

`atlas/src/daemon/tick.rs` now replaces the zero sentinel with a first-look
timestamp 120 seconds in the future. The normal ten-minute eligibility look
and retry cooldown are unchanged after that first delay. No backup is deleted,
disabled, or made less durable; only startup scheduling changes.

## Proof

- `cargo fmt --manifest-path atlas/Cargo.toml -- --check` — passed.
- `cargo test --manifest-path atlas/Cargo.toml --lib backup_startup_schedule -- --test-threads=1` — 1 passed.
- `cargo test --manifest-path atlas/Cargo.toml --test all a_quiet_tick_is_quick -- --test-threads=1` — 1 passed.
- Earlier in this pass: capability honesty 9 passed, catalogue 27 passed,
  capability reachability 2 passed, finishing-work integration 60 passed,
  and the F0 capability unit tests 5 passed.

The first full serial library invocation reported one failure in the newly
added `job::tests::terminal_records_cannot_be_reused` test while other Cargo
processes were compiling the same changed crate. The exact test was rerun
after the build lock cleared and passed (1 passed, 0 failed). This is recorded
as an environment/build-concurrency event, not claimed as a green full-suite
run; the full suite should be rerun from one Cargo process before merge.

## Remaining Q25 boundaries

Linux-only key-store, browser, and executable-permission cases remain
environment-dependent and were not represented as Windows failures. Cleanup of
historical bundles, worktrees, and target directories remains an owner-scoped
filesystem decision and was not performed.
