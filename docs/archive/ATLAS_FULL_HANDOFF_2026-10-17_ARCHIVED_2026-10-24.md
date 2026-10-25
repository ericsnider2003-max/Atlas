# Atlas full handoff — singular candidate and complete workspace accounting

**Prepared:** 2026-10-17
**Purpose:** give the next agent or session one complete operational handoff without requiring chat memory.
**Authority:** this handoff describes the current workspace; `docs/STATUS.md` remains the short current-status card, and `docs/OWNER_READINESS_RESET_2026-10-16.md` remains the owner acceptance gate.

## Start here

Work only from:

```text
C:\Users\erics\Atlas\atlas-publish
```

Read, in order:

1. `AGENTS.md`
2. `docs/STATUS.md`
3. this file
4. `docs/OWNER_READINESS_RESET_2026-10-16.md`
5. `docs/ATLAS_VERIFICATION_HARNESS.md`
6. `docs/ATLAS_FULL_VALIDATION_PLAN.md`
7. `docs/PROJECT_FILE_INVENTORY_2026-10-17.md`

Do not use older handoffs as current authority. They remain historical evidence.

## Canonical candidate

- Repository: `C:\Users\erics\Atlas\atlas-publish`
- Branch: `candidate/2026-10-17-foundation`
- Canonical source revision: `32b7f8bef77c2dc67af4cadea8522b6afaeef70f`
- Current evidence/documentation tip: `8229439a candidate: assert hub response shapes`
- Remote default observed in the prior handoff: `origin/main` at `d3d5bca2e69bb6284ac513d306c736d3f8c125ca`
- Candidate executable: `atlas\target\release\atlas.exe`
- Installed executable: `C:\Users\erics\AppData\Local\Atlas\atlas.exe`
- Installed version: `atlas 0.1.0-dev`
- Candidate and installed SHA-256: `FA61A885DE97051B38F0A53BB94D84C43FA4F97AB659DE5F6383EF5DB418F71E`

The executable source and installed executable are hash-identical. Do not claim that any later unbuilt source change is installed.

## What is proven on the installed candidate

Latest run: `verification/runs/20261017T233822200Z-d6a88b52/`.

It passed 14 substantive real-process contracts against the installed executable and a disposable `ATLAS_HOME`:

- executable identity;
- machine diagnosis with observed memory and disk lines;
- durable queue enqueue;
- authenticated Hub token handoff;
- clean authenticated `/health` response with exact `ok` body;
- clean authenticated `/hub` response containing the Atlas page title;
- clean authenticated `/hub/live.json` response with object-shaped JSON;
- daemon cold-start liveness;
- daemon forced-stop/restart liveness;
- queue replay after daemon restart;
- queue replay in a separate process;
- approval-gated `remote start 1 --yes` transition;
- completion and completed-state replay;
- missing-request failure reporting.

The ledger also records daemon working-set samples. This proves the named contracts only; it does not prove the complete capability catalogue, visual accessibility, provider readiness, or owner acceptance.

Earlier committed evidence runs remain append-only. The most recent sequence is recorded in `git log` and under `verification/runs/`; failed and superseded runs are intentionally preserved.

## Source and test state

- The full serialized default aggregate previously passed 7,175 tests, failed 0, ignored 9, across 7,184 tests.
- The serialized lean aggregate previously passed 7,021 tests, failed 0, ignored 8.
- Default and lean compile gates and strict library Clippy gates were green in the prior validation pass.
- `.cargo/config.toml` sets one test worker because process-global clock/settings and real-tool tests are unsafe to overlap.
- The five regressions found in the unsafe parallel invocation passed under the canonical serialized command.
- The current pass did not change Atlas product source; it changed the independent verification harness and evidence/docs.

## Explicitly unproven or still open

These remain open and must not be represented as closed:

1. Visual UI and accessibility acceptance; the computer-use runtime failed to initialize with `The system cannot find the path specified`.
2. A guided owner working session and explicit owner acceptance.
3. Full settings matrix: default, on, off, invalid, live changes, persistence, restart, migration, and combinations.
4. Complete UI surface/state/keyboard/screen-reader matrix.
5. The remaining 150 source-backed capabilities not run through real runtime contracts.
6. Real provider/model/speech/voice/desktop/device/phone/sync/update integrations where dependencies are unavailable or not yet exercised.
7. Memory-pressure, disk-pressure, network-loss, permission-loss, provider-failure, corruption, update, rollback, and machine-restart campaigns.
8. Policy normalization for the live-state restore evidence.
9. Full feature and phone-model lanes gated by missing LLVM/Clang or other dependencies.
10. Private mesh remains not built.

The installed candidate is therefore still **not ready to call an operating system**.

## Preserved workspace boundary

No files were deleted or broadly moved during this handoff. The current workspace intentionally contains historical documents, scratch outputs, continuation targets, verification runs, and worktrees. They are preserved because the filing survey found 9,839 pinned items and 1,917 duplicate groups; automatic cleanup would risk destroying source or evidence.

The current inventory is summarized in `docs/PROJECT_FILE_INVENTORY_2026-10-17.md`. The full read-only survey is outside the repository at:

```text
C:\Users\erics\Atlas\atlas-publish-filing-survey.json
C:\Users\erics\Atlas\atlas-publish-filing-survey.md
```

Do not commit the full survey blindly. It is a planning artifact for a later owner-reviewed migration.

## Exact next work order

1. Preserve this candidate and do not add product features.
2. Run the installed minimum operating-foundation journey in `docs/OWNER_READINESS_RESET_2026-10-16.md`.
3. If computer-use becomes available, capture visual Hub/Settings/navigation/accessibility evidence against this exact hash.
4. Otherwise, continue expanding the independent runner with safe HTTP/CLI contracts and mark visual work unverified.
5. Build a settings matrix and capability contract inventory; do not mark source-backed capabilities as passed without real evidence.
6. Record each external dependency as `blocked_external`, not passed.
7. Run `node .filing/filing.mjs render`, then `node .filing/filing.mjs check` after status/doc changes.
8. Never use `git reset --hard`, `git checkout --`, broad deletion, or blind cleanup.

## Useful commands

```powershell
Set-Location C:\Users\erics\Atlas\atlas-publish
node .filing/filing.mjs status
node .filing/filing.mjs facts --json
node .filing/filing.mjs check --cold
pwsh -NoProfile -File .\verification\Invoke-AtlasVerification.ps1 -AtlasExe 'C:\Users\erics\AppData\Local\Atlas\atlas.exe'
git status --short --branch
git log --oneline -12
```

## Handoff truth

The singular candidate is now identifiable, install-hash-matched, independently exercised, and evidence-backed at its tested boundaries. It is not yet owner-accepted as a dependable operating system. The next session must continue closing installed proof gaps, not restart architecture or treat the large historical workspace as a second candidate.
