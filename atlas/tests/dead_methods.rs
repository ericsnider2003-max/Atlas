//! **The two thirds of the public surface the anti-hollow spine could not
//! see.**
//!
//! Every reachability guard in this tree matches `pub fn` at the start of a
//! line. In Rust that is a free function and nothing else: a method inside an
//! `impl` block is indented, and so is every trait implementation and every
//! function in a nested `mod`. Counted on 16 Sep 2026:
//!
//! ```text
//! pub fn at column zero (what the spine scans):  1072
//! pub fn indented (what it cannot see):          1988
//! ```
//!
//! So `dead_capabilities.rs` reports on **35%** of the public surface, and the
//! 140-entry `KNOWN` list in `new_capabilities_are_wired.rs` is 140 out of a
//! third. Applying that file's own four-way classification to the other
//! two thirds finds:
//!
//! ```text
//! cross-module caller     ~1320   fine, something calls it
//! TEST-ONLY                 260   built, tested, no production caller
//! helper                    ~300   its own module uses it
//! ORPHAN                     51   no caller anywhere, and no test either
//! ```
//!
//! **260 + 51 = 311 dead capabilities that no guard in this tree had ever
//! counted**, against the 140 it had. The real figure was never 140; it is
//! about 451, and the spine was reporting the third it happened to match.
//!
//! This is not a backlog that appeared. It is a backlog that was always there
//! and could not be seen, which is precisely the failure mode the whole spine
//! exists to prevent — arriving through the detector rather than through the
//! code.
//!
//! ## Why two lists, and why a separate file
//!
//! Same reasoning as `name_collisions.rs`: folding this into
//! `dead_capabilities.rs` would move that file's numbers by hundreds in one
//! change, and a ratchet that jumps by 266 in a single commit is a ratchet
//! nobody can review. The existing guards keep their scope and their history.
//! This one owns the surface they never covered.
//!
//! `TEST_ONLY_METHODS` is the direct analogue of `KNOWN` — proven by a test,
//! called by nothing. `ORPHAN_METHODS` is worse and smaller: no caller *and*
//! no test, so nothing in this repository has ever established that it works.
//!
//! ## What to do when this fails
//!
//! **A name appeared:** wire it, or add it with a reason. **A name
//! disappeared:** it got a caller — delete the line. Orphans are the ones
//! worth attacking first: 50 functions that are public, uncalled and untested
//! are 50 chances to be wrong with nothing to catch it.

use std::collections::{BTreeSet, HashSet};

