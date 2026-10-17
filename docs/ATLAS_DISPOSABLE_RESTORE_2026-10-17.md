# Atlas disposable restore evidence — 2026-10-17

**Status:** real-backup restore contract passed with 125/128 direct file-hash matches; three intentional/live-state differences remain recorded.

## Boundary

This was run against the installed candidate at `C:\Users\erics\AppData\Local\Atlas\atlas.exe`, with `ATLAS_HOME` set to the disposable profile:

    ..\disposable-restore-20261017\

The owner profile was not used as the restore destination. The source backup was the live v2 backup:

    C:\Users\erics\AppData\Local\Atlas\data\backups\state-1792227445\

Its manifest recorded 128 files, 12 directories, and 3 owned roots.

## Contract run

1. Copied the live backup into the disposable profile's `data/backups`.
2. Ran `atlas backups restore 1` through the installed executable.
3. Verified that `data/state/.restore-journal.json` was absent after completion.
4. Independently hashed every manifest entry after restore. Results:
   - calls: 5/5 matched;
   - notes: 5/5 matched;
   - state: 115/117 matched;
   - trash ledger: intentionally preserved by restore, not overwritten.

Result: **passed with explicit live-state exceptions** for the installed real-process, disposable-profile, manifest-backed restore contract: 125/128 entries matched directly.

## Limits

The three non-matching entries are recorded rather than hidden: `capabilities_used.json` and `last_turn_of_yours.json` were mutated by startup after restore, while the trash ledger is intentionally retained as live recovery state. This did not restore the owner profile. The disposable profile retains an archived recovery directory, as expected for preserved recovery material. Owner acceptance and a policy decision about live-state hash normalization remain open.
