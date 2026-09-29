//! The built-in calendar, driven through the daemon.
//!
//! Atlas keeps your calendar itself — offline, in its own store — and the
//! native phone calendar is a sync target, not a dependency. The store and the
//! time-reading are unit-tested in `calendar`; these drive the two intents
//! through a whole daemon: putting something on, reading it back, and being
//! honest when it can't read a time.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

/// When these turns happen: noon on a Friday (30 Oct 2026, UTC).
///
/// They were at `100` -- one minute past midnight on 1 Jan 1970 -- which
/// only worked because scheduling and the agenda read the wall clock rather
/// than the turn's time. Since 28 Sep 2026 they read the turn's time, and at
/// 00:01 "tomorrow" is rightly asked about (it could mean later today).
const NOON: u64 = 1_793_361_600;

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-cal-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn cfg() -> Config {
    daytime(Config::load(Path::new("config")).unwrap())
}

/// The handlers read "tomorrow" off the wall clock, and between midnight and
/// 4 am "tomorrow" is asked about rather than booked (round 10). So a test
/// run in those hours (UTC, the shipped zone) is given a zone where it's
/// morning instead. Found at 00:44 UTC on 26 Sep, when every calendar test
/// that says "tomorrow" failed at once.
fn daytime(mut c: Config) -> Config {
    if (atlas::store::now() / 3600) % 24 < 5 {
        c.tools.as_mut().expect("the shipped config has tools").time_zone = "Asia/Shanghai".into();
    }
    c
}


fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    let mut d = Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()));
    // The day's first turn earns the day's brief, which leads its reply
    // (28 Sep 2026: at `NOON` rather than 00:01 it does). Taken here, so the
    // turns under test answer only themselves.
    let _ = d.turn("what time is it", NOON - 120);
    d
}

#[test]
fn scheduling_with_a_time_puts_it_on_and_reads_back() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "add");
    let reply = d.turn("schedule dentist tomorrow at 9am", NOON);
    assert!(reply.to_lowercase().contains("calendar"), "should confirm, got: {reply}");
    assert!(reply.to_lowercase().contains("dentist"), "should name it, got: {reply}");
    // It's actually stored.
    assert_eq!(d.calendar.len(), 1);
    // And it reads back.
    let agenda = d.turn("what's on this week", NOON);
    assert!(agenda.to_lowercase().contains("dentist"), "agenda should list it, got: {agenda}");
}

#[test]
fn scheduling_a_repeat_stores_it_once_and_reads_it_on_each_day() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "repeat");
    let reply = d.turn("schedule standup every weekday at 9am", NOON);
    // Confirmed, and the confirmation says it repeats.
    assert!(reply.to_lowercase().contains("standup"), "should name it: {reply}");
    assert!(reply.to_lowercase().contains("every weekday"), "should say it repeats: {reply}");
    // One series stored, not one row per day.
    assert_eq!(d.calendar.len(), 1, "a repeat is stored once, not materialised");
    // But the week's agenda shows it on more than one day (it expands).
    let agenda = d.turn("what's on this week", NOON);
    let hits = agenda.matches("standup").count();
    assert!(hits >= 2, "a weekday repeat should show on several days this week: {agenda}");
}

#[test]
fn a_reminder_is_confirmed_and_then_fires_once_as_it_comes_due() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "reminder");
    // Schedule something well ahead with a reminder.
    let reply = d.turn("schedule dentist tomorrow at 9am, remind me 15 minutes before", NOON);
    assert!(reply.to_lowercase().contains("remind"), "should confirm the reminder: {reply}");
    assert_eq!(d.calendar.len(), 1);

    // Find the event's start, then tick from inside its 15-minute lead window.
    let start = d.calendar.next(0).expect("event is there").start;
    let out = d.tick(start - 600); // 10 minutes before
    assert!(
        out.iter().any(|line| line.to_lowercase().contains("reminder") && line.contains("dentist")),
        "the reminder should fire inside its window: {out:?}"
    );
    // It doesn't fire again on the next tick still inside the window.
    let again = d.tick(start - 300);
    assert!(
        !again.iter().any(|line| line.to_lowercase().contains("reminder") && line.contains("dentist")),
        "a reminder must not repeat within the same occurrence: {again:?}"
    );
}

#[test]
fn a_request_without_a_time_asks_for_one_rather_than_guessing() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "notime");
    let reply = d.turn("schedule a haircut", NOON);
    assert!(
        reply.to_lowercase().contains("when") && d.calendar.is_empty(),
        "with no time it should ask and store nothing, got: {reply}"
    );
}

