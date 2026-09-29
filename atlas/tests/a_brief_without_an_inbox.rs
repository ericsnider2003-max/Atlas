//! The morning run, built from this machine rather than from an inbox.
//!
//! `brief.rs` was the most complete dead module in the tree. It had triage,
//! weighting, a bounded output, a "nothing sends" rule enforced by the type
//! system, a `most_blocking` detector, drafting, a time budget, a resumable
//! chain, and a spoken form that leads with what to do rather than with a
//! count. Every piece of it worked.
//!
//! It had exactly one call site:
//!
//! ```ignore
//! let b = crate::brief::run(&[], &[], &crate::brief::BriefConfig::default());
//! ```
//!
//! Empty items, empty commitments — and a config that could not be turned on,
//! because `BriefConfig` was not a field on `ToolsConfig` at all. So `run`
//! returned at its first line, every time, and the brief was a function that
//! always gave the same empty answer. Nothing failed. Nothing was ever wrong.
//! It simply never said anything.
//!
//! The root cause was the input type: an `Item` was an email, and this build
//! has no mail reader and is meant to work offline first. So the redesign is
//! not "find an inbox" — it is that mail is **one source among several**, and
//! the others are already sitting on this machine.
//!
//! These tests are about that: real sources, a real `Daemon`, and a brief that
//! says something true without a network.

use atlas::backlog::Blocker;
use atlas::booking::{Proposal, Slot, State};
use atlas::brief::{
    from_backlog, from_bookings, from_handoffs, from_jobs, from_posts, from_upkeep, gather,
    handled_since, run, spoken, BriefConfig, Item, Outcome, Source, Sources, Weight,
};
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::household::Inbox;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::publish::{Channel, Publisher};
use atlas::scheduler::Scheduler;
use atlas::store::Store;
use std::path::{Path, PathBuf};

const NOW: u64 = 1_700_000_000;
const THREE_DAYS: u64 = 3 * 24 * 3600;

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-brief-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn on() -> BriefConfig {
    BriefConfig::default()
}

fn empty_sources<'a>(
    inbox: &'a Inbox,
    backlog: &'a atlas::backlog::Backlog,
    scheduler: &'a Scheduler,
    proposals: &'a [Proposal],
    publisher: &'a Publisher,
    upkeep: &'a [String],
) -> Sources<'a> {
    Sources { inbox, backlog, scheduler, proposals, publisher, upkeep, mail: &[], online: true }
}

// ===================== the config could not be turned on ================

#[test]
fn the_brief_can_be_turned_on_at_all_now() {
    // The root cause under the root cause. `BriefConfig` was not a field on
    // `ToolsConfig`, so the only value it could ever have was its own
    // `Default` — and that was `enabled: false`. There was no switch.
    let c = Config::load(Path::new("config")).unwrap();
    let tools = c.tools.as_ref().expect("the shipped config has a tools section");
    assert!(tools.brief.enabled, "the brief ships off again, so it computes nothing");
}

#[test]
fn it_ships_on_because_it_now_has_something_to_read() {
    // Ships on *and* the code defaults on — `tests/shipped_config.rs` holds
    // the two together, and this says why the value is what it is.
    assert!(BriefConfig::default().enabled);
    assert!(
        !BriefConfig::default().draft_replies,
        "drafting replies is on with no mail reader to reply to, which promises \
         something nothing can keep"
    );
}

// ===================== each source converts itself ======================

#[test]
fn a_friends_note_at_the_door_is_something_only_you_can_settle() {
    let mut inbox = Inbox::default();
    inbox.add("the quarterly numbers, when you get a minute", "Priya", NOW);

    let items = from_handoffs(&inbox, NOW);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].source, Source::Handoff);
    assert_eq!(items[0].from, "Priya");
    assert_eq!(items[0].outcome, Outcome::Yours, "Atlas cannot decide to keep or bin this");
}

#[test]
fn a_note_that_has_sat_for_two_days_stops_being_information() {
    let mut inbox = Inbox::default();
    inbox.add("have a look at this", "Priya", NOW);
    let fresh = from_handoffs(&inbox, NOW + 60);
    let old = from_handoffs(&inbox, NOW + THREE_DAYS);

    assert_eq!(fresh[0].weight, Weight::Info);
    assert_eq!(old[0].weight, Weight::Urgent, "a note you have ignored for days is not news");
}

