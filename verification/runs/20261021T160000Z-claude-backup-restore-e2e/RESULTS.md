# Backup and restore, end to end — 2026-10-21 (Claude)

Candidate: a copy of the installed `atlas.exe`, SHA-256 `B0CA6B2F…` (merged candidate 6af6238b), in `bin/`. Profile: disposable `ATLAS_HOME` = `home/`. Throwaway source folder `src/` (3 files: `a.txt`, `b.bin` 300 KB random, `sub/c.md`), destination `dest/`. Driven through Atlas's typed console (`atlas.exe`, one line per request) and its CLI. Not owner acceptance; the off-laptop copy (F2) still needs Eric.

| Step | What was done | Result |
|---|---|---|
| 1 Folder backup | Typed `back up the folder <src> to <dest>` | Reply at once: "Backing up … in the background … I'll tell you when it's done and checked". Copy made in ~2 s at `dest/src backup 2026-10-21 10-46-01/`. **Hashes match** for all 3 files (`src-hashes.txt` vs `dest-hashes.txt`). Job record `jobs.json`: `succeeded`, "3 files, 293.0 KB. Checked: same file count and the same SHA-256 for every file. Nothing already there was touched." Hub job: Done. **Pass.** |
| 2 Follow-up | Typed `is the backup of src finished?` | Reply: "I'm preparing your brief." **Wrong answer** (no local model; the question is not matched to the job). Also the promised "I'll tell you when it's done" never appeared in the console within 60 s (it is recorded in the Hub). Both are in the no-model command floor and announcements, owned by the reliability session; reported there, not changed here. |
| 3 State backup | Typed `back up everything` | "Backed up 40 files for you." `data/backups/state-1792604854/` (41 entries incl. manifest, 16 KB). **Pass.** |
| 4 List | `atlas backups list` | Printed `0: 1792604854 — 40 files, 0 bytes`. **Bug:** size never measured (always 0), raw timestamp. **Fixed** (below). |
| 5 Restore | Overwrote `data/state/memory.json` with junk, deleted `data/state/jobs.json`, ran `atlas backups restore 0` | "Restored 40 file(s)." `memory.json` byte-identical to before (`cmp`), `jobs.json` back with its original hash. **Pass.** |

## Fix made

`atlas/src/safety.rs`: `list_backups` now measures each backup's real size with the same rules as the file count (`real_files_and_bytes`, skipping half-written `.part-` files and the manifest). `atlas/src/main/everyday.rs`: the list reads "taken 5 minutes ago — 40 files, 16.0 KB". Test: `safety::backup_size_tests::a_backup_lists_its_real_size_not_zero`.

## Still open

- Off-laptop copy of a backup (F2): needs Eric (a USB drive or cloud folder he chooses).
- Restore is CLI-only; no Hub button or spoken intent.
- Follow-up questions about a running or finished job, and the completion announcement in the console (step 2): reliability session.
