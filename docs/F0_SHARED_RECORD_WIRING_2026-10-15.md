# F0 shared capability record wiring — 2026-10-15

## Change

The capability voice/status sentence path and the complete capability listing
now read `CapabilityRecord` availability and evidence instead of branching
directly on the legacy `State` enum. The existing words and markers remain
stable: working, switched off, waiting, untested, and planned entries are
rendered as before. Missing records are skipped safely rather than panicking.

## Proof

- `cargo fmt --manifest-path atlas/Cargo.toml -- --check` — passed.
- `CARGO_TARGET_DIR=target_record_verify cargo test --manifest-path atlas/Cargo.toml --lib capability -- --test-threads=1` — 5 passed.
- `CARGO_TARGET_DIR=target_record_verify cargo test --manifest-path atlas/Cargo.toml --test all capability_routine -- --test-threads=1` — 22 passed.
- Earlier focused proof remains green: capability honesty 9 passed, catalogue
  answer 1 passed, and F0 evidence tests 5 passed.

## Next

Wire the same record view into the remaining Hub/status surfaces and any
voice-facing capability summaries, then rerun the focused catalogue and
conversation gates. Device-level proof remains separate and is not claimed.