#[test]
fn something_waiting_on_you_outranks_something_waiting_on_the_world() {
    let mut b = atlas::backlog::Backlog::default();
    b.record("renew the certificate", Blocker::NeedsApproval, NOW);
    b.record("look up the train times", Blocker::Offline, NOW);

    let items = from_backlog(&b);
    let approval = items.iter().find(|i| i.subject.contains("certificate")).unwrap();
    let offline = items.iter().find(|i| i.subject.contains("train")).unwrap();

    assert_eq!(approval.weight, Weight::Urgent, "a thing waiting on you is not information");
    assert_eq!(offline.weight, Weight::Info, "a thing that clears itself is");
}

#[test]
fn most_blocking_finally_has_something_to_work_on() {
    // `most_blocking` was written to say "X is holding up 3 other things" and
    // nothing ever gave it a `conflicts_with` to count. Offline the answer is
    // the blocker, which is exactly the sentence worth hearing.
    let mut b = atlas::backlog::Backlog::default();
    for what in ["check the train times", "look up the invoice", "fetch the forecast"] {
        b.record(what, Blocker::Offline, NOW);
    }
    let items = from_backlog(&b);
    let brief = run(&items, &[], &on());

    let blocking = brief.blocking.expect("nothing was named as the hold-up");
    assert!(blocking.contains("no connection"), "got: {blocking}");
    assert!(blocking.contains('3'), "it did not say how many: {blocking}");
}

#[test]
fn a_job_that_failed_needs_you_and_a_job_that_is_due_is_your_day() {
    let mut s = Scheduler::default();
    let failed = s.at("back up the vault", NOW - 100);
    let soon = s.at("post the update", NOW + 1800);
    s.at("the fortnightly thing", NOW + 20 * 86_400);
    s.complete(failed, NOW, "disk full", false);

    let (items, day) = from_jobs(&s, NOW);

    assert_eq!(items.len(), 1, "the wrong number of jobs needed you: {items:?}");
    assert_eq!(items[0].weight, Weight::Urgent, "Atlas tried and it broke; that is not info");
    assert_eq!(day.len(), 1, "a job due in a fortnight is not the shape of this morning");
    assert!(day[0].id.contains(&soon.to_string()));
    assert_eq!(day[0].at_minute, 30, "minutes from now, so the brief does not go stale");
}

#[test]
fn work_that_ran_and_worked_is_counted_not_listed() {
    // "Here is what I handled" is half the shape of the whole module. Without
    // it the brief is only ever a list of demands.
    let mut s = Scheduler::default();
    for i in 0..4 {
        let id = s.at("tidy up", NOW + i);
        s.complete(id, NOW + i, "done", true);
    }
    assert_eq!(handled_since(&s, NOW), 4);
    assert_eq!(handled_since(&s, NOW + 10), 0, "it counted work from before the brief looked");
}

#[test]
fn a_proposed_time_nobody_answered_is_urgent_because_silence_is_a_no() {
    let p = Proposal {
        id: 1,
        from: "Sam".into(),
        about: None,
        their_words: "could we do Tuesday or Wednesday morning".into(),
        times: vec![Slot { start: NOW + 3600, mins: 60 }],
        at: NOW - THREE_DAYS,
        state: State::NeedsYou,
    };
    let items = from_bookings(&[p], NOW);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].source, Source::Booking);
    assert_eq!(items[0].weight, Weight::Urgent, "it becomes a no by default, which is not info");
}

#[test]
fn a_post_waiting_on_your_yes_is_drafted_not_yours() {
    // `Outcome::Drafted` is exactly this, and `publish`'s rule and this
    // module's first rule are the same rule: nothing sends without approval.
    let mut p = Publisher::default();
    let id = p.draft(Channel::X, "the release is out");
    p.request_approval(id);

    let items = from_posts(&p);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].outcome, Outcome::Drafted);
    assert_eq!(items[0].draft.as_deref(), Some("the release is out"));
}

#[test]
fn a_post_still_being_written_is_not_in_the_brief() {
    let mut p = Publisher::default();
    p.draft(Channel::X, "half a thought");
    assert!(from_posts(&p).is_empty(), "an unfinished draft was put in front of you");
}

