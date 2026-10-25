# Atlas chat handoff — 2026-10-18

## Start here

Work only in `C:\Users\erics\Atlas\atlas-publish`.

Read in this order:

1. `AGENTS.md`
2. `docs/STATUS.md`
3. this handoff
4. `docs/PLAN_COVERAGE_REGISTER_2026-10-18.md`
5. `docs/OPERATING_FOUNDATION_TRACE_2026-10-18.md`
6. `docs/OWNER_READINESS_RESET_2026-10-16.md`

Do not use root-level historical handoffs or parallel worktrees as current authority.

## Current candidate

- Branch: `candidate/2026-10-17-foundation`
- HEAD: `f1e9baa2` (`docs: trace foundation requirements to proof slices`)
- Previous candidate-boundary commit: `f24aa667`
- Installed executable: `C:\Users\erics\AppData\Local\Atlas\atlas.exe`
- Installed/candidate SHA-256: `FA61A885DE97051B38F0A53BB94D84C43FA4F97AB659DE5F6383EF5DB418F71E`
- No product source changes were made in the latest slice; the durable foundation was verified as already implemented.

The worktree is intentionally dirty because preserved historical plans, sessions, worktrees, and verification artifacts remain untracked. Do not clean, move, delete, stage, or merge them blindly.

## What was completed

1. Created and committed the efficiency plan and candidate-boundary classification.
2. Created and committed the plan-coverage register so original plans remain preserved and mapped to implementation status.
3. Created and committed the operating-foundation trace mapping all eight readiness requirements to modules, tests, and evidence gaps.
4. Verified the shared durable job/restart implementation with the focused test command:

   `cargo test --locked --lib job::tests -- --test-threads=1`

   Result: **22 passed, 0 failed**.

5. Ran the installed runtime harness:

   `pwsh -NoProfile -File .\verification\Invoke-AtlasVerification.ps1 -AtlasExe 'C:\Users\erics\AppData\Local\Atlas\atlas.exe'`

   Result: **14/14 contracts passed**.

6. Updated the filing ledger for `system-audit`, `i922b`, and the filing-review work. Cold filing check is green with 55 historical warnings.

## Evidence

Latest runtime ledger:

`verification/runs/20261018T055342147Z-0d8aba7d/results.jsonl`

It is tied to the installed hash above and covers identity, machine diagnosis, Hub health/page/live responses, daemon start/restart, durable queue replay, approval-gated start, completion, completed replay, and missing-request failure.

## Important quality boundary

The completed slice is high-confidence for the tested durable foundation. Atlas as a whole is not yet owner-accepted or proven as a dependable operating system.

Still open:

- owner normal-session acceptance;
- real UI/accessibility and responsiveness proof;
- memory/resource behavior during useful work;
- full settings and capability-family matrix;
- live voice, provider, device, phone, sync, update, and desktop-action proof;
- full-feature toolchain lane requiring `libclang.dll`;
- policy normalization for live-state restore evidence;
- 55 inherited historical filing warnings;
- final review of which current evidence/docs belong in a future candidate commit.

## Plan preservation rule

Nothing in the original plans is discarded. Plans and requirements are classified as implemented, in progress, blocked, queued, or explicitly superseded with a reason. Historical copies remain preserved; implementation occurs in the canonical `atlas/` lane.

## Next exact work

1. Re-read the plan register and foundation trace.
2. Select the next smallest missing foundation slice from the trace, not from an old backlog.
3. Map it to existing modules before changing code; avoid duplicate ownership.
4. Add or identify one focused regression test.
5. Implement only that slice.
6. Run the focused test with one test worker.
7. Build/install only if source changed and the slice crosses the installed boundary.
8. Run the independent runtime contract.
9. File the evidence and update `docs/STATUS.md`/the relevant filing item.

Useful commands:

```powershell
Set-Location C:\Users\erics\Atlas\atlas-publish
node .filing\filing.mjs status
node .filing\filing.mjs check --cold
git status --short --branch
pwsh -NoProfile -File .\verification\Invoke-AtlasVerification.ps1 -AtlasExe 'C:\Users\erics\AppData\Local\Atlas\atlas.exe'
```

Do not claim the owner gate closed from these automated results. The next owner-scoped action is a short normal working session on the installed candidate.

