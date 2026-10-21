# Atlas plan coverage register — 2026-10-18

**Status:** active implementation register  
**Rule:** no plan is deleted, silently superseded, or treated as complete because its document was moved. Every discovered plan remains either linked to implementation, preserved as history, or explicitly blocked.

Last updated: 2026-10-20

## Current implementation lanes

| Lane | Authority | Status | Implementation rule |
|---|---|---|---|
| Operating foundation | `docs/OWNER_READINESS_RESET_2026-10-16.md` | active | finish installed startup, request, Hub, durable work, restart/recovery, resource, backup/restore, and owner-acceptance proof before expansion |
| Efficiency and proof | `docs/EXECUTION_EFFICIENCY_PLAN.md` | active | one vertical slice, focused regression test, installed verification, filing update |
| Independent runtime testing | `docs/ATLAS_VERIFICATION_HARNESS.md` | baseline slice complete | expand contracts by capability family; preserve all results append-only |
| Full validation | `docs/ATLAS_FULL_VALIDATION_PLAN.md` | queued | build settings/UI/capability-family matrix after candidate boundary is stable |
| Foundation implementation | `docs/FOUNDATION_IMPLEMENTATION_2026-10-15.md` and `atlas/docs/BUILD_PLAN.md` | active/partially complete | continue F0/F1/F2/F3+ in dependency order; do not remove declared capabilities |
| System/audit gaps | `docs/ATLAS_SYSTEM_AUDIT_2026-10-17.md`, `atlas/docs/GAPS.md`, `atlas/docs/AUDIT.md` | active backlog | convert each genuine source gap into a small issue with acceptance test and evidence class |
| Replica/research adaptation | preserved under `C:\Users\erics\Atlas\replica\` | queued, not discarded | use as requirements and benchmark research only; implementation remains in canonical `atlas/` |

## Preservation guarantee

The following remain preserved outside the candidate source boundary and are not deleted by filing cleanup:

- root and worktree plan/roadmap/backlog/handoff documents;
- `docs/archive/` and `docs/sessions/`;
- `_pkg/`, `new/`, `replica/`, and historical worktrees;
- verification ledgers, failed runs, and disposable diagnostics.

Their classification as historical or queued does not mean their requirements are abandoned. Before implementation, each plan item must be mapped to one of: implemented, in progress, blocked external/owner, queued, or explicitly superseded by a newer plan with a written reason.

## Implementation process

For each plan family:

1. extract the requirements and preserve the source document;
2. map each requirement to current modules/capabilities;
3. create or identify a focused acceptance test;
4. implement the smallest vertical slice;
5. run the focused source test;
6. build/install only when the slice reaches the installed boundary;
7. run the independent runtime contract;
8. record the result and update the filing item.

## Current next slice

The filing/singular-candidate review is complete as a classification decision but not as a commit decision. The next implementation slice is the operating-foundation journey, while the plan register remains the guard against losing the broader roadmap.

