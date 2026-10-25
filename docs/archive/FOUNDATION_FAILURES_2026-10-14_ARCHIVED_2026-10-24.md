# Foundation pass failure record — 2026-10-14

The full suite initially reported 7,082 passed, 15 failed, and 9 ignored.
The complete research record is in the replica workspace at
`C:/Users/erics/Atlas/replica/ATLAS_FOUNDATION_FAILURES_2026-10-14.md`.

## Fixed locally

- The proprietary-material scanner walked generated `target_*`, worktree, and
  archived audit outputs, causing a 172-second timeout. The guard now excludes
  those non-shipped trees while retaining the relevant repository surface. The
  targeted scan passes in 5.94 seconds.
- The night-owl synthetic UTC fixture did not pin the test clock. It now pins
  UTC and passes alone.
- The animation/GIF test passes alone and in the clean serialized rerun up to
  the later process-level termination.

## Resolved environment-dependent failures

- One full serialized integration run reached `round10` and terminated
  abnormally with Windows exit code `0xffffffff`; the implicated test passed
  in isolation. A clean rerun from the serial unit baseline passed the complete
  integration suite, so this remains recorded as an intermittent
  environment/process event rather than an open product failure.

## Verification commands

- `cargo check --manifest-path atlas/Cargo.toml`
- `cargo test --manifest-path atlas/Cargo.toml --no-run`
- Full-suite verification must run with working directory `atlas/`, because
  `.cargo/config.toml` intentionally binds Cargo temp paths to the checkout's
  local `scratch/` directory.
- targeted fingerprint scan: passed in 5.94 seconds
- targeted night-owl fixture: passed
- targeted animation/GIF test: passed
- targeted video-studio test: passed after accepting either the bundled tool or
  a supported `ffmpeg`/`ffprobe` installation on `PATH`

## Isolated reruns since the initial suite

- All `research_off_the_tick` tests: passed.
- All `off_the_tick` tests: passed.
- All `speed_measured` tests: passed.
- All `a_no_comes_with_a_way_to_yes` tests: passed.
- All but one `a_brief_without_an_inbox` test: passed; the remaining
  night-owl fixture is now fixed with a UTC clock pin and passes alone.
- `round10::a_call_or_a_video_is_not_time_away`: passed in isolation; the
  serialized suite termination at this point remains documented above.
- A later standalone `cargo test --lib` after the long integration run reported
  two output-root/signing-receipt failures; each passes in isolation (0.76s and
  12.07s). These are additional shared-state cleanup/isolation findings, not
  reproducible product failures. The serial unit rerun subsequently passed.

## Final canonical verification

- `cargo check --manifest-path atlas/Cargo.toml`: passed.
- `cargo test --manifest-path atlas/Cargo.toml --no-run`: passed.
- `cargo test --manifest-path atlas/Cargo.toml --lib -- --test-threads=1`:
  1,259 passed, 14 ignored.
- `cargo test --test all -- --test-threads=1`: 7,136 passed, 9 ignored.
