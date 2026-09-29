# Merge set — 14 September 2026

Every file this session changed or added, against
`ATLAS_COMPLETE_HANDOVER_20260913.zip`. Paths are relative to the tree root,
so this unzips straight over `atlas_v2/`. The full tree is in
`ATLAS_COMPLETE_HANDOVER_20260914.zip` if you would rather take the whole
thing.

Read `docs/HANDOVER_2026-09-14.md` for the reasoning behind all of it.

## Changed (32)

| File | Why |
|---|---|
| `config/tools.yaml` | **Read this one before overwriting.** 18 `enabled:` flags flipped false → true (the shipped file had almost every feature switched off against the code's own defaults — 14 of 18 test failures). `research` and the top-level `enabled` stay **false**, now matching their own `Default` impls. **If you have local edits here, merge by hand** — take the flag changes, keep your own paths, device names and `models.memory_budget_mb`. |
| `src/settings.rs` | `registry()` derives every default by building the list twice, once against `ToolsConfig::default()`. `toggle()` lost its `default_on` parameter (48 call sites) and the inline `Setting` literals carry placeholders. There is no longer anywhere to declare a default that disagrees with the code. |
| `src/kin.rs` | `Delivered`/`DeliveredFile`/`Arrived`, `Door::receive_handoff{,_file}`, `as_waiting`, `send_handoff{,_file}`, JSON escaping, `MAX_HANDOFFS_PER_WINDOW`, `MAX_HANDOFF_FILE_BYTES`, `Refused::TooBig`. |
| `src/server.rs` | `Action::Handed`, `route_handoff`; `SignalListener::poll_once` returns `Arrived`; `handle_one` routes through the router functions instead of reimplementing them; the body cap is per endpoint. |
| `src/household.rs` | `Received`, `ReceivedFile`, `Inbox`, `HANDOFF_FOLDER`, `MAX_WAITING` — where a friend's note or file waits. |
| `src/daemon.rs` | `returning::welcome`/`full_brief` wired (`happened_while_away`, `returning_cfg`, `pending_brief_detail`, the away path rewritten); `receive_handoff`; the signal-poll call site updated for `Arrived`. |
| `src/http.rs` | `post_json_with_token` — the token belongs in the header. `request` refactored onto a private `with_header`. |
| `src/tray.rs` | `safe_name` is `pub(crate)`, so `kin` can sanitise a peer's claimed filename at the door. |
| `src/main.rs` | `atlas share` (notes and `--file`) and `atlas handoffs list/keep/drop`. |
| `tests/bug_sweep.rs` | `MODULE_REFERENCE_` doc exemption matched by prefix, not by exact date. `DEAD_CAPABILITY_CEILING` 288 → 287 with the written reason. |
| `tests/guards.rs` | Eight new structural-guard entries (handoff path, file cap, filename sanitising, per-endpoint body cap, doorstep folder, the settings split). |
| `tests/hub_settings.rs` | The shipped-config check now allows named, explained differences (`DELIBERATELY_DIFFERENT`) instead of demanding an exact match it only passed by accident. |
| `tests/lanes_research.rs` | Asserts research ships *off* but fully configured. |
| `tests/research_wired.rs` | Behaviour tests turn research on themselves rather than depending on a shipped default. |
| `tests/opsec_recovery_undo.rs` | Two stale tests rewritten around `opsec.rs`'s ambiguous/unambiguous split. |
| `tests/signal_listener.rs` | Updated for `Arrived`; three new real-socket tests for `/handoff`. |
| `src/audio.rs` | Device enumeration is per-platform now (`-sources alsa`, `-f avfoundation`, dshow) instead of asking every OS the Windows question. `probe_devices` keeps its signature; new `probe(cmd, inputs)` behind it. `looks_builtin` learned Linux and macOS device names. |
| `src/adapt.rs` | Header named `atlas setup`, which does not exist — now `atlas adapt`. `summary()` pluralises "app" like it already did "display". |
| `src/config.rs` | `Config::load` applies `config/machine.yaml` over the shipped generic layer. Absent is the normal case and changes nothing. |
| `tests/adapt.rs` | One assertion updated: the machine file's header names a real command now. |
| `tests/wiring.rs`, `tests/capability_wiring.rs` | `adapt` removed from both unwired lists. |
| `tests/bug_sweep.rs` (again) | The dead-capability sweep read `src/` non-recursively and matched callers by substring. Both fixed; modules keyed by path so `levels.rs`/`market/levels.rs` stop merging. True count 292 → 318 → 319, fully written up in the file. |
| `src/vault.rs` | `seal_for_real`/`unseal_for_real` deleted — superseded by `seal_bytes`/`unseal_bytes` by the code's own account, and misleadingly named while the real path is `seal_aead`. |
| `src/index.rs` | `roots_of` falls back to `default_roots()` when none of the configured roots exist. The shipped allowlist is `%USERPROFILE%`-based, so indexing had zero usable roots on Linux and macOS. |
| `src/lib.rs` | Declares the new `upgrade` module. |
| `README.md`, `docs/FIRST_BOOT.md` | Point at `docs/INSTALLING.md`; FIRST_BOOT marked superseded, kept for its Windows ambushes. |
| `src/audio.rs` (again) | `level_db` (RMS over raw PCM, in dBFS), `wav_bytes` (the 44-byte header), `samples_from_le`, `window_samples`, `stream_args` (ffmpeg streaming PCM to stdout with no `-t`). No dependency added. |
| `src/voice.rs` | `listen_until_you_stop` — reads the stream a 250ms window at a time, measures each, and lets `endpoint.rs` decide when you have finished. `listen()` tries it first and falls through to the fixed path on any problem. |
| `src/hearing.rs` | `load_from`/`save_to`, so what it measured about each microphone outlives the process. |
| `src/main.rs` (again) | `voice_loop` picks the microphone with `hearing` — measuring which one can actually hear you — instead of guessing from the device's name. |
| `tests/bug_sweep.rs`, `tests/dead_capabilities.rs` (again) | The detector now counts a function passed by name (`.map(crate::nudge::link_broke)`) as used, and skips comment lines. Six functions were wrongly listed as dead. Ceiling 319 → 314, orphans 11 → 9. |
| `tests/wiring.rs`, `tests/capability_wiring.rs` (again) | `endpoint` and `hearing` removed from both unwired lists. |
| `docs/OUTSTANDING_TASKS_2026-09-13.md` | One sentence reworded (it tripped the stale-doc guard). Superseded by the 09-14 file. |
| `src/contents.rs` | **The half that was missing.** `from_folder`, `names_on_disk`, `says_for`, `line_for{,_folder}`, `parse`, `save`, `load`, `rebuild`, `master_path`, `MASTER_FILE`, `HEADER`, `MAX_SAYS_CHARS`. Nothing in the program had ever built a `Contents` from a folder, so an index nothing wrote could not drift. |
| `src/daemon.rs` (again) | `contents` field (the notes index — `index` is the *file* index), `notes_dir`, `load_index`, `index_drift`, `rebuild_index`, `index_drifted`, `last_index_check`. The drift check runs hourly above the speak gate; the nudge is raised last in the tick and only if nothing else took the floor. `Intent::RebuildIndex` / `Intent::WhatIHave` dispatch. |
| `src/nudge.rs` (again) | `what_i_know_of`'s doc corrected: it claimed to be reached from the daypart brief and was reached from nowhere. Now says where it actually is reached from, and why the daypart greeting was the wrong home. |
| `src/intent.rs`, `src/brain.rs`, `src/policy.rs`, `src/session.rs`, `src/categories.rs`, `src/connectivity.rs` | `Intent::RebuildIndex` and `Intent::WhatIHave(String)` placed in the six exhaustive matches. `RebuildIndex` is `ProceedAndReport`; both are `Need::Local`. |
| `config/commands.yaml` | `rebuild_index` and `what_i_have` command specs. **`"rebuilding the index"` is listed first and verbatim** — it is the exact `relief` string `nudge::drifted` offers, and if it stops parsing, saying yes to the offer runs nothing. |
| `src/main.rs` (again) | `atlas index [status\|show\|rebuild]`, and its line in `USAGE`. |
| `tests/guards.rs` (again) | Six new entries for the index path. The `commands.yaml` needle carries a trailing comma on purpose: without it the guard matched the comment that quotes the phrase, and passed while the phrase itself was deleted. |
| `tests/dead_capabilities.rs` (again) | `nudge::what_i_know_of` removed from `ORPHANS`. Named orphans 9 → 8. |
| `tests/new_capabilities_are_wired.rs` (again) | `contents::what_to_open` and `nudge::drifted` removed from `KNOWN`. |
| `tests/bug_sweep.rs` (again) | `DEAD_CAPABILITY_CEILING` stays **314**, written up because it did *not* move: three functions left the set and three new private helpers entered it. |
| `docs/HANDOVER_2026-09-14.md` (again) | Section 13. |
| `docs/OUTSTANDING_TASKS_2026-09-14.md` (again) | 1a resolved, 1b is eight not nine, the ceiling and suite counts corrected, a new first-live-run item and a new standing caution. |
| `src/trace.rs` | **The recording.** `log_path`, `append`, `load`, `compact`, `model_name`. `to_line`/`from_lines` described a file format nothing ever wrote or read, so every reader in the module computed over a list only a test had filled. |
| `src/daemon.rs` (again) | `trace` field, `trace_path` (in the **store**, not a fixed path), `model_in_use`, `record_model_call`. Both model call sites recorded — `Brain::decide` and `research::run`, and there are only two. `Reached::NotNeeded` is deliberately not recorded. `Intent::ModelTrace` dispatch. |
| `src/intent.rs`, `src/brain.rs`, `src/policy.rs`, `src/session.rs`, `src/categories.rs`, `src/connectivity.rs` (again) | `Intent::ModelTrace` in the six exhaustive matches. `AutoProceed`, `Need::Local`. |
| `config/commands.yaml` (again) | `model_trace` phrases — the caller `nudge::trace_line` spent its existence waiting for. |
| `src/main.rs` (again) | `atlas trace [status\|failures\|compact]`, and its `USAGE` line. |
| `src/upgrade.rs` (again) | `data/index.md` added to `YOURS`; the `data/state` line now names the model-call log. |
| `.gitignore` (again) | `/data/index.md`. |
| `tests/guards.rs` (again) | Six more entries for the recorder, including both call sites and the store-relative path. |
| `tests/bug_sweep.rs` (again) | `DEAD_CAPABILITY_CEILING` **314 → 312**, with the reason. |
| `tests/dead_capabilities.rs` (again) | `nudge::trace_line` removed. Named orphans 8 → 7. |
| `tests/new_capabilities_are_wired.rs` (again) | `trace::to_line`, `trace::from_lines` removed. |
| `src/brief.rs` | **Redesigned.** `Source` (7 variants), `Item.source`, `Item::headline`, and the collectors: `from_handoffs`, `from_backlog`, `from_jobs`, `from_bookings`, `from_posts`, `from_upkeep`, `handled_since`, `gather`, `from_here`, `Sources`. `draft_replies` now gates whether the brief *composes*, not whether a draft is shown. Ships `enabled: true`. |
| `src/voice.rs` | `brief: BriefConfig` added to `ToolsConfig` — it was not there at all, so the brief could never be turned on. |
| `config/tools.yaml` (again) | New `brief:` section. |
| `src/nudge.rs` (again) | `daypart_with_brief` takes a `&Brief` instead of raw items — it was running a second brief over the first one's output, which emptied `drafted` and killed its own relief. |
| `src/daemon.rs` (again) | `brief_now`, `upkeep_questions`, `last_brief`. `Intent::Outstanding` briefs for real; the Outstanding panel lists the brief's items with their source; the daypart greeting carries the run when there is one. |
| `tests/council_brief.rs` | `item()` sets `Source::Mail`; the off case is built explicitly now that the default ships on. |
| `tests/shipped_config.rs` (again) | `brief` added to the flag list. |
| `tests/bug_sweep.rs` (again) | **`DEAD_CAPABILITY_CEILING` removed entirely**, with `the_number_of_unused_capabilities_does_not_grow` and its duplicate copy of the deadness rule (`calls`/`whole_word`/`after`). 14 tests → 13. |
| `tests/dead_capabilities.rs` (again) | `helper_tested` no longer has a ceiling — it counts live tested code. `test_only` and `helper_untested` are now **exact** (143, 33) rather than headroomed. New guard against the summed ceiling coming back. |
| `tests/guards.rs` (again) | The `fn calls` entry repointed from `bug_sweep.rs` to `dead_capabilities.rs` — one file owns the rule now. |
| `docs/MODULE_REFERENCE_2026-09-14.md` (again) | Regenerated. |

## New (18)

| File | What |
|---|---|
| `tests/returning_wired.rs` | 16 tests. `returning.rs` reached from a real `Daemon`. |
| `tests/handing_to_a_friend.rs` | 29 tests. Notes and files end to end, including two real round trips over a socket. |
| `tests/shipped_config.rs` | 4 tests. The shipped config and the registry against the code's own defaults. |
| `tests/audio_enumeration.rs` | 8 tests. The three platform listing formats, the command chosen per platform, and a real ffmpeg run. |
| `tests/fitting_a_new_machine.rs` | 11 tests. `adapt` end to end, including the guard that no shipped config carries a username. |
| `docs/MODULE_REFERENCE_2026-09-14.md` | Regenerated against the current tree. |
| `docs/genref.py` | The generator, kept so the next regeneration is a re-run rather than a rewrite. |
| `docs/HANDOVER_2026-09-14.md` | This session, in full. |
| `docs/OUTSTANDING_TASKS_2026-09-14.md` | Updated task list. Supersedes the 09-13 one. |
| `src/upgrade.rs` | What survives replacing the binary, and what does not. Backs `atlas update`. |
| `tests/updating_without_reinstalling.rs` | 8 tests. A populated install, a real update over the top, every file checked byte for byte. |
| `tests/dead_capabilities.rs` | 4 tests. The same set the ceiling counts, split four ways, with the nine true orphans named and explained individually. |
| `docs/INSTALLING.md` | Installing, and updating without losing anything — the two separated. |
| `.gitignore` | The repo had none. Keeps `data/` and `config/machine.yaml` out of the source tree. |
| `tests/new_capabilities_are_wired.rs` | 3 tests. **Names all 148** built-but-never-called functions and fails when a new one appears — or when one gets wired and is left on the list. |
| `tests/listening_until_you_stop.rs` | 14 tests. The level meter, the wav header, the streaming invocation, and the endpointer against synthetic audio. |
| `tests/which_ear_it_listens_with.rs` | 9 tests. Picking the microphone that measures loudest, remembering it, and not paying the Bluetooth cost for nothing. |
| `src/mend.rs` | `about_approval` and `plainer` — the question builders the module never had. `about_ambiguity` was written and deleted in the same pass; see the note left in its place. |
| `src/daemon.rs` (again) | `park_for_you`. The unattended `RequireApproval` branch sets the request aside instead of promising to wait and keeping no record. `called()` lost its `allow(dead_code)`. |
| `tests/daemon.rs` | `unattended_atlas_says_it_will_wait_rather_than_acting` renamed and rewritten — it asserted the promise; it now asserts the record. |
| `tests/a_question_that_survives_you_leaving.rs` | 12 tests. |
| `tests/wiring.rs`, `tests/capability_wiring.rs` (again) | `mend` off both; ceiling **27 → 26**. |
| `tests/dead_capabilities.rs` (again) | Test-only **141 → 142**, raised deliberately for `mend::worth_trying` (blocked on the selfwork ruling). |
| `src/store.rs` | **`save` skips a write whose bytes are already on disk.** An idle tick went from 13 writes + 13 renames to none; `persist()` 1032µs → 133µs. Atomicity on the writing path untouched. |
| `tests/writing_only_what_changed.rs` | 8 tests, each on something that must still happen. |
| `tests/common/mod.rs` | **New.** The one `calls`/`whole_word`/`after` rule, previously byte-identical in two test files that keep two lists which must agree. |
| `tests/dead_capabilities.rs`, `tests/new_capabilities_are_wired.rs` (again) | Both now `mod common;` instead of carrying their own copy. |
| `tests/wiring.rs`, `tests/capability_wiring.rs` (again) | Every "delete it from this list" failure now names the other two lists. |
| `tests/ceiling.rs` | A note recording the fourth copy of the wiring rule that was written and deleted — it got a different answer on its first run. |
| `src/models.rs` | **Wired.** `budget_bytes` (measured, not configured), `choose_for`, `explain_for`, `layers_here`, `is_running`, `footprint_mb`, `llm_config_for`. Default `memory_budget_mb` 6000 → **0** = measured. |
| `config/tools.yaml` (again) | `memory_budget_mb: 3500` → **0**. The old comment reasoned about 15.7GB of RAM — a measurement, in a config file. |
| `src/main.rs` (again) | Derives an `LlmConfig` from the chosen model when no `tools.llm` is written. **A hand-written one always wins.** |
| `src/daemon.rs` (again) | `which_model`, `http_tool`; `Intent::WhichModel`. |
| `tests/wiring.rs` | `models`, `gguf` off `UNWIRED_BASELINE`; ceiling **34 → 27**. |
| `tests/capability_wiring.rs` | `models` off `CAPABILITY_UNWIRED` — the third list tracking the same fact, and it only surfaced on a full run. |
| `tests/which_model_fits_here.rs` | 23 tests against real GGUF bytes in a real folder. |
| `tests/gguf_models.rs` (again) | The shipped-config assertion inverted: it used to require a non-zero budget. |
| `src/revise.rs` | **The missing half.** `standing` (the lessons, as the model reads them), `wanted_in`, `subject_of`, `MAX_STANDING`. |
| `src/daemon.rs` (again) | `mending`, `pending_correction`, `pending_edit`, `last_said`; `got_it_wrong`, `correction_wanted`, `record_correction`, `apply_lesson`, `mending_line`. **`context()` now carries what has been learned** — the line the whole loop depends on. |
| `src/intent.rs`, `src/brain.rs`, `src/policy.rs`, `src/session.rs`, `src/categories.rs`, `src/connectivity.rs` (again) | `GotItWrong(String)`, `ApplyLesson`, `HowAmIDoing`. `ApplyLesson` is `ProceedAndReport`, **not** `RequireApproval` — the proposal is the approval, and grading it otherwise made saying yes write nothing. |
| `config/commands.yaml` (again) | `got_it_wrong`, `apply_lesson` (phrases match `Home::plain()` exactly), `how_am_i_doing`. |
| `tests/telling_it_twice.rs` | 25 tests. |
| `src/council.rs` | **The missing half.** `hardware_room` (seats briefed on the measured machine), `parse_opinion`, `is_hardware_question`, `empty_chairs`, `word_at`. Nothing ever built an `Opinion` before this. |
| `src/nudge.rs` (again) | `convene_with(room, question)`; `convene` now delegates to it and has a production caller. |
| `src/daemon.rs` (again) | `ask_the_room` — five blind seats, each recorded in the flight recorder as `council`. `Intent::AskTheRoom` dispatch. |
| `src/intent.rs`, `src/brain.rs`, `src/policy.rs`, `src/session.rs`, `src/categories.rs`, `src/connectivity.rs` (again) | `Intent::AskTheRoom(String)`. `ProceedAndReport` — reversible, but it spends your patience. |
| `config/commands.yaml` (again) | `ask_the_room` phrases, including "should i upgrade". |
| `tests/a_room_that_disagrees.rs` | 25 tests. |
| `tests/dead_capabilities.rs` (again) | Exact test-only count **143 → 141** (`nudge::convene` and `nudge::offer_to_mend` wired). |
| `tests/a_brief_without_an_inbox.rs` | 25 tests. Every offline source, the offline filter, and a real `Daemon` briefing from its own state. |
| `tests/flight_recorder.rs` | 24 tests. A real `Daemon` with a real `Llm`, then what landed on disk — including the test that puts a passphrase in a prompt and reads the file back looking for it. |
| `tests/knowing_the_map.rs` | 34 tests. A real folder, an index written from it, the drift that appears when the folder moves on, the nudge, and the rebuild. Includes the guard that the nudge's own relief string parses to a real intent. |

## After merging

```
cargo test --no-fail-fast
```

`--no-fail-fast` matters: plain `cargo test` stops at the first failing
*binary*, which is how a tree with 18 failures reported only 1 on the first
run here. Expect **212 binaries, 4,242 tests, 0 failures**.

## Nine things to know if the merge goes sideways

1. **`SignalListener::poll_once` changed its return type** from
   `Option<kin::Incoming>` to `Option<kin::Arrived>`. Any caller of your own
   needs a `match` on the two variants — `daemon.rs`'s call site shows the
   shape.
2. **`settings::toggle` lost an argument.** If the main chat has added
   toggles since the 13 Sep baseline, each will have one `true`/`false` too
   many. Delete the one immediately before the `Weight`.
3. **Research and voice now ship off.** That is deliberate, not a merge
   accident — see section 2 of the handover. Turn them on in the hub once.
4. **`audio::probe_devices` kept its signature** but is no longer
   Windows-only; `probe(cmd, inputs)` is the general form. If the main chat
   added callers, they keep working.
5. **`Config::load` now reads `config/machine.yaml` if it is there.** That
   file is per-machine and must never be committed — the new `.gitignore`
   covers it. No file, no change in behaviour.
6. **`Voice::listen()` now tries a streaming, endpointed recording first**
   and falls back to the configured fixed-length one. No signature changed,
   so every caller and every test double keeps working. If the main chat has
   its own record path, this is the file to look at.
9. **`brief::Item` gained a required `source` field.** Any `Item { .. }`
   literal the main chat has added needs one. `Source::Mail` reproduces the
   old behaviour exactly.
8. **`Daemon` has two fields called something like "index".** `index` is the
   *file* index (`index.rs`, your documents); `contents` is the notes index
   (`contents.rs`). If the main chat has touched `daemon.rs`, check that no
   `self.index` became `self.contents` in the merge — they are unrelated.
7. **`DEAD_CAPABILITY_CEILING` went 287 → 314, and that is not a regression.**
   The detector was blind to `src/market` and `src/platform` and matched
   callers by substring; fixing it revealed 26 functions that were dead all
   along. If you merge only part of this set, take `tests/bug_sweep.rs` and
   `tests/dead_capabilities.rs` together or the two will disagree — a test
   asserts they must match exactly.
