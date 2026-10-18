# Atlas execution-efficiency plan

**Status:** draft for owner approval  
**Date:** 2026-10-17  
**Objective:** turn Atlas work into short, user-visible proof cycles instead of broad implementation batches.

## Position

Atlas is moving slowly because engineering, verification, filing, and owner/device proof are being mixed together. The next milestone is not more capability code. It is one dependable installed operating loop, proved end to end, with a small number of repeatable commands.

## Operating rules

1. One canonical lane: `atlas-publish`, one active branch, no new worktrees unless an isolated experiment is explicitly approved.
2. One vertical slice at a time: start → request → visible result → background completion/failure → restart → recovery.
3. No broad feature expansion until the minimum operating foundation is accepted.
4. Run the narrowest relevant check first. Run the full serialized suite only at a release checkpoint.
5. Every claim is labeled `source-tested`, `installed-tested`, `device-tested`, or `owner-accepted`.
6. No repeated prose handoffs: `docs/STATUS.md` is the current state; verification ledgers are evidence; historical notes stay historical.
7. No cleanup, merge, deletion, or provider/device action without an explicit decision and rollback path.

## Bottlenecks to remove

| Bottleneck | Correction | Measure |
|---|---|---|
| Parallel worktrees and duplicate branches | Freeze new worktrees; maintain one candidate lane | zero unapproved active worktrees |
| Large full-suite runs | Focused test → installed contract → owner journey | time per proof slice |
| Slow or unsafe builds | one Cargo worker, reuse target, low-priority release build only when needed | build duration and peak memory |
| Documentation drift | update only `docs/STATUS.md` plus the evidence ledger | one current next command |
| Source proof mistaken for usability | maintain separate evidence boundaries | every gap has one evidence class |
| Feature surface too broad | freeze integrations/content/replica work | no new P1 feature work before acceptance |

## Phases

### P0 — establish the fast lane

**Goal:** make the next sessions cheap and unambiguous.

**Builds:** none. Reconcile current candidate identity, preserve existing changes, list only active blockers, and define one verification command per slice.

**Done when:** `docs/STATUS.md` names one branch, one executable hash, one next proof, and one owner action.

**Verify:** `node .filing/filing.mjs status` and `node .filing/filing.mjs check --cold`

### P1 — prove the minimum operating loop

**Goal:** prove what a normal user can do on the installed laptop.

**Journey:** launch → typed request → visible truthful result → background work → Hub status → completion or durable failure → restart → replay/recovery.

**Done when:** each step has an installed-process ledger entry and the owner can complete the journey without debugging Atlas.

**Verify:** `pwsh -NoProfile -File .\verification\Invoke-AtlasVerification.ps1 -AtlasExe 'C:\Users\erics\AppData\Local\Atlas\atlas.exe'`, followed by the short owner journey in `OWNER_READINESS_RESET_2026-10-16.md`.

### P2 — remove the highest-friction failures

**Goal:** fix only failures observed in P1, in user-visible order: responsiveness/memory, truthful status, restart/recovery, safe backup/restore, then voice.

**Rule:** each fix gets one focused regression test and one installed re-check. If it does not improve the journey, stop and revert the slice.

**Done when:** the same journey passes twice on the installed candidate, with measured memory and response observations.

### P3 — acceptance and controlled expansion

**Goal:** leave the reset only when the owner accepts the operating foundation.

**Then:** promote remaining gaps to `closed`, `blocked`, or `deferred`; choose at most one next capability family; build it as another vertical slice.

**Never:** resume broad integrations or catalog-wide implementation because source tests are green.

## Capacity and cut line

Use 60–90 minute proof slices. Reserve 20–30% for recovery and filing. If a slice needs more than one build, one broad test run, or more than one unresolved design decision, split it. To save time, cut new integrations, replica research, content tooling, and nonessential catalog cleanup first.

## Immediate next action

Do P0 only, then run P1 with the already-installed candidate. Do not modify product code until the first failing user-visible step is identified. The next implementation task must be the smallest change that makes that step pass.

## Owner decisions requested

1. Approve freezing feature expansion until P3 acceptance.
2. Approve one canonical branch/worktree and no parallel implementation lanes.
3. Approve judging progress by installed user journeys, not test count.

