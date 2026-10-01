//! The dead-capability list, decided rather than counted.
//!
//! `bug_sweep.rs` used to hold a ceiling — a single number, the sum of the
//! four groups below, that could only shrink. It is gone, and the way it
//! failed is the most useful thing in this file.
//!
//! Two faults. The first: "292" tells you something is wrong somewhere and
//! nothing about what, so every encounter with it ended in reading a
//! justification comment and moving the number. A ceiling is a smoke alarm,
//! not a diagnosis, and a sum of four unrelated measurements is a smoke alarm
//! wired to four different rooms.
//!
//! The second is worse, and it is why the number is gone rather than split.
//! One of the four groups — **helper, also tested** — is live, tested,
//! running code. When the brief was rebuilt to read this machine instead of
//! an inbox, the gathering was written as ten small well-named functions and
//! the ceiling went *up* by six. Inlining all ten into one unreadable
//! function would have brought it back down. **A ratchet that rewards the
//! worse version of the same code is worse than no ratchet, because it gets
//! obeyed.**
//!
//! So the four groups are still computed — they are genuinely different
//! problems — but only three are held to anything:
//!
//! | group | what it means | held to |
//! |---|---|---|
//! | **orphan** | nothing calls it. Not production, not its own module, not even a test. The `hollow` pattern in its purest form, and the only group that is straightforwardly wrong. | named, one by one, with a reason each |
//! | **test-only** | a real capability with a test and no production caller. Built, proven, never reached — the group worth working through. | an exact count |
//! | **helper, untested** | called only inside its own module, no direct test. `pub` for no reason anything can see. | an exact count |
//! | **helper, also tested** | called inside its own module, tested directly. This is just *code*. | nothing, deliberately |
//!
//! Exact counts, not ranges. The ranges were set "so an unrelated commit does
//! not fail", and that slack is what let test-only sit at 147 under a ceiling
//! of 160 without anyone noticing it could have been 143. A number that may
//! only fall is a ratchet; a number with headroom is a suggestion.

use std::collections::{BTreeMap, BTreeSet};

// The "does anything call this" rule, from `tests/common/mod.rs`. It used to
// live here *and* in the other file, byte-identical — two lists that have to
// agree, agreeing because two copies happened to match. One copy now.
mod common;
use common::{called_names, calls};

/// Every function with no caller anywhere in the tree, and what each needs.
///
/// A new entry here fails the build **by name**, which is the whole point:
/// the ceiling could only tell you the number went up.
const ORPHANS: &[(&str, &str)] = &[
    (
        "brief::as_chain",
        "the morning brief as a resumable chain, so a run that dies halfway \
         says where it stopped. Needs the morning run to actually execute as a \
         chain, which is a change to how the brief is driven rather than one \
         more call.",
    ),
    (
        "brief::from_mail",
        "build the brief from mail rules rather than raw categories. Waits on \
         mail being a live source; `mail.rs` itself is wired but nothing feeds \
         its categories into the brief.",
    ),
    (
        "brief::vet_draft",
        "run a drafted reply past `draft`'s critique before it is shown. Wants \
         the brief to be drafting replies in the first place.",
    ),
    // selfwork::run_tests came off this list on 20 Sep 2026: closing the
    // self-improvement loop's Build stage gave it a caller. `prove_in_a_copy`
    // runs the suite in a copy of the tree, before and after the candidate, to
    // tell whether the proving test passes and nothing else broke. The ruling
    // this entry was waiting on — is Atlas allowed to run its own tests — was
    // answered by *where*: in a scratch copy, never the tree you run, and the
    // change only lands with your say-so through `land_it`.
    // vault::seal_bytes and vault::unseal_bytes came off this list on 18 Sep
    // 2026, and the decision they were blocked on turned out to be a
    // different question than the one recorded here. The open question was
    // whether DPAPI should wrap the vault's own AEAD path as defence in
    // depth; that is still open and still needs Windows hardware. What gave
    // them a caller is a second secret that did not exist when this entry was
    // written: the household key phrase. `sync::KeptKey::keeping` seals it
    // with the OS where there is an OS to ask, keeps it as text where there
    // is not, and `at_rest_says` tells the person which of the two they got
    // rather than leaving them to assume the better one.
];

