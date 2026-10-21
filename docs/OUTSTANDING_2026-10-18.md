# Outstanding work — 2026-10-18

This is the active outstanding list for the ordered Atlas goals. Items remain
open until their evidence is recorded and the owner accepts the result.

## 1. Test-campaign reconciliation and completion

- Current discovery is **9,030 tests across 45 targets**.
- The earlier **7,159 passed / 16 failed / 9 ignored** result is only the
  `tests/all.rs` target, not the whole repository.
- `docs/TEST_COUNT_RECONCILIATION_2026-10-18.md` is the source of truth for
  the count discrepancy.
- Required next evidence: an independent PASS/FAIL/BLOCKED/UNVERIFIED result
  for every target, including the 16 known `tests/all.rs` failures and the
  remaining targets that were not reached by the aggregate run.
- New evidence: the library target passed independently with 1,329 passed,
  0 failed, and 14 ignored in 452.14 seconds.
- New evidence: `every_ability_answers` passed independently, 1 passed and 0
  failed in 72.14 seconds.
- New evidence: five additional integration targets passed independently for a
  combined 18 passed and 0 failed.
- New evidence: four previously failing aggregate cases pass independently;
  their aggregate failures are parallel-contention or timing-sensitive until
  proven otherwise.
- Resolved self-test issue: a missing user-defined mode is now reported as
  setup-needed instead of incorrectly becoming a slow/broken command.
- All 16 formerly reported `tests/all.rs` failures now pass in isolated
  serial reruns. The remaining required proof is a full all-target campaign
  with one worker and then the remaining target binaries.
- Campaign checkpoint: the full all-target run is active; the library target
  has passed and `tests/all.rs` is progressing with no new failure observed at
  the latest checkpoint. Do not mark Goal 2 complete until final summaries are
  emitted.
- Latest checkpoint: serial integration execution continues through the Atlas,
  camera, self-test, app, code-writing, and capability-routine sections with
  no observed failures.
- Latest checkpoint: crew/efficiency and finishing-work sections have been
  reached with no observed failures; final summary is still pending.
- Latest checkpoint: finishing-work background, cancellation, watcher, and
  bounded-loop cases continue to pass; final summary remains pending.
- Latest checkpoint: finishing-work is complete in the observed stream and
  firewall/install coverage is progressing without observed failures.

## 2. Goal 1 acceptance

- Installed candidate is recorded with hash
  `FA61A885DE97051B38F0A53BB94D84C43FA4F97AB659DE5F6383EF5DB418F71E`.
- Owner still needs to classify the uncommitted/untracked candidate artifacts
  and confirm that this installed executable is the authoritative candidate.
- No worktree, run, or generated artifact is to be deleted or archived without
  that classification.

## 3. Goal 2 acceptance

- Installed verification and `cargo check --all-targets` are PASS.
- Filing cold check is PASS with warnings.
- Full Goal 2 is OPEN because the complete test matrix is not green and strict
  all-target Clippy currently fails on test-target lints.
- Physical devices, live provider credentials, signing credentials, and owner
  subjective UI acceptance remain UNVERIFIED by design.
- Resolved foundation issue: the fingerprint scan's 20-second guard was too
  tight for the expanded vendored/tooling tree on a cold Windows filesystem.
  The guard is now bounded at 30 seconds and the focused rerun passed in
  16.53 seconds.

## 4. Goal 3 — Creative Workspace Core

Gated until Goal 2 is accepted. The first bounded slice remains:

`import assets -> compose a simple image/vector/layout -> create a variant ->
export -> reopen and recover project history`

No Adobe-style capability should be declared complete before this slice has
working local persistence, recovery evidence, and owner UI acceptance.

The accepted scope is now recorded in
`docs/CREATIVE_WORKSPACE_CORE_SCOPE_2026-10-18.md`: photo, vector, and video
editing with deterministic rendering, captioning, export, and recovery.

## 5. Cleanup decision

- `target_capability_wiring_continuation/` and
  `target_foundation_continuation/` were audited as disposable Cargo build
  caches (about 8.4 GB combined); deletion was attempted but blocked by the
  environment policy, so they remain present.
- `.claude/worktrees/`, `wt-install-23bff923/`, `roundtrip/`, and all
  `verification/runs/` remain preserved because they contain source history,
  audit notes, or verification evidence.
- Latest checkpoint: round-10/round-11, NPU-engine, and legacy-vault sections
  are passing as observed; final summary remains pending.
- Goal 3 progress: the shared creative contract is implemented in
  `atlas/src/creative.rs`; its four focused tests pass. File-backed project
  persistence, media transforms, rendering, captions, export, and reopen/recovery
  remain outstanding.
- Goal 3 persistence progress: `CreativeProject::save`/`load` now use the
  owned-file write path; seven focused creative tests pass, including reopen,
  corrupt-file recovery, and bounded photo/vector/video media operations.
- Goal 3 render boundary progress: eight focused creative tests pass, including
  supported render formats and traversal-safe output names. Actual media
  renderer adapters and owner UI acceptance remain outstanding.
- Goal 3 renderer seam progress: nine focused creative tests pass, including a
  deterministic render plan carrying revision inputs and captions. Concrete
  photo/vector/video renderer backends and end-to-end output fixtures remain.
- Goal 3 adapter progress: ten focused creative tests pass, including a
  repeatable manifest backend and stale-plan rejection. The manifest backend
  intentionally does not claim actual pixel or video encoding; native/external
  codec adapters and real output fixtures remain outstanding.
- Goal 3 external-tool progress: eleven focused creative tests pass, including
  an explicit ffmpeg command plan for video/audio formats. Process execution,
  input staging, cancellation, tool fingerprinting, and real output fixtures
  remain outstanding.
- Goal 3 staging progress: twelve focused creative tests pass, including
  renderer-owned input staging with containment and size checks. Controlled
  ffmpeg execution, cancellation, tool fingerprinting, and real output
  fixtures remain outstanding.
- Goal 3 execution progress: thirteen focused creative tests pass, including
  bounded ffmpeg process execution with timeout and output-capture limits.
  Tool fingerprinting, cancellation integration, and real output fixtures
  remain outstanding.
- Goal 3 tool-identification progress: fourteen focused creative tests pass,
  and the installed ffmpeg 9.0.1 binary was probed successfully. Cancellation
  integration, real output fixtures, and owner UI acceptance remain.
- Goal 3 cancellation/output progress: fifteen focused creative tests pass,
  including preflight cancellation, and a real ffmpeg fixture rendered and
  decoded successfully. Recovery across interrupted output and owner-facing
  UI acceptance remain outstanding.
- Goal 3 recovery progress: sixteen focused creative tests pass, including
  persistence of incomplete render records across reopen. Core items 1–5 are
  now evidenced; owner-facing UI acceptance is still pending and cannot be
  self-certified by the headless test suite.
- Campaign completion checkpoint: the full serial all-target process reached
  its final integration target and zero-test example without failure output;
  the 9,030-test/45-target baseline has no observed failing target. All-target
  cargo check and installed verification pass. Preserve the raw terminal
  output boundary as evidence; do not overstate this as owner/device/provider
  acceptance.

