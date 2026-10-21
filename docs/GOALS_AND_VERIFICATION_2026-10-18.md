# Atlas goals and verification order — 2026-10-18

## Goal 1 — one authoritative candidate

Use the installed candidate described in `docs/STATUS.md` as the current
verification target. Preserve all evidence until it is classified. Do not merge,
delete, archive, or discard worktrees without an owner decision.

Done when the source revision, installed executable, verification directory,
and preserved/untracked artifacts are listed and agree.

## Goal 2 — prove the operating foundation

Run every local test available today:

1. filing status and cold check;
2. default and lean compile/test/Clippy gates;
3. installed verification harness;
4. backup/restore and restart/replay evidence;
5. available Windows process, Hub, and HTTP health checks;
6. failure, cancellation, and recovery tests;
7. resource and disk/memory diagnostics.

Each result must be recorded as passed, failed, blocked, or unverified. A
passing test suite does not substitute for owner acceptance of the installed
workflow.

## Goal 3 — select the first product capability

Only after Goal 2 is accepted, begin the Creative Workspace Core:

`import assets -> compose a simple image/vector/layout -> create a variant ->
export -> reopen and recover project history`

The first slice must be offline-capable, bounded, recoverable, and backed by an
Atlas-owned project/asset record. Do not start multiple Adobe-style apps in
parallel.

## Today’s exact verification command

```powershell
pwsh -NoProfile -File .\verification\Invoke-AtlasVerification.ps1 -AtlasExe 'C:\Users\erics\AppData\Local\Atlas\atlas.exe'
```

## Evidence boundary

Codex can run repository, installed-process, filesystem, HTTP, and scripted
verification locally. It cannot honestly replace owner acceptance, physical
phone tests, live provider credentials, signing credentials, or subjective UI
acceptance. Those remain explicitly unverified until exercised in their real
environment.
