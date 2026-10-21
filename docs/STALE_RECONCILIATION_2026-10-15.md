# Filing stale-reference reconciliation — 2026-10-15

Last updated: 2026-10-20

This is the first work item from the open-work audit. It records every
warning emitted by the canonical cold filing check without treating historical
documents as current instructions.

## Result

`node .filing\\filing.mjs check --cold` is green with 45 warnings. The warnings
belong to 15 older documents. No current `docs/STATUS.md`, `docs/INDEX.md`, or
`docs/OPEN_WORK_AUDIT_2026-10-15.md` warning was emitted.

The warnings fall into two classes:

- **Missing historical paths:** old handoff/catalog/runbook links point at
  files that were moved or superseded.
- **Changed-since-true:** the referenced source or workflow changed after the
  old document's recorded truth date.

Neither class is a reason to rewrite history automatically. The safe next
action is to preserve the old document, add a replacement pointer where a
current reader might encounter it, and archive or repair only after confirming
that no active handoff still depends on the old path.

## Complete warning inventory

| Historical document | Warning evidence | Classification | Safe next action |
|---|---|---|---|
| `00_START_HERE.md` | 7 missing handoff/catalog paths; 5 referenced files changed after 2026-09-29 | both | add a supersession pointer, then archive after owner review |
| `06_SESSION_2026-09-22.md` | missing old state-of-play handoff; capabilities changed | both | preserve as session history; point to current status |
| `10_ENCODER_AND_CONTEXT_2026-09-22.md` | `embed/README.md` changed | changed-since-true | preserve; no source rewrite |
| `11_SESSION_2026-09-22_backlogs_context_primitive.md` | `embed/README.md` changed | changed-since-true | preserve; no source rewrite |
| `27_MERGE_HANDOFF_2026-09-26.md` | 4 missing handoff paths; 5 source/test files changed | both | replace with a current merge note after integration target is chosen |
| `27_WSHOBSON_AGENTS_FOR_ATLAS_2026-09-25.md` | `THIRD_PARTY_NOTICES.md` changed | changed-since-true | preserve; verify licensing separately |
| `29_MERGE_HANDOFF_2026-09-26_first_package_rebuilt.md` | 1 missing improvements path; 4 source/test files changed | both | preserve; replace after branch integration decision |
| `33_MERGE_2026-09-26_three_chats.md` | capability source/docs changed | changed-since-true | preserve; current status is authoritative |
| `36_PHONES_WITHOUT_A_MAC_2026-09-26.md` | Android/iOS workflows changed | changed-since-true | preserve; validate against current workflows before device work |
| `37_MERGE_2026-09-27_26b_and_friends.md` | missing old state-of-play handoff | missing historical path | add current-status pointer |
| `38_HANDOFF_2026-09-27_windows_ios_android.md` | missing catalog path | missing historical path | add current-status pointer |
| `NEW_CHAT_START_HERE.md` | 4 missing paths; 5 source/config/docs changed | both | replace as a current entry point, preserve old copy |
| `OPEN_GAPS.md` | missing improvements handoff path | missing historical path | leave as historical gap record; link current audit |
| `OUTSTANDING_2026-09-26.md` | missing dead-capabilities catalog | missing historical path | leave as historical record; use current catalogue |
| `WINDOWS_SIGNING_TODAY.md` | Windows workflow changed | changed-since-true | validate against current workflow before signing work |

## Deliberately not automated

- No historical file was deleted or moved.
- No missing path was recreated with guessed content.
- No source, branch, worktree, or Replica path was cleaned up.
- The broad filing survey was discarded because it proposed 66 moves across
  9,517 files, including generated/session material; it is not a trustworthy
  work plan for this repository.

## Next executable step

Create a short current entry-point replacement that links to `docs/STATUS.md`,
`docs/INDEX.md`, and the open-work audit, then re-run the filing check. The
remaining historical warnings should be retired in small owner-reviewed
batches, not through a bulk archive.