#[test]
fn an_empty_calendar_says_so() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "empty");
    let reply = d.turn("what's on today", NOON);
    assert!(reply.to_lowercase().contains("nothing"), "got: {reply}");
}

#[test]
fn the_phone_bridge_merges_in_and_offers_atlas_events_out() {
    // The native phone calendar is read/written by the phone app; this drives
    // the in-tree seam it uses. A phone event merges in; an Atlas event is
    // offered back out for the phone to add.
    use atlas::calendar::{Event, Source};
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "phone");
    d.turn("schedule mine tomorrow at 10am", NOON);
    let incoming = vec![Event {
        id: 0,
        title: "theirs".into(),
        start: 2_000_000_000,
        end: 2_000_003_600,
        all_day: false,
        place: None,
        note: None,
        space: atlas::earned::Space::Personal,
        kind: atlas::calendar::EventKind::Meeting,
        repeat: atlas::calendar::Repeat::Once,
        remind_before_mins: None,
        source: Source::Phone,
        phone_key: Some("PH-1".into()),
        except: Vec::new(),
        zone: None,
        created: 0,
    }];
    assert_eq!(d.calendar.merge_from_phone(incoming, 100), 1);
    // Atlas's own event, and only that, is offered back to the phone.
    let out = d.calendar.for_phone();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].title, "mine");
}

#[test]
fn a_clash_is_pointed_out_but_still_scheduled() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "clash");
    d.turn("schedule call tomorrow at 2pm for 60 minutes", NOON);
    let reply = d.turn("schedule review tomorrow at 2:30pm for 60 minutes", NOON);
    assert!(reply.to_lowercase().contains("runs into"), "should flag the clash, got: {reply}");
    // Both are still on — a clash is yours to sort out, not a refusal.
    assert_eq!(d.calendar.len(), 2);
}

#[test]
fn a_plain_event_scheduled_through_the_daemon_is_personal() {
    // With no businesses set up, everything the schedule intent files is
    // yours -- the classifier only ever tags a business you actually have.
    use atlas::earned::Space;
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "personal-side");
    // The schedule intent stamps events with the real clock, not the turn's
    // timestamp, so reach the event by identity rather than by a time window.
    d.turn("schedule dentist tomorrow at 9am", NOON);
    assert_eq!(d.calendar.len(), 1);
    let e = d.calendar.next(0).expect("the event is on the calendar");
    assert_eq!(e.title, "dentist");
    assert_eq!(
        e.space,
        Space::Personal,
        "a plain event goes on your own side of the firewall"
    );
}

#[test]
fn an_event_naming_a_business_you_have_is_filed_on_that_side() {
    // End to end: a business set up on the roster, then a spoken schedule that
    // names it lands on that side of the firewall, and the agenda can show
    // just that business.
    use atlas::earned::Space;
    use atlas::kin::{Pairings, Peer};
    use atlas::roster::Roster;

    let dir = tmp("biz-side");
    // Seed a business the daemon will read from the same store.
    let mut pairings = Pairings::default();
    pairings.peers.push(Peer::new("Maya", "tok"));
    let mut roster = Roster::default();
    roster.add("Northwind", "Maya", &pairings).unwrap();
    roster.save(&Store::new(dir.clone())).unwrap();

    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(
        &c,
        &p,
        None,
        Store::new(dir),
        Proactive::new(ProactiveConfig::default()),
    );

    let reply = d.turn("schedule the Northwind review tomorrow at 2pm", NOON);
    assert!(reply.contains("Northwind"), "the confirmation should name the business side, got: {reply}");

    let e = d.calendar.next(0).expect("the event is on the calendar");
    assert_eq!(
        e.space,
        Space::Business("Northwind".into()),
        "an event naming a business you have is filed on that side"
    );
}

#[test]
fn blocking_off_time_reads_as_a_block_not_a_meeting() {
    use atlas::calendar::EventKind;
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "timeblock");
    let reply = d.turn("block off two hours tomorrow at 9am for deep work", NOON);
    assert!(
        reply.to_lowercase().contains("blocked off"),
        "reserving focus time should say so, got: {reply}"
    );
    let e = d.calendar.next(0).expect("the block is on the calendar");
    assert_eq!(e.kind, EventKind::TimeBlock, "a blocked-off stretch is a time block");

    // A plain meeting the same day stays a meeting.
    d.turn("schedule a call tomorrow at 2pm", NOON);
    let now = NOON;
    let kinds: Vec<_> =
        d.calendar.occurrences_between(now, now + 7 * 86_400).iter().map(|e| e.kind).collect();
    assert!(
        kinds.contains(&EventKind::TimeBlock) && kinds.contains(&EventKind::Meeting),
        "the two should read differently: {kinds:?}"
    );
}