/// How many sit in each group that measures something real.
///
/// **Two of the four original numbers are gone**, and it is worth being exact
/// about why, because the mistake is easy to make again.
///
/// `helper_tested` counted functions that their own module calls and that a
/// test covers. That is *live, tested code*. It ran in production every time
/// the module ran. Counting it as a "dead capability" meant that building a
/// feature out of ten well-named private pieces looked exactly like adding ten
/// dead functions, while inlining all ten into one long function looked like
/// progress. It had no ceiling worth setting and it is not counted any more.
///
/// `DEAD_CAPABILITY_CEILING` in `bug_sweep.rs` was the sum of all four, so it
/// inherited that fault and added its own: one number that could move for four
/// unrelated reasons tells you nothing about which. It is gone too.
///
/// What is left measures deadness:
///
/// * `ORPHANS` — nothing anywhere calls it. Exact, by name, with a stated
///   reason each. The strongest of the three because it cannot drift.
/// * `test_only` — only the tests call it. Built, proven, never reached. This
///   is the backlog.
/// * `helper_untested` — its own module calls it, nothing tests it, and it is
///   `pub` for no reason anything can see. An API-surface measure, not a
///   deadness one, but a real problem: most of these should be private.
///
/// Exact, not ranges. The previous values were set as ranges "so an unrelated
/// commit does not fail", and the slack is exactly what let 147 sit under a
/// ceiling of 160 for weeks without anyone noticing it could have been 143.
/// A number that may only fall is a ratchet; a number with headroom is a
/// suggestion. Raising either of these should take a sentence saying why.
// 141 -> 142 (14 Sep): `mend` came off the unwired list, which un-skips every
// function in it -- and `worth_trying`, the paper-over refusal, is blocked on
// the `selfwork::run_tests` ruling. Named in KNOWN with that reason rather
// than absorbed. The Question half is wired, and `about_ambiguity` (written
// in the same pass) was deleted rather than given an invented caller.
// 142 -> 141 (14 Sep): `nudge::offer_to_mend` got a production caller. The
// correction loop is wired -- a second correction on a separate occasion is
// offered as an edit, and saying yes writes it where `context()` reads it.
// 143 -> 142 (14 Sep): `nudge::convene` got a production caller. The council
// is wired to `Intent::AskTheRoom`, and a question that is not about hardware
// takes the general room `convene` has always built.
// 141 -> 140 (14 Sep): `workspace::input_blocked_apps` got a caller when
// dictation was wired. It is one of the two lists that decide whether Atlas
// may type into a window — the other being `dictate.never_into` — and it had
// been sitting there deciding nothing.
// 140 -> 141 (14 Sep): wiring `quickinput` moved it out of the
// module-unwired list and its unused surface into this one. `voice_failed`,
// `voice_worked`, `tick` and `backspace` belong to a box that opens *itself*
// when speech keeps failing -- and the daemon cannot currently tell a spoken
// turn from a typed one. `Arrival` is only `Directed`/`OpenMic`, and `turn()`
// uses `Directed` for the command line too, so wiring `voice_failed` to an
// unrecognised turn would pop a typing box at someone who is already typing.
// Carrying "this arrived as speech" on the turn is the real fix and is a
// change to the arrival plumbing, not a caller. Named rather than faked.
// 141 -> 139 (15 Sep, mail merge): `mail::credential_source`,
// `mail::Provider::from_address`/`imap_host`, and `unsub::one_click` all got
// real production callers as part of the mail feature (`check_mail`,
// `check_unsubscribe`, `carry_out_unsubscribes`) merged in from a parallel
// session. Net -2 against that session's own reported -1 (142 -> 140)
// because this tree's ceiling started from 141, not 142.
// 139 -> 140 (16 Sep): net +1 from the `look`/`backends`/`language` wiring
// pass. Wiring those three cleared several entries, and `language::notes` and
// `language::live_line` were added against them -- both format a multi-speaker
// conversation and need speaker diarization, which the current whisper path
// does not produce. Named rather than faked, per the guard's own instruction.
//
// This number and the `KNOWN` list in `new_capabilities_are_wired.rs` are two
// counts of the same set, updated in two files. That pass moved the list and
// not this constant, and the package shipped reporting 225/225 green while
// both tests here were red -- the drift this file's own header warns about,
// arriving through the one route a ratchet cannot defend: nobody ran it.
// 140 -> 139 (17 Sep): `interrupt::mute_from` got a caller --
// `daemon.rs:3547`, beside the `unmute_from` call already there -- so telling
// Atlas to stop talking about something now reaches the thing that records
// it. Lowered, and deleted from `KNOWN` in `new_capabilities_are_wired.rs` in
// the same change, which is the pair the comment below is about.
// 139 -> 404 (17 Sep). **Nothing was built and nothing broke.** Three
// corrections to what this scan can see, applied together because applying
// them one at a time would have moved this number three times for reasons
// that only make sense as a set:
//
//   1. **Trimming.** `line.strip_prefix("pub fn ")` ran on the raw line, so
//      only functions at column zero counted: 1,121 of 3,216. Every method
//      inside an `impl` block was invisible, and 31 whole modules -- `log`,
//      `intent`, `memory`, `outbox`, `crew`, `clients`, `scheduler`,
//      `timing` among them -- declare no top-level function at all and were
//      outside this measurement entirely, without anyone deciding that.
//      `dead_methods.rs` in this same suite has always trimmed, so the two
//      files disagreed about what a public function is.
//   2. **Inline tests.** Whole source files were read, so a `#[cfg(test)]`
//      block counted as the program. `dead_methods.rs` already strips them;
//      this file did not.
//   3. **Stale prose.** Two ORPHANS entries carried reasons that no longer
//      matched the tree, and both are gone from the list rather than
//      explained on it.
//
// This file previously argued against tightening `calls()` because it would
// move four ceilings "by an amount nobody could review". That caution is
// right about unreviewed jumps and wrong as a reason to keep a number that is
// measuring a third of the tree. The jump is reviewable: orphans did not
// grow at all, no entry needed a new excuse, and the two that were retired
// were the two whose own reasons said they did not belong.
// 404 -> 403 (18 Sep). `daemon::turn` got its first caller: `prompt_line`
// now takes a turn instead of parsing the line against the phrase list and
// running the result. That one call is the whole difference between typing
// at Atlas and talking to it -- it is what puts the model, the assembled
// context and the conversation thread on the typed path, all of which
// existed and were reachable only by voice.
//
// 403 -> 410 (18 Sep, later the same day). **This one went up, and it went up
// because work was done rather than skipped.** `overnight` was wired to the
// tick and `delegate` became reachable through it, so both left
// UNWIRED_BASELINE -- and a module on that list is not measured here at all.
// Their contents did not become dead; they became *visible*. Nothing was
// built and left unreached.
//
// What is genuinely still unreached inside them, and why:
//
//   overnight::morning_detail   the long form behind the brief. Needs a way
//                               to ask for it ("tell me more about last
//                               night"), which is an intent, not a wiring fix.
//   overnight::delegation_for   only ever returns Some under the `delegate`
//   overnight::spend_turns      brain, which is not wired.
//   delegate::*                 the same: reached, not run.
//
// The honest alternative was to leave both modules on the unwired list, where
// none of this would be counted and the ceiling would have read 403 while
// twenty-odd functions sat unmeasured. A number that stays still by not
// looking is the failure this file exists to prevent.
// 410 -> 408 (18 Sep). `sync` was wired too: captures append to a
// `sync::Log`, and `Intent::Sync` writes a bundle into a folder both machines
// can see and takes in what the other side left. `sync::make_bundle` and
// `sync::already_seen` gained real callers -- the second one because reading
// the same folder twice must not count the same events again, which is the
// normal case rather than an edge one.
// 408 -> 405 (18 Sep). Three of `crew`'s own introspection methods --
// `queued`, `in_hand` and `why_waiting` -- got their first callers. "What's
// queued" used to answer only about posts waiting to be sent, while the
// errand queue those three were written to describe went unmentioned.
// 405 -> 403 (18 Sep), on the merge of the two trees. The two numbers this
// moved by are worth keeping apart, because they nearly cancelled and mean
// opposite things.
//
// **Down 5**, from work the other tree had done and this guard had never
// measured, because that tree's copy of this file was the older, under-
// counting one (its ceiling read 137 against the same bucket):
//
//   persona::prompt_for       the character now reaches the model. Its own
//                             doc named the bug it was sitting out.
//   retention::discard_audio  a recording no longer outlives its transcript.
//   safety::trouble           the trash says which kind of trouble it is in.
//   uia::with                 a stale entry; nothing in that pass touched it.
//   pipeline::refinement_is_warranted
//
// **Up 3**, and these are honest: `pipeline::record_build`,
// `record_refinement` and `record_review` are public, tested and called by
// nothing. They came in with the other tree's `pipeline.rs`, which this
// guard had not been measuring either.
//
// A fourth movement was caught and reversed rather than absorbed. The first
// pass of the merge lost the `Intent::Queued` arm, which is the only caller
// of `crew::queued`, `crew::in_hand` and `crew::why_waiting` -- so the
// number read 406, and three capabilities that had been wired on 18 Sep were
// silently dead again. Found by dumping both trees' sets and diffing them,
// which is the check worth repeating on any merge this size: a ceiling that
// moves by the right amount for the wrong reasons is indistinguishable from
// one that moves correctly.
// 403 -> 402 (18 Sep 2026, config-key sitting). `should_be_discreet` was
// reachable only from its own tests: `discreet_with_strangers` was a setting
// nothing read, and `reach_you` never looked at the camera at all. Wiring the
// setting -- through `keep_it_to_yourself`, to `Note::shown` rather than to
// routing, which is where `notify::route` had already ruled discretion
// belongs -- gave it a production caller.
// 402 -> 401 (18 Sep 2026, sealing). `b64::decode` had no production caller
// -- everything in the tree encoded and nothing read back, which is its own
// kind of tell. A sealed bundle's body and an OS-sealed key phrase are both
// base64 on the way in and out, so the half that was missing is the half
// sealing needs.
// 401 -> 400 (19 Sep 2026, the catalogue). `capability::in_area` grouped the
// inventory by area and was reached only from `capability::full`, which
// nothing outside the tests called -- "what can you do?" was answered by a
// one-line summary and the grouped form existed to be asserted about.
// `atlas catalog` prints the catalogue by area for a platform, so `in_area`
// now has the production caller it was written for.
// 400 -> 396 (19 Sep 2026, the wiring pass).
//
// `confirmed.rs` got its first production caller. It is the read-back that
// catches "turn it off on Instagram" heard as "on Instagram and Facebook" --
// and Instagram's settings page changes both. The module was written for
// Atlas making the change itself, which nothing in this tree does, so there
// was no caller to write; `atlas walkthrough turn-off` is the one place a
// security change actually gets started, and it started with no read-back
// and no yes. `before_a_run`, `answer`, `record`, `how_to_undo` and the
// `Run` it steps through are all reached from there now.
//
// `reference::stale_warning` moved with them, from the other direction: it
// says how old a shelf is and had no caller at all, because `nothing_found`
// claimed to hold shelves that have never been fetched and so never had an
// age to report. It says which of the two it is now.
//
// The net is four rather than the count of those, because the same pass added
// public functions of its own, and a new one nothing outside its module calls
// lands in this same group. The number is what the tree measures; the
// attribution above is what changed.
// 396 -> 390 (19 Sep 2026, the access pass). Six, and they were one
// capability rather than six omissions.
//
// `signin.rs` says of itself that "access is per-site and revocable from one
// page", and none of it worked: nothing called `grant`, so there was never a
// grant; nothing loaded or saved an `Access`, so one could not have survived
// a restart; the access page was handed an empty list of sites, so the
// section listing the open doors never rendered; and the revoke buttons it
// draws posted to routes that did not exist. `may_start` refusing with
// "I don't have access to that" was the only answer the module could give,
// and it was right every time for the wrong reason.
//
// `atlas access` is the way in, the Access page is the way out, and
// `note_use` records every attempt -- which is what makes `superseded_by`
// and `probably_changed` answerable at all.
// 390 -> 384 (19 Sep 2026, the trip pass). Six more, and they were two
// modules waiting on the same missing thing.
//
// `going_away.remind_days_before` is "remind you this many days before a trip
// you've told it about" and there was no way to tell it about a trip -- no
// date anywhere in the tree. `periodic_nudge` takes `days_since_last` and
// nothing kept a last. `codes.check_days_before` was a third threshold on
// that same absent date. And `codes` could say what was missing while
// `used_one`, `logins_available` and `Set::running_low` had no callers, so
// there was no way to tell it anything had changed.
//
// `goingaway::Away` is the date, written down. `atlas away` and `atlas codes`
// are the way in.
// 384 -> 380 (19 Sep 2026, the statements pass). Four, and the same shape
// as the two before it: a complete capability with nothing at either end.
//
// `finance::parse_csv` reads a bank export and had no caller. `money::sort_one`
// puts a line in a bucket and had no caller. `summarise`, `spoken`,
// `new_or_grown` and `work_spend` had none either -- and the daemon's money
// branch called `money::spoken(&money::summarise(&[]), &[])`, an empty slice,
// so "Nothing to go on." was the only answer it could give. The reason was
// that nothing had ever read a statement, not that there was nothing in it.
// Same defect as `goingaway::spoken(&[])` and `messaging::spoken(&[], ..)`.
//
// `atlas money <export.csv>` is the way in.
// 380 -> 381 -> 380 (19 Sep 2026, the footprint). **The number did not move
// and something real happened**, which is the one case this file's exact-
// equality rule cannot express on its own, so it gets written down here.
//
// `install::total_mb` was renamed `download_mb` in the same afternoon's
// install pass, for a reason that had nothing to do with this file: `atlas
// install` printed "4801MB to fetch" beside a 341MB download, because the
// piece list was filtered and the number under it was not. The rename was
// about that.
//
// This scan reads bare names. `total_mb(` in `main.rs` had been enough to
// mark `retention::Usage::total_mb` reached, and it is a different function
// in a different module. The moment the install one stopped being called,
// this count went **up to 381** and a capability that had been dead since it
// was written appeared -- while nothing had been deleted and nothing had
// broken. Wiring it brought the count back to 380.
//
// So the arithmetic for the day reads 384 -> 380 -> 381 -> 380, and a reader
// diffing only the endpoints would see the statements pass and nothing else.
// The rise is the part worth keeping: it is the only evidence that
// `retention::total_mb` was ever dead.
//
// What it is: `atlas reclaim` walks your home folder and your temp folder,
// names other people's caches and old installers, tells you how much you
// could get back, and said nothing at all about Atlas's own data folder.
// `survey` walks that folder, `usage` groups it by what each file is for, and
// `total_mb` is the answer to "how much". `report_own_footprint` in `main.rs`
// prints it on both ways out of `atlas reclaim`, against the budget from your
// own `retention:` block, and deletes nothing -- the hourly housekeeping pass
// owns that folder and two things pruning it on two rules is how a backup
// goes missing.
//
// The general lesson, third time in this tree: a collision does not only hide
// a name, it holds a dead function up. Clearing one is how you find out.
// 380 -> 378 (19 Sep 2026, the platforms list). Two, and they are the two
// halves of an answer that had never been given.
//
// `messaging.platforms` is a list you write and nothing read. Two of the six
// things you can put in it can never work -- WhatsApp has no personal-account
// interface, Signal is deliberately closed -- so writing `[whatsapp]` got you
// exactly the silence that writing `[telegram]` did, on the one list where
// that difference is the whole question. `Platform::what_it_permits` and
// `Platform::to_connect` say which is which and what each needs, and
// `worth_setting_up` is the sort order; none of the three had a caller.
//
// `messaging::what_you_asked_for` is the answer, reached from `Intent::Mail`
// when you ask about messages. It still refuses to give a count: nothing in
// this build reads messages, and the sentence it replaced was
// `messaging::spoken(&[], ..)` returning "0 messages, all group chat" -- a
// count of an inbox nothing had read, stated as fact. The line worth keeping
// is that an honest answer about the *list* needs no reader, while an honest
// answer about `your_names` does -- which is why that one went the other way
// in the same pass, to `#[serde(skip)]` and PROMISES_ABOUT_WHAT_IS_NOT_BUILT.
//
// 378, then 379, and the extra one is `mesh::honest` -- **not a regression**.
// `Platform::honest` was the third member of that answer, and calling it made
// `mesh::Mesh::honest` look reached, which it is not: `dead_methods.rs` has
// carried it as dead for weeks. Renamed to `Platform::what_it_permits` at the
// call rather than after a guard caught it, and `mesh::honest` goes back to
// being counted as what it is. Second time in one afternoon that clearing a
// bare-name collision raised this number by revealing something already dead
// (`retention::total_mb` was the first), which is the strongest argument yet
// for not letting two modules share a word.
// 379 -> 373 (19 Sep 2026, the private network). Six, and the split is the
// same one the platforms list got an hour earlier.
//
// `mesh` is on CAPABILITY_UNWIRED and stays there: `choose` picks between
// SameNetwork, Mesh, Cable and Cloud, and only Cloud is built -- syncing goes
// through a folder both machines can see, and nothing in this tree reaches
// another device directly. That is the transport, and it is honestly missing.
//
// The advice is not missing. `honest`, `needs_a_server`,
// `third_party_in_the_path`, `setup_steps`, `what_it_adds` and
// `works_without` were all written, all correct, and all reached by tests
// alone -- and not one of them needs a transport to be true. `mesh.kind` was
// the setting underneath: a string nothing parsed, so `kind: tailscale` and
// `kind: banana` were the same setting.
//
// `atlas mesh` is the way in, and it opens by saying the transport is not
// built, because a page of setup steps without that line is how somebody
// spends ten minutes installing Tailscale and then finds nothing uses it.
// `prefer_direct` went the other way in the same pass -- its only reader is
// `choose` -- to `#[serde(skip)]` and PROMISES_ABOUT_WHAT_IS_NOT_BUILT.
// 373 -> 372 (19 Sep 2026, the day's run). `daily::rolls_at` got a caller.
//
// It is the hour your day turns over -- the middle of the longest quiet
// stretch, which `Rhythm` works out from when you actually stop -- and
// nothing asked it. The brief had just been wired to `brief.at_hour`, a fixed
// clock time, and that was the wrong fix for the right gap: a brief is worth
// having when you *start*, and seven o'clock greets a night session at its
// fourth hour while missing the morning that began at ten.
//
// `daily::arriving` asks instead whether you went away and came back, against
// a day boundary that is yours rather than the calendar's. So the hour Atlas
// had worked out and never used is now the hour the answer depends on, and
// "I worked through the night" and "I am starting my day" stopped being the
// same sentence to it.
// 372 -> 374 -> 372 (19 Sep 2026, the numeric half). Up two, then back down,
// and the round trip is the useful part.
//
// `judgment::weighed_together` and `only_partly_measured` are the composite
// half of the substrate. They went up because the first caller proposed for
// them would have meant inventing five dimensions and a weight for each --
// numbers that appeared in no spec and rested on no measurement. Holding
// them for that caller would have been a promise about something nobody
// intends to build.
//
// The real caller was in personal Atlas all along. `Post::quality` was:
//
//     held_at_three * 0.5 + completion * 0.3 + kept() * 20.0 * 0.2
//
// Two fractions and a *rate*, brought into range by a hand-fitted twenty --
// so if the real save rate is 0.005 the "0.2 weight" was really 0.02, and if
// it is 0.15 it was really 0.6. The number written as the weight and the
// number doing the weighing were different and nothing said so.
// `reach::quality_against` normalises each part by how much that part varies
// across your own posts, which calibrates itself and makes the weights mean
// what they say.
// 372 -> 374 (19 Sep 2026). Nothing was built and nothing was wired: two
// functions lost their only caller outside their own tests when that caller
// moved to its own copy. The number says, correctly, that the copies here had
// no caller in this crate.
// 374 -> 354 (20 Sep): the wiring session after the merge. marks (10),
// mind (7), flow's Run/needs_approval/deny, handshape's any_motion /
// any_held / overlaps, enrol::permitted -- each got a production caller,
// and three more (overlay::begin, sandbox::promote, selfwork::begin) were
// cleared by bare-name collision with the new mind wiring, which is the
// scan hazard the 19 Sep handoff warned about, recorded there.
// 354 -> 335 (20 Sep): the orphan-and-unwired pass. sync's can_clash/subject
// (a racing delete that merged clean and lost one side -- a real bug, not a
// dead method), the whole attention state machine (suspend/allows/may_speak/
// was_halted -- pause left running work going, and a scheduled job talked over
// the quiet you asked for), lanes' push_online/waiting_for_network/
// waiting_for_gap (needs_net was false on every task ever queued, so the
// offline hold was unreachable), memory's prefer/record_workflow/touch_project/
// habits (three of five stores had no writer; the one reader could only return
// None), publish's edit/request_approval (every drafted post was an empty Draft
// forever), connectivity::allows, mail::smtp_port, outbox::spoken_notice,
// undo::possible, and the daemon's say_interruptibly/finish_saying (every reply
// was uninterruptible and "carry on" finished nothing), learned/another_way
// (research threw away every answer and never offered a second route),
// keep_awake (overnight work started on a dying battery). Each got a real
// production caller or was the named half of a duplicated fact.
// 335 -> 330 (20 Sep, the grants pass). The whole app-permission gate was
// dead: `Daemon.permissions` was constructed empty and referenced nowhere.
// Wiring it end to end gave callers to `check`/`consume`/`new_session`/
// `grant`/`grant_in_instruction`/`span_from_answer`/`granted_apps` — an
// unknown app is now asked about before Atlas touches it, "use Excel to..."
// grants Excel for that task, the answer's breadth (once / this session /
// always) is honoured, only "always" survives a restart, and the granted
// apps show on the permissions page. `grants::message` (Verdict::message)
// stays test-only — the gate matches the Verdict directly.
// 330 -> 329 (20 Sep, dead-settings pass): clipboard::refers_to_clipboard
// gained a caller — only_on_request now gates whether the clipboard is in
// scope for pronoun resolution, where before it was silently in scope every
// turn.
// 329 -> 324 (20 Sep, optimization): five pub fns that were duplicates of a
// live path and reached by nothing were deleted, not wired — server::json (an
// alias for Reply::ok), handshape::middle_tip/ring_tip (the private tip() covers
// all five fingers), identity::remember_proof (a second writer for proved_at
// that record() already owns), look_paint::css_class (no caller, no test). Not
// needed is different from unwired-but-useful.
// 325 -> 324 (20 Sep): the draft revise loop (`draft::revise`) gained a
// production caller in the mail-draft path, and `revise` in turn calls
// `draft::improved` -- which until now only tests reached. One capability
// left the test-only bucket by being wired to real work.
// 324 -> 325 (20 Sep): the built-in calendar's bridge to the native phone
// calendar -- `calendar::for_phone` and `merge_from_phone` -- is built and
// reached only by tests, because its real caller is the phone app's EventKit
// / CalendarProvider adapter, which isn't in this tree (same boundary as the
// Android client). Net +1: those two arrived (+2) and `time::say_date` gained
// a production caller when `calendar::Event::say_when` began calling it (-1).
// 325 -> 323 (20 Sep): closing the self-improvement loop's Build stage
// (`Daemon::attempt_own_fix` drafts a fix, proves it in a copy, records the
// build and review) gave real production callers to four functions that only
// tests had reached -- `selfwork::run_tests`, `pipeline::record_build`,
// `pipeline::record_review` and `pipeline::review`. The loop's own new pieces
// (`draft_fix`, `files_named`, `lines_touched`) all have callers, so they add
// nothing to this bucket. Net -2.
//
// 323 -> 322 (21 Sep): integrated verification's `named_existing_file` calls
// `craft::Lang::of_path` to tell a real source path in a request from a word
// that only looks like one, so `of_path` now has a production caller.
// 322 -> 321 (21 Sep): `outbox::mark_discarded` gained a production caller.
// `Daemon`'s Mail intent now routes "throw away the reply to Jane" to a new
// `discard_draft`, which marks the held draft `Discarded` so it leaves the
// waiting set and the auto-send sweep can never email it. It was the
// counterpart to `mark_sent`, tested but reached by nothing until now.
// 321 -> 320 (21 Sep): `connectivity::deferral_message` gained a production
// caller. `research()`'s offline branch hand-wrote its own "no connection,
// on the outstanding list" sentence beside a comment admitting it was "the
// spoken half"; it now calls `deferral_message(&Intent::Research(..))`, the
// one place that answers "the network was needed and isn't here". Research is
// the only `Need::Internet` intent, so this is that function's real home --
// one fact, one copy, not two.
// 320 -> 319 (21 Sep): `vault::should_lock` gained a production caller.
// `Daemon::tick` now asks it among the security housekeeping and calls
// `vault::lock` when the idle limit has passed, so an ordinary `atlas vault`
// unlock no longer stays open for the life of the process. `lock_after_mins`
// was a promise nothing kept until this.
// 319 -> 318 (21 Sep): `capability::offline_count` gained a production caller.
// The spoken `Intent::Capabilities` handler now has an offline branch: "can you
// work offline" / "what can you do without internet" answers with the count of
// capabilities that survive the network being unplugged, which is the tree's
// primary promise. Before this the question fell through to the catch-all and
// was matched against a single capability by keyword.
// 318 -> 317 (21 Sep): `register::opinions_welcome` gained a production caller.
// `Persona::prompt_for` now gates a "volunteer a view rather than wait to be
// asked" instruction on it, the sibling of the `humour_welcome()` call one line
// down. It also carries that instruction to the `AboutAtlas` register, which the
// predicate always named but the hand-written prompt arm never told the model.
// 317 -> 316 (21 Sep): `safety::backups` gained a production caller. `atlas
// backups list` called `list_backups`, which returns an empty Vec whether the
// folder is empty or unreadable, so a permissions error printed "No backups
// yet." -- the false reassurance `backups` was written to refuse by returning
// `Err` for an unreadable folder and `Ok(empty)` only for a genuinely missing
// one. The list command now surfaces that error instead of hiding it.
// 316 -> 315 (21 Sep): `backlog::dismiss` gained a production caller. The
// daemon's backlog offer ("Want me to do it now?") now tracks the offered item,
// and a no at that question dismisses it -- so a refused task leaves the list
// instead of being raised again on the next quiet tick.
// 315 -> 314 (21 Sep): `certainty::aged` gained a production caller. The
// `from_notes` recall path (`daemon.rs`) now ages the note its answer rests on:
// the ranking already weighed freshness to choose the winner, but the winner
// was spoken as though checked today. `aged` reads the shelf that note sits on
// and, when it has run out, attaches "that rests on something from ... that
// changes over weeks" -- and leaves a note still inside its shelf untouched.
// 314 -> 313 (21 Sep): `wants::slowest` gained a production caller. The
// `Recommend` intent ("what could make you faster") builds `Observations` from
// the turn's timings and then only spoke `recommend`/`ask`, which fall silent
// when nothing here is worth changing. The daemon's handler now leads that
// reply with `obs.slowest()` -- the stage in fact slowest right now, from the
// same timings -- so the self-report names its own bottleneck.
// 313 -> 312 (21 Sep): `consolidate::over_budget_on_purpose` gained a
// production caller. `Atlas::learned` (`daemon.rs`) trims the knowledge store
// and spoke `dropped_note` when something was cut, but stayed silent in the
// case that function exists for: nothing dropped because the excess is all
// settled facts and things about your own setup, which trim refuses to lose.
// The handler now also notes `over_budget_on_purpose`, so a cap being exceeded
// on purpose is said rather than hidden -- exactly what its own doc asks for.
// 312 -> 311 (21 Sep): `timing::why_slow` gained a production caller. The
// on-demand self-audit ("anything to look at?") ran `refresh_signals`, which
// rebuilds `self.signals` from undo/misunderstanding/unused alone and
// overwrites the `GotSlower` signal the hourly tidy had pushed -- so a machine
// whose turns had visibly slowed answered without a word about speed. The
// handler now asks the timing window directly and speaks `why_slow` when
// `got_slower` reports a real regression.
// 311 -> 310 (21 Sep): `capability::full` gained a production caller. The
// spoken `Intent::Capabilities` handler answered "what can you do" with
// `summary` (the counts) and had no way to give the itemised list. A request
// to see the list -- "list them all", "in detail", "one by one" -- now returns
// `full`, the per-area breakdown with its own legend, so the question that
// wants the list gets the list instead of the tally.
// 310 -> 309 (21 Sep): `thread::resume_line` gained a production caller. The
// turn path prepended `pending_brief` (what happened while you were away) and
// had nothing for the other kind of gap -- present the whole time but an hour
// on something else, then turning back to Atlas. `thread.rs` was written so a
// return "picks up where it left off" by naming the topic rather than
// greeting; nothing asked it. The reply-assembly point now leads with
// `resume_line` when there is no away-brief, so "We were on the deploy
// script." reaches you on the first turn back and, because the append that
// follows resets `last_active`, is not said again on the next.
// 309 -> 308 (21 Sep): `daemon::work_a_decision` gained a production caller.
// Its own doc said it should be "reached from the assistant rather than
// sitting behind an intent of its own, because a decision usually arrives as
// a question you are already halfway through asking" -- and nothing reached
// it, so a line like "should I take the contract or keep the retainer" fell
// through to "I didn't catch that, go ahead?". The `Intent::Unknown` branch
// now runs `decision_help`, which fires on a deciding phrase
// (`decide::wants_working`) and works the decision -- returning the first
// move, the question underneath -- above the policy gate alongside
// `from_notes`, because laying a decision out changes nothing and needs no
// approval.
// 308 -> 310 (21 Sep): the clipboard round trip got its two hollow ends wired
// to a real OS clipboard through the platform layer (`read_clipboard` /
// `write_clipboard`, both reaching a real daemon caller and neither dead). The
// +2 here is not those -- it is the two mock accessors that drive them from a
// test (`mock::set_clipboard`, `mock::clipboard_now`), the same kind of
// test-only scaffolding as `mock::focus_on`. A rise, not paid-down progress,
// but an honest one: the capability it tests is now real, where before the
// daemon claimed "it's back on your clipboard" over a field nothing drained.
// 310 -> 309 (21 Sep): `freshness::should_recheck` gave the recalled-note path
// in `daemon::from_notes` a real caller. The age caveat already told you a note
// was old; `should_recheck` is the separate judgement of whether Atlas can do
// anything about it -- worth rechecking by shelf, stale by state, and from a
// source it can re-read alone -- so the answer now offers to go back to the
// file it came from, and only when going back is actually in reach.
// 309 -> 308 (21 Sep): `index::search_content` gave `daemon::find_files` a real
// caller. "Find me the thing about the budget" searched only filenames, so a
// note whose name never says "budget" came back as "nothing matches". Now,
// when the filename pass finds nothing, the daemon reads inside the indexed
// documents and code and answers with the line the word was found on -- the
// expensive search, run only once the cheap one is empty.
// 308 -> 307 (21 Sep): `thread::asked_before` gained a production caller. Its
// own doc says repeating an answer verbatim "makes an assistant feel like it
// isn't listening", and nothing asked it -- so a question put twice in the
// same thread was answered a second time as if for the first. `run_command`
// now asks it right after appending the turn (so the just-added turn is
// skipped and an earlier identical `said` is found) and, when the question
// has come round before, leads the reply with "you asked this a little
// earlier" instead of replaying it silently. The record keeps the
// substantive answer, not the nudge.
// 307 -> 306 (21 Sep): `messaging::note_on` gained a production caller. The
// "messages" reading spoke a summary and threw the senders away -- a brand's
// first approach vanished into a count of "3 messages" and Atlas remembered
// no one between reads, even though its own daemon comment claimed the sorting
// "finally runs". `note_the_senders` in `daemon::execute_inner` now groups the
// kept messages by sender, asks `note_on` for a note on each (which is where
// the folder is decided from what they wrote), merges those into a stored
// contact book so it accumulates across reads, and says a *new* work or
// prospect contact out loud rather than burying it. `filed` stays dead: it
// takes a `&Person` and this path formats its own line, so nothing calls it.
// 306 -> 303 (21 Sep): the Intent::Booking handler wired three booking
// capabilities at once -- `to_decide` (lay a proposed time out against your
// calendar), `answered` (read your accept/decline/counter), and `could_offer`
// (times to offer instead). The whole "times with other people" flow now runs
// end to end, and only your explicit accept writes to the calendar.
// 303 -> 302 (21 Sep): `files::convert` got a production caller. The
// `Intent::Files` handler is documented as "convert this / join these" and its
// phrases include "convert this", but every one of them landed in `find_files`
// and came back as a keyword search -- "convert this pdf to text" looked the
// disk over for files named after the words *pdf*, *to* and *text*. A new
// `files_request` (`daemon.rs`) reads a spoken conversion ahead of the search:
// `convert_answer` maps the two named formats to `files::Sort` and asks
// `files::convert` what the change costs and what it cannot do, so a sheet to
// text now says it loses its formulas rather than searching for a file. Only
// what is not a recognised conversion still falls through to `find_files`.
// 302 -> 301 (21 Sep): `why::note_full` got a production caller. The
// "why did you do that?" feature shipped with its read half wired --
// `Intent::Why` reads `self.decisions` -- but the daemon field was a bare
// `Vec` nothing ever pushed to, so every "why is that?" answered from an
// empty list. The field is now a `why::Record`, and `run_command` writes the
// turn's routing to it through `note_full`: whether the model was reached
// (`decision.model`) and the tier's reason why. Asking why is excluded from
// the record so a bare "why" cannot surface its own routing as the answer.
// 301 -> 300 (21 Sep): `wants::asked_for_something_missing` got a production
// caller. `recommend()` turns each unsupported request into a concrete
// suggestion ("you asked me to X and I couldn't"), but the daemon fed it an
// empty list -- the Recommend handler's own comment admitted the signal had no
// source. A turn that lands as an unanswerable `Intent::Unknown` now records
// the request on `Daemon::wants_seen`, and `Intent::Recommend` reads it back,
// so "what am I missing?" names a thing you actually asked for.
// 300 -> 299 (21 Sep): `rehearse::touches_anything_irreversible` got a
// production caller. A rehearsal marks its irreversible steps with a "!", but
// only in `detail`, which is stored for reading and never spoken -- so the
// line you actually hear before saying "go" gave a step count and no hint
// that one of those steps closes an app or types into it and cannot be undone.
// The `Intent::Rehearse` handler now reads `touches_anything_irreversible`
// and appends the warning to the spoken reply when it is true.
// 299 -> 298 (21 Sep): `person::beyond_me` got a production caller. It is the
// narrow test for a message that needs a person rather than an assistant, and
// nothing asked it -- a line like "I can't go on" fell through the addressing
// check straight into ordinary parsing, where Atlas would try to run it as a
// command. `Daemon::turn_from` now checks `beyond_me` right after "was that
// meant for me?" resolves and, when true, returns `person::NOT_A_THERAPIST`
// before any mode, flow, parser or model can touch the words.
// 298 -> 297 (21 Sep): `person::hard_day` got a production caller, the sibling
// of `beyond_me` one gate above it. `beyond_me` catches the message that needs
// a person; `hard_day` catches the ordinary rough day, where redirecting is
// wrong and performing concern is worse. `Daemon::turn_from` now checks
// `person::having_a_hard_time` right after the crisis gate and, when true,
// offers to take the outstanding backlog items -- the concrete work Atlas could
// actually pick up -- off your plate, saying little when there is nothing
// outstanding rather than faking sympathy. Before this a "rough day" fell
// through to the parser and was matched against the wrong thing.
// 297 -> 296 (21 Sep): `undo::on_its_own` got a production caller. It returns
// the things Atlas did without being asked -- the review its own doc names --
// and nothing reached it: "what did you do" gave the whole log, which counts
// the unprompted ones but offers no way to see only those. `Intent::History`
// now recognises "what did you do on your own" / "without asking" as a
// distinct `Asking::WhatOnYourOwn` and answers from `on_its_own`, so the
// question a person asks to check what Atlas took upon itself finally lands.
// 296 -> 295 (21 Sep): `triage::can_wait` got a production caller. It returns
// everything the sort says needs nothing from you -- "so you can not look at
// it", per its own doc -- and nothing reached it: the "clear out my inbox"
// summary named only what needs you and left the rest uncounted. The mail
// handler in `daemon.rs` now reads `can_wait` and ends the summary with how
// many you can leave unread, so a re-sort actually shrinks the inbox you face.
// 295 -> 294 (21 Sep): `capability::working` gained a production caller. The
// spoken `Intent::Capabilities` handler answered "what can you do" with the
// summary counts and "list them all" with the whole catalogue, but had nothing
// for "what can you do right now" / "what's working" -- that question fell
// through to the keyword match and came back "I don't have anything for that."
// The handler now reads `working`, the filter of just the usable capabilities,
// and names them, which is the answer distinct from both the tally and the
// full list.
// 294 -> 293 (22 Sep 2026): `session::transcript` gained a production caller.
// The new `Intent::Recap` handler ("recap our conversation", "what have we
// been talking about") reads this session's recent turns back to you.
// `transcript` assembled those turns and was documented "for handing to the
// model", but nothing ever called it -- the daemon builds model context from
// `thread` instead, so the one thing that turns the session's own turns into a
// readable transcript was proven by tests and reached by nothing. Recap is the
// question that wanted it, distinct from `History` (the log of what Atlas did).
// 293 -> 292 (22 Sep): `knowhow::for_symptom` gained a production caller. The
// new `Intent::Diagnose` handler (`Daemon::diagnose_symptom`, reached by
// "troubleshoot …", "why won't …") scores a symptom you describe against every
// shipped procedure's snags and speaks the likely cause and fix. Its sibling
// `for_request` was already wired as `known_procedure`; the symptom half was
// proven by `fit_knowhow.rs` and reached by nothing until now.
// 292 -> 291 (22 Sep): `route::all_routes` gained a production caller. The new
// `ways_in_help` (`Daemon`, in the `Unknown` chain beside `decision_help`)
// answers "what are all the ways you could get me that?" with the whole
// ordered menu of approaches for that sort of problem -- cheapest and most
// reliable first, no repeats -- where `plan` and `another_way` only ever named
// the single best next one. `all_routes` built exactly that list and was
// proven by `stance_route.rs` and reached by nothing until now.
// 291 -> 290 (22 Sep): `modes::leave` gained a production caller. `enter` was
// wired into `Intent::SetMode` and `leave` was not, so a mode could be turned
// on and never cleanly turned off -- the exact thing its own doc warns makes a
// mode people stop using. The `SetMode` handler now reads a leaving word ("mode
// off", "go into normal mode", "mode normal") and, when a mode is actually on,
// calls `leave` to restore what was open before, drop the mode's rules and
// clear the active mode. Wired into the existing handler, so no new intent.
// 290 -> 288 (22 Sep): `earned::may_act_alone` and `earned::rope` both gained a
// production caller. The new `Intent::ActAlone` handler
// (`Daemon::what_i_can_do_alone`, reached by "what can you do on your own" /
// "where do you still ask me first") reads the autonomy ledger back to you --
// what Atlas will do unattended and where it still asks first. That record was
// written every turn (`note`, `taken_back`) and read by nothing a person could
// reach: `how_am_i_doing` reports corrections, not trust. Both methods were
// proven only by their own tests until now.
// 288 -> 287 (22 Sep): `knowhow::as_plan` gained a production caller. The new
// `Intent::WalkThrough` handler (`Daemon::walk_me_through`, reached by "walk me
// through ...", "how do I ...") matches a shipped procedure with `for_request`
// and reads its steps back as a numbered plan. `known_procedure` reached the
// same match but only ever said `announce` -- "I know this one, N steps" -- so
// the function that turns a procedure into steps to follow was proven only by
// its own test until now.
// 286 -> 285 (22 Sep): `capture::Notebook::correct` gained a production caller.
// 285 -> 282 (22 Sep): meaning search wired end to end — `recall::set_embedding`
// and `recall::unembedded` are driven by `Daemon::embed_backlog` off the tick,
// and `recall::needs_a_model` by `doctor`, now that `meaning.rs` exists to
// produce the vectors they were all waiting on.
// The new `Refile` intent ("file that under the roof job", "that's actually a
// task", reached through `Daemon::refile_note`) is the door to it: capture only
// ever *added* notes, so the kind it guesses for a note and the handles it
// picks were never correctable by anything a person could say. `correct` --
// which changes a note's kind or adds a handle and marks it confirmed, so the
// fix outlives the guess and `find` reaches it by the handle you used -- was
// written the day capture was and reached only by its own test until now.
// 282 -> 277 (22 Sep): five dead-weight orphan methods deleted —
// reference::perishable, backends::spec, perf::current_interval, stale::to_stop,
// bars::has_time (all public, uncalled, untested; see tests/dead_methods.rs and
// tests/new_capabilities_are_wired.rs, where the same five came off KNOWN).
// 277 -> 276 (22 Sep): earned::wrong_lately gained a production caller.
// 276 -> 270 (22 Sep): six market/* trading primitives wired to real consumers
// 270 -> 269 (22 Sep): timeframe::viable_range_pips made a free function that
// regime::tradeable calls (one definition of the range-vs-cost threshold).
// 269 -> 263 (22 Sep): B4 cloudsync provider-compare surface + B3 reminder
// (scheduler::in_secs) + B2 opportunity (weigh_opportunity) wired, and
// mend::atlas_can_fix came off by bare-name collision (see dead_methods.rs).
// (structure higher/lower highs/lows -> Structure::say; timeframe::spans ->
// replay blackout; claims::over -> `atlas marks check over <n>`), off KNOWN too.
// 263 -> 256 (23 Sep): decision-list remainder -- bars::back_to
// (structure::before_the_turn), cdp::links (research's headless-browser
// fallback), events::is_window (standdown's banded-release sentence),
// otherside::against/is_asked_for/needs_evidence ("argue the other side"),
// settings::idle_but_on (hub settings banner).
// 256 -> 255 (23 Sep): crew::ask_to_stop -- "cancel the build" calls one
// errand off by name (single-errand control, which_errand).
// 255 -> 253 (23 Sep, round-3 ports): files::safe_to_unpack -- the zip bomb
// guard now stands in front of `zipread`, which searches inside archives;
// panel::transient -- the tick fades the Waking panel (`panel::faded`).
// 253 -> 252 (23 Sep, round 4): fit::worth_replanning -- the tick measures
// the machine again every hour and replans when it has changed enough
// (`fit.replan_on_change`).
// 252 -> 246 (24 Sep, round 5): input::down and input::up -- the
// push-to-talk key (`hotkey::Gate`) feeds the hold-to-talk machine;
// consult::report_result, handoff::write_brief, strategy::what_was_learned --
// the hand-off loop (`fixloop`, `atlas fix`) drives all three;
// vault::usable_unattended -- the vault opened on your sign-in refuses the
// kinds it rules out.
// 246 -> 249 (24 Sep, round 6 merge): three functions from the main chat's
// clock and transport (3007af9, 7ec88ee) -- hlc::resuming_from,
// sync::clock_at, transport::bind_local_ephemeral -- are reached only by
// tests so far (named in new_capabilities_are_wired.rs KNOWN). Measured at the merge rather than raised to hide it; the chat that
// wrote them wires or removes them.
// 249 -> 247 (25 Sep, merging the main chat's b70be92): it deleted
// hlc::resuming_from and sync::clock_at (serde already persists the clock);
// transport::bind_local_ephemeral stays test-only, as named.
// 247 -> 256 (25 Sep, merging the main chat's 6b7e938): its step 5 raised
// this by nine on master (256 -> 265 there, reasons below in its own words):
// the courier's build and apply sides, reached only by tests until step 6.
// 255 -> 256 (24 Sep): transport::bind_local_ephemeral, the loopback ephemeral
// bind the direct-sync transport uses in tests (production binds the fixed
// SYNC_PORT so peers know where to dial). Genuinely test-only, not a
// regression -- the one net-new proven-but-uncalled method the phone-as-peer
// transport added. Also in dead_methods::TEST_ONLY_METHODS and
// new_capabilities_are_wired::KNOWN.
// 256 -> 259 -> 256 (25 Sep, corrected the same day). The release-signature
// root (`release`) briefly raised this by three. That was wrong: `release` is
// in wiring::UNWIRED_BASELINE, and this scan leaves baseline modules out, so
// once it was baselined the three stopped counting and the ceiling was stale.
// Committed in 0436ddf with this guard red, because the final check that day
// ran the `all` suite but not this standalone target after the last edit.
// Every standalone target is now re-run after the final edit of a step.
// 256 -> 265 (25 Sep 2026, update-courier step 5). `release` left
// UNWIRED_BASELINE because `update_courier::heard` now checks every release
// notice through it, so its functions are measured here for the first time.
// The nine that only tests reach are the build side and the apply side of the
// courier -- keygen/sign/announce, key rotation, the downloaded-file check,
// rollback by your own hand -- whose callers are step 6 and don't exist yet.
// Each is named in KNOWN (new_capabilities_are_wired.rs) with that reason, and
// this comes back down as step 6 wires them.
// 265 -> 260 (same day, step 6 begun): `atlas release keygen/sign/announce`
// wired signing_key_from_seed, anchor_of, anchor_configured, seal_manifest
// and update_courier::announcement.
// (On this branch, 256 -> 251 for the same five, merging 773454a.)

