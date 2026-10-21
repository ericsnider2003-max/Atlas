# Singular-candidate classification — 2026-10-18

**Status:** reviewed classification; no migration applied  
**Scope:** current `candidate/2026-10-17-foundation` worktree  
**Purpose:** define what belongs to the Atlas candidate without destroying preserved history or evidence.

Last updated: 2026-10-20

## Candidate boundary

The candidate consists of the tracked repository source, tests, required configuration/assets, the independent verification harness, and current authoritative documents:

- `atlas/`
- `verification/Invoke-AtlasVerification.ps1`
- `.filing/`
- `AGENTS.md`
- `docs/STATUS.md`
- `docs/INDEX.md`
- current readiness, handoff, harness, inventory, and execution-plan documents in `docs/`

The candidate executable is identified separately by its manifest and SHA-256. Runtime outputs are evidence, not source.

## Classification of preserved material

| Material | Classification | Treatment |
|---|---|---|
| `atlas/` source/tests/config/assets | canonical | remains candidate source |
| `.filing/`, `AGENTS.md`, `docs/STATUS.md`, `docs/INDEX.md` | canonical control plane | update only through the filing workflow where applicable |
| current `docs/` readiness, handoff, harness, inventory, and plan files | canonical documentation | preserve and keep linked from the current index |
| `verification/Invoke-AtlasVerification.ps1` | canonical verification | preserve as the independent runner |
| `verification/runs/` | append-only evidence | preserve; never overwrite a prior run |
| `verification/launch-diagnostics/` | evidence/diagnostics | preserve until an owner-approved retention policy exists |
| `docs/sessions/` and `docs/archive/` | historical record | preserve; do not treat as current instructions |
| `roundtrip/`, `target_*_continuation/` | continuation/experimental material | preserve outside the candidate source boundary |
| `.claude/worktrees/`, `wt-*` | parallel or historical worktrees | preserve; no merge or deletion without a separate decision |
| root-level legacy handoffs, gap files, bundles, logs, and scratch outputs | historical or disposable, currently uncertain | do not move/delete automatically; retain for owner review |

## Decision

The workspace is already logically singular at the source boundary even though it is physically dirty. The untracked material is not an additional Atlas candidate; it is preserved history, evidence, or continuation work. No cleanup or integration should be inferred from this classification.

## Remaining filing-review blocker

The candidate can be integrated or published only after a reviewed change inventory identifies which uncommitted control-plane/evidence files should be committed with the candidate. This is a review/staging decision, not a code defect. Until that decision is made, the safe state is:

1. keep the current candidate branch;
2. keep evidence append-only;
3. do not merge, delete, or bulk-archive;
4. continue product/runtime fixes only in the canonical source lane.

