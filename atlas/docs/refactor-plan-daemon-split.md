# Splitting daemon.rs and main.rs — the plan

> **STALE once it has been carried out — a plan, dated 27 Sep 2026.** Every line number below is from branch `w-refactor` (base `654e8f4`) and moves the moment the four parallel branches merge. Re-take them with the commands in §0 before starting; the *names* and the *order* are what this document is for.

Written by the refactor branch, which did the ground work (§1) and deliberately moved no code: four other branches were editing `daemon.rs`, `main.rs`, `hub.rs`, `hublive.rs`, `server.rs` at the same time, and a split made in parallel with them would have turned every one of their merges into a hand merge.

**Carried out 29 Sep 2026 for §3 and §7 (branch `r6-split`), steps 4–7 of §8 still open.** Pure moves, one commit per file, the §0 guards as green as their baseline after each batch with no list edited. What the tree has now:

* `src/daemon.rs` (≈3.7k lines: enums, the struct, `new`, the free functions, `mod` lines) and sixteen children in `src/daemon/`: `late`, `conversation` (the trailing `impl` blocks), `running`, `on_itself`, `model`, `errands`, `helping` (the small settings readers and decision/opportunity help, 460 lines that sat between errands and messages), `messages`, `hands` (the plan's `windows.rs`), `away` (dictation, notifications, the work log, returning, the brief), `tick`, `inbox`, `making`, `reading` (with `inbox`, `making` and `away`, the plan's `requests.rs` split by topic), `execute`, `turn`.
* `src/main.rs` (≈1.2k lines: `USAGE`, `main()` and the trial guard) and eleven children in `src/main/`, each `#[path]`-declared and glob-imported: `serving` (plan: `daemon`), `args`, `peers`, `trading` (plan: `market`), `secrets` (plan: `vault`), `updating` (plan: `update`), `carrying` (plan: `sync`), `media`, `hubcmds` (plan: `hub`), and the plan's `tools.rs` as `everyday` and `setup`. `daemon`, `market`, `vault`, `sync`, `hub` and `tools` are library modules, so the binary's own module of the same name would make `market::x` mean one thing inside main and another everywhere else — the same reason §3 renamed `crew`, `lifecycle` and `selfwork`.
* `capability::MODULES_IN_TREE` is unchanged: `tests/catalogue.rs` counts a split module's children as the module, so `daemon` and `main` are still one name each.

---

## 0. Before starting: re-take the map

After the merge, regenerate the tables in §3–§5 from the merged file:

```sh
# every method of the big impl block, with its first line
awk '/^impl<.a> Daemon<.a> \{/{n++} n==1 && /^    (pub(\([a-z]+\))? )?fn [a-z_0-9]+/ {match($0,/fn [a-z_0-9]+/); print NR": "substr($0,RSTART+3,RLENGTH-3)}' src/daemon.rs
# every arm of execute_inner and its length
grep -n 'fn execute_inner' src/daemon.rs
# the pending_* slots and where turn_from consumes them
grep -n '^    \(pub \)\?pending_' src/daemon.rs
grep -n 'self\.pending_[a-z_]*\.take()' src/daemon.rs
```

Then run the source-reading guards once on the unsplit tree and keep the output: `--test all -- guards promised wiring dead_methods declared retrospective reading_a_module`, plus the targets `guards`, `name_collisions`, `capability_wiring`, `dead_capabilities`, `new_capabilities_are_wired`, `what_a_stranger_gets`. Each step below must leave every one of those exactly as green as this baseline, **with no list edited**. A list that has to change during a pure move means a guard is keyed wrong, not that the code changed.

## 1. Already done (ground work on `w-refactor`)

The split is safe for the tests only if they keep reading the whole module. That is now true:

| Helper (`tests/common/mod.rs`) | What it guarantees after the split |
|---|---|
| `source_of("daemon")` | `src/daemon.rs` + every `.rs` under `src/daemon/`, sorted, + any `assets/` file it `include_str!`s |
| `read_source_path("src/daemon.rs")` | the same, for tests and lists that name a path |
| `source_file_set()` | every `.rs` under `src/`, recursively |
| `split_parent(path)` / `push_module` / `fold_split_modules` | `src/daemon/late.rs` is counted as module `daemon`, not `late` |
| `is_fn_definition(line)` | `pub(super) fn x(` is a definition, not a call to `x` |