#[test]
fn upkeep_atlas_will_not_settle_alone_gets_a_line() {
    let items = from_upkeep(&["the notes index has drifted".to_string()]);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].source, Source::Upkeep);
    assert_eq!(items[0].weight, Weight::Info, "housekeeping never outranks a person");
}

// ===================== how it reads ====================================

#[test]
fn an_item_reads_as_a_thing_rather_than_as_a_mechanism() {
    // "me — fix the printer" and "a scheduled job — back up" are both worse
    // than the subject alone. Mail keeps the old shape exactly.
    let mail = Item {
        id: "1".into(),
        source: Source::Mail,
        from: "Priya".into(),
        subject: "the numbers".into(),
        weight: Weight::Urgent,
        outcome: Outcome::Yours,
        draft: None,
        conflicts_with: None,
    };
    assert_eq!(mail.headline(), "Priya — the numbers");

    let blocked = Item { source: Source::Blocked, from: "me".into(), ..mail.clone() };
    assert_eq!(blocked.headline(), "the numbers", "it led with 'me —'");
}

#[test]
fn the_brief_leads_with_what_to_do_not_with_a_count() {
    let mut b = atlas::backlog::Backlog::default();
    b.record("renew the certificate", Blocker::NeedsApproval, NOW);
    let said = spoken(&run(&from_backlog(&b), &[], &on()));
    assert!(said.starts_with("Start with"), "got: {said}");
    assert!(said.contains("certificate"));
}

// ===================== offline is the ordinary case ====================

#[test]
fn with_the_router_down_it_does_not_lead_with_an_email() {
    // Not dropped forever — they come back the moment the check succeeds.
    // Leaving them in means a brief whose first instruction is something you
    // cannot act on.
    let inbox = Inbox::default();
    let backlog = atlas::backlog::Backlog::default();
    let scheduler = Scheduler::default();
    let publisher = Publisher::default();
    let mail = vec![Item {
        id: "m1".into(),
        source: Source::Mail,
        from: "Priya".into(),
        subject: "the numbers".into(),
        weight: Weight::Urgent,
        outcome: Outcome::Yours,
        draft: None,
        conflicts_with: None,
    }];

    let offline = Sources {
        inbox: &inbox,
        backlog: &backlog,
        scheduler: &scheduler,
        proposals: &[],
        publisher: &publisher,
        upkeep: &[],
        mail: &mail,
        online: false,
    };
    assert!(gather(&offline, NOW).0.is_empty(), "an unreachable email led the brief");

    let online = Sources { online: true, ..offline };
    assert_eq!(gather(&online, NOW).0.len(), 1, "mail vanished when the network was up");
}

#[test]
fn everything_but_mail_survives_the_router_going_down() {
    // The point of the whole redesign: a machine with no connection still has
    // a brief worth reading.
    let mut inbox = Inbox::default();
    inbox.add("the numbers", "Priya", NOW);
    let mut backlog = atlas::backlog::Backlog::default();
    backlog.record("renew the certificate", Blocker::NeedsApproval, NOW);
    let scheduler = Scheduler::default();
    let publisher = Publisher::default();

    let s = Sources {
        inbox: &inbox,
        backlog: &backlog,
        scheduler: &scheduler,
        proposals: &[],
        publisher: &publisher,
        upkeep: &[],
        mail: &[],
        online: false,
    };
    let b = run(&gather(&s, NOW).0, &[], &on());
    assert!(!b.is_empty(), "offline, the brief had nothing to say");
    assert_eq!(b.yours.len(), 2);
}

// ===================== reached from the running program ================

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

#[test]
fn a_real_daemon_builds_a_brief_out_of_its_own_state() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "real");
    d.backlog.record("renew the certificate", Blocker::NeedsApproval, NOW);

    let b = d.brief_now(NOW);
    assert!(!b.is_empty(), "the daemon built an empty brief from a populated backlog");
    assert!(b.yours.iter().any(|i| i.subject.contains("certificate")));
}

