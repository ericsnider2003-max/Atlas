# Canonical integration review — 2026-10-15

## Current state

The canonical `main` worktree is at `d3d5bca2e6`, with 76 pre-existing
uncommitted paths before this session's documentation/comment changes. The
tracked foundation diff is currently 77 files, approximately 1,474 insertions
and 375 deletions. There are also untracked continuation targets, generated
capability material, filing records, and managed worktree directories.

Replica is out of scope and was not inspected for integration.

## Change groups observed

| Group | Evidence | Current treatment |
|---|---|---|
| Capability evidence/catalogue | `atlas/src/capability.rs`, generated capabilities, wiring tests | source-backed; retain pending owner-selected integration target |
| Durable jobs/evidence | `atlas/src/job.rs`, `atlas/src/evidence.rs`, daemon/task/hub changes | source-backed; focused tests already green |
| Local knowledge/file safety | contents, facts, meaning, store, daemon reading/index paths | source-backed; 34 index tests green |
| Sync/handoff/provider/phone wiring | connecting, clients, phone, hub, update and related tests | synthetic gates green; real device/provider proof remains external |
| Generated/config/feature gates | `Cargo.toml`, generated capabilities, `.gitignore`, workflows | requires regeneration and final diff review before commit |
| Filing and handoff records | `.filing/`, `docs/`, root agent instructions | current session records; preserve until integration is decided |

## Verification already obtained

- Focused index, council, and restart tests: 79 passed, 0 failed.
- Latest canonical aggregate recorded in filing status: 7,166 passed, 0 failed,
  9 ignored.
- Default and lean all-target compile gates and strict library Clippy gates are
  green.
- Full-feature validation is still blocked by the missing `libclang.dll`
  dependency of `llama-cpp-sys`.
- Capability evidence projection tests: 7 passed.
- Capability wiring tests: 4 passed.
- The wiring ceiling did not reveal an uncalled capability in this pass; the
  remaining `Untested` state is therefore primarily runtime/device/provider
  evidence, not a blanket missing-call-site defect.

## What can be done without an integration decision

1. Review each change group for duplicate ownership and generated-file drift.
2. Run the narrowest affected test target after each group review.
3. Check feature boundaries and generated capability output.
4. Produce a proposed commit/integration order for owner approval.

## What must not be done implicitly

- Do not merge branches or delete managed worktrees.
- Do not stage or commit the existing foundation changes as one undifferentiated
  commit.
- Do not claim installed executable, device, provider, or live-model behavior
  from synthetic tests.

## Next work item

Continue with the capability triage: verify which `Untested` entries have a
source-only acceptance test, which are genuinely missing code, and which are
only waiting on hardware/provider/owner evidence. Then promote only the real
repo-workable gaps into implementation slices.

