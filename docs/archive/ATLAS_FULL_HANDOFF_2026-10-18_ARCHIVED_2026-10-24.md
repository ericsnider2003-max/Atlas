# Atlas full handoff — singular candidate, installed proof, and filed work

**Prepared:** 2026-10-18  
**Purpose:** one durable handoff containing the work completed in the current continuation and the exact remaining boundaries.  
**Authority:** `docs/STATUS.md` is the short current card; this file is the full operational handoff; `docs/OWNER_READINESS_RESET_2026-10-16.md` is the owner-acceptance gate.

## Start here

```powershell
Set-Location C:\Users\erics\Atlas\atlas-publish
Get-Content .\AGENTS.md
Get-Content .\docs\STATUS.md
Get-Content .\docs\ATLAS_FULL_HANDOFF_2026-10-18.md
node .filing\filing.mjs status
node .filing\filing.mjs check --cold
```

Work only in the canonical `atlas-publish/atlas/` lane. Replica and disposable worktrees are preserved evidence/artifacts, not candidate source.

## Singular candidate identity

- Repository: `C:\Users\erics\Atlas\atlas-publish`
- Branch: `candidate/2026-10-17-foundation`
- Canonical Atlas source is committed; the latest filing/docs consolidation commit is `e5bb594f` (`docs: consolidate canonical filing and session evidence`).
- Installed executable: `C:\Users\erics\AppData\Local\Atlas\atlas.exe`
- Candidate executable: `atlas\target\release\atlas.exe`
- Installed version: `atlas 0.1.0-dev`
- Candidate and installed SHA-256: `FA61A885DE97051B38F0A53BB94D84C43FA4F97AB659DE5F6383EF5DB418F71E`

The installed binary and candidate artifact are hash-identical. Do not claim later unbuilt source changes are installed.

## Work completed in this continuation

1. Read the root and canonical filing system, current status, handoffs, open items, and session records.
2. Confirmed the canonical repository and rejected root-level copies/replicas as source of truth.
3. Ran the independent installed verifier against the exact installed candidate.
4. Recorded run `verification/runs/20261018T074642623Z-b54b15d3/`.
5. Verified 14/14 real-process contracts: identity, machine diagnosis, enqueue, authenticated Hub handoff, clean `/health`, `/hub`, and `/hub/live.json` responses, daemon cold/restart liveness, queue replay, approval-gated start, completion, completed-state replay, and missing-request failure.
6. Ran a disposable-profile installed public-command smoke pass: `index status`, `trace status`, `remote list`, `mobile`, `budget status`, `away check`, `codes list`, `access list`, `afterme where`, and `catalog --platform windows` all returned successfully.
7. Ran installed `selftest --no-model` in disposable `ATLAS_HOME`; 174/174 sentences completed, 0 reported “to fix,” and 13 were waiting on a switch or installation. Report: `C:\Users\erics\AppData\Local\Temp\atlas-smoke-917c5fca-85f1-45b6-89f2-9dde11a860f8\data\selftest\latest.md`.
8. Filed the verification and selftest results through `.filing/filing.mjs` notes for `i922b` and `full-validation`.
9. Committed the canonical filing/session records and ledger as `e5bb594f`; no Atlas product source was changed in this continuation.
10. Verified `cargo fmt --check` and `git diff --check` both pass.

## Installed proof currently established

The exact candidate is independently exercised at the named boundaries above. The latest verifier ledger is append-only and preserves earlier failures/superseded runs. The filing check is green with inherited historical warnings.

This proves the tested contracts only. It does not prove the complete capability catalogue, UI accessibility, settings matrix, live providers, phone/device integrations, or owner acceptance.

## Still open and explicitly unverified

- Visual UI, keyboard, screen-reader, and accessibility acceptance; the computer-use runtime has not been available.
- A guided normal working session and explicit owner acceptance.
- Full settings matrix: defaults, on/off, invalid values, live changes, persistence, restart, migration, and combinations.
- The remaining source-backed capabilities not covered by real external contracts.
- Live provider, speech, voice, desktop, phone, sync, update, and hardware paths where dependencies or accounts are absent.
- Memory/disk pressure, network/provider loss, permission loss, corruption, update/rollback, and machine-restart campaigns.
- Policy normalization for live-profile restore evidence.
- Full-feature and phone-model lanes gated by external toolchains/dependencies.
- Private mesh remains unbuilt.

The installed candidate remains **not ready to call an operating system** until the owner-acceptance gate is completed.

## Workspace boundary

The canonical source and filed docs are committed. The worktree still contains preserved untracked verification runs, launch diagnostics, disposable continuation targets, and `wt-install-23bff923`. These were intentionally not deleted or hidden. They must be classified through the filing workflow before any cleanup or migration; do not use broad deletion, `git reset --hard`, or `git checkout --`.

## Exact continuation order

1. Run `node .filing\filing.mjs status` and `node .filing\filing.mjs check --cold`.
2. Run the installed verifier again only when new evidence is required.
3. Continue the minimum operating-foundation journey in `docs/OWNER_READINESS_RESET_2026-10-16.md`.
4. If UI automation is unavailable, expand the independent runner with safe real CLI/HTTP contracts and mark UI work unverified.
5. Build the settings/capability contract inventory; never promote rehearsal-only or dependency-blocked results to passed.
6. Record each result through `.filing/filing.mjs`, render, and check.
7. Close the next session with:

```powershell
node .filing\filing.mjs close "<summary>" --next "<exact next command>"
```

## Handoff truth

There is one designated, hash-identified Atlas candidate. Its canonical filing and this handoff are now filed. The candidate has meaningful installed runtime proof, but the product is not yet owner-accepted as a dependable operating system.