/// Indented `pub fn` with a test and no production caller.
///
/// The `KNOWN` list of `new_capabilities_are_wired.rs`, for methods.
const TEST_ONLY_METHODS: &[&str] = &[
    // --- 28 Sep: the second scan ----------------------------------------------
    //
    // `serve_once` lost its one caller when settings-only mode moved to the
    // threaded door (one connection at a time held every other one); it is
    // kept as the one-connection shape the server tests measure against.
    // `with_answer_wait` and `age_finished` exist so the tests can reach
    // "the browser was told busy" and "ten minutes later" without waiting
    // ninety seconds or ten minutes.
    // --- 18 Sep: delegate and overnight, now measured -----------------------
    //
    // Wiring `overnight` took both modules off UNWIRED_BASELINE, so their
    // methods are counted here for the first time. These are genuinely
    // uncalled, not newly dead: they are the `delegate` brain's half of the
    // night, and that brain drives another application on your behalf. It is
    // a decision, not a default, and it is named in OUTSTANDING_TASKS.

    "asking::query",
        // `attention::halt` was here until 17 Sep. It is now called from
    // `daemon.rs`'s `hear` match, on `Heard::Panic`.
    //
    // Worth recording why it sat here: `firstrun.rs` ends setup by telling
    // every new user to say "stop everything" if Atlas gets something wrong.
    // `attention::hear` recognised the phrase and returned `Heard::Panic`;
    // the daemon's match had no arm for it, so it fell through to ordinary
    // parsing and came back as "I don't know that". The emergency stop was
    // recognised, listed here as dead, taught to the user by name, and wired
    // to nothing. Found by the 17 Sep whole-tree audit.
    // `backlog::dismiss` came off 21 Sep 2026: the daemon's backlog offer
    // ("Earlier you asked me to X but Y. Want me to do it now?") now tracks the
    // offered item in `pending_backlog`, and a no at that question calls it so
    // the task leaves the list for good. Until then a no only cleared the
    // question and `next_offer` raised the same task again on the next quiet
    // tick -- "no" meant "ask me again later" forever.
    // 20 Sep: the calendar's bridge to the native phone calendar. Exercised by
    // tests (your_own_calendar), but with no production caller in this tree --
    // the phone app's EventKit / CalendarProvider adapter is what drives them,
    // and it lives outside this tree, same as the Android client.
    "calendar::for_phone",
    // capture::correct came off 22 Sep 2026: the new Intent::Refile handler
    // (Daemon::refile_note) reaches it -- "file that under the roof job" /
    // "that's actually a task" rewrites a captured note's kind or adds a handle
    // and marks it confirmed. Capture only ever added notes until now.
    // cdp::scroll left 30 Sep 2026: `operate` calls `Platform::scroll`,
    // the same bare name, so this scan can no longer tell them apart.
    // `cdp::url` came off 21 Sep 2026, but by bare-name collision rather than by
    // being wired: `motion.rs`'s animation sniffer holds the CSS token
    // `"url(http"` as a string literal, and the scan reads `url(` there without
    // knowing it is not a call. The method itself is still reached by nothing in
    // production and proven only by its own module's test. Same false-positive
    // class as the `grants::grant` note below; a stale entry until now.
    // codes::running_low came off 19 Sep 2026. It hardcoded three and had no
    // caller while `gaps` compared against `cfg.warn_at` -- two answers to
    // one question, and the one a person could change was not the one the
    // named predicate gave. `gaps` asks it now, so `warn_at` is the rule.
    "connectivity::unpin",
    "consult::costs_an_attempt",
    "consult::is_ready",
    "consult::unproductive",
    "craft::as_goal",
    // `craft::of_path` came off 21 Sep: integrated verification's
    // `named_existing_file` calls it to tell a real source path in the request
    // from a word that only looks like one, so the ladder is picked from a file
    // the project actually has.
    "daemon::offline_coverage",
    "daemon::pending_offer",
    // daemon::turn was wired 18 Sep 2026. It is the directed-arrival wrapper
    // around `turn_from` -- built for a line you address to Atlas on purpose,
    // which is exactly what typing is -- and nothing had ever called it. The
    // voice loop calls `turn_from` directly; `prompt_line` parsed against the
    // phrase list and ran the result, so typing never reached the model, the
    // context or the conversation. See tests/typing_is_a_conversation_too.rs.
    "decide::can_explain",
    // decide::written came off 21 Sep 2026 by bare-name collision, not by being
    // wired: the new plainchange path calls `plainchange::written(effect)`, and
    // the reachability scan reads `written(` there without knowing it is a
    // different module's function. `decide::written` is still reached by nothing
    // in production; a stale entry the scan can no longer keep, cleared here so
    // the list stays truthful. Same class as `cdp::url` above.
    "delivery::message",
    // `earned::may_act_alone` and `earned::rope` came off 22 Sep 2026: the new
    // `Intent::ActAlone` handler (`Daemon::what_i_can_do_alone`, reached by
    // "what can you do on your own" / "where do you still ask me first") reads
    // both to answer what Atlas will do unattended and where it still asks. The
    // `earned` record was written every turn (`note`, `taken_back`) and read
    // back by nothing a person could reach -- `how_am_i_doing` reports
    // corrections, not trust. Now the ledger that decides whether Atlas acts
    // alone can be asked what it says.
    // 25 Sep: a measurement the listening tests use; its one caller
    // (`saved_against_fixed`) reported a statistic nothing read.
    "endpoint::clip_ms",
    "enrol::is_final",
    // The context-free form of book recall. Production always has a live
    // thread, so the daemon's three call sites all go through
    // `recall_in_context`; this delegate stays because the fact-ranking tests
    // exercise scoring without having to invent a conversation first, and
    // because "recall with no context" is the honest name for what they test.
    "facts::recall",
    "faithful::not_checked",
    // `firewall::allowed` and `grants::allowed` came off 30 Sep 2026: the
    // camera's permission (`daemon::camera::camera_allowed`) calls
    // `Verdict::allowed`, which the bare-name scan counts for both.
    "flow::optional",
    "flow::producing",
    "flow::retrying",
    "fxday::position_of",
    "goal::machine_checks",
    "goal::runnable_unattended",
    "grade::clears",
    "grants::message",
    "handshape::recognise",
    // `handshape::worked_out` and `hearing::record_turn` came off 26 Sep 2026
    // with 25j: teaching a gesture (`handloop`, H6) and the desk mic's heard
    // turns reach them.
    // `hlc::peek` is deliberately NOT listed, though its only real caller
    // (`sync::clock_at`) was deleted 24 Sep 2026 as dead. The reachability scan
    // matches bare method names, and `.peek(` appears on iterators in `main.rs`
    // and `finance.rs`, so it counts `hlc::peek` as reached — the same
    // one-word-is-not-an-address collision as `cdp::url` and `overlay::dismiss`.
    // Its truthful home is `tests/one_word_is_not_an_address.rs`; listing it
    // here would fail `no_method_gains_or_loses_dead_status_silently`, which
    // sees the collision as a caller.

    // 28 Sep 2026: lets the tests reach "ten minutes later" without waiting.
    "hubjobs::age_finished",
    "identity::reason",
    // `input::is_talking` came off 26 Sep 2026 with 25j: the Windows key hook
    // (`hotkeys`, H1) asks it.
    "integrations::panel",
    // 30 Sep 2026, the prompt diet: a conversation turn is offered the
    // capabilities tool and the few tools the router picks
    // (`router::Router::for_turn`), not every core command; `for_sentence`
    // is kept for the tests that measure the old shape.
    // (`ToolBook::retrieved_for` lost its one caller and was deleted.)
    "intent::for_sentence",
    "interrupt::forget_stale",
    // `knowhow::for_symptom` came off 22 Sep 2026: the new `Intent::Diagnose`
    // handler (`Daemon::diagnose_symptom`, reached by "troubleshoot …", "why
    // won't …") scores a symptom you describe against every shipped
    // procedure's snags and names the likely cause and fix. Its sibling
    // `for_request` was already wired as `known_procedure`; the symptom half
    // was proven by `fit_knowhow.rs` and reached by nothing.
    "knowhow::offline_coverage",
    // One `ladder` entry left with `ladder.rs` on 28 Sep 2026.
    "learned::about",
    "learned::forget_stale",
    "levels::other_targets",
    // `lifecycle::acquire` and `lifecycle::stop_all` were here until 17 Sep.
    // The merge that day gave both a caller inside `lifecycle.rs` itself --
    // `self.sup.acquire(..)` at :240 and `self.sup.stop_all()` at :302 -- so
    // the supervisor's admission and shutdown paths are now reached through
    // the module's own surface rather than sitting beside it.
    "live::closed",
    "live::is_forming",
    "live::settled_bars",
                    // mend::atlas_can_fix came off 22 Sep 2026 by bare-name collision, not
                    // by being wired: cloudsync::atlas_can_fix now has a caller (setup_guidance)
                    // and the reachability scan matches the bare name `atlas_can_fix(` in mend too.
                    // mend::atlas_can_fix is still reached by nothing in production.
    "mesh::free",
    // mock::clipboard_now and mock::set_clipboard are test scaffolding for the
    // mock's real in-memory clipboard: seed what was "copied" and read back what
    // a write-back landed. The clipboard capability itself (read_clipboard /
    // write_clipboard) is wired into the daemon; these two only drive it from a
    // test, like focus_on. Added 21 Sep 2026.
    "mock::clipboard_now",
    "mock::focus_on",
    "mock::set_clipboard",
    // What a window says, so reading it and typing into it for you
    // (`delegate`) can be driven in a test. A fixture by nature. 24 Sep 2026.
    "mock::set_window_text",
    "mock::with_slow_app",
    // modes::leave came off 22 Sep 2026: wired into `Intent::SetMode`, which
    // already drove `enter`. "mode off" / "go into normal mode" now leaves a
    // mode and restores what was open before, where there had been no way out.
    "nudge::offering",
    "opportunity::atlas_can_judge",
    // `overlay::dismiss` came off 21 Sep 2026 by bare-name collision, not by
    // being wired: wiring `backlog::dismiss` (see below) added a `.dismiss(`
    // call to `daemon.rs`, and the scan counts any cross-module `dismiss(` as a
    // caller of every `dismiss`. `overlay::dismiss` is still called by nothing
    // in production -- the daemon draws overlays through `overlay::around`, never
    // dismisses one -- and stays proven only by `tests/overlay.rs`. Same
    // one-word-is-not-an-address class as the `grants::grant` note below.
    "overnight::delegation_for",
    "overnight::spend_turns",
    // Four `person::` methods came off 19 Sep 2026 in one change, because
    // they were all waiting on the same missing piece. `Noticed` had four
    // variants and no producer, so `say_once` decided nothing, `usual_hours`
    // and `worked_at` counted hours nobody read, and `gone_quiet` named
    // projects nobody mentioned. `person::noticing` is that producer.
    "person::about",
    // `persona::prompt_for` was here until 18 Sep: `Brain::system` calls it for every model turn -- the character now reaches the model.
    // The three `pipeline` recorders, 18 Sep, on the merge of the two trees.
    // `pipeline.rs` came from the other tree, which measured a different set
    // and never counted these. `selfwork` is the caller they were written
    // for and does not call them yet; wiring them means deciding what it
    // records and when, which is a design decision rather than a missing
    // line. Same entry as in `new_capabilities_are_wired.rs`.
    // record_build / record_review came off 20 Sep: the self-improvement loop's
    // Build stage now calls them (`Daemon::attempt_own_fix`).
    // presence::should_be_discreet came off 18 Sep 2026. It is what
    // `presence::keep_it_to_yourself` asks, which `Daemon::reach_you` now
    // asks before deciding how much of a note to say out loud -- so the
    // camera's reading finally reaches the code that acts on it.
    "prose::is_your_style",
    "prose::leave_alone",
    "prose::you_undid",
    // `publish::cancel`, `publish::schedule`, `publishing::rules` and
    // `quickinput::backspace` came off 26 Sep 2026 with 25j: scheduling and
    // cancelling a post (G2), a format's rules in media editing (G8), and
    // `typebox` reach them.
    // `release::given_by_the_person_at_this_device` came off 26 Sep 2026:
    // `atlas update undo` makes it from a typed yes (update-courier step 2).
    // `recall::set_embedding` and `recall::unembedded` came off 22 Sep 2026:
    // `meaning.rs` is the encoder seam that was always missing, and
    // `Daemon::embed_backlog` drives both off the tick.
    // `rehearse::touches_anything_irreversible` came off 21 Sep 2026: the
    // `Intent::Rehearse` handler in `daemon.rs` now reads it to warn out loud
    // when a rehearsed command has a step that can't be undone -- the "!" mark
    // was only ever in the stored `detail`, never the spoken line.
    "revise::repeat_rate",
    // `roster::may_see` was here until 17 Sep. The merge wired it in
    // `chat.rs` at :289, :349 and :403 -- every message the chat path shows
    // now passes the roster's visibility check first, which is what the
    // method was built for.
    // `safety::safe_to_write` was here until 17 Sep, and it is the most
    // pointed entry this list has had. It answers exactly one question --
    // "Can Atlas safely add to the trash right now?" -- and `Trash::expire`
    // was writing the ledger without asking it, from a read that collapses
    // Unreadable and Corrupt to an empty list. One bad read therefore wrote
    // `[]` over the ledger and made every file in `data/trash/`
    // unrecoverable.
    //
    // So the method that would have prevented it existed, was listed here as
    // reached by nothing, and sat one line away from the code that needed it.
    // `expire` calls it now.
    // `safety::trouble` was here until 18 Sep: `Daemon::tick` calls it on the trash ledger before expiring.
    "sandbox::files",
    // `session::transcript` came off 22 Sep 2026: the new `Intent::Recap`
    // handler reads this session's recent turns back to you ("recap our
    // conversation"). It assembled those turns for the model and nothing ever
    // called it, because the daemon builds model context from `thread`.
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
    // Six `signin::` methods came off 19 Sep 2026 in one change, because they
    // were one capability with no way in and no way out. Nothing called
    // `grant`, so there was never a grant; nothing loaded or saved an
    // `Access`, so one could not have survived a restart; the access page was
    // handed an empty list of sites; and `revoke`/`revoke_all` had buttons
    // that posted to routes which did not exist. `atlas access` is the way
    // in, the Access page is the way out, and `note_use` records every
    // attempt -- which is what makes `superseded_by` answerable at all.

    // 28 Sep 2026: its one caller, settings-only mode, moved to the
    // threaded door (one connection at a time held every other one); kept as
    // the one-connection shape the server tests measure against.
    "server::serve_once",
    // 28 Sep 2026: lets the tests reach "the browser was told busy"
    // without waiting ninety seconds.
    "server::with_answer_wait",
    "signin::may_fill",
    // thread::asked_before came off on 21 Sep: run_command now calls it after
    // appending the turn, so a question asked again gets a "you asked this a
    // little earlier" lead-in instead of the reply replayed verbatim.

    "tier::slow",
    // time::say_date came off on 20 Sep: calendar::Event::say_when calls it to
    // print an event's date, so it has a real caller now.
    // `timing::why_slow` came off 21 Sep 2026: the on-demand self-audit
    // ("anything to look at?", `Daemon::execute_inner`'s WorkOnYourself arm)
    // now speaks it when `timing::got_slower` reports a real regression. That
    // answer was silent on speed -- `refresh_signals` rebuilds `self.signals`
    // from undo/misunderstanding/unused alone and overwrites the GotSlower
    // signal pushed during the hourly tidy, so a machine that had measurably
    // slowed said nothing about the one thing a person actually feels.
    // 26 Sep 2026: its only caller was trading-system code that left personal Atlas on 26 Sep 2026 (Eric: personal Atlas keeps general trading knowledge and nothing specific to his own trading system). General market knowledge, kept for the next trading caller.
    "timeframe::spans",
    "timing::worst",
    "trace::broke",
    // `transport::bind_local_ephemeral` binds the direct-sync listener to an
    // OS-chosen loopback port. Production always binds the fixed `SYNC_PORT`
    // (`Server::bind`) so peers know where to dial; the ephemeral bind is for
    // tests that need a real socket without racing for one fixed port, and for
    // a device that only wants to be reachable from itself. Added 24 Sep 2026
    // with the direct same-network / configured-peer transport.
    "transport::bind_local_ephemeral",
    "triage::adjust",
    "tts::voice_file",
    "uia::disabled",
    "uia::valued",
    // `uia::with` was here until 18 Sep. It gained a caller; nothing in
    // this pass touched `uia`, so the entry was stale -- which is the other
    // direction this ratchet checks and the reason it checks both.
    // `undo::on_its_own` came off 21 Sep 2026: `Intent::History` now
    // recognises "what did you do on your own" / "without asking" as a
    // distinct question and answers it from `on_its_own`, the review of the
    // unprompted actions its own doc was written for.
    // `vault::should_lock` came off 21 Sep 2026: `Daemon::tick` now asks it
    // among the security housekeeping and calls `vault::lock` when it says so.
    // Until then the vault was locked only in `take_it_back`; an ordinary
    // `atlas vault` unlock stayed open for the life of the process and
    // `lock_after_mins` was honoured by nothing.
    // `vault::weakly_sealed` was here until 17 Sep. The merge wired it at
    // `main.rs:3027`, so a vault sealed with a weak passphrase is now
    // reported rather than merely detectable.
    // `wants::asked_for_something_missing` wired 21 Sep 2026: a turn that lands
    // as an unanswerable `Intent::Unknown` now records the request on
    // `Daemon::wants_seen`, and `Intent::Recommend` reads it back -- so "what
    // am I missing?" turns a real ask Atlas couldn't do into a concrete
    // suggestion, the source `Observations` was built for and never had.
    "watch::down_for",
];

