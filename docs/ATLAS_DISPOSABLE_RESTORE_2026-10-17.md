# Atlas disposable restore evidence — 2026-10-17

**Status:** one real-backup restore contract passed; full restore acceptance remains open.

## Boundary

This was run against the installed candidate at `C:\Users\erics\AppData\Local\Atlas\atlas.exe`, with `ATLAS_HOME` set to the disposable profile:

    verification/disposable-restore-20261017/

The owner profile was not used as the restore destination. The source backup was the live v2 backup:

    C:\Users\erics\AppData\Local\Atlas\data\backups\state-1792227445\

Its manifest recorded 128 files, 12 directories, and 3 owned roots.

## Contract run

1. Copied the live backup into the disposable profile's `data/backups`.
2. Ran `atlas backups restore 1` through the installed executable.
3. Verified that `data/state/.restore-journal.json` was absent after completion.
4. Verified that three restored Markdown notes matched their manifest SHA-256 values exactly:
   - `Call notes 2026-09-29 20-11 Discord.md`
   - `Call notes 2026-10-01 20-40 Discord.md`
   - `Call notes 2026-10-05 21-35 Discord.md`

Result: **passed** for the installed real-process, disposable-profile, manifest-backed restore contract.

## Limits

This does not close owner acceptance of restore. It sampled three restored files rather than independently hashing all 128 entries, and it did not restore the owner profile. The disposable profile also retains an archived recovery directory from the earlier safe restore attempt, as expected for preserved recovery material.