#[test]
fn asking_what_is_outstanding_now_answers_from_something() {
    // This is the call site that was `run(&[], &[], &default())`.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "outstanding");
    d.backlog.record("renew the certificate", Blocker::NeedsApproval, NOW);

    let said = d.turn("what's outstanding", atlas::store::now());
    // Deliberately *not* asserting on the word "certificate": this reply is
    // the brief followed by `backlog.summary()`, and the summary names the
    // item too — so a brief that had gone back to computing over nothing
    // still passed. Found by reverting this call site and watching the test
    // stay green. "Start with" is produced by `brief::spoken` and by nothing
    // else in the reply.
    assert!(
        said.contains("Start with"),
        "the brief contributed nothing; this is the backlog summary alone: {said}"
    );
    assert!(said.contains("certificate"), "the brief named no real item: {said}");
}

#[test]
fn handled_counts_only_what_happened_since_the_last_brief() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "since");
    let id = d.scheduler.at("tidy up", NOW);
    d.scheduler.complete(id, NOW, "done", true);

    let first = d.brief_now(NOW + 10);
    assert_eq!(first.handled, 1);
    let second = d.brief_now(NOW + 20);
    assert_eq!(second.handled, 0, "the same work was reported handled twice");
}

#[test]
fn a_daemon_with_nothing_on_says_so_rather_than_inventing_work() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "quiet");
    let b = d.brief_now(NOW);
    assert!(b.is_empty());
    assert_eq!(spoken(&b), "Nothing needs you. I'll get on with the rest.");
}

#[test]
fn the_doorstep_is_read_fresh_rather_than_held() {
    // The server thread writes the doorstep when a friend sends something, so
    // a copy kept on the daemon would be the stale one.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "doorstep");
    assert!(d.brief_now(NOW).is_empty());

    let mut inbox = Inbox::load(&d.store);
    inbox.add("the quarterly numbers", "Priya", NOW);
    inbox.save(&d.store).unwrap();

    let b = d.brief_now(NOW + 1);
    assert!(
        b.yours.iter().any(|i| i.from == "Priya"),
        "a handoff that arrived after startup was invisible: {:?}",
        b.yours
    );
}

#[test]
fn a_drifted_index_reaches_the_brief_as_upkeep() {
    // `guesses_worth_checking` and the index drift both had nowhere to go but
    // the log, which nothing reads.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "upkeep");
    let items = from_upkeep(&["the notes index has drifted — 2 notes".to_string()]);
    let b = run(&items, &[], &on());
    assert!(b.yours.iter().any(|i| i.source == Source::Upkeep));
    // And that the daemon's own collector is the thing that would supply it.
    assert!(d.brief_now(NOW).is_empty(), "a clean daemon invented upkeep to report");
}

// ===================== the morning greeting ============================

#[test]
fn the_morning_greeting_carries_the_run_rather_than_a_count() {
    use atlas::nudge::{daypart_with_brief, Part, Trigger};
    let mut b = atlas::backlog::Backlog::default();
    b.record("renew the certificate", Blocker::NeedsApproval, NOW);
    let brief = run(&from_backlog(&b), &[], &on());

    let n = daypart_with_brief(Part::Morning, &brief);
    assert_eq!(n.trigger, Trigger::Daypart);
    assert!(n.message.starts_with("Good morning"));
    assert!(n.message.contains("certificate"), "the greeting carried no brief: {}", n.message);
}

#[test]
fn the_greeting_is_handed_a_brief_rather_than_running_a_second_one() {
    // It used to take raw items and run the brief itself. Given a caller that
    // had already run one, that meant running it twice a tick — and the second
    // run was handed the first one's output, so `drafted` was always empty by
    // the time the relief was read off it.
    let mut p = Publisher::default();
    let id = p.draft(Channel::X, "the release is out");
    p.request_approval(id);
    let brief = run(&from_posts(&p), &[], &on());

    let n = atlas::nudge::daypart_with_brief(atlas::nudge::Part::Morning, &brief);
    assert!(
        n.relief.is_some(),
        "the morning greeting could not offer the one thing it is built to offer"
    );
}

// ===================== mail keeps its place ============================

