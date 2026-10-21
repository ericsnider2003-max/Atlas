# Desktop E2E fixes, 2026-10-21 (Claude, usage-max routed)

Source run: `verification/runs/20261021T033717Z-claude-desktop-e2e/RESULTS.md` (2026-10-20 desktop test).
Branch: `fix/desktop-e2e-1020` (worktree `C:\Users\erics\Atlas\wt-e2e1020`), based on `candidate/2026-10-17-foundation` @ `2ce43335`. Not pushed. The candidate branch was not touched.
Owner acceptance: **not claimed**. Everything below was verified by tests, by a disposable-profile run, or by one short real-profile run (start → 150 s → clean stop), with no screen control.

## Step 1: did a fix already exist somewhere?

- Every branch and worktree from October is already merged into the candidate. The only unshipped code is `wt-integ2` (Oct 13 memory redesign WIP, 55 files), which addresses none of these issues and was not used.
- The filing system has earlier source-level claims for #1 (false claims, 10-07), #3 (restart reconciliation, 10-08/10-15) and the tray (10-12). None was verified on the installed build, and the 10-20 run showed all three still broken. The other issues had no earlier fix recorded.
- So every item below is **newly fixed** at the root, except where noted as partly existing.

## Results

| # | Issue | Existing or new | Root cause found | How verified | Still open |
|---|---|---|---|---|---|
| 1 | "On it" / "Backing up… now" with no file, job or failure | Partly existed (a phrase list), but it missed these sentences | Phrase list missed "On it.", "Making…", "Backing up… now". The free-online path streamed the reply *before* the guard ran | `backed::` unit tests with the exact sentences (+ false-positive cases from two reviews); `tests/truthful_replies.rs` through the real daemon | Owner re-run on the desktop |
| 1b | No capable model → plain refusal | New | — | `not_started_because`: names the missing acting model and the next step; "did it finish?" with no job says so | — |
| F | Floor: works with no model, low memory | New | Create/read/move file, folder backup and "what's using my memory" all depended on the model | `tests/the_floor_without_a_model.rs` (19). Over 20 commands: p50 **64 ms**, p95 **192 ms** (status 2.1 s by design). Floor adds ~2 MB private (test process) | Live low-memory path (real model folder) untested end to end |
| F2 | Resident speech recogniser | New | whisper-cli started per utterance | Owned loopback `whisper-server`; 20 synthetic clips. q5_1 (60 MB, made locally): 122 MB working set / 651 MB private, p50 831 ms, WER 4.8%. base.en: 206 MB / 735 MB, 932 ms, 8.3%. Installed q5_1 | Private memory (651 MB) is larger than hoped; `audio_ctx 750` gives ~0.4 s but cuts >15 s clips. The 2.6–3.5 s per turn was not reproduced (start-up was not the main cost) |
| 2 | Hub: no Failed/Done; Waiting≈Blocked; counters disagree | New | Pill, Home and Outstanding used three formulas over different data | One `Counts` from `open_work`; Failed / Interrupted / Done today lanes; distinct icons; Activity markers; self-work hidden. Unit + hub integration tests | Visual check by owner |
| 3 | Running jobs vanish after restart | Partly existed (reconcile code), but it only logged | Downloads/errands wrote no job record; reconcile ran on first tick and wasn't saved; cancelled errands stuck "Running" | Reconcile before the Hub serves; errand job records; Cancelled ends Cancelled. Tests | Live restart during a real download not exercised |
| 4 | 17 MB `index.unreadable.*.bak` every restart | New | Store refused to *load* files over 16 MiB but *saved* any size; index is 17.0 MB | Save/load share a budget; index gets 64 MiB and compact JSON. Disposable + **real profile**: index loaded, nothing set aside, 17.0 → 13.9 MB. 6 old copies (100.5 MB) moved to the **Recycle Bin**, hashes in `evidence/install-result.json` | Copies inside older backup snapshots (`data/backups/state-*`) left untouched |
| 5 | 3.6 GB download at start without asking | New | `keep_everything_here` top-up never asked; code-tool step also ran unasked | Real profile: **0 downloads**; one ask filed: "Atlas can fetch 8 missing pieces (about 16.2 GB)…". Unit tests (once, idempotent, unattended never) | Approve via "finish setting up" or Connections → Download (no Approve button on the Hub item). No metered-network check exists |
| 6 | Camera opens for gestures unasked | New | A waiting question opened the camera every 2 s whenever gaze was on; the gesture setting was checked after the frame | Gestures default off and gate the camera. Real profile: **0 frames** | Profile still has `gaze.enabled: on` (left as Eric set it) |
| 7 | Always-on-top caption overlay covers buttons | New | Default on; centred; captioned everything spoken, including maintenance failures | Default off, smaller, lower, click-through; maintenance failures never spoken. Profile setting switched off (backup `settings.yaml.before-e2e1020-20261021`) | Visual check |
| 8a | "quit Atlas" doesn't quit; no Quit control | New | No quit intent | Intent + Hub header button + window button + `/hub/quit`; stop file checked every 2 s. Real stop 2.4 s | — |
| 8b | No tray icon | New | Icon loaded by the wrong resource id; created late | Fixed id, created right after the lock, outcome logged. Real profile log: "the icon by the clock is up" | Visual check |
| 8c | Mic refusal with no reason | New | Model-authored refusal | Deterministic reason + nearest real abilities. Test | — |
| 8d | Missing file, no next step | New | — | Closest names in the folder + "find the file …". Tests | — |
| 8e | 25–45 s before usable | New | Window opened before the Hub; offline page retried every 30 s | Window waits for the Hub; offline page polls every 1 s. Hub ready: **7.6 s** real profile, 2.5–3 s disposable | — |
| 8f | Talk: Enter doesn't send, jumps to top | New | No key handler; redirect reloads at top | Enter sends (Shift+Enter newline, IME safe); lands on latest message | Visual check |
| 8g | Housekeeping/backup error spam | New | Backup copied Atlas's own locked files; housekeeping failed on files in use; every repeat announced | Volatile files skipped and listed; in-use files counted; partial backups never prune; repeats announced once. Real profile: 0 failures in 2.5 min | Longer real run |
| 8h | Atlas/Settings tabs: transparent two-thirds | New | Window background was translucent | Opaque background; window kept within the work area | Visual check (content column stays left-aligned by design) |
| 8i | Camera errors in jargon | Partly existed (one prefix strip) | Seeing models (faces, objects, screen words) **exceeded the NPU helper's budgets and always failed**; the helper's reason was quoted | Budgets fit the models (bounded); reasons go to the log. Tests | Live camera look not exercised |

