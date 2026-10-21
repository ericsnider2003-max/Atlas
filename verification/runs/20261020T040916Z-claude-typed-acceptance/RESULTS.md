# Typed acceptance run, 2026-10-20T04:09Z (Claude)

Handoff: `docs/CLAUDE_DESKTOP_ATLAS_ACCEPTANCE_HANDOFF_2026-10-20.md`
Candidate: `C:\Users\erics\AppData\Local\Atlas\atlas.exe`, SHA-256 `9DB1759459BF14B8DD3C989A7B89135CC8E921EE141533D8988DFD3806234392` (matches handoff).
Profile: disposable `ATLAS_HOME` = `atlas-home/` in this folder. Working directory `cwd/`.

## Evidence boundary

Desktop control was NOT available: the computer-use request for Atlas, File Explorer and Notepad was refused because this session ran as a scheduled run ("can't be approved during a scheduled run"). The typed requests below were sent through Atlas's own typed console (`atlas.exe` with no arguments, one line per request), which is the same `prompt_line` path a typed request takes. Visual UI, tray, window and Hub-page appearance remain **unverified**. Nothing here is owner acceptance.

## Steps

| Step | Request (typed) | Visible response | Elapsed | Result |
|---|---|---|---|---|
| 1 Launch | `atlas.exe` (console) | "atlas ready. type a command, 'help', or 'quit'." | 2.5 s | pass (console surface only; window/tray unverified) |
| 2 Create file, try 1 | `Create a text file named atlas-test.txt in my Desktop folder containing the current date, then verify the file.` | "It's 9:09 PM on Monday 19 October" | 30.3 s | **fail**: no file created (checked `C:\Users\erics\OneDrive\Desktop`, the real Desktop folder, and `C:\Users\erics\Desktop`) |
| 2 Create file, try 2 (same text) | same | "Opening Notepad to create the file." | 4.0 s | **fail**: no file, no Notepad process found 5 s later |
| 3 Explain/progress/verify | (depends on 2) | n/a | n/a | fail (blocked by 2) |
| 6 Bounded failure | `read the file ...\cwd\does-not-exist.txt` | "I can't read it because ... doesn't exist." | 36.3 s | pass on cause; no recovery path offered; slow |
| 4 Follow-up context | `what did I just ask you to do?` | "You asked me to read ...does-not-exist.txt, and I confirmed that it didn't exist." | 4.8 s | pass (context retained one turn) |
| 5 Restart reconciliation | not re-run here | see installed verifier runs (queue start/done/replay across restart) | | covered by `Invoke-AtlasVerification.ps1`, not by a live background job |
| 7 Hub states | not run (no desktop control) | | | unverified |
| 8 Backup/restore | not re-run | see `docs/ATLAS_DISPOSABLE_RESTORE_2026-10-17.md` | | prior evidence only |
| 9 Camera/audio/video | not run (no desktop/hardware consent in a scheduled run) | | | unavailable (environment) |

## Root causes found

1. **No language model is installed.** `atlas-home/data/logs/atlas.log`: "couldn't start the model server: no language model in the models folder fits this machine". `C:\Users\erics\AppData\Local\Atlas\models` holds only voice/vision ONNX files; a search of `C:\Users\erics` found no `.gguf` file over 100 MB. Free-form requests therefore cannot be understood locally. Fix needs a ~2.5 GB model download (`atlas get`), which needs Eric's approval.
2. **Typed requests went to free online services and were logged as `model: none`.** With no local model and `models.online_second: true` (installed `config/tools.yaml:556`), `models.rs` (~1805-1811) routes every call to `freeonline::FreeOnline` (Kilo, Pollinations, OVH). `atlas-home/data/state/model-calls.jsonl` shows 4 calls, prompts of 1,429-4,506 characters, all labelled `"model":"none"`. Eric's real profile `data/state/model-calls.jsonl` has 215 of 244 calls labelled "none" (last on 17 Oct). Whether this should happen is Eric's decision; the mislabel is a truthfulness defect.

## Repair made (reproduced 4/4 before changing code)

`atlas/src/daemon/model.rs`: `model_in_use` now records `"free-online"` instead of `"none"` when there is no local model and `online_second` is on (helper `label_without_local_model`, unit test `model_label_tests`). See the session summary for build, install and verifier results.

## After the repair

- Rebuilt with `cargo +1.98.1-x86_64-pc-windows-msvc build --release` (34 min). New SHA-256 `ACEF04E191EC1727E0D1C3852416EE32CCA4905498AC8A99BB375B0231FBA03D`, installed to `C:\Users\erics\AppData\Local\Atlas\atlas.exe`. Rollback: `atlas.exe.before-claude-label-20261020` (the 9DB17... build). The older `atlas.exe.pre-charter-20261020` rollback is untouched.
- Installed verifier run `20261020T053206687Z-5981f03b`: 14/14 passed. (Run `20261020T050614391Z-ca86aab3` had 3 Hub HTTP failures caused by the runner, not Atlas: PowerShell 7 is not installed and Windows PowerShell 5.1 did not load `System.Net.Http`; rerun with `Add-Type -AssemblyName System.Net.Http` preloaded.)
- Label confirmed on the installed build (profile `atlas-home-2/`): the same file request was logged `"model":"free-online"`, took 77 s, and failed truthfully ("no free online model answered ..."). Still no file created, because there is no local model.

## Tests after the repair (MSVC toolchain, serial)

- `--lib`: 1,346 passed, 0 failed, 14 ignored.
- `--test all`: 7,171 passed, 4 failed, 9 ignored (1,015 s).
  - 3 media failures (`audio_enumeration::asking_the_real_machine...`, `atlas_looks_when_you_ask::it_asks_once...`, `::watching_for_a_while`): "program not found" for ffmpeg. Rerun with `C:\Users\erics\AppData\Local\Atlas\tools\ffmpeg` on PATH: 22 passed, 0 failed. Environment, not source.
  - 1 real failure, not from this repair: `nothing_from_any_version_was_lost` reports `NEW_CHAT_START_HERE.md` and `OUTSTANDING_2026-09-26.md` missing. Both were moved to `docs/archive/*_ARCHIVED_2026-10-19.md` by another session's uncommitted archive pass; the test does not recognise the renamed copies. Needs that archive pass to add the moves to `atlas/tests/fixtures/what_each_version_built_dropped.tsv` or teach the test the `_ARCHIVED` naming.
- Repair left uncommitted, like the rest of the working tree (Goal 1 owner decision covers committing it).