#[test]
fn mail_is_still_a_source_for_the_day_there_is_a_reader() {
    // Named rather than removed. Dropping it would make adding a reader a
    // redesign instead of a change at one line.
    let inbox = Inbox::default();
    let backlog = atlas::backlog::Backlog::default();
    let scheduler = Scheduler::default();
    let publisher = Publisher::default();
    let upkeep: Vec<String> = Vec::new();
    let s = empty_sources(&inbox, &backlog, &scheduler, &[], &publisher, &upkeep);
    assert!(s.mail.is_empty(), "this build has no mail reader, and should say so by being empty");
    assert!(Source::Mail.needs_network());
    assert!(!Source::Handoff.needs_network(), "a note already on this disk needs no network");
}

// ===================== when the day's run happens =======================
//
// `brief.at_hour` read "Hour the run happens" and nothing read it: the brief
// was built only when you asked for it by name. Wiring it as a clock was the
// obvious fix and the wrong one -- **a brief is worth having when you start,
// and you do not start at the same time every day.** Seven o'clock greets a
// night session at its fourth hour and misses the morning that began at ten.
//
// So arrival decides. Two conditions, in `daily::arriving`: a real gap since
// your last turn, and a day that has turned since the last brief -- measured
// against the hour `Rhythm` worked out from when you actually stop, not the
// calendar's midnight.

use atlas::daily::{arriving, day_of_with, Arrival, DailyConfig, Rhythm};

const HOUR: u64 = 3600;
const DAY: u64 = 86_400;
/// The day after the one `NOW` falls in.
///
/// Anchored to `NOW` rather than picked as a round number: the fixtures
/// record backlog items at `NOW`, and a day chosen a year later leaves them
/// stale, so the brief comes back empty and every assertion here passes or
/// fails for a reason that has nothing to do with arrival. Cost an hour.
const DAY0: u64 = NOW / DAY + 1;

/// A timestamp at a given hour of a given day.
fn at(day: u64, hour: u64) -> u64 {
    day * DAY + hour * HOUR + 90
}

fn daily() -> DailyConfig {
    DailyConfig::default()
}

// ===================== the night, which is the whole point =============

#[test]
fn working_through_the_night_is_not_starting_a_day() {
    // Pinned here (28 Sep 2026): this is about a time of day, and it passed
    // only when cargo found `.cargo/config.toml` (run from the crate's folder).
    atlas::localclock::pin_offset(Some(0));
    // The distinction this was rebuilt for. The day rolls at four; you have
    // been typing since eleven. Nothing has started.
    let d = daily();
    let day = DAY0;
    let eleven_pm = at(day - 1, 23);
    let four_am = at(day, 4);
    assert!(four_am - eleven_pm > 4 * HOUR, "the fixture is not a night");

    // The date did turn -- that is what makes this the interesting case.
    assert_ne!(day_of_with(eleven_pm, 4), day_of_with(four_am, 4));

    // And it still is not a new day for you, because you never went away.
    assert_eq!(arriving(at(day, 3) + 60, 0, four_am, 4, &d), Arrival::StillGoing);
}

#[test]
fn the_brief_is_held_until_you_come_back_rather_than_missed() {
    // The follow-through, and the reason `StillGoing` is not simply "no".
    // You work eleven until six, through the rollover. You sleep. You come
    // back at two in the afternoon -- and the brief you did not get at four
    // is what you get then.
    let d = daily();
    let day = DAY0;

    // 6am, still going, no brief given.
    assert_eq!(arriving(at(day, 6) - 120, 0, at(day, 6), 4, &d), Arrival::StillGoing);

    // 2pm, eight hours later, and no brief has been given today.
    assert_eq!(arriving(at(day, 6), 0, at(day, 14), 4, &d), Arrival::Starting);
}

#[test]
fn a_night_owls_day_ends_when_they_stop_rather_than_at_midnight() {
    // `rolls_at` is the middle of your longest quiet stretch, which `Rhythm`
    // works out. Nobody declares their hours.
    let mut r = Rhythm::default();
    for _ in 0..20 {
        r.note_day();
        // Awake 10am to 4am, asleep 5am to 9am.
        for h in [10, 12, 14, 16, 18, 20, 22, 23, 0, 1, 2, 3] {
            for _ in 0..5 {
                r.saw(h, true);
            }
        }
    }
    let rolls = r.rolls_at(&daily());
    assert!(
        (5..=9).contains(&rolls),
        "the quiet stretch was not found: rolls at {rolls}"
    );

    // 1am is still the previous day for this person, and would be a new one
    // under a calendar rollover.
    let day = DAY0;
    assert_eq!(day_of_with(at(day, 23), rolls), day_of_with(at(day + 1, 1), rolls));
}

