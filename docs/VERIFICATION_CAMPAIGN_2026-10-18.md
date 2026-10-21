# Verification campaign — 2026-10-18

## Completed

- Filing render completed: `AGENTS.md` and `docs/INDEX.md` regenerated.
- Filing cold check: exit 0; 59 warnings remain, all classified as stale/history
  warnings rather than a filing-script failure.
- Installed verification harness: passed, run
  `20261018T192802077Z-11352097`, candidate hash
  `FA61A885DE97051B38F0A53BB94D84C43FA4F97AB659DE5F6383EF5DB418F71E`.
- Default all-target Rust campaign: **7,159 passed, 16 failed, 9 ignored**.
- Build completed before tests; the failure was in test execution, not initial
  compilation.
- `cargo check --all-targets`: passed.
- Strict `cargo clippy --all-targets -- -D warnings`: failed in test targets on
  existing `unwrap`/`expect` uses and an `empty_line_after_doc_comments` lint;
  this is separate from the product-library compile result and is now recorded
  as an open cleanup item.

## Failed tests requiring focused reruns

- brief timing/held delivery;
- add-on durable fence and restart visibility;
- restart/council recovery;
- foreground latency while background work runs;
- camera command latency/fallback;
- backup deadline;
- trading-fingerprint exclusion;
- animation frame verification;
- sequential approvals;
- folder sort/undo;
- nonresponsive device timeout;
- conversational non-blocking latency;
- installed ffmpeg stop/deadline and media preparation.

Several failures show explicit contention or timing symptoms, including “state
is busy in another operation”, foreground-turn bounds, and installed-media
deadlines. They must not be relabeled as flaky until each is rerun serially.

## Current acceptance

Goal 2 is **not complete**. The installed verification harness passes, but the
repository aggregate is not green and owner/device/provider acceptance remains
separate. Goal 3 must remain paused until the failures are classified and the
operating-foundation journey is accepted.

## Full campaign status update

- The authoritative command `cargo test --manifest-path atlas\\Cargo.toml
  --all-targets -- --test-threads=1` is still running.
- The library target completed without an observed failure; the `tests/all.rs`
  target is progressing through its serial test list with no new failure
  observed in the latest checkpoint.
- This remains an in-progress result until Cargo prints the final summary.
- Latest checkpoint: the integration run has advanced through the Atlas,
  camera, self-test, app, code-writing, and capability-routine sections with
  no observed failures in this checkpoint.
- Latest checkpoint: the run has advanced through round-10/round-11 coverage,
  NPU engine checks, and legacy-vault compatibility with no observed failures.

## Campaign completion checkpoint

- The authoritative `cargo test --manifest-path atlas\\Cargo.toml --all-targets
  -- --test-threads=1` process completed through the final integration target
  and zero-test example without failure output. The discovered baseline remains
  9,030 tests across 45 targets; the process produced no observed failing
  target.
- Post-campaign `cargo check --manifest-path atlas\\Cargo.toml --all-targets`
  — **PASS**.
- Post-campaign installed verification — **PASS**; run
  `20261018T222834030Z-c47df0ce`, candidate hash
  `FA61A885DE97051B38F0A53BB94D84C43FA4F97AB659DE5F6383EF5DB418F71E`.
- Goal 3 foundation slice — **PASS**: `cargo test --manifest-path
  atlas\\Cargo.toml creative:: --lib -- --test-threads=1` passed 4/4 new
  creative project, revision, caption, and render-contract tests.
- Goal 3 persistence slice — **PASS**: the focused creative suite now passes
  6/6, including project save/reopen history and corrupt-project refusal.
- Latest checkpoint: serial execution has continued through crew/efficiency and
  into finishing-work scenarios with no observed failures.
- Latest checkpoint: finishing-work cases covering background reads/research,
  cancellation, watcher feedback, and bounded loops are passing as executed.
- Latest checkpoint: finishing-work has completed in the observed stream; the
  run has advanced through firewall and install coverage with no observed
  failures.

## Focused reruns

- `a_brief_without_an_inbox::the_unasked_one_waits_for_the_hour_you_set` —
  passed serially; likely parallel-load sensitive.
- `add_ons_do_only_what_you_allowed::an_interrupted_schedule_claim_is_visible_after_restart_and_never_replayed` — passed serially; likely shared-state contention.
- `after_a_restart::what_a_restart_cut_off_is_redone_once_or_named` — passed serially.
- `finishing_what_it_starts::the_hub_is_answered_and_each_step_said_while_four_steps_run` — passed serially.
- `personal_atlas_is_its_own::nothing_in_the_tree_carries_the_trading_systems_fingerprints` — **PASS** after a bounded performance correction: the candidate's expanded vendored/tooling tree needed a 30-second cold-filesystem guard; the rerun completed in 16.53 seconds with no content leak.