/// Indented `pub fn` with no caller anywhere and no test either.
///
/// Worse than test-only: nothing in this repository has ever demonstrated that
/// these run at all.
const ORPHAN_METHODS: &[&str] = &[
    // 26 Sep 2026, claims::over and session::is_overlap: their only caller was
    // trading-system code that left personal Atlas (Eric: general trading
    // knowledge only). Kept for the next trading caller.
    // backends::spec came off 22 Sep 2026: deleted. A redundant lookup accessor
    // (`self.specs.iter().find(...)`) that nothing called; `choose`/`eligible`
    // reach the spec table directly. Also removed from KNOWN in
    // new_capabilities_are_wired.rs in the same change.
    // bars::back_to came off 23 Sep 2026: `structure::before_the_turn` reads
    // the structure through a view bounded at the bar before a break, and
    // `atlas market` prints it -- the use its own doc named.
    // bars::has_time came off 22 Sep 2026: deleted. Trivial `!time.is_empty()`
    // predicate with no consumer. Also removed from KNOWN in the same change.
    // cdp::links came off 23 Sep 2026: research falls back to the headless
    // browser (`Browser::links`) when curl's results page carries no links.
    // crew::ask_to_stop came off 23 Sep 2026: "cancel the build" calls one
    // errand off by name (`Daemon::apply_errand_pick`), per `which_errand`.
    // crew::in_hand, crew::queued and crew::why_waiting were wired 18 Sep
    // 2026: "what's queued" used to answer only about posts waiting to be
    // sent while the errand queue -- which these three were written to
    // describe -- went unmentioned.
    // daemon::work_a_decision came off 21 Sep 2026: the `Intent::Unknown`
    // branch now runs `decision_help`, which works a deciding line instead of
    // failing to catch it. That call in daemon.rs is its first in-module
    // caller, so it is no longer an orphan here.
    // earned::wrong_lately came off 22 Sep 2026: what_i_can_do_alone's
    // "still checking with you first" list now notes a recent wrong streak
    // (>= 2, the same threshold rope demotion reads), completing that handler.
    // events::is_window came off 23 Sep 2026: `standdown::Blackout::plain`
    // says a banded release (the BoJ) has no fixed time -- read as the band.
    // language::good_enough came off 27 Sep 2026: deleted. `Heard::good_enough`
    // compared confidence to `min_confidence` and nothing asked it; the one
    // real reader of that threshold is the recent-clips check in the same
    // file, which is unchanged. Also removed from KNOWN in
    // new_capabilities_are_wired.rs in the same change.
    "session::is_overlap",
    // `log::warn` was an ORPHAN until 17 Sep -- no caller AND no test, so
    // nothing in the repository had ever shown that Atlas's warning level
    // works at all. `Daemon::persist` now calls it when state cannot be
    // written to disk, which is the most warning-shaped event in the system
    // and had been discarding its errors entirely (sixteen `let _ = ...`).
    //
    // That pairing is the interesting part: the one severity nobody had
    // wired, and the one failure nobody was reporting, were the same gap seen
    // from two directions.
    // otherside::needs_evidence came off 23 Sep 2026: "argue the other side"
    // names the evidence-needing angles it could not argue (`not_raised`).
    // `outbox::mark_discarded` was an ORPHAN until 21 Sep -- the counterpart
    // to `mark_sent`, defined and tested but never reachable. `Daemon`'s Mail
    // intent now calls it from `discard_draft`: "throw away the reply to Jane"
    // marks the held draft `Discarded` so it leaves `waiting()` and can no
    // longer be picked up by the auto-send sweep once `may_email_clients` is
    // on. Before this a rejected draft sat `Waiting` forever, one setting away
    // from being emailed.
    // perf::current_interval came off 22 Sep 2026: deleted. Accessor on the
    // adaptive scan interval that nothing read. Also removed from KNOWN in the
    // same change.
    // reference::perishable came off 22 Sep 2026: deleted. Self-documented as
    // superseded ("the actual decision comes from freshness::shelf_for"); no
    // consumer. Also removed from KNOWN in the same change.
    // `register::opinions_welcome` came off 21 Sep 2026: `Persona::prompt_for`
    // now gates a "volunteer a view rather than wait to be asked" instruction
    // on it, the sibling of the `humour_welcome()` call one line down. It also
    // newly reaches the `AboutAtlas` register, which the predicate names but
    // the hand-written arm never told the model about.
    // settings::idle_but_on came off 23 Sep 2026: the hub settings page
    // banners switches that are on while their capability is blocked.
    // stale::to_stop came off 22 Sep 2026: deleted. Public `|entry-stop|`
    // accessor with no consumer (the used sibling `to_target` is private). Also
    // removed from KNOWN in the same change.
    // `wants::slowest` came off 21 Sep 2026: the `Recommend` intent ("what
    // could make you faster") builds `Observations` from the turn's timings
    // and then only spoke `recommend`/`ask`, which fall silent when nothing
    // here is worth changing. `Daemon`'s handler now leads that reply with
    // `obs.slowest()` -- the stage in fact slowest right now, from the same
    // timings -- so the self-report names its own bottleneck instead of going
    // quiet on the one question it exists to answer.
];

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
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
            if let Ok(text) = std::fs::read_to_string(&path) {
                // A child of a split module (`src/daemon/late.rs` beside
                // `src/daemon.rs`) is part of that module: its methods keep the
                // `daemon::` name the lists use, and a call from a sibling file
                // stays an in-module call (27 Sep 2026; none exist yet).
                crate::common::push_module(out, &path, stem, text);
            }
        }
    }
    let mut out = Vec::new();
    walk(std::path::Path::new(dir), &mut out);
    out
}