Every test that read `src/daemon.rs`, `src/main.rs`, `src/hub.rs`, `src/hublive.rs` or `src/server.rs` as text now reads through these. The whole-tree guards (`dead_methods`, `dead_capabilities`, `new_capabilities_are_wired`, `capability_wiring`, `name_collisions`, `bug_sweep`, `common::ambiguous_free_functions`) fold a split module's pieces into one module. `declared.rs` requires each `src/<m>/<child>.rs` to be declared by `src/<m>.rs`; `wiring.rs` reads `src/<m>/` as part of `<m>`.

**Rules the moves must keep, or the guards above go blind:**

1. **Parent file stays**: `src/daemon.rs` remains (holding the struct, `new`, and `mod late;` …), never `src/daemon/mod.rs`. Every helper keys a split module on the existence of `src/<m>.rs` beside `src/<m>/`. A `mod.rs` layout reads as `platform/` does today — root only — and the children drop out of every name-keyed list.
2. **Visibility**: a private `fn` that moves to a child and is called from `daemon.rs` or a sibling becomes `pub(super) fn`. A `pub fn` stays `pub fn`. Children get the struct's private fields for free (a child module sees its ancestors' private items), so no field changes visibility. Nothing becomes `pub(crate)` just to compile: `pub(crate) fn` is a *candidate* in `dead_capabilities` and would add names to its lists.
3. **Indentation**: each child is `impl<'a> Daemon<'a> {` at column zero with methods at four spaces, like today. `dead_methods` counts indented `pub fn`; the `function_body` / `body_of` helpers in the tests slice from a signature to the next four-space `fn`.
4. **One `use` line**: `use super::*;` at the top of each child, and whatever the parent `use`s that the child needs. No re-exports from children — the tests reach nothing by path inside `daemon`.
5. **No renames in the same commit as a move.** `promised.rs`, `guards.rs` and ~120 test files look functions up by signature text (`"fn crew_job("`, `"pub fn persist_after("`, `"pub fn turn_from("`). A move keeps the text; a rename breaks the lookup, and doing both at once hides which one broke it.

## 2. Why `src/main/`, not `src/cli/`, for main.rs

The brief said `src/cli/*`. **`src/cli.rs` already exists** — a library module (`pub mod cli;` in lib.rs: `tail_after`, reading flags off argv). A binary-side `src/cli/` folder beside it would

* be taken by every helper in §1 as *children of the library's* `cli` module (folded into it by name);
* fail `declared.rs`, which requires `src/cli.rs` to declare each child;
* and `mod cli;` in `main.rs` would compile `src/cli.rs` a second time, into the binary.