// ===================== coming in =======================================

#[test]
fn the_first_time_you_come_in_on_a_new_day_is_what_earns_it() {
    let d = daily();
    let day = DAY0;
    // Away since last night, back at nine.
    assert_eq!(arriving(at(day - 1, 22), at(day - 1, 9), at(day, 9), 4, &d), Arrival::Starting);
}

#[test]
fn coming_back_later_the_same_day_does_not_earn_a_second_one() {
    // Pinned here (28 Sep 2026): this is about a time of day, and it passed
    // only when cargo found `.cargo/config.toml` (run from the crate's folder).
    atlas::localclock::pin_offset(Some(0));
    let d = daily();
    let day = DAY0;
    // Brief at nine. Out for the afternoon. Back at six.
    assert_eq!(arriving(at(day, 9), at(day, 9), at(day, 18), 4, &d), Arrival::BackAgain);
}

#[test]
fn a_lunch_break_is_not_going_away() {
    // `back_after_minutes` is sleep-sized on purpose: an hour out is the same
    // stretch of work, and a brief on return from lunch is an interruption
    // wearing a greeting.
    let d = daily();
    let day = DAY0;
    assert_eq!(arriving(at(day, 12), 0, at(day, 13), 4, &d), Arrival::StillGoing);
    // And the setting is read rather than a number that happens to match.
    let twitchy = DailyConfig { back_after_minutes: 30, ..daily() };
    assert_eq!(arriving(at(day, 12), 0, at(day, 13), 4, &twitchy), Arrival::Starting);
}

#[test]
fn the_very_first_turn_is_an_arrival() {
    // No previous turn to be a gap from, and treating that as "still going"
    // would mean a fresh install never gets a brief at all.
    let d = daily();
    assert_eq!(arriving(0, 0, at(DAY0, 9), 4, &d), Arrival::Starting);
}

// ===================== reached from the running program ================

#[test]
fn the_daemon_is_what_reaches_it_rather_than_this_test() {
    let raw = crate::common::source_of("daemon");
    let code: String = raw
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");

    assert!(code.contains("crate::daily::arriving("), "nothing asks whether you arrived");
    assert!(
        code.contains("crate::daily::Arrival::Starting"),
        "the arrival is computed and not acted on"
    );
    assert!(code.contains("self.rhythm.rolls_at("), "it uses the calendar's day, not yours");
    assert!(
        code.contains("self.last_turn_of_yours = t;"),
        "nothing records when you last did something, so there is no gap to measure"
    );
    // `= t`, not `= clock()`. It was `clock()` for an hour, which is right in
    // production and wrong everywhere the time is supplied: a test driving a
    // night at one in the morning had its gap measured against the real wall
    // clock, came out as zero, and Atlas concluded you were sitting there.
    assert!(
        !code.contains("self.last_turn_of_yours = clock()"),
        "the turn's own time is being ignored in favour of the wall clock"
    );
    // The clock trigger this replaced, gone rather than left beside it.
    assert!(!code.contains("this_hour == bcfg.at_hour"), "the fixed-hour run is back");

    let yaml = std::fs::read_to_string("config/tools.yaml").expect("tools.yaml");
    assert!(yaml.contains("back_after_minutes:"), "nowhere to set what counts as going away");
    assert!(yaml.contains("not_before_hour:"), "nowhere to set the earliest hour");
    assert!(!yaml.contains("at_hour: 7"), "the old name is still settable and no longer read");
}

#[test]
fn asking_works_at_any_hour() {
    // Pinned here (28 Sep 2026): this is about a time of day, and it passed
    // only when cargo found `.cargo/config.toml` (run from the crate's folder).
    atlas::localclock::pin_offset(Some(0));
    // `not_before_hour` governs the unasked brief only. Four in the morning
    // is a bad time to be handed a list and a fine time to ask for one.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "asked-at-4am");
    d.backlog.record("renew the certificate", Blocker::NeedsApproval, NOW);

    let four_am = at(DAY0, 4);
    let said = d.turn("what\'s outstanding", four_am);
    assert_eq!(said.matches("Start with").count(), 1, "asking was refused by the hour: {said}");

    // And the control: the *unasked* brief is refused at that hour, so this
    // test is about `not_before_hour` rather than about four o'clock being
    // an ordinary time. Without it, a brief that fired at any hour would
    // pass this just as well.
    let mut quiet = daemon(&c, &p, "unasked-at-4am");
    quiet.backlog.record("renew the certificate", Blocker::NeedsApproval, NOW);
    let ticked = quiet.tick(four_am).join(" ");
    assert_eq!(ticked.matches("Start with").count(), 0, "it interrupted at four: {ticked}");
}

