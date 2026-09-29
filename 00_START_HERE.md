# Atlas — the complete handoff

> **28 Sep 2026 — this repository is personal Atlas only.** The separate Atlas for the trading work and the files that ran beside it, their history, the root `docs/` exports of the Project chats and the generated `catalogs/` left this repository on this date; lines in the records below about that work were replaced with a marker. Everything removed is kept verbatim in a private extract outside this repository, and the git history before this commit still contains it. `atlas/tests/personal_atlas_is_its_own.rs` keeps it out.


> **27 September 2026: start with `38_HANDOFF_2026-09-27_windows_ios_android.md`.** The branch to take is **`handoff-0927`**. It contains everything below plus `friend-ready`, doc 37 (both of them), and the phone and Windows builds. The current list of what's left is `OUTSTANDING_2026-09-27.md`; per-platform steps are in `platforms/`.

**25 September 2026.** This is everything: all the code, every document, every reference, and how to build it, test it and pick it up. It supersedes every earlier archive and every earlier `00_START_HERE.md` (the 22 September one is kept, verbatim, at `docs/handoff/history/00_START_HERE_2026-09-22.md`).

Nothing here is installed on any machine, and nothing here can place a trade.

---

## 1. Where the code is

| | |
|---|---|
| **The repository** | `~/Atlas/atlas-current` on your laptop (le3o), a git repo. This file is at its root. |
| **The branch to take** | **`round11`** (updated 26 Sep, round 11). It holds everything: the durable base, the main chat's work up to `24bf4b4` (friends reach each other with no Tailscale between people), and this chat's rounds 1–11. `master` is already inside it, so taking it is a fast-forward. |
| **How to take it** | With the main chat idle: `git checkout master` then `git merge --ff-only round11`. If master has moved since `24bf4b4`, use `git merge round11` instead. Every merge from master so far has conflicted only on guard bookkeeping. |
| **Other branches** | `round6` to `round10` and `round8m` are the steps on the way. Each is contained in `round11`, and nothing needs them. |
| **This package** | `ATLAS_COMPLETE_HANDOFF_2026-09-25.zip`. It holds the working tree of `round8m`, plus `atlas-all-branches.bundle`, the full git history of every branch. `git clone atlas-all-branches.bundle atlas` restores it. |

---

## 2. Read in this order

0. **`37_MERGE_2026-09-27_26b_and_friends.md`**: the newest. The Atlas Project chat's 26a and 26b (Atlas answers instead of replying with documents, `32b_…`) merged onto master as branch **`friend-ready`**, the Awaiting Merge folder accounted for, and what it took for Atlas to work on a friend's machine: the model server starting itself, and nobody called Eric.
0. **`35_SESSION_2026-09-26_the_whole_hub_phones_and_accessibility.md`**: the newest work. It covers the rest of the hub built to the locked design, Atlas standing alone on phones (`atlas/mobile/`), the phone and tablet layout rethought for the iPhone Duo and iPads, and accessibility to WCAG 2.2 AA and EN 301 549 (`atlas/docs/ACCESSIBILITY.md`). `34_…` covers the hub design reaching the tree.
0. **`33_MERGE_2026-09-26_three_chats.md`**: the branch to take, `all-merged`. It merges all three chats: master, the 10 Sep rebuild, and the third chat's line (23 Sep to 25h). It says what was in the "Awaiting Merge" folder item by item, and where the chats had built the same thing twice. `29_…`, `28_…` and `27_…` are the earlier steps. The third chat's own records are docs 20–32 with its names (listed in 33 §3), and its start-here is kept at `docs/handoff/history/00_START_HERE_2026-09-25h_third_chat.md`.
1. **This file**: what's where, how to build and test, the rules.
2. **`OPEN_GAPS.md`**: every gap still open, why, what closes it, and who has to act.
3. **`MASTER_BUILD_PLAN.md`**: the plan of record and the completeness map.
4. **`26_SESSION_2026-09-25_round11_the_seventeen_built.md`**: the newest round, with round 10's 17 ideas built. Then `25_…` (round 10), `24_…` (round 9), `23_…` (round 8), `22_…` (round 7), `21_SESSION…` (round 6) and `21_TAILNET…` / `20_PHONE…` (the main chat's phone work).
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
6. **`atlas/docs/CAPABILITIES.md`**: what Atlas can do, where each capability runs, and in what state. It's generated from the built binary.
7. **`docs/handoff/MODULES.md`**: every source and test file with its purpose line.
8. **`docs/handoff/REFERENCES.md`**: everything Atlas was built from: algorithms, specs, papers, open-source projects read, crate licences, external programs, models and data.