/// Drop `#[cfg(test)]` blocks.
///
/// Without this, a method's own unit tests count as production callers and the
/// whole guard reports nothing. The four-way classification only means
/// anything if "called" means called by code that ships.
fn without_test_blocks(body: &str) -> String {
    let mut out = Vec::new();
    let mut in_test = false;
    let mut opened = false;
    let mut depth: i32 = 0;
    for line in body.lines() {
        if line.trim_start().starts_with("#[cfg(test)]") {
            in_test = true;
            opened = false;
            depth = 0;
            continue;
        }
        if in_test {
            // Ended when the braces it opened close again -- on any line, not
            // only one that also opens a brace (2 Oct 2026: a test module
            // whose last line is a lone `}`, in daemon/making.rs, swallowed
            // every daemon file after it and hid all their calls).
            if line.contains('{') {
                opened = true;
            }
            depth += line.matches('{').count() as i32;
            depth -= line.matches('}').count() as i32;
            if opened && depth <= 0 {
                in_test = false;
            }
            continue;
        }
        out.push(line);
    }
    out.join("\n")
}

/// Bare names called anywhere in `text`, skipping comments.
///
/// **Definition lines are skipped, and that is load-bearing.** `pub fn foo(`
/// contains `foo(`, so counting it would make every function a caller of
/// itself, every candidate a "helper", and this guard would report an empty
/// backlog while a real one of 266 sat behind it. The first run of this file
/// did exactly that and reported that 265 of its own 266 entries had gained
/// callers -- a detector agreeing with nothing, which is the failure this
/// whole tree is built to catch, produced by the catcher.
fn called_names(text: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    for line in text.lines() {
        let t = line.trim_start();
        // Any definition line, not only `pub fn` / `fn` (27 Sep 2026): after
        // daemon.rs is split into child modules its methods are spelled
        // `pub(super) fn` / `pub(crate) fn`, and each of those lines would
        // otherwise count as a call to itself.
        if t.starts_with("//") || crate::common::is_fn_definition(t) {
            continue;
        }
        let b = line.as_bytes();
        let mut j = 0;
        while let Some(rel) = line[j..].find('(') {
            let at = j + rel;
            let mut s = at;
            while s > 0 {
                let c = b[s - 1];
                if c.is_ascii_alphanumeric() || c == b'_' {
                    s -= 1;
                } else {
                    break;
                }
            }
            if s < at {
                out.insert(line[s..at].to_string());
            }
            j = at + 1;
        }
    }
    out
}

