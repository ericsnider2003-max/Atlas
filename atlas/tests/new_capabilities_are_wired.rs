//! The backlog of built-and-never-called capabilities, held where it is.
//!
//! `dead_capabilities.rs` counts four groups and names the orphans. This
//! file does one narrower job, for the largest group: **a function with a
//! test and no production caller may not be a new one.**
//!
//! The reason it needs its own file and its own list is arithmetic. There
//! are 150 of these, this session cleared a handful, and several sessions
//! work on this tree in parallel. A ceiling that only counts cannot tell
//! "cleared three, added three" from "did nothing", and that is exactly the
//! state a long-running backlog drifts into: real work going in, the number
//! never moving, and no way to see why. Naming every one of them makes both
//! directions visible.
//!
//! ## What to do when this fails
//!
//! **If you added a function and a test for it:** wire it to a caller in the
//! same change. That is the whole point — this is the moment where wiring is
//! cheapest, because the design is still in your head.
//!
//! **If you genuinely cannot yet** — it needs a ruling, or hardware, or a
//! caller that does not exist — add it to `KNOWN` with that reason recorded
//! somewhere findable, and say so in the handover. The list is allowed to
//! grow. It is not allowed to grow silently.
//!
//! **If you wired one:** delete its line. The test tells you which.

/// Every function that has a test and no production caller, as of the
/// 14 September 2026 sessions.
///
/// Sorted, so a diff of this list is readable.
const KNOWN: &[&str] = &[
    // --- 18 Sep: what wiring overnight and sync exposed ---------------------
    //
    // `overnight` and `sync` left UNWIRED_BASELINE, so their contents are
    // measured here for the first time. `sync::already_seen` and
    // `sync::make_bundle` came OFF this list -- they have real callers now.
    // These went on, and each is genuinely uncalled rather than unmeasured:
    //
    //   delegate::*                 reached through `Session::delegation_for`,
    //                               which returns None unless the night's
    //                               brain is `delegate`. That brain drives
    //                               another application on your behalf, spends
    //                               turns in a real conversation, and is a
    //                               decision rather than a default.
    //   overnight::delegation_for   the same gate, from the other side.
    //   overnight::spend_turns
    //   overnight::morning_detail   the long form behind the brief. Needs a
    //                               way to ask for it ("tell me more about
    //                               last night"), which is an intent, not a
    //                               wiring fix.

    // --- 17 Sep: what the ratchet could not see -----------------------------
    //
    // 265 arrived at once when this guard learned to read an indented
    // `pub fn`. **Nothing was built.** It matched only column zero, so it was
    // watching 1,121 of 3,216 functions -- and this is the guard whose whole
    // job is to stop new unwired capabilities entering the tree.
    //
    // That is the answer to "why does the backlog grow no matter how much we
    // work off". The ratchet had a 65% blind spot, so two thirds of every new
    // public function passed it without being named. The work was real; the
    // thing meant to stop the inflow was only watching a third of the door.
    //
    // From here the list is the whole surface. It may still grow -- it may
    // not grow silently, which it has been doing.
    "accounts::asked_to_weaken",
    "accounts::instead",
    "asking::query",
    // b64::decode came off 18 Sep 2026 -- `sync::read_bundle` and
    // `KeptKey::phrase` both read base64 back now. Until then everything in
    // the tree encoded and nothing decoded.
    // backends::spec and bars::has_time came off 22 Sep 2026: both deleted as
    // dead weight (see tests/dead_methods.rs). A method that no longer exists
    // cannot be found by this scan, so it leaves KNOWN in the same change.
    // bars::back_to came off 23 Sep 2026 (decision-list remainder, doc 15).
    // booking::answered / could_offer / to_decide came off 21 Sep 2026: the new
    // Intent::Booking handler logs a proposed time, assesses it against the
    // calendar (to_decide), and carries out your answer (answered) with
    // alternatives to offer (could_offer). Only an accept writes to the
    // calendar; nothing is sent. See tests/times_with_other_people.rs.
    // 20 Sep: the calendar's bridge to the native phone calendar. The store
    // and time-reading are wired and driven through the daemon; these two are
    // the sync seam, and their caller is the phone app's EventKit /
    // CalendarProvider adapter, which isn't in this tree -- the same boundary
    // as the Android client. `sync_native` in DEAD_IN_WIRED says the same.
    "calendar::for_phone",
    // callrec::silent (24 Sep 2026 audit): a recorder of silence kept to the
    // clock. Call notes records through the sound devices, which exist only
    // on Windows, so the consent steps -- above all "a no after a yes stops
    // their side and deletes it" -- are driven on Linux through this stand-in
    // (`Notes::starter`). Nothing in the running program should record
    // silence instead of a call, so it stays test-facing on purpose.
    "callrec::silent",
    // capability::working came off 21 Sep 2026: the spoken Intent::Capabilities
    // handler now answers "what can you do right now" / "what's working" by
    // naming the usable set, where before that question fell through to the
    // keyword match. `summary` gives the counts and `full` the whole catalogue;
    // `working` is the just-the-usable filter neither of those is. See
    // tests/what_is_working_right_now.rs.
    // capture::correct came off 22 Sep 2026: the new Intent::Refile handler
    // (Daemon::refile_note, reached by "file that under the roof job" / "that's
    // actually a task") is the door to it. Capture only ever *added* notes, so
    // the kind it guesses and the handles it picks were never correctable by
    // anything a person could say; `correct` -- which rewrites the kind or adds
    // a handle and marks the note confirmed -- was reached only by its own test
    // until now. See tests/refile_corrects_a_captured_note.rs.
    // cdp::links came off 23 Sep 2026 (decision-list remainder, doc 15).
    // `cdp::url` came off 21 Sep 2026 by bare-name collision, not by being
    // wired: `motion.rs`'s animation sniffer holds the CSS token `"url(http"` as
    // a string literal, and the scan reads `url(` there without knowing it is
    // not a call. The method is still reached by nothing in production. A stale
    // entry the scan could never keep, cleared here so the list stays truthful.
    // `certainty::aged` came off 21 Sep 2026: the `from_notes` recall path ages
    // the note its answer rests on, so a fact pulled from a stale note is said
    // with its age attached rather than as though checked today. See
    // `tests/reading_it_back.rs`-style coverage in `tests/recall_wired.rs`.
    // codes::running_low came off 19 Sep 2026. It hardcoded three and had no
    // caller while `gaps` compared against `cfg.warn_at` -- two answers to
    // one question, and the one a person could change was not the one the
    // named predicate gave. `gaps` asks it now, so `warn_at` is the rule.
    // `confirmed::read_back` is the version for Atlas making the security
    // change itself: it refuses when the vault is locked, because it would
    // have to sign in. Nothing in this tree drives a settings page, so that
    // caller does not exist. What is wired is `saying_it_back`, the read-back
    // without the preconditions for acting -- a walkthrough is you making the
    // change, and passing `true` for a vault nobody opened would have been a
    // lie in the shape of a precondition. See `tests/reading_it_back.rs`.
    // 25 Sep: its only caller, `tune::full_pass`, was removed as a duplicate;
    // Atlas has no runner for machine checks, so nothing orders them yet.
    "checks::first_pass",
    // 25 Sep: as `first_pass` -- no runner for machine checks exists; the
    // no-list itself stays enforced by tests/guards.rs.
    "checks::is_refused",
    // 26 Sep 2026: its only caller was trading-system code that left personal Atlas on 26 Sep 2026 (Eric: personal Atlas keeps general trading knowledge and nothing specific to his own trading system). General market knowledge, kept for the next trading caller.
    // connectivity::deferral_message came off 21 Sep 2026: `research()`'s
    // offline branch hand-wrote its own deferral sentence beside a comment
    // calling it "the spoken half"; it now calls `deferral_message`, so the
    // named message has its production caller and the duplicate is gone.
    "connectivity::unpin",
    // consent's wording helpers: choose an announcement by name, preview what
    // could be said to a call. No page offers either yet; the recorder steps are
    // all wired (call notes, 24 Sep 2026).
    // See consent::announcement_named.
    // consolidate::over_budget_on_purpose came off 21 Sep 2026: `Atlas::learned`
    // (`daemon.rs`) trimmed the store and spoke only when something was dropped,
    // staying silent in the case this function names -- nothing dropped because
    // the excess is all settled facts and own-setup notes trim won't lose. The
    // handler now notes it, so a budget exceeded on purpose is said not hidden.
    // consolidate::size_note came off 22 Sep 2026: the new `KnowledgeSize`
    // intent ("how much do you know") reads it through `Daemon::knowledge_store_size`.
    "consult::costs_an_attempt",
    "consult::is_ready",
    "consult::unproductive",
    "craft::as_goal",
    // craft::of_path came off 21 Sep 2026: integrated verification's
    // `named_existing_file` calls it to tell a real source path from a word
    // that only looks like one. The dead_capabilities ceiling was lowered for
    // it (322 -> 323 note) but this list was left behind -- the two-file drift
    // these guards exist to catch. Removed here to bring the ledgers back level.
    "credentials::needs_you_awake",
    // crew::ask_to_stop came off 23 Sep 2026 (single-errand control, doc 16).
    // crew::in_hand, crew::queued and crew::why_waiting came off 18 Sep 2026:
    // "what's queued" now answers about the errand queue as well as posts.
    "daemon::pending_offer",
    // daemon::turn was wired 18 Sep 2026. It is the directed-arrival wrapper
    // around `turn_from` -- built for a line you address to Atlas on purpose,
    // which is exactly what typing is -- and nothing had ever called it. The
    // voice loop calls `turn_from` directly; `prompt_line` parsed against the
    // phrase list and ran the result, so typing never reached the model, the
    // context or the conversation. See tests/typing_is_a_conversation_too.rs.
    // daemon::work_a_decision was wired 21 Sep 2026. Its own doc pointed at
    // the assistant door -- a decision arrives as a question you are halfway
    // through asking, so it lands as `Intent::Unknown` -- and nothing had ever
    // opened it. `decision_help` now spots a deciding phrase in an
    // unrecognised line and works it, above the policy gate with `from_notes`.
    "decide::can_explain",
    "diagnose::yours",
    // `earned::may_act_alone` and `earned::rope` came off 22 Sep 2026: wired
    // into `Intent::ActAlone` (`Daemon::what_i_can_do_alone`), which reads the
    // autonomy ledger back to you -- what Atlas will do on its own and where it
    // still asks first.
    // earned::wrong_lately came off 22 Sep 2026: what_i_can_do_alone's
    // "still checking with you first" list now notes a recent wrong streak
    // (the same >= 2 signal rope demotion reads), a real production caller.
    "editcraft::check_cuts",
    "editcraft::is_scheduling_rather_than_capturing",
    "editcraft::too_many_effects",
    // 25 Sep: a measurement the listening tests use; its one caller
    // (`saved_against_fixed`) reported a statistic nothing read.
    "endpoint::clip_ms",
    "enrol::is_final",
    "events::collisions",
    // events::is_window came off 23 Sep 2026 (decision-list remainder, doc 15).
    "faithful::not_checked",
    // Came off 26 Sep 2026 when the Atlas Project chat's 25i/25j were merged
    // in, which call them: files::after_scan, files::pdf_is_really_a_scan,
    // finance::needs_credentials, fit::limits, fit::worth_replanning,
    // grading::recommended_setup, grading::the_mistake, handoff::read_answer,
    // handshape::adopt, input::is_talking, strategy::what_was_learned,
    // sync::how_to_carry, thread::fold_input.
    "files::scan_steps",
    "fixtures::box_range",
    "fixtures::from_path",
    "fixtures::ramp",
    "fixtures::zigzag",
    "flow::optional",
    "flow::producing",
    "flow::retrying",
    "fxday::position_of",
    "gaze::in_use",
    "goal::machine_checks",
    "goal::runnable_unattended",
    "grade::clears",
    "handshape::recognise",
    "hub::crumbs",
    "hub::index_rows",
    "hub::works_without_voice",
    // 28 Sep 2026: a test seam -- "ten minutes later" without waiting ten minutes.
    "hubjobs::age_finished",
    "identity::reason",
    "improve::automatic_cost_mb",
    "install::required",
    // `knowhow::as_plan` wired 22 Sep 2026 into `Intent::WalkThrough`
    // (`Daemon::walk_me_through`): "walk me through freeing up memory" matches a
    // shipped procedure with `for_request` and reads its steps back as a
    // numbered plan. `known_procedure` reached the same match but only ever
    // said `announce` ("I know this one, N steps") -- this is the producer that
    // turns the match into the steps themselves.
    // `knowhow::for_symptom` wired 22 Sep 2026 into `Intent::Diagnose`
    // (`Daemon::diagnose_symptom`): a symptom you describe, matched against the
    // shipped procedures' snags for the likely cause and fix.
    // One `ladder` entry left with `ladder.rs` on 28 Sep 2026.
    // language::good_enough deleted 27 Sep 2026: an orphan method (see
    // ORPHAN_METHODS in dead_methods.rs), no caller and no test.
    // 30 Sep 2026, the prompt diet: a conversation turn is offered the few
    // tools `router::Router::for_turn` picks, not every core command; kept
    // for the tests that measure the old shape against the new.
    "intent::for_sentence",
    "language::live_line",
    "language::notes",
    "ledger::relevant_rule",
    "levels::bounce_rate",
    "levels::other_targets",
    "live::closed",
    "live::is_forming",
    "live::settled_bars",
    // 28 Sep 2026: for the tests about a time of day; the program reads the machine's clock.
    "localclock::pin_offset",
    "mail::may_touch",
    "mail::what_the_trail_says",
    "mesh::free",
    // mock::clipboard_now / mock::set_clipboard: mock accessors for the real
    // in-memory clipboard, to drive the wired read_clipboard/write_clipboard
    // from tests. Test-only by nature, like focus_on. Added 21 Sep 2026.
    "mock::clipboard_now",
    "mock::focus_on",
    "mock::set_clipboard",
    // What a window says, for driving `delegate` against the mock. A fixture.
    "mock::set_window_text",
    "mock::with_slow_app",
    // mod::what_am_i came off 19 Sep 2026. `platform::what_am_i` worked out
    // which machine Atlas had landed on and nothing outside the tests ever
    // asked. `atlas catalog` and `capability::on_platform_full` ask, because
    // a listing headed "what Atlas does on Android" has to know whether that
    // is the machine it is on or one of the five it is not.
    // modes::leave wired 22 Sep 2026 into `Intent::SetMode`: "mode off" / "go
    // into normal mode" leaves a mode and restores what was open before, the
    // counterpart to the `enter` the handler already drove.
    "nudge::offering",
    // The general "delegate an arbitrary task to an online worker and check
    // what comes back" primitive. Its two reusable halves ARE wired — the
    // research errand delegates its summary to a Cloudflare worker and runs
    // `online::verify_result` over the result — but `dispatch_task` as a
    // single call waits on a general "do XYZ online" intent, which does not
    // exist yet. Built and tested against a mock so the contract is pinned.
    "online::dispatch_task",
    "opportunity::atlas_can_judge",
    // otherside::against came off 23 Sep 2026 (decision-list remainder, doc 15).
    // otherside::is_asked_for came off 23 Sep 2026 (decision-list remainder, doc 15).
    // otherside::needs_evidence came off 23 Sep 2026 (decision-list remainder, doc 15).
    // outbox::mark_discarded came off 21 Sep 2026: the Mail intent's
    // `discard_draft` marks a held draft `Discarded` so the auto-send sweep
    // can never email it. Its dead_capabilities ceiling was lowered (322 ->
    // 321) but this list was left behind -- same two-file drift as
    // `craft::of_path` above. Removed here to keep the ledgers level.
    "overnight::delegation_for",
    "overnight::spend_turns",
    "panel::place_on_second",
    "panel::window_args",
    // perf::current_interval came off 22 Sep 2026: deleted as dead weight (see
    // tests/dead_methods.rs); a deleted method cannot be found by this scan.
    // Four `person::` methods came off 19 Sep 2026 in one change, because
    // they were all waiting on the same missing piece. `Noticed` had four
    // variants and no producer, so `say_once` decided nothing, `usual_hours`
    // and `worked_at` counted hours nobody read, and `gone_quiet` named
    // projects nobody mentioned. `person::noticing` is that producer.
    // `person::hard_day` was here until 21 Sep 2026: `Daemon::turn_from` now
    // reaches it through `person::having_a_hard_time`, the sibling gate of
    // `beyond_me`, offering to take outstanding backlog work off your plate on
    // an ordinary rough day.
    // `persona::prompt_for` was here until 18 Sep: the character now reaches the model on every turn.
    // `pipeline::refinement_is_warranted` was here until 18 Sep: wired in the merge.
    // The three `pipeline` recorders, 18 Sep, on the merge of the two trees.
    //
    // `pipeline.rs` came from the other tree, which measured a different set
    // and never counted these. They record a build, a review and a refinement
    // against a piece of work; `selfwork` is the caller they were written for
    // and it does not call them yet. Listed rather than wired, because wiring
    // them means deciding what `selfwork` records and when -- a design
    // decision, not a missing line.
    // record_build and record_review gained real callers on 20 Sep when the
    // self-improvement loop's Build stage was closed (`Daemon::attempt_own_fix`
    // drafts a fix, proves it in a copy, and records the build and review).
    "posix::works_here",
    // presence::should_be_discreet came off 18 Sep 2026 -- `reach_you` asks
    // it through `presence::keep_it_to_yourself` before deciding how much of
    // a note to say out loud. See tests/dead_capabilities.rs for why that is
    // a knock rather than a routing change.
    "prose::is_your_style",
    "prose::leave_alone",
    "prose::you_undid",
    // The three `recall` meaning-search functions came off 22 Sep 2026, when
    // `meaning.rs` closed the gap they were waiting on: `embed_backlog` drives
    // `unembedded`/`set_embedding` off the tick, and `doctor` asks
    // `needs_a_model` to name an encoder that is asked for and not installed.
    "reference::correct_it",
    "reference::gone_off",
    "reference::never_used",
    // reference::perishable came off 22 Sep 2026: deleted as dead weight (see
    // tests/dead_methods.rs); a deleted method cannot be found by this scan.
    "reference::trading_mb",
    "register::announcement_for",
    // `release` left wiring::UNWIRED_BASELINE on 25 Sep 2026 (update-courier
    // step 5): `update_courier::heard` checks every release notice through
    // `release::accept` and `release::verified_notice`, so the module is
    // measured here now. `atlas release keygen/sign/announce` came off it the
    // same day (step 6 begun): signing_key_from_seed, anchor_of,
    // anchor_configured, seal_manifest and update_courier::announcement all
    // have callers. What is left is the rest of the *apply side*, whose
    // callers don't exist yet: key rotation (seal_rotation, apply_rotation),
    // the downloaded-file check before installing (check_artifact), and
    // rolling back only by your own hand (given_by_the_person_at_this_device).
    // All four came off 26 Sep 2026 (update-courier step 2, `update_apply`):
    // `atlas release rotate/recover` seals and applies a key change and the
    // courier applies one it hears; installing re-checks the downloaded file;
    // `atlas update undo` makes the approval from a typed yes.
    // `register::opinions_welcome` came off 21 Sep 2026: `Persona::prompt_for`
    // gates its opinion-volunteering instruction on it, beside the existing
    // `humour_welcome()` call, so it now shapes every chat and about-Atlas turn.
    // `rehearse::touches_anything_irreversible` came off 21 Sep 2026: the
    // `Intent::Rehearse` handler now speaks its warning when a rehearsed
    // command has an irreversible step, instead of hiding the "!" mark in the
    // stored-only `detail`.
    // `retention::discard_audio` was here until 18 Sep: a recording no longer outlives its transcript.
    "revise::repeat_rate",
    // `roots::models_dir` was here until 28 Sep: the microphone's own thread
    // looks there for the Silero voice model (`micthread`).
    // `route::all_routes` was here until 22 Sep: "what are all the ways you
    // could get me that?" now reaches it. `ways_in_help` in the `Unknown`
    // chain reads the sort of problem from the sentence and lays out the whole
    // ordered menu of approaches -- cheapest and most reliable first -- rather
    // than the single way `plan`/`another_way` pick. Proven by `stance_route.rs`
    // and reached by nothing until now.
    "route::stuck_spoken",
    "route::switching",
    // 28 Sep 2026: settings-only mode moved to the threaded door; kept as the one-connection shape the server tests measure.
    "server::serve_once",
    // 28 Sep 2026: a test seam -- "answered busy" without waiting ninety seconds.
    "server::with_answer_wait",
    "session::is_overlap",
    // `safety::trouble` was here until 18 Sep: the trash says which kind of trouble its ledger is in.
    "session::overlap_hours",
    // `session::transcript` came off 22 Sep 2026: the new `Intent::Recap`
    // handler reads this session's turns back to you ("recap our conversation"),
    // so the one thing that turned the session's turns into a readable
    // transcript finally has the production caller it was written for.
    // settings::idle_but_on came off 23 Sep 2026 (decision-list remainder, doc 15).
    // Four came off 19 Sep 2026 with the access pass, and three of them by
    // bare-name collision rather than by being wired: `atlas access` calls
    // `Access::grant` and `Access::revoke`, and the scan reads `.grant(` and
    // `.revoke(` without knowing whose. `grants::grant`, `grants::revoke` and
    // `outreach::revoke` are still called by nothing. Kept in
    // `tests/one_word_is_not_an_address.rs` instead, which is the guard for
    // exactly this and says so by name.
    //
    // `signin::find_account` is the real one: the sign-in path looks up the
    // grant for the account you named, so a credential that has stopped
    // working is said before the attempt rather than after.
    // Eight `signin::` functions came off 19 Sep 2026 in one change, because they
    // were one capability with no way in and no way out. Nothing called
    // `grant`, so there was never a grant; nothing loaded or saved an
    // `Access`, so one could not have survived a restart; the access page was
    // handed an empty list of sites; and `revoke`/`revoke_all` had buttons
    // that posted to routes which did not exist. `atlas access` is the way
    // in, the Access page is the way out (filled by `hub_rows`), and
    // `note_use` records every attempt -- which is what makes both
    // `superseded_by` and `probably_changed` answerable at all.
    "signin::may_fill",
    // stale::to_stop came off 22 Sep 2026: deleted as dead weight (see
    // tests/dead_methods.rs); a deleted method cannot be found by this scan.
    "tier::slow",
    "timeframe::spans",
    // total_ms and worst were masked until 20 Sep by a bare-name collision
    // with the dead `endpoint::Turn` duplicate (its `total_ms`/`worst`); with
    // that duplicate deleted they show as what they are — the live
    // `timing::Turn`'s totals, tested but not yet read by production, which
    // reports `worst_stage`/`note` instead. `timing::total_ms` was deleted
    // 25 Sep 2026 rather than wired: nothing needed it.
    // `timing::why_slow` came off 21 Sep 2026: the on-demand self-audit
    // ("anything to look at?") now speaks it when `timing::got_slower` reports
    // a real regression, so a machine that has measurably slowed says why
    // instead of staying silent on the one thing a person feels.
    "timing::worst",
    "trace::broke",
    // `transport::bind_local_ephemeral` binds the direct-sync listener to an
    // OS-chosen loopback port. Production binds the fixed `SYNC_PORT`
    // (`Server::bind`) so peers know where to dial; the ephemeral bind is for
    // tests that need a real socket without racing for the one fixed port, and
    // for a device that only wants to be reachable from itself. Added 24 Sep
    // 2026 with the direct same-network / configured-peer transport; also in
    // `dead_methods::TEST_ONLY_METHODS`.
    "transport::bind_local_ephemeral",
    // `triage::can_wait` came off 21 Sep 2026: the "clear out my inbox" path
    // in `daemon.rs` now names how many messages the sort says need nothing,
    // so the summary ends with what you can leave unread rather than only what
    // needs you.
    "tts::audition_line",
    "tts::voice_file",
    "tune::mechanism_for",
    "uia::disabled",
    "uia::valued",
    // `uia::with` was here until 18 Sep: a stale entry -- nothing in that pass touched it.
    // `undo::on_its_own` came off 21 Sep 2026: `Intent::History` answers
    // "what did you do on your own" from it -- the review of the unprompted
    // actions, distinct from the whole log "what did you do" returns.
    // `vault::should_lock` came off 21 Sep 2026: `Daemon::tick` calls it in
    // the security housekeeping and re-locks the vault when it says the idle
    // limit has passed.
    "viewing::weave",
    // `wants::asked_for_something_missing` wired 21 Sep 2026: a turn that lands
    // as an unanswerable `Intent::Unknown` records the request on
    // `Daemon::wants_seen`, and the `Recommend` handler reads it back into
    // `obs.unsupported_requests` -- so "what am I missing?" names a thing you
    // actually asked for and Atlas couldn't do.
    // `wants::slowest` wired 21 Sep 2026: the `Recommend` intent handler now
    // leads its reply with `obs.slowest()`, the measured bottleneck.
    "watch::down_for",
    "workspace_view::why_this_took_so_long",
];

