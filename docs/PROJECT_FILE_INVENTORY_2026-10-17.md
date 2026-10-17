# Atlas project file inventory — 2026-10-17

This is the reviewed inventory summary for the full handoff. It records what exists and why it remains preserved. It is not permission to delete or move anything.

## Measured workspace totals

The inventory was measured from `C:\Users\erics\Atlas\atlas-publish` on 2026-10-17:

| Area | Count / state | Meaning |
|---|---:|---|
| Git-tracked files | 1,963 | Candidate repository contents already tracked by Git |
| Untracked files | 8,385 | Historical docs, outputs, worktrees, continuation material, and disposable evidence; preserved |
| Verification run directories | 26 | Append-only runtime evidence directories currently present |
| Filing survey files | 2, outside repo | Read-only plan and Markdown summary at `C:\Users\erics\Atlas\atlas-publish-filing-survey.*` |
| Candidate worktree | 1 | `candidate/2026-10-17-foundation` at `8229439a` |
| Additional Git worktrees | 19 | Historical/parallel branches and detached snapshots; not candidate source |

The Git status is intentionally dirty because the workspace contains preserved untracked material. “Dirty” does not mean the canonical candidate source is ambiguous; the handoff above identifies the candidate explicitly.

## Canonical areas

- `atlas/` — canonical Atlas source, tests, assets, configuration, and build output.
- `verification/Invoke-AtlasVerification.ps1` — independent real-process verification harness.
- `verification/runs/` — append-only candidate manifests, JSONL ledgers, stdout/stderr, clean Hub responses, disposable configs, and runtime artifacts.
- `docs/STATUS.md` — owner-written current state.
- `docs/ATLAS_FULL_HANDOFF_2026-10-17.md` — complete operational handoff.
- `docs/ATLAS_VERIFICATION_HARNESS.md` — harness contract and evidence boundary.
- `docs/OWNER_READINESS_RESET_2026-10-16.md` — readiness and owner acceptance gate.
- `.filing/` — filing ledger and generated-document tooling.
- `AGENTS.md` — generated facts card; never hand-edit it.

## Preserved historical and parallel areas

The following are intentionally left in place because they may contain source context, prior evidence, or recoverable work:

- root-level dated session, merge, handoff, backlog, and gap documents;
- `docs/archive/`, `docs/sessions/`, and untracked dated project records;
- `.claude/worktrees/` and `wt-install-23bff923/`;
- `roundtrip/`, `target_capability_wiring_continuation/`, and `target_foundation_continuation/`;
- `verification/launch-diagnostics/` and older `verification/runs/`;
- all additional Git worktrees listed by `git worktree list`.

The filing survey reported 206 proposed moves, 9,839 pinned items, 1,917 duplicate groups, 7 owner decisions, 1 secret-named item, 0 Windows hazards, 0 broken links, and 511 stale references. Those findings require owner decisions before migration. No survey proposal has been applied.

## Worktree snapshot

The candidate worktree is:

```text
C:\Users\erics\Atlas\atlas-publish [candidate/2026-10-17-foundation] @ 8229439a
```

Other worktrees are historical or parallel branches. They are not part of the singular candidate and must not be merged or deleted without a separate reviewed decision.

## Evidence retention rule

Keep every verification run, including failures and superseded passes. The latest committed run is:

```text
verification/runs/20261017T233822200Z-d6a88b52/
```

The run is tied to candidate hash `FA61A885DE97051B38F0A53BB94D84C43FA4F97AB659DE5F6383EF5DB418F71E` and installed executable `C:\Users\erics\AppData\Local\Atlas\atlas.exe`.

## Cleanup decision

No cleanup is authorized by this inventory alone. A future cleanup must first review the external survey, classify each proposed move as canonical/history/disposable/uncertain, preserve hashes, and use the filing migration workflow with an undo path.