fn unwired_modules() -> BTreeSet<String> {
    std::fs::read_to_string("tests/wiring.rs")
        .expect("tests/wiring.rs")
        .lines()
        .filter_map(|l| l.trim().strip_prefix('"').and_then(|r| r.split('"').next()).map(String::from))
        .collect()
}

struct Found {
    test_only: BTreeSet<String>,
    orphans: BTreeSet<String>,
}

fn current() -> Found {
    let src: Vec<(String, String)> =
        read_tree("src").into_iter().map(|(m, b)| (m, without_test_blocks(&b))).collect();
    let calls_by_module: Vec<(String, HashSet<String>)> =
        src.iter().map(|(m, b)| (m.clone(), called_names(b))).collect();

    let test_text: Vec<String> = std::fs::read_dir("tests")
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("rs"))
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .collect();
    let test_calls: HashSet<String> =
        test_text.iter().flat_map(|t| called_names(t)).collect();

    let unwired = unwired_modules();
    let mut found = Found { test_only: BTreeSet::new(), orphans: BTreeSet::new() };

    for (module, body) in &src {
        if unwired.contains(module) || module == "main" || module == "lib" {
            continue;
        }
        let own_calls = called_names(body);
        for line in body.lines() {
            // Indented only. Column-zero `pub fn` is the existing guards' job,
            // and counting it here would double-count the whole backlog.
            if !(line.starts_with(' ') || line.starts_with('\t')) {
                continue;
            }
            let trimmed = line.trim_start();
            let Some(rest) = trimmed.strip_prefix("pub fn ") else { continue };
            let name: String =
                rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            if name.is_empty() || name.ends_with("_for_test") {
                continue;
            }
            if calls_by_module.iter().any(|(m, c)| m != module && c.contains(&name)) {
                continue;
            }
            let own = own_calls.contains(&name);
            let tested = test_calls.contains(&name);
            match (own, tested) {
                (true, _) => {}
                (false, true) => {
                    found.test_only.insert(format!("{module}::{name}"));
                }
                (false, false) => {
                    found.orphans.insert(format!("{module}::{name}"));
                }
            }
        }
    }
    found
}