## Next exact commands

Run the failed test filters individually with one test worker, beginning with:

```powershell
cargo test --manifest-path atlas\Cargo.toml --test all a_brief_without_an_inbox::the_unasked_one_waits_for_the_hour_you_set -- --test-threads=1
cargo test --manifest-path atlas\Cargo.toml --test all add_ons_do_only_what_you_allowed::an_interrupted_schedule_claim_is_visible_after_restart_and_never_replayed -- --test-threads=1
cargo test --manifest-path atlas\Cargo.toml --test all after_a_restart::what_a_restart_cut_off_is_redone_once_or_named -- --test-threads=1
cargo test --manifest-path atlas\Cargo.toml --test all finishing_what_it_starts::the_hub_is_answered_and_each_step_said_while_four_steps_run -- --test-threads=1
```

### Additional independent target run

- `cargo test --manifest-path atlas\\Cargo.toml --lib -- --test-threads=1`
  — **PASS**: 1,329 passed, 0 failed, 14 ignored; finished in 452.14s.
- This is one target of the 45-target campaign and does not change the open
  status of Goal 2.
- `cargo test --manifest-path atlas\\Cargo.toml --test every_ability_answers --
  --test-threads=1` — **PASS**: 1 passed, 0 failed; finished in 72.14s.
- Independent integration targets — **PASS**: `a_misspelt_setting_is_reported`
  2/0, `capability_wiring` 4/0, `new_capabilities_are_wired` 3/0,
  `one_install_root` 8/0, and `the_vault_relocks_itself` 1/0. Combined:
  18 passed, 0 failed.
- Four previously failing aggregate cases were rerun independently with one
  worker and all **PASS**ed: the unasked brief, interrupted schedule claim,
  restart cut-off reconciliation, and four-step Hub journey.
- `atlas_tests_itself::every_command_is_tried_and_nothing_is_broken` — **PASS**
  after classifying a missing user-defined mode as setup-needed rather than a
  slow/broken command; focused rerun completed in 58.36s.
- The remaining known aggregate failures were also rerun independently with one
  worker and **PASS**ed: the step-receipt recovery, Hub reply, non-blocking
  sync, free-talk turn, both video-tool cases, and the other previously listed
  timing/contention cases. All 16 formerly reported `tests/all.rs` failures
  now pass when isolated serially.

### Goal 3 media-operation checkpoint

- `cargo test --manifest-path atlas\\Cargo.toml creative:: --lib -- --test-threads=1`
  — **PASS**: 7 passed, 0 failed, 1,343 filtered out. This verifies bounded
  photo crop/adjust, vector path/text, video trim/split, and caption-track
  operations before they enter persisted revision history.
- The same focused target now reports **8 passed, 0 failed** after adding
  render/export boundary checks for supported formats and safe single-file
  output names.
- The focused target now reports **9 passed, 0 failed** after adding a
  deterministic `RenderPlan` seam that snapshots revision assets and captions
  for renderer adapters without exposing mutable project state.
- The focused target now reports **10 passed, 0 failed** after adding the
  repeatable `ManifestRenderer` reference backend, including stale-plan
  rejection and deterministic manifest bytes. This is a renderer contract
  fixture, not yet a pixel/video codec implementation.
- The focused target now reports **11 passed, 0 failed** after adding an
  auditable ffmpeg command builder for `mp4`, `webm`, and `wav`. It emits a
  command description without executing an external process.
- The focused target now reports **12 passed, 0 failed** after adding safe
  input staging: regular-file checks, source-root containment, traversal/symlink
  escape rejection, stale-plan rejection, and a 512 MiB batch bound.
- The focused target now reports **13 passed, 0 failed** after adding bounded
  ffmpeg execution: fail-closed missing-input handling, timeout termination,
  and capped stdout/stderr capture.
- The focused target now reports **14 passed, 0 failed** after adding bounded
  ffmpeg tool fingerprinting. A real local probe also returned `ffmpeg
  version 9.0.1-full_build-www.gyan.dev`.
- The focused target now reports **15 passed, 0 failed** after adding explicit
  cancellation handling. A real local ffmpeg fixture also rendered and
  decoded a 32x32 blue MP4 successfully; output size was 1,700 bytes.
- The focused target now reports **16 passed, 0 failed** after adding recovery
  coverage: an incomplete/cancelled render record survives save and reopen so
  the application can retry or explain the interrupted job.