So the commands go to **`src/main/<group>.rs`**, declared from main.rs with a path attribute (a crate root's `mod x;` looks in `src/x.rs`, not `src/main/x.rs`):

```rust
#[path = "main/trade.rs"]
mod trade;
```

`source_of("main")` then includes them, `split_parent` folds them into `main` (so the `stem == "main"` exemptions in `dead_capabilities`, `dead_methods` and `name_collisions` keep applying), and `declared.rs` finds `mod trade;` in main.rs.

## 3. daemon.rs → `src/daemon/*.rs`

Current shape: struct `Daemon` 275–1073 (the fields), one `impl<'a> Daemon<'a>` 1074–20368 (303 methods), `Drop` 20369, free functions 20387–21490, then 20 short `impl<'a> Daemon<'a>` blocks and free helpers from 21491 to the end (23809). Order of the moves, smallest risk first; build and run §0's guards after **each** file.

| # | File | Moves (current lines) | Contents |
|---|---|---|---|
| 1 | `late.rs` | 21491–end | `Capture`, `WorkingForYou`, `ask_seats`, the outcome structs, `CountedLlm`, and every trailing `impl` block (21657, 22138, 22335 … 23779) with the free helpers between them (`site_named_in`, `mail_host`, `undo_intent`, `words_after`, `gesture_asked`, `key_spoken`). Already separate `impl` blocks: a cut and paste. `pub fn` free functions here (`undo_intent`, `gesture_asked`, `key_spoken`) are reached as `crate::daemon::x` — add `pub use late::{undo_intent, gesture_asked, key_spoken};` in daemon.rs so no caller changes. |
| 2 | `running.rs` | 19614–20368, 20369–20386 | `persist`, `persist_trouble`, `run`, `shut_down`, `converse`, `converse_inner`, `keyboard_followup`, `degrade`, `sound_allows_speaking`, `say`, `say_interruptibly`, `finish_saying`; the `Drop` impl. `new` (1075–1532) **stays** in daemon.rs beside the struct it builds. |
| 3 | `on_itself.rs` | 17586–18461 | `got_it_wrong`, `correction_wanted`, `record_correction`, `pending_edit_for_test`, `apply_lesson`, `what_i_can_do_alone`, `knowledge_store_size`, `refile_note`, `mending_line`, `mid_self_work`, `work_on_myself`, `refine_own_fix`, `attempt_own_fix`, `attempt_own_fix_with`, `prove_in_a_copy`, `what_the_stage_needs`, `land_it`, `park_for_you` |
| 4 | `errands.rs` | 174–208 (free `crew_job`, `crew_room`), 209–274 (`SeatCall`, `CouncilOutcome`, `ImproveOutcome`, `CrewLink`), 6353–7187 | `errand_candidates` … `hand_off_as`, `keep_unfinished`, `keep_window_jobs`, `pick_up_after_restart`, `take_crew_news` (419 lines). |
| 5 | `messages.rs` | 7643–9948 | signals, groups, chats and friends: `with_signal_listener` … `receive_left`, `readings`, `current_work` excluded (tick). Includes Tor (`start_tor`, `keep_tor_getting_through`) and peers (`peer_upkeep` 9719, 184 lines). |
| 6 | `windows.rs` | 15367–17071 | looking and hands (`gesture_answers` … `steer_displays`, `read_one_handed_thing`), call notes (15846–15968), window jobs (`start_working_for_you` 15969 … `type_into`, `window_job_candidates`), `reach_you`, audio outputs, the panel (17003–17071). |
| 7 | `tick.rs` | 9949–12181 | `readings`, `current_work`, **`tick` (9971–11308, 1,338 lines)**, `persist_after`, `observe`, the wire and sync (`wire_bytes` … `carry_to_your_other_devices`, 370 lines), `run_the_night`. `tick` moves whole; breaking it up is §3b, a separate change. |
| 8 | `execute.rs` | 3866–6352 | `execute_timed`, `execute`, `health_steps`, `execute_inner` (2,358 lines). Moves whole; the handler extraction is §4. |
| 9 | `turn.rs` | 1533–3865 | `turn`, `turn_from` (998 lines), `answer_locally`, `answer_before_the_model`, `about_now`, `run_command` (644 lines), and the small helpers to `find_files`. |
| — | *stays in daemon.rs* | 1–1532, 20387–21490 | the enums, `resolve_tools`, the struct, `new`, `mod` lines, and the free functions (`command_needs_connection` … `settings_fingerprint`) — they are used from several children; moving them to `daemon/helpers.rs` is a follow-up with no dependencies. |

**Three names differ from the brief's list, on purpose.** `crew`, `lifecycle` and `selfwork` are already crate modules (`pub mod crew;`, `pub mod lifecycle;`, `pub mod selfwork;` in lib.rs), and daemon.rs has `use crate::crew::{self, Crew};` — so `mod crew;` in daemon.rs does not compile (E0255, the name is defined twice), and the other two would compile but make `selfwork::x` inside `daemon` mean a different module from everywhere else in the tree. Hence `errands.rs`, `running.rs`, `on_itself.rs`.

Two ranges are not in the brief's list of nine and need a home; proposed:

| # | File | Moves | Contents |
|---|---|---|---|
| 10 | `requests.rs` | 12182–15366 and 17072–17585 | the things you ask for that are not window work: `context`, mail (`check_mail`, `check_unsubscribe`, Outlook), drafts and outreach, building and improving projects, calendar and bookings, knowledge and folders, animation, code explanation, research, watching videos, flows; dictation, notifications, the work log, returning (`happened_while_away`), `brief_now`. Split further by topic once it stands alone. |
| 11 | `model.rs` | 18462–19613 | `which_model`, the model server, `ask_the_room` (201 lines), the notes index and meaning search (`load_index` … `reload_library`), `self_check`, `facts_answer`, `from_notes`, `browser_cfg`. |

**3b. After the moves** (each its own change): `tick` into named steps (`tick_crew`, `tick_peers`, `tick_mail`, …) in the order they run today; `turn_from` into `turn_from` + `answer_pending` (§5) + `route`.

## 4. `execute_inner`: 149 arms into named handlers

`execute_inner` (3995–6352) is one `match intent` with 149 arms. Measured: **75** are already one-liners (`Intent::Updates(what) => self.updates_said(what),`), **29** are 3–15 lines, **44** are longer than 15 lines. The longest:

`Capture` 5627 (128) · `ReviewPost` 6059 (114) · `Queued` 4450 (111) · `SignIn` 5206 (110) · `Capabilities` 4965 (99) and 4925 (40) · `UseClipboard` 4719 (71) · `Show` 4845 (62) · `Pair` 5476 (61) · `Outstanding` 4369 (60) · `Why` 5111 (57) · `MachineHealth` 4665 (54) · `Mail` 5831 (52), 5883 (41), 5791 (40) · `Recommend` 4226 (49) · `BriefOn` 5940 (49) · `WhatIHave` 4088 (48) · `ForgetPeer` 5579 (48) · `WorkOnYourself` 6227 (47) · `History` 5064 (47) · `Undo` 4573 (46) · `Rehearse` 4790 (46) · `Resume` 4324 (45) · `AcceptPairing` 5537 (42) · `CreateAccount` 5168 (38) · `Unlock` 5442 (34) · `SetMode` 4632 (31) · `TravelPrep` 6173 (30) · `Unknown` 6326 (26).

Method, in `execute.rs` after move 8:

1. Every arm longer than 2 lines becomes `fn on_<snake_case_intent>(&mut self, <the bound fields>) -> String`, placed directly below `execute_inner` in arm order, body moved verbatim. The arm becomes `Intent::Capture(what) => self.on_capture(what),`. Where one intent has several arms with guards (`Mail` ×3, `Capabilities` ×2), keep the guards in the match and give each body its own handler (`on_mail_sort`, `on_mail_check`, …) — do not fold guards into one handler; the arm order *is* the precedence.
2. An arm that `return`s early returns from its handler instead — identical, since `execute_inner` returns the arm's value.
3. Do it in batches of ~10 arms, largest first, building and running the guards after each batch. Several tests look for text that sits inside these arms (`promised.rs`, `guards.rs`, `what_a_stranger_gets.rs`, `dead_methods.rs` line ~308 names "`Daemon::execute_inner`'s WorkOnYourself arm" in a comment); they search the whole daemon text through `source_of`, so the text moving into a handler is invisible to them. A test that slices the body of `execute_inner` specifically would break — none does today (checked: no test names `fn execute_inner`).
4. End state: `execute_inner` is a 149-line table, one arm per line, readable as the list of what Atlas can do.

## 5. The 28 `pending_*` slots → one ordered list

The struct holds 28 separate `pending_*` fields (lines 473–910): `pending_landing` (a `Vec`, not a question — leave it out), `pending_correction`, `pending_edit`, `pending_offer`, `pending_wanted`, `pending_job`, `pending_backlog`, `pending_brief`, `pending_brief_detail`, `pending_clipboard`, `pending_change_effect`, `pending_window_confirm`, `pending_panel`, `pending_security`, `pending_signin`, `pending_routine`, `pending_routine_run`, `pending_mail_sort` (a `bool`), `pending_post_approval`, `pending_post_when`, `pending_press`, `pending_storage`, `pending_undo`, `pending_bring_back`, `pending_decision`, `pending_media_keep`, `pending_unscanned`, `pending_media_original`.

Each is "Atlas asked you something and the next thing you say may be the answer." `turn_from` consumes them in a fixed order that is today only implicit in code position: `pending_brief` 1677, `pending_job` 2078, `pending_decision` 2174, `pending_bring_back` 2192, `pending_unscanned` 2200, `pending_media_keep` 2213, `pending_media_original` 2231, `pending_undo` 2242, `pending_storage` 2250, `pending_press` 2258, `pending_post_approval` 2267, `pending_post_when` 2297, `pending_mail_sort` 2302, `pending_security` 2320, `pending_signin` 2329, `pending_window_confirm` 2338, `pending_panel` 2359, `pending_correction` 2383, `pending_wanted` 2418, `pending_brief`/`pending_brief_detail` 2501.

Target:

```rust
enum Asked {
    Brief(String, Option<Vec<crate::returning::Happened>>),
    Job(u64),
    Decision(Option<crate::decide::Move>),
    BringBack(String),
    // … one variant per slot, carrying exactly the slot's current payload
}
pending: Vec<(u64 /* asked at */, Asked)>,
```

with `fn answer_pending(&mut self, said, t) -> Option<String>` walking a **fixed precedence table** (the order above, written once as a `const` list of discriminants) rather than insertion order — the precedence is behaviour today and must not change by accident. Steps, one commit each:

1. Add `Asked` and `pending` beside the old fields; nothing reads it. Build.
2. For each slot, in the order above: every `self.pending_x = Some(v)` becomes `self.ask(Asked::X(v))`, every `self.pending_x.take()` becomes `self.take_asked(AskedKind::X)`; delete the field. One slot per commit; run the slot's own tests (grep the test tree for `pending_x`: 17 test files mention a `pending_` name) and the §0 guards.
3. Replace the chain in `turn_from` with `answer_pending`. Add the test that pins the precedence: two questions open at once, the answer goes to the one earlier in the table.
4. `pending_*` fields that tests set directly (`pub pending_clipboard`, `pub pending_change_effect`, `pub pending_panel`, `pub pending_landing`, `pending_edit_for_test`) get a small `pub fn` each so the tests keep a way in.

What this fixes besides tidiness: an unanswered question today lives forever in its slot and can capture an unrelated sentence days later; with `asked at` on each entry, answers older than a turn or two can be dropped in one place.

## 6. `Daemon` fields into sub-structs

239 fields. Group by what reads them, one sub-struct per group, each moved in its own commit with `self.x` → `self.group.x` (a mechanical, compiler-checked rename; the §0 guards look for method names, not field paths — but grep the test tree for each field name first, since `pub` fields are set from tests):

| Sub-struct | Fields (current lines) |
|---|---|
| `Sight` | `eyes` 329, `last_sighting`, `looked_at`, `watching_me`, `looking` 366, `album`, `track`, `pointing_at` 386, `last_call_look` 429 |
| `Hands` | `hands` 358, `pace`, `steering_hand`, `steering_until` 350, `steering_at`, `carrying`, `carried_from` |
| `Speech` | `dictation` 347, `talk_queue` 722, `unsaid` 769, `typing_thread` 912, `typing_stop`, `typing_busy`, `typing_said`, `heard_note` 649, `turn_was_typed` 656 |
| `Crew` | `crew` 731, `crew_links` 742, `long_work` 739, `errand_question` 745, `last_errand_pick`, `held_by_pause` 750, `unfinished` 778, `redoing`, `left_over`, `left_windows` 785, `saved_windows`, `working_for_you` 434, `next_window_job` |
| `Peers` | `chats` 586, `signal_listener` 537, `peer_dir` 1018, `friend_host`, `tor` 1025, `tor_instead`, `tor_binary`, `tor_connections`, `peer_tries` 1056, `phones_heard` 1052 |
| `Sync` | `synclog` 932, `sync_server` 939, `seen_up_to` 946, `last_sync_check` 959, `said_about_sync` 839 |
| `ModelSide` | `llm` 278, `backends` 403, `starts_model_server` 716, `model_look_at`, `model_server_trouble` 725, `tier_mix` 543, `tiers` 684, `fit` 313, `fit_measured` |
| `Knowledge` | `library` 458, `meaning` 465, `search_check_due` 468, `index_drifted` 758, `last_index_check` 765, `facts` 635, `known`, `vocab` 643 |
| `Away` | `input_away` 793, `back_from`, `last_beat` 797, `worked_until`, `away_after` 806, `last_present` 789, `worked_while` 968, `working_since` 1067 |
| `Asked` (§5) | the 27 `pending_*` question slots |

Leave `cfg`, `plat`, `store`, `session`, `memory`, `log` and the other fields read everywhere on `Daemon` itself.

## 7. main.rs → `src/main/*.rs`

`main()` is 160–1218 (a 1,058-line `if words... == Some("x")` dispatcher); the rest is 95 `run_*` functions and their helpers (1219–end, 11576). Order, each a cut-and-paste with `use super::*;` and, per §2, a `#[path]` line in main.rs:

| # | File | Functions (first line) |
|---|---|---|
| 1 | `main/daemon.rs` | `model_connection` 1219, `run_daemon` 1225, `voice_loop` 1773, `handle` 2007, `look` 2067, `describe` 2089, `fake_monitors` 2101, `dash_bodies` 2124 |
| 2 | `main/args.rs` | `pairings_dir` 1429, `flag_value`, `has_flag`, `first_bare_arg`, `listening_port`, `keep`, `prompt_line` 6724, `ask_line` 7058, `ask_quietly` 4781 |
| 3 | `main/peers.rs` | `run_invite` 1503, `run_accept`, `run_gate`, `gate_with_identity` 6786, `run_share` 5051, `run_trust`, `run_hand`, `run_handoffs`, `run_telegram` 6114, `run_nearby`, `run_mesh`, `run_group` 11093 |
| 4 | `main/market.rs` | `run_fed` 2319, `run_read`, `bars_from_file`, `trade_cfgs` 2574, `trading_cfg`, `run_trade`, `run_market` 5509, `run_multiframe`, `run_money` 8198 |
| 5 | `main/vault.rs` | `firstrun_found` 4177, `run_firstrun`, `run_vault` 4305, `run_handover`, `show_recovery_key`, `run_accounts` 4854, `run_profiles`, `run_codes` 8430, `run_access`, `run_afterme` |
| 6 | `main/update.rs` | `update_install` 7011, `update_undo`, `update_sender`, `run_feedback`, `update_failures`, `update_auto`, `run_update` 7243, `run_release` 11259, `run_install_page` 11155 |
| 7 | `main/sync.rs` | `carry_shrinkable` 7381, `carry_file`, `mtime_secs`, `run_carry` 7781, `remote_how`, `laptop_reachable`, `run_remote`, `run_sync` 9052, `run_sync_setup` |
| 8 | `main/media.rs` | `editor_named` 9364, `platform_named`, `span`, `run_ffmpeg`, `run_video` (537 lines), `installed_editors`, `hook_named`, `as_performance`, `run_content`, `difficulty_named` |
| 9 | `main/hub.rs` | `run_hub` 6351, `report_install`, `run_metrics`, `unwired_from_wiring_test` 6683, `run_hub_address` 10376, `run_mobile` 8901, `run_phone` 10820 |
| 10 | `main/tools.rs` | everything else: `run_craft`, `run_doctor`, `run_file`, `run_picture`, `run_screen`, `run_shared`, `run_clients`, `run_agefile`, `run_notes`, `run_fix`, `run_voice_lab`, `run_doc`, `your_zone`, `run_calendar`, `run_tasks`, `run_household` (376 lines), `run_search_check`, `run_trace`, `run_index`, `run_backups`, `run_mail`, `run_watching`, `run_refusals`, `run_reclaim`, `report_own_footprint`, `run_audition`, `run_enrol_voice`, `run_adapt`, `run_walkthrough`, `run_quickinput`, `t_source`, `run_away`, `account_names`, `run_catalog`, `run_budget`, `run_startup`, `run_backends`, `run_wireguard`, `run_home`, `run_get`, `run_plugins`, `send_plugin`, `describe_plugin`, `run_edits`, `run_call_check` — split by topic once it stands alone |

Watch for `guards.rs` rows on `src/main.rs` (≈30 rows, e.g. `"fn unwired_from_wiring_test"`): they read through `read_source_path`, which folds `src/main/*` in, so they need no edit. The last step, turning `main()`'s dispatcher into a table from command word to `run_*`, is its own change and changes behaviour only if two branches of today's chain overlap — write the test that lists every command word first.

## 8. Order overall

1. Merge the four parallel branches; re-take §0.
2. daemon moves 1–11 (§3), one commit each.
3. main moves 1–10 (§7), one commit each. Independent of 2; do not interleave.
4. `execute_inner` handlers (§4), batches of ten.
5. `pending_*` → `Asked` (§5), one slot per commit.
6. Field sub-structs (§6), one group per commit.
7. `tick` and `turn_from` into named steps (§3b).

Steps 2–3 change no behaviour and no test. Steps 4–7 change no behaviour; step 5 adds the precedence test and the stale-question drop is a separate, behavioural change after it.