// The two ratchets below repeat themselves, and that is the point.
//
// They were one shared helper until `retrospective.rs::no_test_asserts_nothing`
// caught them: with the assertions behind a call, neither test body contained
// the word `assert`, and a person checking whether these tests check anything
// could not tell by reading them. Rewriting the helper as a macro did not fix
// it either -- a macro is a differently-spelled helper and the body still says
// nothing. So the assertions are written out, twice. In a test file, being
// visibly true beats being concise.

#[test]
fn no_method_gains_or_loses_dead_status_silently() {
    let found = current();
    let known: BTreeSet<String> = TEST_ONLY_METHODS.iter().map(|s| s.to_string()).collect();

    let added: Vec<&String> = found.test_only.difference(&known).collect();
    assert!(
        added.is_empty(),
        "these methods are built, tested, and called by nothing:\n  {}\n\nWire one in the same \
         change, or add it to TEST_ONLY_METHODS with the reason. The list may grow; it may not \
         grow silently.",
        added.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n  ")
    );

    let cleared: Vec<&String> = known.difference(&found.test_only).collect();
    assert!(
        cleared.is_empty(),
        "these are listed as having no caller and now have one:\n  {}\n\nGood -- delete those \
         lines so the list keeps meaning something.",
        cleared.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n  ")
    );
}