use std::collections::BTreeSet;

// The "does anything call this" rule, from `tests/common/mod.rs`. It used to
// live here *and* in the other file, byte-identical — two lists that have to
// agree, agreeing because two copies happened to match. One copy now.
mod common;
use common::{called_names, calls};




fn read_tree(dir: &str) -> Vec<(String, String)> {
    fn walk(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            if path.is_dir() {
                walk(&path, out);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let name = path
                .strip_prefix("src")
                .unwrap_or(&path)
                .with_extension("")
                .to_string_lossy()
                .replace('\\', "/");
            if let Ok(text) = std::fs::read_to_string(&path) {
                out.push((name, text));
            }
        }
    }
    let mut out = Vec::new();
    walk(std::path::Path::new(dir), &mut out);
    // `src/daemon/*.rs` counts as `daemon` once daemon.rs is split (27 Sep
    // 2026); nothing is folded today.
    common::fold_split_modules(out)
}

/// The two files that discuss capability names in prose rather than calling
/// them. Counting a comment explaining why something is dead as a caller is
/// enough to make it look alive, which happened the first time this was
/// written.
const META: &[&str] = &["dead_capabilities.rs", "bug_sweep.rs", "new_capabilities_are_wired.rs"];

fn current() -> BTreeSet<String> {
    let src = read_tree("src");
    let tests: Vec<String> = std::fs::read_dir("tests")
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("rs"))
        .filter(|p| {
            !META.contains(&p.file_name().unwrap_or_default().to_string_lossy().as_ref())
        })
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .collect();

    let unwired: BTreeSet<String> = std::fs::read_to_string("tests/wiring.rs")
        .expect("tests/wiring.rs")
        .lines()
        .filter_map(|l| l.trim().strip_prefix('"').and_then(|r| r.split('"').next()).map(String::from))
        .collect();

    // Indexed once per file instead of rescanned per candidate. This guard
    // took 28.6s on 17 Sep 2026 and the two written later took 0.35s doing
    // comparable work; the rule was never slow, the shape of its use was.
    // `guards.rs::the_index_agrees_with_the_rule_it_replaces` proves the two
    // give the same answer over every (file, name) pair that can differ.
    // The program, and its own inline tests, told apart -- the same correction
    // `dead_capabilities.rs` took in this change. These two files are the two
    // halves of one measurement; fixing one alone does not resolve a
    // disagreement, it moves it.
    let prod: Vec<(String, String)> = src
        .iter()
        .map(|(m, t)| (m.clone(), common::split_production_and_tests(t).0))
        .collect();
    let inline_tests: Vec<String> = src
        .iter()
        .map(|(_, t)| common::split_production_and_tests(t).1)
        .filter(|t| !t.trim().is_empty())
        .collect();

    let src_index: Vec<(String, std::collections::HashSet<String>)> =
        prod.iter().map(|(m, b)| (m.clone(), called_names(b))).collect();
    let test_index: std::collections::HashSet<String> = tests
        .iter()
        .chain(inline_tests.iter())
        .flat_map(|t| called_names(t))
        .collect();

    let mut out = BTreeSet::new();
    for (module, body) in &prod {
        let stem = module.rsplit('/').next().unwrap_or(module);
        if unwired.contains(stem) || stem == "main" {
            continue;
        }
        for line in body.lines() {
            // **Trimmed.** This matched only column zero, so the ratchet that
            // is supposed to stop new unwired capabilities entering was
            // watching 1,121 of 3,216 functions. A ratchet with a 65% blind
            // spot is why the backlog kept growing no matter how much of it
            // was worked off.
            let t = line.trim_start();
            let rest = match t.strip_prefix("pub fn ") {
                Some(r) => r,
                None => match t.strip_prefix("pub(crate) fn ") {
                    Some(r) => r,
                    None => continue,
                },
            };
            let name: String =
                rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            if name.is_empty() || name.ends_with("_for_test") {
                continue;
            }
            if src_index.iter().any(|(o, c)| o != module && c.contains(&name)) {
                continue;
            }
            let defined_on = format!("pub fn {name}(");
            let defined_on_crate = format!("pub(crate) fn {name}(");
            if body
                .lines()
                .filter(|l| {
                    let t = l.trim_start();
                    !t.starts_with(&defined_on) && !t.starts_with(&defined_on_crate)
                })
                .any(|l| calls(l, &name))
            {
                continue; // its own module uses it -- a helper, not a capability
            }
            if test_index.contains(&name) {
                out.insert(format!("{stem}::{name}"));
            }
        }
    }
    out
}

