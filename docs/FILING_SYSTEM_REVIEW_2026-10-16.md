# Filing system review and singular-candidate refactor

**Reviewed:** 2026-10-16  
**Mode:** migrate/review first; no file moves or archival applied yet  
**Survey:** 9,549 files, 66 proposed document moves, nothing deleted

## Executive finding

The filing system itself is substantially implemented, but the repository is not yet a singular, clean Atlas candidate. The current documentation layer knows that distinction; the worktree and installed device do not yet embody it.

The immediate merge target is therefore not “merge every historical document.” It is:

1. preserve the full local Atlas capability work;
2. separate canonical current truth from historical records and disposable artifacts;
3. create one reproducible candidate from the local changes;
4. build and install that exact candidate;
5. run the independent verification harness against its hash.

## Implemented

- Root AGENTS.md contains a generated facts card.
- docs/STATUS.md is the owner-written current status.
- docs/INDEX.md is the generated open-item, session, and document index.
- .filing/ledger.jsonl records work append-only.
- .filing/filing.mjs provides status, facts, render, check, item, close, survey, apply, undo, archive, lesson, and merge-lane commands.
- CLAUDE.md and GEMINI.md import the shared AGENTS.md.
- .gitattributes marks the ledger and index for union merging.
- The filing check detects stale references, broken links, missing item homes, cold-start gaps, and competing work claims.
- The owner readiness reset and independent verification-harness direction are filed as current work.

## Partially implemented

- The card and index answer current status and open work, but the cold-start contract still has unanswered standard questions.
- Historical documents are indexed, but many remain in competing root-level and atlas/docs/ families.
- The filing system has merge support, but the source tree has not yet been consolidated into one reviewed candidate commit.
- The verification harness has a real-runtime baseline runner, but not yet the full capability contract and variation ledger.
- docs/STATUS.md names the current foundation, but does not yet name one immutable candidate build as the source of runtime truth.

## Not implemented or not proven

The facts scan currently reports:

- no repository-defined schedules;
- no declared output directory;
- no declared backup destination;
- no current runbook for restore/recovery;
- no current README identified by the filing facts;
- no environment template identified;
- 45 inherited stale-document warnings;
- no GitHub commit containing the current 139-file local change set;
- no installed build corresponding to the current local worktree;
- no full real-runtime capability campaign.

These are not all code defects. Some are missing filing facts, some are historical cleanup, and some are external runtime proof still waiting on a candidate build.

## Survey findings requiring owner-aware treatment

The survey found competing historical families for:

- outstanding documents;
- handoff documents;
- plan documents;
- dated current-session records;
- legacy atlas/docs/archive/ material.

It also found 66 medium-confidence proposals to move root historical documents into docs/. Those moves are not automatically desirable: many are audit history, some paths are pinned by code or build tooling, and moving them would create churn without making runtime truth clearer.

The authoritative current structure should be:

- AGENTS.md — generated facts card;
- docs/STATUS.md — owner-approved current state and next command;
- docs/INDEX.md — generated ledger/index;
- docs/sessions/ — dated evidence records;
- docs/archive/ — byte-preserved superseded documents;
- verification/ — independent runtime verification and run artifacts.

Historical root files should only move or archive when their pinned status and replacement are known.

## Singular-candidate refactor

The candidate boundary should be:

    canonical source + tests + required config/assets
            |
            v
    one reviewed integration commit
            |
            v
    immutable executable + manifest + SHA-256
            |
            v
    installed device build and verification ledger

The following must not be part of the candidate source snapshot:

- .claude/worktrees/;
- wt-* checkouts;
- target* build directories;
- generated run output that is not a source fixture;
- abandoned bundles, logs, and scratch state;
- historical documents that have not been selected as current.

They must be preserved or archived according to the migration plan, not silently deleted.

## Merge order

1. Freeze new feature work.
2. Inventory and classify the 139 tracked changes and untracked additions.
3. Run source integrity checks and targeted tests.
4. Create a recoverable snapshot of the complete local state.
5. Decide which local source changes belong to the candidate.
6. Commit the candidate as one coherent integration change.
7. Re-render and check the filing system.
8. Build the candidate and record its hash.
9. Install only that candidate.
10. Run the verification harness and compare every result against the candidate manifest.

No reset, blind deletion, or remote merge should happen before step 5.
