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

## Later on 24 Oct: Eric's answers applied (commits 3c425e54 … a48b8e19)

Merged candidate d55bef25 (the Hub redesign, installed) into this branch; the merge was clean.

- **Q1, build through GitHub CI**
  - The `gh` command-line tool is logged in as Eric, with `repo` and `workflow` scopes, so a push is possible. Nothing is pushed yet: Dispatch coordinates.
  - iOS: the workflow now fetches and checks the bundled models before XcodeGen. It signs with Apple's automatic signing; the ASC key secrets are already set.
  - Android: the workflow signs only if Eric adds `ANDROID_SIGNING_P12_B64` and `ANDROID_SIGNING_PASS` as repository secrets. Neither is set today. The key is written to the runner's temp folder only for the signing step and deleted afterwards. The unsigned APK is always uploaded. Uploading the key is Eric's one-way door, and I cannot enter it for him.
  - `the_phone_stands_alone` now holds that rule: only those two secrets, read only by the signing step.
- **Q2, the correction log between Eric's own devices**
  - Each written line, already redacted, travels as a sealed sync event `corr:<id>`, and each forget follows the same way.
  - A device takes these only from a sealed bundle. It keeps nothing while "stop learning" is on there, drops duplicates, and refuses a line the scrubber would still change (an email or a key, for example).
  - The file itself is still in no backup and no carrier. `kin.rs` and the signal door are checked never to mention it.
  - 25 correction-log tests pass, 4 of them new for two devices: both directions with forget, forget everything, nothing over unsealed sync, and nothing kept where learning is stopped.
- **Newest wins, with the replaced version kept and undoable**
  - The merge says "I kept the newer one" instead of "Which?".
  - The losing version is kept in `sync_replaced` (the 50 newest).
  - "what did sync replace?" says what was replaced. "put back what sync replaced" makes it the newest change on both devices. Tested with two devices.
- **Bug fixed on the way**
  - An event a device already held, relayed back by the other device, was taken as fresh. It was stored twice and hid the real clash.
  - Fixed in both sync paths in `daemon/tick.rs`, as one line each, and the merge now counts such an event once. Tested.
- **Bug found, not fixed (facts module)**
  - The same fact changed on two devices ("my car is a Honda" on the laptop, "my car is a Toyota" on the phone) never clashes: each device files it under its own id, so both devices end up believing both.
  - The fix belongs in how facts are identified, and it needs an owner.

All suites run pass: sync 59, correction log 25, asking the laptop 11, files_remote 33, phone 15 + 5, hublive 61, floor 20, lib 41, and the project's own quality checks.

Still to do: sync over Tailscale (transport and retry), and the Sync page plus the queued wording in the Hub. The Hub files are staged above, for Dispatch to sequence.

## Pushed and built on GitHub (24 Oct 2026)

- Branch `feat/phone-standalone-2026-10-23` pushed to origin with `gh`. Final commit 995bb448.
- **Meaning layer: fp16, by Eric's choice.**
  - File: `all-MiniLM-L6-v2-fp16.onnx`, 45,297,825 bytes. The app now carries 442,234,805 bytes in all.
  - Compared with the full-size model on 26 texts: mean cosine 1.0000, the same nearest meaning 14 of 14, and both 13 of 14 right. It takes 12.1 ms per text against 8.3 ms on this laptop.
  - It loads because the encoder now ignores the symbolic shape notes in these exports (`read_model`).
- **Hub on the phone:**
  - Laptop requests now sit in the Queued lane, or In progress, with their status line.
  - The Sync page is shown on the phone. It opens on "Right now": last heard, what waits for the laptop, and what sync replaced, with a "Put back the most recent" button.
  - The Offline note now says "queued until your laptop is reachable".
- **Android: build passed.**
  - Run: https://github.com/ericsnider2003-max/Atlas/actions/runs/40096236612
  - Artifact `Atlas-Android-0.1.7-7-unsigned`: the APK is 504,355,464 bytes and expires on 23 Nov.
  - The first Android build since 28 Sep to pass. Fixes on the way:
    1. The phone build now includes the ONNX engine (`--features phone-llm,onnx`), and the bundle's paths no longer depend on it.
    2. Gradle now has a 4 GB heap, needed to pack the model (`gradle.properties`).
    3. Kotlin is raised from 2.0.21 to 2.2.10 to match the standard library that a dependency brings. That mismatch had broken every build since 28 Sep.
  - The APK is unsigned: no Android signing key exists anywhere. There are no `ANDROID_*` secrets in either repository, no key file is committed, and `~/Atlas/signing` holds only `tools`.
- **iOS: build failed at Apple signing.**
  - Run: https://github.com/ericsnider2003-max/Atlas/actions/runs/40092372149
  - The Rust core (with `onnx`) and the bundled models built. The archive then failed with "Your account has reached the maximum number of certificates", followed by "No profiles for 'com.ericsnider.atlas'".
  - Eric's Apple account needs old Apple Development certificates revoked, or a distribution certificate given to the workflow (`DIST_CERT_P12` / `DIST_CERT_PASSWORD`). Neither can be done from here.