#[test]
fn no_new_capability_is_built_without_a_caller() {
    let found = current();
    let known: BTreeSet<String> = KNOWN.iter().map(|s| s.to_string()).collect();

    let added: Vec<&String> = found.difference(&known).collect();
    assert!(
        added.is_empty(),
        "these were built with a test and nothing calls them:\n  {}\n\n\
         Wire each one to a caller in the same change -- that is the moment it \
         is cheapest, because the design is still in your head. If it genuinely \
         cannot be wired yet (needs a ruling, hardware, or a caller that does \
         not exist), add it to KNOWN in this file and say why in the handover. \
         The list may grow; it may not grow silently.",
        added.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n  ")
    );
}

#[test]
fn a_capability_that_got_wired_is_taken_off_the_list() {
    // The other direction, and the reason this is a list rather than a
    // ceiling: without it, clearing three and adding three looks identical
    // to doing nothing at all.
    let found = current();
    let known: BTreeSet<String> = KNOWN.iter().map(|s| s.to_string()).collect();

    let cleared: Vec<&String> = known.difference(&found).collect();
    assert!(
        cleared.is_empty(),
        "these are listed as having no caller and now have one:\n  {}\n\n\
         Good -- delete those lines from KNOWN so the list keeps meaning \
         something. If you wired a whole module, check UNWIRED_BASELINE \
         (tests/wiring.rs) and CAPABILITY_UNWIRED (tests/capability_wiring.rs) \
         for its name in the SAME change.",
        cleared.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n  ")
    );
}

#[test]
fn the_list_is_sorted_and_free_of_duplicates() {
    // So that a diff of it reads as "these were cleared, these were added"
    // rather than as a reshuffle.
    let mut sorted = KNOWN.to_vec();
    sorted.sort_unstable();
    assert_eq!(KNOWN, &sorted[..], "KNOWN is not in sorted order");
    let unique: BTreeSet<&&str> = KNOWN.iter().collect();
    assert_eq!(unique.len(), KNOWN.len(), "KNOWN has a duplicate");
}