// 255 -> 254 (24 Sep): look_paint::live_height -- the mark follows Atlas's
// real voice level while it speaks (`speaking`, the window and the overlay).
// 254 -> 250 (24 Sep): delegate's advance, composed and user_returned (Atlas
// now works the window in front for you).
// ...then 251 -> 253 the same day, and the rise is the module arriving, not
// work going backwards: `consent` was on the unwired baseline, so none of its
// functions were counted at all. Wiring call notes brought it in; its
// recorder steps are all called now, and the two left are wording helpers
// (`announcement_named`, `script`) for choosing and previewing what's said
// to a call, which no page offers yet.
// 253 -> 254 (24 Sep, the audit): `callrec::silent`, a stand-in recorder of
// silence. The sound devices exist only on Windows, so the consent steps --
// "a no after a yes stops their side and deletes it" -- are driven on Linux
// through it (`Notes::starter`). Recording silence in place of a real call is
// never right for the running program, so it stays test-facing on purpose.
// 254 -> 184 (25 Sep, the backlog pass): all 253 read and sorted (doc 30).
// 62 removed -- 33 that nothing needed and 29 that production already did
// another way, each with its tests pointed at the production path or
// deleted. 11 wired to real callers: the day rhythm counts days, a failed
// fetch re-checks the connection, build sandboxes are cleaned up, approvals
// name what kind of thing is being approved, the index isn't rescanned below
// the battery floor, "better then" is caught, the panel question gets an
// answer, doctor flags a personal path in the shareable settings, window jobs
// that ask are kept and resolved, a failed walk-through teaches the
// procedure, and `atlas mail setup` uses the (corrected) Outlook advice.
// Three became test-only when their only caller went (see KNOWN in
// new_capabilities_are_wired.rs). Of the 184 left, about 145 need a ruling --
// they speak, act outside the machine, touch security or trading, or are
// halves of features never built -- and 19 are test support.
//
// 184 -> 169 on 25 Sep 2026, working Eric's rulings (security group and H4):
// wired -- the two-factor refusal words gave way to real on/off with the
// read-back, the confirmed read-back itself, sign-in for real (the vault login
// split, autofill), sign-up for real (password into the vault, the result
// said), the monthly unused-login line, the vault-recovery weaknesses, the hub's
// wrong-token slow-down, the trusted-peer door on the hub, the agreed call
// wording; removed -- the self-grant expiry (B3: grants don't lapse) and the
// end-date countdown and announcement (F5: no end-date reminder).