#[test]
fn the_unasked_one_waits_for_the_hour_you_set() {
    // Pinned here (28 Sep 2026): this is about a time of day, and it passed
    // only when cargo found `.cargo/config.toml` (run from the crate's folder).
    atlas::localclock::pin_offset(Some(0));
    let (c, p) = (cfg(), plat());
    assert_eq!(c.tools.as_ref().unwrap().brief.not_before_hour, 7, "the shipped hour moved");

    let mut d = daemon(&c, &p, "not-before");
    d.backlog.record("renew the certificate", Blocker::NeedsApproval, NOW);
    let day = DAY0;

    // Up at five, having been away all night. An arrival, and too early.
    let early = d.tick(at(day, 5)).join(" ");
    assert_eq!(early.matches("Start with").count(), 0, "it interrupted at five: {early}");

    // Eight, still the first time in.
    let later = d.tick(at(day, 8)).join(" ");
    assert_eq!(later.matches("Start with").count(), 1, "the held brief never arrived: {later}");
}

#[test]
fn it_is_given_once_and_then_not_again_that_day() {
    // Pinned here (28 Sep 2026): this is about a time of day, and it passed
    // only when cargo found `.cargo/config.toml` (run from the crate's folder).
    atlas::localclock::pin_offset(Some(0));
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "once-a-day");
    d.backlog.record("renew the certificate", Blocker::NeedsApproval, NOW);
    let day = DAY0;

    let first = d.tick(at(day, 9)).join(" ");
    assert_eq!(first.matches("Start with").count(), 1, "{first}");

    for hour in [10u64, 14, 20] {
        let again = d.tick(at(day, hour)).join(" ");
        assert_eq!(again.matches("Start with").count(), 0, "said again at {hour}: {again}");
    }
}

#[test]
fn turning_it_off_turns_it_off() {
    // Pinned here (28 Sep 2026): this is about a time of day, and it passed
    // only when cargo found `.cargo/config.toml` (run from the crate's folder).
    atlas::localclock::pin_offset(Some(0));
    // What `enabled` could not do before: it was read inside `run` and
    // `from_here`, both of which only ran when you asked -- so switching it
    // off meant "answer nothing when I ask", which is not what a person
    // setting `enabled: false` on a morning brief wants.
    let p = plat();
    let day = DAY0;

    // The control. Without it, "nothing was said" is also what you get from
    // a brief with nothing in it.
    let on_cfg = cfg();
    let mut on = daemon(&on_cfg, &p, "off-control");
    on.backlog.record("renew the certificate", Blocker::NeedsApproval, NOW);
    assert_eq!(on.tick(at(day, 9)).join(" ").matches("Start with").count(), 1);

    let mut off_cfg = cfg();
    off_cfg.tools.as_mut().unwrap().brief.enabled = false;
    let mut off = daemon(&off_cfg, &p, "off");
    off.backlog.record("renew the certificate", Blocker::NeedsApproval, NOW);
    assert_eq!(
        off.tick(at(day, 9)).join(" ").matches("Start with").count(),
        0,
        "a disabled brief still went out"
    );
}

#[test]
fn a_quiet_morning_says_nothing_rather_than_saying_there_is_nothing() {
    // `spoken` on an empty brief returns "Nothing needs you. I'll get on with
    // the rest." That is the right answer to somebody who just asked, and the
    // wrong thing to volunteer every morning for the rest of your life.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "quiet-morning");
    let day = DAY0;

    assert!(d.brief_now(at(day, 9)).is_empty(), "this machine had something to say");
    let said = d.tick(at(day, 9)).join(" ");
    assert_eq!(said.matches("Nothing needs you").count(), 0, "it volunteered an empty brief: {said}");
}