#[test]
fn no_orphan_method_appears_or_disappears_silently() {
    let found = current();
    let known: BTreeSet<String> = ORPHAN_METHODS.iter().map(|s| s.to_string()).collect();

    let added: Vec<&String> = found.orphans.difference(&known).collect();
    assert!(
        added.is_empty(),
        "these methods are public, called by nothing, and tested by nothing -- so nothing here \
         has ever shown they work:\n  {}\n\nWire one, delete it, or add it to ORPHAN_METHODS \
         with the reason.",
        added.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n  ")
    );

    let cleared: Vec<&String> = known.difference(&found.orphans).collect();
    assert!(
        cleared.is_empty(),
        "these orphans now have a caller or a test:\n  {}\n\nGood -- delete those lines.",
        cleared.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n  ")
    );
}

#[test]
fn both_lists_are_sorted_and_free_of_duplicates() {
    for (list, name) in [(TEST_ONLY_METHODS, "TEST_ONLY_METHODS"), (ORPHAN_METHODS, "ORPHAN_METHODS")] {
        let mut sorted = list.to_vec();
        sorted.sort_unstable();
        assert_eq!(list, &sorted[..], "{name} is not sorted");
        let unique: BTreeSet<&&str> = list.iter().collect();
        assert_eq!(unique.len(), list.len(), "{name} has a duplicate");
    }
}

#[test]
fn this_guard_covers_the_surface_the_others_do_not() {
    // The premise, asserted rather than assumed. If the tree were ever
    // refactored so that methods were rare, this guard would be pointless and
    // should say so instead of passing silently on an empty scan.
    let src = read_tree("src");
    let (mut col0, mut indented) = (0usize, 0usize);
    for (_, body) in &src {
        for line in without_test_blocks(body).lines() {
            if line.starts_with("pub fn ") {
                col0 += 1;
            } else if line.trim_start().starts_with("pub fn ")
                && (line.starts_with(' ') || line.starts_with('\t'))
            {
                indented += 1;
            }
        }
    }
    assert!(
        indented > col0,
        "indented pub fn ({indented}) no longer outnumbers column-zero ({col0}); the premise of \
         this file has changed and its header needs rewriting"
    );
}
