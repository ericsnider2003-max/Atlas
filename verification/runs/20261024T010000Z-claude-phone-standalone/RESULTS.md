# Standalone-first phone: packaging and outbox (24 Oct 2026, Claude)

- Branch: `feat/phone-standalone-2026-10-23` (worktree `wt-phone`), built on `feat/hub-redesign-2026-10-22`.
- Commit: a34a6038. Nothing is pushed.
- Decisions: Eric, 22 Oct (standalone-first). Dispatch, 24 Oct: approved downloading the int8 embedder and Qwen3-0.6B; 1.7B held; newest-wins approved; no edits to hub, hublive or tick.
- Not yet decided by Eric: Android/iOS build tools (Q1) and correction-log sync (Q3). Until then, everything is proven on the laptop.

## What the app now carries

| File | Bytes | SHA-256 | Source (Apache-2.0) |
|---|---|---|---|
| `all-MiniLM-L6-v2-int8.onnx` | 22,972,370 | afdb6f1a…48bdb1 | Xenova/all-MiniLM-L6-v2 @751bff37 |
| `vocab.txt` | 231,508 | 07eced37…2038a3 | same |
| `Qwen3-0.6B-Q4_K_M.gguf` | 396,705,472 | ac2d9771…0d524a | unsloth/Qwen3-0.6B-GGUF @50968a44 |
| **Total** | **419,909,350** | | |
| Later (held): `Qwen3-1.7B-Q4_K_M.gguf` | 1,107,409,472 | b139949c…181897 | unsloth/Qwen3-1.7B-GGUF @d7f544ee |

How the files get into place:

1. `mobile/fetch-bundled-models.sh` fetches the files, checks each one's size and hash, and stops the build on any mismatch. It ran against the downloaded files and reported "have" for all three. The 420 MB cache is gitignored.
2. Android (`build.gradle.kts`, `AtlasCore.kt`) and iOS (`project.yml`, `AtlasCore.swift`) copy the files into `<home>/bundled` once per app version.
3. The core (`src/bundled.rs`) checks every hash, then moves each file into place, so the phone keeps one copy. A file whose hash is wrong is never used, and Atlas says so.

## Measured

- **Real bundle into a fresh phone home:** 3 files, 419,909,350 bytes, hashes checked and moved in 11.6 s. This was a development build; a release build hashes much faster. Afterwards `phonemodel::present` picks `Qwen3-0.6B-Q4_K_M.gguf`, the inbox is empty, and a second start reports every file as already in place.
- **The phone never downloads a model by itself** once the bundled one is in place (`fetch_by_itself` sees `have_one`).
- **Bundled model speed,** laptop CPU only, 4 threads, llama.cpp `llama-bench`:
  - prompt reading: 503 tokens/s;
  - generation: 58.5 tokens/s.
  - For comparison, the laptop's graphics chip (Vulkan) does 3,559 and 86.9.
  - A phone CPU will be slower than the laptop CPU. A real phone test is still pending (Q1).
- **Quality,** offline (`--offline`), greedy decoding, 12 phone-style questions checked automatically (`qcheck.mjs`):
  - Bundled Qwen3-0.6B Q4_K_M: **8/12**, 6.1 s per answer including model load.
  - Laptop Qwen3-1.7B Q8_0 for reference: **8/12**, 8.4 s per answer.
  - Both get basic facts wrong (Sydney given as Australia's capital) and fail clock arithmetic. Time, dates and sums must stay with plain code (the command floor), never the model.
- **int8 meaning layer: does NOT run in Atlas's engine.**
  - tract fails analysing the int8 export: `Failed analyse for node #293 "/Unsqueeze" AddDims`.
  - The input order is the same as the full model's, so this is the graph, not the wiring.
  - Fixed so it fails honestly: the encoder now proves a model compiles when it loads. The int8 file is reported ("can't run here … meaning search is off") instead of loading and then answering every text with nothing.
  - **Decision needed.** Options:
    - (a) ship the fp16 export (about 45 MB; download needs approval; tract support to be proven);
    - (b) add ONNX Runtime to the phone (about 15–20 MB of library; runs int8 natively, and Atlas already uses ort for the laptop's NPU);
    - (c) ship the full-size model (90 MB).
- **Outbox:**
  - The test `with_the_laptop_off_the_phone_answers_queues_honestly_and_it_runs_when_the_laptop_is_back` uses two copies of Atlas sharing a sync folder.
  - With no model, no laptop and no internet, the phone tells the time from its own code.
  - It queues a laptop request saying "is queued for your laptop".
  - "What's waiting for my laptop?" answers `"check what time it is": Queued until your laptop is reachable (not seen yet).`
  - Two syncs into nothing neither lose nor double it.
  - The laptop comes back, takes it and runs it, and the answer reaches the phone. Then "Done on your laptop.", and "anything queued for the laptop" answers "Nothing is waiting for your laptop."

## Tests (all pass)

| Suite | Result |
|---|---|
| lib: `bundled`, `phonemodel`, `meaningnative`, `remote` | 15 passed, 3 ignored (the by-hand real-file runs) |
| `asking_the_laptop_from_the_phone` | 10 |
| `files_remote` | 33 |
| `phone_mode` | 5 |
| `the_phone_stands_alone` | 15 |
| `the_floor_without_a_model` | 20 |
| `sync` | 58 |
| Project checks: `dead_methods`, `dead_capabilities`, `new_capabilities_are_wired`, `no_silent_discards`, `no_quiet_nothings`, `personal_atlas_is_its_own` | pass |
| By hand: the real bundle; the int8 comparison | the bundle passes; the int8 comparison fails as described above |

## Not proven

- **The phone's own llama.cpp engine (`phone-llm`)** was not built. It needs CMake, which falls under Q1. The 4-bit model was proven with the laptop's llama.cpp instead, which uses the same GGUF reader.
- **The Kotlin and Swift changes** have not been compiled: there is no Android SDK, Xcode or CI run here (Q1).
- **No real phone was used** (Q1).

## Staged for Dispatch, not edited (Hub files belong to the reliability session's merge)

1. **Queued lane** (`hublive/settings_and_outstanding.rs`, `open_work`): add each `remote::Queue` request in Waiting or Running state from the phone's `asked_of_the_laptop` store. Use `remote::queue_status(r, laptop_heard, now)` as its reason, with `laptop_heard` worked out as `askthelaptop.rs` does. That puts "Queued until your laptop is reachable (last seen 14:05)" on Home, Now, Outstanding and `live.json`.
2. **Sync page on the phone** (`hub.rs`, `Page::here`): stop hiding `Page::Sync` in phone mode. The page also needs to show last sync, pending outbox items, unsent changes, and newest-wins replacements with undo. That is sync work 3, still to build.
3. **Offline page note** (`hub.rs:397`): "what's queued to go when it's back" should become "what's queued until your laptop is reachable".