---

## 3. What's in the repository

| path | what it is | size |
|---|---|---|
| `atlas/` | **Personal Atlas**, the assistant, in Rust. `src/` has 378 files (186,342 lines); `tests/` has 389 files (113,686 lines, 6,002 tests), plus 736 tests inside `src/`. | |
| `atlas/docs/` | Atlas's own documents: setup (`SETTING_UP.md`, `FIRST_BOOT.md`, `ON_YOUR_PHONE.md`, `VOICE_SETUP.md`), how it works (`WHAT_RUNS_WHERE.md`, `HOW_IT_WORKS_ON_ITSELF.md`, `OFFLINE.md`), the generated `CAPABILITIES.md`, earlier handovers and module references, and `live/` (demo scenes, renders and the round-8 test models) | 61 entries |
| `atlas/config/` | shipped settings: `tools.yaml` (every external tool, every switch), `apps.yaml`, `commands.yaml`, `policy.yaml`, `layouts.yaml`, `indexing.yaml`, `labels/`, `guessable/` | |
| `atlas/ATLAS.bat` | the Windows menu (set up, run, update, build = option 7) | |
| *[row removed 28 Sep 2026: trading-system material]* |
| `embed/` | the meaning encoder (all-MiniLM-L6-v2 via tract): source, Windows and Linux builds, vocab. The 86 MB model is one hash-pinned download (`embed/models/GET_THE_MODEL.md`). | |
| `llm/` | the local language model install path, with a hash-pinned starter GGUF (`GET_THE_MODEL.md`) | |
| `catalogs/` | `CODE_CATALOG.md` and `DEAD_CAPABILITIES.md`, **regenerated 25 Sep** from these trees by `docs/handoff/catalog.py` | |
| `docs/` | the main Atlas Project's documents, 8–23 Sep, verbatim (89 files) | |
| `docs/improvements-project/` | **all 80 documents from this chat's claude.ai project**, verbatim, 10–25 Sep. They were exported from the project itself, not retyped. | 80 files |
| `docs/handoff/` | this handoff's generated inventories (`MODULES.md`, `REFERENCES.md`), the scripts that regenerate them (`catalog.py`, `extract_refs.py`), the published report page (`atlas-improvements-report.html`), and `history/` | |
| `00`–`23_*.md` | the session records, in order (§5) | |
| `OPEN_GAPS.md`, `MASTER_BUILD_PLAN.md` | the open-gaps register and the plan | |
| `MANIFEST.txt` | SHA-256 of every tracked file, retaken for this handoff | |
| `verify.sh` | the one command that says whether the tree is green | |

---

## 4. What Atlas is, in one screen

A personal assistant that runs on your own machines. It works **offline first**, and reaches online only as a second option you switch on. It's built **in house**: the algorithms are written here, and open-source projects were read as references (clean-room, with the licence named in each module's header). Third-party code comes in only as the Rust crates listed in `REFERENCES.md` §5.

- **The core loop.** `atlas/src/daemon.rs` holds the tick and every intent, and `main.rs` the command line. Around the core:
  - Understanding: `brain.rs` and `intent`.
  - The hub: `hub.rs`, `hublive.rs` and `server.rs`, the page you open on your phone.
  - Memory: `facts`, `recall`, `meaning` and `memory`.
  - Voice: `voice`, `voiceid`, `speaker`, `vad`, `diarize` and `hearing`.
  - Sync between your machines: `sync`, `hlc`, `transport`, `cloudsync` and `courier`.
  - Keeping things: `vault` and `recovery`.
  - Background work: `crew`.
  - Making things: `motion`, `filmstrip`, `scene3d`, `meshio`, `gifenc` and `pngcodec`.
  - Mail: `imap` and `mailthread`.
  - Time: `calendar`, `recur`, `cronspec` and `civil`.
  - Self-work: `fixloop`, `selfwork` and `crew`.
