# Merged candidate: desktop e2e fixes + outstanding fixes, 2026-10-21 (Claude)

Merged into `candidate/2026-10-17-foundation`: `fix/desktop-e2e-1020` (e50e271f) and `fix/outstanding-2026-10-21` (84ff14b1), with no textual conflicts. The training branch was not merged.

- Merge review (usage-max reviewer): no semantic conflict found in tick/safety/tune/model. One gap fixed: low-memory close suggestions now respect the never-close list `tune.keep` (5be237dd).
- Full suite, all targets, under heavy machine load: 9,155 passed, 20 failed. 3 failures need a system ffmpeg (not installed on this PC). The other 17, re-run alone: 116/116 passed (load timing). See `full-suite-summary.txt`.
- Release build SHA-256 `B0CA6B2F85DBD1C261FE9ACAE7EF126281DC86C575897B453EC7FD979B589427`; installed-runtime verifier **14/14** (`installed-verifier-results.jsonl`).
- Installed to `C:\Users\erics\AppData\Local\Atlas\atlas.exe`. Rollback: `atlas.exe.before-merge-20261021` (3B38DE03…). Atlas left stopped.