// 26 Sep 2026: the two lines above met in one tree when the third chat's line
// (23e -> 25h) was merged into ours: 251 here, 169 there. Measured on the
// merged tree: 167. It is lower than both because the third chat's 25g pass
// removed 62 unused functions from modules both lines share (the ones it
// removed that our side had since wired -- `vault::Kind::usable_unattended`,
// `panel::Panel::transient` -- were put back and are reached, not counted).
// Later on 26 Sep 2026, 167 -> 107 when the Atlas Project chat's 25i and 25j
// came in on top: 25j wired the calls behind its new commands (goals, later,
// scheduled posts, sorting mail, taught gestures, the Windows key hook, the
// fix loop's config), which reached sixty functions that were only tested.
// 107 -> 101 (26 Sep 2026): the trading-system modules left personal Atlas,
// and six functions that only their tests reached went with them.
// 100 -> 99 (27 Sep 2026): `language::Heard::good_enough` was deleted -- no
// production caller, no real test (ORPHAN_METHODS in dead_methods.rs). This
// file counted it as tested only because the lists naming it wrote
// `language::good_enough`, which `called_names` reads as a reference.
// 99 -> 103 (28 Sep 2026, the second scan): four reached only by tests, each
// on purpose -- `server::with_answer_wait` and `hubjobs::age_finished` are
// seams so the tests reach "answered busy" and "ten minutes later" without
// waiting; `localclock::pin_offset` pins the clock for the tests about a time
// of day; `server::serve_once` lost its one caller when settings-only mode
// moved to the threaded door. Not counted here: `hub::sync_page`, test-only
// already on merge-0928 (the count there was 100 against this 99).
// 103 -> 102 (28 Sep 2026, the microphone's own thread): `roots::models_dir`
// is where the microphone's thread looks for the Silero voice model when
// cutting in by voice (`micthread::MicWork::models_dir`); only tests reached
// it before.
// 102 -> 101 (28 Sep 2026, personal Atlas separated from the trading work):
// `ladder.rs` left personal Atlas (tests/personal_atlas_is_its_own.rs), and
// with it one function only its own tests reached. Nothing was wired; the
// count fell because the module went.
// 101 -> 100 (29 Sep 2026): overlay::window_style is applied now -- the
// overlay is made see-through by Windows' colour key (`overlaywin::see_through`).
// 100 -> 101 (30 Sep 2026, the prompt diet): `intent::ToolBook::for_sentence`
// -- every core command plus the ones a sentence reads like -- is no longer
// what a conversation turn is offered (`router::Router::for_turn` is); the
// tests that measure the old shape still call it.
// 101 -> 100 (30 Sep 2026, merging the other chat's 30 Sep work):
// `capture::found` is called -- notes can be asked for.
const TEST_ONLY_MAX: usize = 100;
// 287 -> 286 (22 Sep): `consolidate::size_note` gained a real caller. The new
// `KnowledgeSize` intent ("how much do you know", "how big is your memory")
// routes through `Daemon::knowledge_store_size`, which reads the store count
// into that function -- the reassurance the module was built to give, reached
// by nothing a person could say until now.
// 33 -> 31 (14 Sep): `run_graded` and `by_grade` gave `regime::r_squared`,
// `regime::r2_critical` and `multiframe`'s alignment real callers. They were
// helpers with no test and no caller; grading a call by how straight the move
// was is the first thing that has ever asked them anything.
// 31 -> 26 (14 Sep, later): the feature table reached five more --
// `levels::nearest`, `events::month`, `Event::touches`, `Event::blackout` and
// `timeframe::infer`'s bar length. Adding levels and news to what a learner
// can see is what asked them.
// 26 -> 28 -> 27 (16 Sep): the `look`/`backends` wiring pass added two, and
// this assertion never ran because the `test_only` assert above it was already
// failing on the 139/140 drift and fires first. `backends::default_specs` was
// `pub` for nothing -- only `Router::new` builds from it -- and is now private,
// which is the clearing this group is for. `look_paint::live_height` stays and
// the ceiling takes the +1: it is the real-audio branch of the mark, and
// nothing in this tree produces an audio level for it to read. Making it
// private would trade the honest count for a dead-code warning, and wiring it
// would mean inventing a level. Named, at the cost of one on the number.
// 27 -> 28 -> 27 (17 Sep): the merge's new `crash.rs` shipped `pub fn doing`,
// which only `crash::caught` in the same file ever calls. Made private rather
// than absorbed into the ceiling -- the same clearing as `default_specs`
// above, and the reason this constant is an equality and not a `<=`.
// 27 -> 128 (17 Sep), from the same three corrections as TEST_ONLY_MAX
// above. Worth one note of its own: the improvements tree, corrected
// separately and from a different starting point, lands on **128** for this
// same bucket. Two measurements that were built apart agreeing to the
// function is the closest thing to a second opinion this tree has.
// 128 -> 3 (17 Sep). This bucket's own description is "`pub` for no reason
// anything can see -- most should be private", and that turned out to be a
// mechanical instruction rather than an observation.
//
// 129 functions had no caller in any other module, no test, and nothing in
// the sibling crate. Each had `pub` removed. **The compiler is the proof**:
// visibility cannot change behaviour, the whole tree plus every test still
// builds, and anything that had actually been used would have failed to
// compile. What was a 128-item backlog is now an enforced property.
//
// It also found the one genuinely dead function among them. With `pub` gone,
// `afterme::Where::reachable_without_you` drew `never used` -- and `gaps()`
// was hand-writing `w == Where::BankBox` where it meant exactly that
// predicate. One fact in two places, the named one uncalled; the same shape
// as `mail::smtp_port` and `time::say_date`. It is wired now, so adding
// another arrangement that cannot be reached in time keeps reporting the gap
// instead of silently not.
//
// 8, not 3, and the difference is worth keeping. Five of the 129 are pinned
// by name in `guards.rs::GUARDS` -- `watch_hands`, `hollow_answers`,
// `reload_library`, `claims::cannot_say`, `feed::no_future_bars` -- each with
// a recorded reason for existing that has nothing to do with visibility.
// That guard caught the change immediately, which is it working. Their `pub`
// was put back rather than the guard's expectations edited: a safety net this
// pass does not own is not the thing to adjust to make this pass's number
// smaller.
//
// So 8 remain, and all 8 are deliberate: 5 pinned guards and 3 nobody has
// looked at yet.
// 8 -> 5 (18 Sep), on the merge of the two trees, and it went UP to 36 first.
//
// The other tree's de-`pub` pass covered the functions in that tree. This one
// carried 31 more module-internal helpers that were `pub` for nothing --
// partly from the three sessions the other tree never saw, partly from files
// where this tree's version was the one kept. Each had `pub` removed and the
// whole tree plus every test still builds, which is the same proof that pass
// used: visibility cannot change behaviour, so anything actually reached
// would have failed to compile.
//
// What is left is exactly the five pinned by name in `guards.rs::GUARDS` --
// `watch_hands`, `hollow_answers`, `reload_library`, `claims::cannot_say`,
// `feed::no_future_bars`. Their `pub` stays for the reason that pass gave:
// a safety net this work does not own is not the thing to adjust to make
// this number smaller.
//
// So 5, and all 5 are somebody else's deliberate decision.
//
// 5 -> 4 (22 Sep): `reload_library` gained a direct test —
// tests/meaning_search.rs drives it by hand to prove an edited note loses its
// stale meaning vector — so it is no longer pub-without-a-test.
const HELPER_UNTESTED_MAX: usize = 4;