- **What it can do and how far each piece has got:** `atlas/docs/CAPABILITIES.md`, where each capability is marked "working", "built, never run for real", or "switched off".
- **The honesty machinery.** The guards (`tests/dead_capabilities.rs`, `dead_methods.rs`, `new_capabilities_are_wired.rs`, `name_collisions.rs`, `catalogue.rs`, `capability_wiring.rs`, `dead_config.rs` and others) fail the build when code is written that nothing reaches, a setting does nothing, or a list of known gaps drifts from the truth. Their constants are listed in `catalogs/DEAD_CAPABILITIES.md`. Each number moves only on purpose, with the reason written beside it.

---

## 5. How it got here: the session records

| file | what happened |
|---|---|
| `02`–`05_SESSION_2026-09-20*` | the second chat's 20 Sep work: orphans, a voice, grants, Cloudflare, coding and auto-delegation |
| `06_SESSION_2026-09-22` | the test layout: **`autotests = false`**, so every test file must be registered (§7) |
| `07`/`08_MERGE_*` | the 22 Sep merge audit, and the completed merge |
| `09`–`12` | search by meaning, the encoder and contextual recall, dead-backlog work, and the local LLM path |
| `13`–`19` | architecture decisions, the decision list wired, pausing one errand, WireGuard to your server, setup readiness, double-click setup and the phone app |
| `20_PHONE_AS_PEER`, `21_TAILNET_SYNC_MOBILE_CORE` | the main chat's phone-as-peer, clock, transport, tailnet sync and GUI-free core (a1b19fb, b70be92) |
| `21_SESSION…round6` | this chat, round 6: rounds 1–5 merged into the durable repo (the build plan had them as "source lost"; they weren't lost); SVG animation to GIF/MP4; the in-house 3-D renderer; the Windows tests run on the real laptop |
| `22_SESSION…round7` | round 7: moving 3-D (keyframes, easing, spin, camera), the renderer rebuilt, checked against Blender 4.2 |
| `23_SESSION…round8` | round 8: 3-D models from files, glass/glow/patterns, a denoiser, lamps aimed at directly, people who speak once, `--people N`, the open-gaps register |
| `25_SESSION…round10` | round 10: 19 gaps a second look found in round 9 and around it (times read sure-but-wrong, reminders dropped in silence, a booking that didn't say when, meetings counted as away, a crash on "İ"), fixed; natural pauses learned per app; the Windows passphrase read in-process; animations refined by word; adaptive sampling built and measured (off: it loses on fine patterns); 17 ranked capability ideas |
| `26_SESSION…round11` | round 11: the 17 ideas built and wired (`workday`): clipboard history (off until you turn it on, memory only), text off the screen by Windows' own OCR, the market's calendar, waiting-for from your sent mail, dated capture and a weekly review, a launcher, the trading check-in, meeting prep, snippets without a keylogger, find any file, an in-house PDF merge/split/sign, a personal CRM, feeds, receipts, habits by strength, FSRS flashcards, checked local translation, and key chords; your day in the brief |
| `24_SESSION…round9` | round 9, the working day: Atlas tells working from away (the keyboard and Windows' own interruption state), `worklog` (where the time went), offers held for a natural break (bounded deferral), a cue for what you were in the middle of when you come back, `when` (times in words), and six workflow bugs fixed |

**This chat's rounds 1–5** (21–24 Sep) are recorded in `docs/improvements-project/`:

| round | doc | what |
|---|---|---|
| *[row removed 28 Sep 2026: trading-system material]* |
| 4 | `round4-gaps-and-ideas-23sep.md` | the open gaps from rounds 1–3 closed |
| 5 | `round5-ideas-built-24sep.md` | 13 more: push-to-talk, a speaker check, your own wake phrase, the hand-off loop (`atlas fix`), the vault opened at sign-in, and others |

**Commits on `round8m`, oldest first:**

- **The durable base:** 4a1164a.
- **The main chat:** ba4da22, 952cf93, 5108c81 and 3c87755.
- **Rounds 1 to 5:**
  - a0e645e: round 1.
  - df33485: round 3.
  - 06e1257: round 4.
  - f1c206a: round 5.
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
  - 68f6b5d: merge fix-ups.
- **Round 6:** 2462bd3, and 551742c for the merge of 3007af9/7ec88ee.
- **Round 7:** 76ed4f9.
- **Round 8:** f785cf0 … c0f6f65, with a1b19fb merged in at b913666.
- **The b70be92 merge:** f6fe3cb.
- **The handoff commits after it.**

---

## 6. Building

**Windows (your laptop).** `atlas\ATLAS.bat`, option 7, builds Atlas, but **it can't yet on le3o**. The Rust toolchain there is `stable-x86_64-pc-windows-gnu`, which needs MinGW's `dlltool.exe`, and that isn't installed (`OPEN_GAPS.md` 1.1). Either:

- install Visual Studio Build Tools (C++ workload), then `rustup default stable-x86_64-pc-windows-msvc`, or
- install MinGW-w64 and put its `bin` on PATH.

**Linux (and the cloud workspace):**

```
cd atlas && cargo build --release            # atlas binary
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
cd embed && cargo build --release
```

Features in `atlas/Cargo.toml`: `desktop-ui` (the eframe window) and `onnx` (on-device vision). Both are on by default. `--no-default-features` builds the GUI-free core the phone path uses.

**Cross-building Windows from Linux** (how every Windows binary this week was made):

```
rustup target add x86_64-pc-windows-gnu      # plus mingw-w64 on the Linux box
cd atlas && cargo build --release --target x86_64-pc-windows-gnu
cargo test --release --no-run --target x86_64-pc-windows-gnu --test all   # a test .exe to run on Windows
```

**Models and pieces.** Atlas downloads and SHA-256-checks its own pieces on first run: whisper.cpp, the whisper model, piper, a voice, and ffmpeg (`atlas/src/getpieces.rs`). The embedder and LLM are the two hash-pinned downloads in `embed/` and `llm/`. Blender is optional.

---

## 7. Testing

```
./verify.sh                    # both crates, the full suite, guards first; exits non-zero if anything is red
./verify.sh --quick            # lib tests, guards, warnings (~2 min)
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
cd embed && cargo test
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
```

- **Register every new test file.** `atlas/Cargo.toml` has `autotests = false`: a `tests/*.rs` that isn't registered never runs, and nothing says so. Add it to `tests/all.rs` as `#[path = "x.rs"] mod x;`, or as a `[[test]]` target. There are 30 targets; `voice_measured` and `vad_measured` run in `--release`.
- **Retake the manifest after changing personal Atlas:**

  ```
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
  ```

- **Regenerate after changing capabilities:**
  - `atlas catalog --markdown > atlas/docs/CAPABILITIES.md`, using the freshly built binary.
  - `python3 docs/handoff/catalog.py` for the catalogues.
  - `python3 docs/handoff/extract_refs.py` for `MODULES.md` and the reference lists.
- **Blender comparisons.** `tests/round7.rs` and `tests/round8.rs` render the same scene in Blender when there is one. They find it with `find_blender`: the tools setting, `Program Files\Blender Foundation\*`, or PATH. Without Blender they say "skipped".

**The last full run**, on `round8m` before the handoff commits:

| suite | result |
|---|---|
| personal Atlas, 29 debug targets | 6,441 passed, 0 failed |
| personal Atlas, release voice targets | 8 passed, 0 failed |
| *[row removed 28 Sep 2026: trading-system material]* |
| round-8 tests, natively on the laptop | 10/10 |

§11 records the run on the final handoff commit.

---

## 8. The standing rules

These are the rules the work has followed. Keep to them.

1. **In house, minimal third party.** Offline first, online second. A new crate is a ruling, not a convenience.
2. **Research first, then build and test everything that can be built, and name what's blocked and why.** Don't stop at a plan.
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
4. **Click, not type**, for anything Eric does. No design may depend on Eric keeping a secret.
5. **Eric doesn't run cargo himself.** `ATLAS.bat` option 7 is his build path.
6. **Don't install software on the laptop unasked** (Blender, MinGW, Build Tools). Document instead.
7. **Don't put things on his screen unasked** (toasts, windows).
8. **The main chat commits to `master` directly.** Deliver as branches, and never disturb master's working tree.
9. **Deleting files on the laptop needs his approval**, every time.
10. **If a web fetch is refused, don't fetch it another way.**

---

## 9. Things learned the hard way about the environment

- **The cloud workspace:**
  - It restarts roughly every 20–30 minutes and kills background jobs. Run long jobs detached, keep each run short, and commit work in progress often.
  - Never kill processes with a pattern that can match your own command line. That killed the shell twice.
  - A full debug build needs about 9 GB. Clear `target/` when disk runs short, and use `CARGO_INCREMENTAL=0`.
- **Moving work to the laptop:**
  - Files can't be written under `.git`. Use `git bundle`: write the bundle into a git-excluded folder, then `git fetch <bundle> branch:branch`.
  - The laptop's `python3` is the Microsoft Store placeholder, so tests must check that `python3 --version` actually runs.
  - The laptop's Rust can't link (§6), so Windows binaries are cross-built and run natively there.
- **Blender:**
  - Blender 4.2's official `bpy` wheel works as a stand-in for testing, behind a two-line `blender` wrapper that runs `python3 <script>`.
  - As a Python module it crashes on exit after the glTF importer. The generated scripts end with `os._exit(0)` for that reason.
- **The guards read source text:**
  - A name inside a string (for example Python inside a Rust string) can make an unrelated function look called.
  - A new function with the same name as an unused one elsewhere makes the unused one look reached.
  - Both happened this week. `name_collisions.rs` and `dead_capabilities.rs` catch them.

---

## 10. What's still open

`OPEN_GAPS.md` has all of it, in four groups:

- **Needs you:**
  - a working build toolchain on the laptop;
  - taking `round8m`;
  - optionally Blender;
  - the inbox app password;
  - your real voice and room;
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
  - a local coding model;
  - a phone;
  - 10 rulings.
- **The main chat's:**
  - 247 functions that only tests reach;
  - hub conformance to the 29 screens;
  - `contents::Contents`.
- **Measured, not solved:**
  - splitting one person in two on call notes;
  - real recordings;
  - the denoiser's small gain.
- **Limits of what's built:**
  - no image textures;
  - no caustics;
  - one-bounce light;
  - no conversational refinement of animations.

---

## 11. The final check on this handoff

The final run is on the `round11` tip (rounds 8–11, merged with master `24bf4b4`). The full table is in `26_SESSION…` §Gate.

| suite | result |
|---|---|
| personal Atlas, debug targets | **6,724 passed, 0 failed** (2 ignored: need whisper and piper), after merging master `24bf4b4` |
| *[row removed 28 Sep 2026: trading-system material]* |
| round 11 natively on Windows (le3o) | see `26_SESSION…` §6 |
| guards | green; `CAPABILITIES.md`, `catalogs/`, `docs/handoff/MODULES.md` and `REFERENCES.md` regenerated |

---|---|
| personal Atlas, debug targets | **6,527 passed** in the full run; its 7 failures (this round's guard and test bookkeeping) fixed and those targets re-run: **6,233 passed, 0 failed**. After merging master `6b7e938`: **6,475 passed, 2 failed** (two add-on permissions for this chat's intents, and the catalogue to regenerate), both fixed and re-run green. **Final, after merging master `773454a`: 6,607 passed, 0 failed** |
| personal Atlas, release voice targets | **8 passed, 0 failed** |
| *[row removed 28 Sep 2026: trading-system material]* |
| rounds 9 and 10 natively on Windows (le3o) | **25/25**; the passphrase read in-process on the real console |
| *[row removed 28 Sep 2026: trading-system material]* |
| guards | green; `CAPABILITIES.md`, `catalogs/`, `docs/handoff/MODULES.md` and `REFERENCES.md` regenerated |

---

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