## Efficiency findings (from the research session)

| Defect | Result | Verified |
|---|---|---|
| 2 s main-loop cap | Fixed: idle 60 s, battery 180 s, deadline-aware (timers, calendar reminders, sessions); doorbell wakes at once | Pure-function tests; stop-file wake within 2 s |
| Low memory only warns | Fixed: ordered relief (Kokoro, model server, resident ASR last), logged before/after | Unit tests; live effect not measured |
| Kokoro loaded forever | Fixed: 5 min idle unload, lazy reload | Tests |
| ASR restarts per sentence | Fixed: resident server (above) | Live server test + benchmark |
| No efficiency mode | Fixed: EcoQoS + below-normal for errands and their children; never the answer path | Tests |
| GPU memory 0, no NPU | Fixed: DXGI dedicated/shared, NPU detection wired | Tests with injected values |
| Stale-item sweep never called | Wired daily; nothing persists lessons yet, so it finds nothing | Tests |
| Temp folder 778 MB | Not a defect: those were active downloads; `tmp` already cleaned after 10 min | Directory listing |
| 50 program copies | Proposal only: keep 3, reclaim 2.27 GB → `docs/PROGRAM_COPIES_CLEANUP_PROPOSAL_2026-10-21.md`. **Nothing deleted; owner approval needed** | — |

## Tests

- Full suite, all 45 targets, run twice. Final: **9,138 passed**, 27 ignored. The only failures are 3 tests that need a system `ffmpeg`, which is no longer installed on this PC (PATH points to removed WinGet folders). Installed Atlas uses its own `tools\ffmpeg`. 4 timing tests failed only while the release build ran alongside, and pass alone.
- Installed-runtime verifier on the new build: **14/14** (`evidence/installed-verifier-results.jsonl`). Under Windows PowerShell 5.1 it needs `Add-Type -AssemblyName System.Net.Http` first.
- Two reviewer passes (usage-max reviewer); all findings fixed.

## Install

- Installed build SHA-256 `3B38DE03B04A498CA1E1C97662CE379DC94E0ACFF77E3E1203BCF271AB20CADC` (commit `70ec6296`), verifier 14/14. The live real-profile check ran on the previous build `575B64E7…`, which differs only in the seeing-model fix.
- Intermediate copy `atlas.exe.before-e2e1020-r2-20261021` (= `575B64E7…`) can be removed along with the other old copies.
- Rollback: `C:\Users\erics\AppData\Local\Atlas\atlas.exe.before-e2e1020-20261021` (= `ACEF04…`, the build from the desktop test).
- Profile changes, each backed up with the `.before-e2e1020-20261021` suffix: `settings.yaml` overlay off; `tools.yaml` `stt_model` → `models/ggml-base.en-q5_1.bin`.
- Atlas was left **stopped**, as it was found.

## Observed, not changed

- Self-audit with "act without asking" started a `cargo` build as a child of Atlas during the real-profile run. Heavy on the CPU; it is the owner's setting.
- A desktop "Outstanding" panel opened for the download ask stayed open after Atlas stopped. By design ("what Atlas opens for you outlives Atlas"); owner decision.
- The download ask totals 16.2 GB (the deep brain and picture models are large).