fn sources() -> BTreeMap<String, String> {
    fn walk(dir: &std::path::Path, out: &mut BTreeMap<String, String>) {
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
                out.insert(name, text);
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(std::path::Path::new("src"), &mut out);
    // `src/daemon/*.rs` counts as `daemon` once daemon.rs is split (27 Sep
    // 2026); nothing is folded today.
    common::fold_split_modules(out.into_iter().collect()).into_iter().collect()
}

/// The two files that talk *about* capability names rather than calling
/// them are skipped.
///
/// This one and `bug_sweep.rs` both explain the detector in prose, and that
/// prose contains things like `contains("seal_bytes(")`. Counted as callers,
/// a comment written to explain why a function is dead is enough to make it
/// look alive — which happened, immediately, to the two functions this file
/// documents most carefully.
const META: &[&str] = &["dead_capabilities.rs", "bug_sweep.rs"];

fn test_sources() -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir("tests") {
        for e in entries.flatten() {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("rs") {
                continue;
            }
            let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
            if META.contains(&name.as_str()) {
                continue;
            }
            if let Ok(t) = std::fs::read_to_string(&path) {
                out.push(t);
            }
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

struct Groups {
    orphans: BTreeSet<String>,
    test_only: usize,
    helper_tested: usize,
    helper_untested: usize,
}

/// The same rule `bug_sweep.rs` uses, split four ways.
fn group() -> Groups {
    let src = sources();
    let tests = test_sources();
    let unwired = unwired_modules();

    // The program, and its own inline tests, told apart.
    //
    // This read whole source files, so a `#[cfg(test)] mod tests` block
    // counted as part of the program: a function called only by its own test
    // looked like a private helper, and one called only by *another* module's
    // test looked like it had a production caller and left the measurement
    // entirely. `dead_methods.rs` in this same suite already strips these --
    // the two files disagreed about what "called" means.
    let prod: Vec<(String, String)> = src
        .iter()
        .map(|(m, t)| (m.clone(), common::split_production_and_tests(t).0))
        .collect();
    let inline_tests: Vec<String> = src
        .iter()
        .map(|(_, t)| common::split_production_and_tests(t).1)
        .filter(|t| !t.trim().is_empty())
        .collect();
    assert!(
        inline_tests.len() > 30,
        "only {} inline test blocks found -- the splitter has stopped working",
        inline_tests.len()
    );
    let mut g = Groups {
        orphans: BTreeSet::new(),
        test_only: 0,
        helper_tested: 0,
        helper_untested: 0,
    };

    // Same indexing as new_capabilities_are_wired, for the same reason: this
    // guard took 60.5s of every verification run. See that file, and the
    // equivalence test in guards.rs that makes the swap safe.
    let src_index: Vec<(String, std::collections::HashSet<String>)> =
        prod.iter().map(|(m, b)| (m.clone(), called_names(b))).collect();
    let test_index: std::collections::HashSet<String> = tests
        .iter()
        .chain(inline_tests.iter())
        .flat_map(|t| called_names(t))
        .collect();

    for (module, body) in &prod {
        let stem = module.rsplit('/').next().unwrap_or(module);
        if unwired.contains(stem) || stem == "main" {
            continue;
        }
        for line in body.lines() {
            // **Trimmed.** This was `line.strip_prefix` on the raw line, so
            // only functions declared at column zero were ever seen -- 1,121
            // of 3,216, with every method inside an `impl` block invisible
            // and 31 whole modules (`log`, `intent`, `memory`, `outbox`,
            // `crew`, `scheduler`, `timing` among them) outside this
            // measurement without anyone deciding that. `dead_methods.rs`
            // trims and sees both, so the two files disagreed about what a
            // public function even is.
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
            // How the four groups are told apart: whether its own module
            // calls it, and whether any test does. `calls` is whole-word, so
            // `unseal_bytes(` no longer counts as a call to `seal_bytes`.
            let defined_on = format!("pub fn {name}(");
            let defined_on_crate = format!("pub(crate) fn {name}(");
            let own = body
                .lines()
                .filter(|l| {
                    let t = l.trim_start();
                    !t.starts_with(&defined_on) && !t.starts_with(&defined_on_crate)
                })
                .any(|l| calls(l, &name));
            let tested = test_index.contains(&name);
            match (own, tested) {
                (true, true) => g.helper_tested += 1,
                (true, false) => g.helper_untested += 1,
                (false, true) => g.test_only += 1,
                (false, false) => {
                    g.orphans.insert(format!("{stem}::{name}"));
                }
            }
        }
    }
    g
}

#[test]
fn every_function_nothing_calls_is_named_and_accounted_for() {
    let found = group().orphans;
    let listed: BTreeSet<String> = ORPHANS.iter().map(|(n, _)| n.to_string()).collect();

    let unexplained: Vec<&String> = found.difference(&listed).collect();
    assert!(
        unexplained.is_empty(),
        "these functions have no caller anywhere — not in the program, not in \
         their own module, not in a test:\n  {}\n\nEach one is code that was \
         written and is reached by nothing. Wire it, delete it, or add it to \
         ORPHANS with what it is waiting on.",
        unexplained.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n  ")
    );

    let fixed: Vec<&String> = listed.difference(&found).collect();
    assert!(
        fixed.is_empty(),
        "these are listed as having no caller and now have one:\n  {}\n\nGood \
         news — remove them from ORPHANS so the list keeps meaning something.",
        fixed.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n  ")
    );
}

#[test]
fn every_orphan_says_what_it_is_waiting_on() {
    // A list of names with no reasons is the ceiling again, just longer.
    for (name, why) in ORPHANS {
        assert!(
            why.len() > 60,
            "{name} is listed with no real explanation of what it needs: {why:?}"
        );
    }
}

#[test]
fn the_two_groups_that_measure_deadness_have_not_piled_up() {

    let g = group();
    assert!(
        g.test_only <= TEST_ONLY_MAX,
        "{} capabilities are reached only by tests, against a ceiling of \
         {TEST_ONLY_MAX}. These are built, proven and never called — the \
         backlog worth working through, not a number to raise.",
        g.test_only
    );
    assert!(
        g.helper_untested <= HELPER_UNTESTED_MAX,
        "{} module-internal helpers have no direct test, against \
         {HELPER_UNTESTED_MAX}. These are `pub` for no reason anything can \
         see — most should be private.",
        g.helper_untested
    );
}

#[test]
fn the_counts_are_exact_so_progress_cannot_hide_in_the_headroom() {
    // A ceiling with room under it stops being a ratchet. These two are set
    // to what the tree actually measures, so doing real work *requires*
    // lowering them and the lowering is where the reason gets written down.
    let g = group();
    assert_eq!(
        g.test_only, TEST_ONLY_MAX,
        "test-only is {} and the ceiling says {TEST_ONLY_MAX}. If work was \
         done, lower it and say what got wired. If it rose, something was \
         built and not reached.",
        g.test_only
    );
    assert_eq!(
        g.helper_untested, HELPER_UNTESTED_MAX,
        "helper-untested is {} against {HELPER_UNTESTED_MAX}",
        g.helper_untested
    );
}

#[test]
fn the_bucket_that_was_not_measuring_deadness_is_gone() {
    // `helper_tested` -- called by its own module, covered by a test -- is
    // live, tested code. It was counted as a dead capability, which meant
    // splitting one long function into ten well-named private ones read as
    // ten new dead functions, and inlining them all back read as progress.
    //
    // `DEAD_CAPABILITY_CEILING` in bug_sweep.rs summed all four groups, so it
    // inherited that fault and added its own: a single number that can move
    // for four unrelated reasons says nothing about which one moved. Both are
    // gone. This test is what stops either coming back by habit.
    let sweep = std::fs::read_to_string("tests/bug_sweep.rs").expect("bug_sweep.rs");
    assert!(
        !sweep.contains("const DEAD_CAPABILITY_CEILING"),
        "the summed ceiling is back. It cannot tell a capability nothing \
         reaches from a small private helper, so it punishes exactly the \
         refactoring worth doing."
    );
    // The needle is built at runtime rather than written as one literal,
    // because this test would otherwise match itself: any spelling of the
    // constant that is precise enough to find a real declaration is also
    // sitting right here in the assertion. Two attempts failed on exactly
    // that before this one -- the same self-reference trap that let a
    // `commands.yaml` guard pass on the comment quoting the phrase it was
    // guarding, turned inside out.
    let me = std::fs::read_to_string("tests/dead_capabilities.rs").expect("this file");
    let declared = format!("const {}_MAX", "HELPER_TESTED");
    assert!(
        !me.contains(&declared),
        "helper_tested has a ceiling again, and it counts live tested code"
    );
}
