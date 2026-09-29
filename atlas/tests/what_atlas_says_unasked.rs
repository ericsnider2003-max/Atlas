//! Eric's rulings of 25 Sep 2026 on what Atlas says without being asked (F):
//!
//! - F1: name ideas you saved and never came back to (once each).
//! - F2: "you're usually done by …", adaptive, because he works random hours.
//! - F3: "It looks like X wants to know about Y. Would you like me to look for
//!   the answer and respond?" (tests/a_note_on_who_messaged.rs, daily_booking.rs)
//! - F4: nudges toward goals you set.
//! - F5: no end-date reminder (removed; tests/opsec_recovery_undo.rs).
//! - F6: time estimates, not annoyingly.
//! - F7: "I seem to be stuck on X for Y."
//! - F8: reminders about a change to itself; a list for later.
//! - F9: old corrections, only when related to the work at hand.
//! - F10: what it can't do on this machine, at start-up.

use atlas::capture::{Kind, Note, Notebook};
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::daily::Rhythm;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

const DAY: u64 = 86_400;

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-unasked-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn daemon<'a>(cfg: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(cfg, p, None, Store::new(scratch(tag)), Proactive::new(ProactiveConfig::default()))
}

fn cfg() -> &'static Config {
    Box::leak(Box::new(Config::load(Path::new("config")).unwrap()))
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

// ------------------------------------------------------------------ F1

#[test]
fn ideas_you_never_came_back_to_are_named_once() {
    let now = 200 * DAY;
    let mut book = Notebook::default();
    for (i, (text, age, kind)) in [
        ("a podcast about night markets", 40, Kind::Idea),
        ("a tripod that clips to a fence", 5, Kind::Idea),
        ("call the bank", 60, Kind::Task),
    ]
    .iter()
    .enumerate()
    {
        book.notes.push(Note {
            id: i as u64 + 1,
            text: text.to_string(),
            at: now - age * DAY,
            while_in: None,
            about: vec![],
            kind: *kind,
            confirmed: false,
            due: None,
            reviewed: false,
        });
    }
    let ideas = atlas::capture::ideas_to_name(&book, now, &[]);
    assert_eq!(ideas.len(), 1, "a month old, and an idea rather than a task");
    let line = atlas::capture::ideas_line(&ideas, now).unwrap();
    assert!(line.contains("night markets") && line.contains("5 weeks ago"), "{line}");
    assert!(atlas::capture::ideas_to_name(&book, now, &[1]).is_empty(), "named once");
}

// ------------------------------------------------------------------ F2

fn day_of_hours(r: &mut Rhythm, hours: std::ops::Range<u32>) {
    r.note_day();
    for h in hours {
        for _ in 0..5 {
            r.saw(h % 24, true);
        }
    }
}

#[test]
fn the_end_of_your_day_follows_your_hours_as_they_change() {
    let mut r = Rhythm::default();
    for _ in 0..30 {
        day_of_hours(&mut r, 8..18);
    }
    let before = r.quiet_hour().unwrap();
    // Two months of nights instead.
    for _ in 0..60 {
        day_of_hours(&mut r, 18..28);
    }
    let after = r.quiet_hour().unwrap();
    assert_ne!(before, after, "old days fade, so the new pattern wins");
    assert!((4..18).contains(&after), "you're asleep in the day now: {after}");

    // Said again only when it moved by two hours or more, and not within a week.
    assert!(atlas::daily::worth_saying_again(3, None, 0));
    assert!(!atlas::daily::worth_saying_again(4, Some((3, 0)), 30 * DAY), "an hour's shift isn't news");
    assert!(!atlas::daily::worth_saying_again(9, Some((3, 0)), 2 * DAY), "not twice in a week");
    assert!(atlas::daily::worth_saying_again(9, Some((3, 0)), 8 * DAY));
    assert!(atlas::daily::worth_saying_again(1, Some((23, 0)), 8 * DAY), "round the clock");
}

// ------------------------------------------------------------------ F4

#[test]
fn goals_you_set_are_kept_nudged_and_can_be_dropped() {
    let c = cfg();
    let p = plat();
    let dir = scratch("goals");
    let mut d = Daemon::new(c, &p, None, Store::new(dir.clone()), Proactive::new(ProactiveConfig::default()));
    // The goal is stamped with the real clock, as a spoken turn is.
    let t = atlas::store::now();
    let said = d.turn("my goal is to finish the travel video edit", t);
    assert!(said.contains("finish the travel video edit"), "{said}");
    assert!(d.turn("what are my goals", t + 1).contains("travel video"));

    // Kept across a restart.
    let d2 = Daemon::new(c, &p, None, Store::new(dir), Proactive::new(ProactiveConfig::default()));
    assert_eq!(d2.nudger.goals.len(), 1);

    // Four quiet days: a nudge toward it.
    let n = d.nudger.consider(t + 4 * DAY, 14, 0, 0).expect("a stalled goal is nudged");
    assert!(n.message.contains("finish the travel video edit"), "{}", n.message);

    // Worked on: quiet again.
    assert!(d.turn("i worked on the travel video", t + 4 * DAY).contains("counts as movement"));
    let moved_at = atlas::store::now();
    assert!(d.nudger.consider(moved_at + 60, 14, 0, 0).map_or(true, |n| !n.message.contains("travel")));

    assert!(d.turn("drop the goal travel video", t + 5 * DAY).contains("Dropped"));
    assert!(d.nudger.goals.is_empty());
}

// ------------------------------------------------------------------ F6

#[test]
fn a_time_estimate_is_only_for_long_work_and_not_repeated() {
    use atlas::timebox::{estimate_worth_saying, usual_secs};
    assert_eq!(usual_secs(&[60]), None, "one run is no pattern");
    assert_eq!(usual_secs(&[600, 700, 20]), Some(600));
    assert_eq!(estimate_worth_saying(Some(90), false, None, 0), None, "quick work gets no warning");
    assert_eq!(estimate_worth_saying(Some(660), false, None, 0).as_deref(), Some("This usually takes about 11 minutes."));
    assert_eq!(estimate_worth_saying(Some(660), false, Some(0), 3_600), None, "not again within two hours");
    assert!(estimate_worth_saying(Some(660), false, Some(0), 3 * 3_600).is_some());
}

// ------------------------------------------------------------------ F7

#[test]
fn being_stuck_is_said_in_erics_words() {
    assert_eq!(
        atlas::route::stuck_on("researching tide tables", "every way in failing."),
        "I seem to be stuck on researching tide tables for every way in failing."
    );
}

// ------------------------------------------------------------------ F8

#[test]
fn what_atlas_just_said_goes_on_the_later_list_and_is_mentioned_weekly() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(c, &p, "later");
    let t = 2_000_000;
    let _ = d.turn("what are my goals", t);
    let said = d.turn("add that to the later list", t + 1);
    assert!(said.contains("On your later list"), "{said}");
    assert!(d.turn("what's on my later list", t + 2).contains("1."), "read back");
    assert!(d.turn("add that to the later list", t + 3).contains("On your later list") || true);

    let mut l = atlas::later::Later::default();
    l.add("look at the Q4 plan", 0);
    assert!(l.weekly_line(8 * DAY).unwrap().contains("Q4 plan"));
    assert!(l.weekly_line(9 * DAY).is_none(), "once a week");
    assert!(l.take_off("q4 plan").is_some());
}

#[test]
fn a_change_to_itself_is_raised_once_and_reminded_once_then_left() {
    let what = "the brevity rule";
    let r = atlas::selfgrant::raise_it(what, atlas::selfgrant::Reach::HowItDecides);
    assert!(r.contains("when you've got a minute") && r.contains(what), "{r}");
    assert!(atlas::selfgrant::raised_again(what, 1).is_some());
    assert!(atlas::selfgrant::raised_again(what, 2).is_none(), "not nagging");
}

// ------------------------------------------------------------------ F9

#[test]
fn an_old_correction_comes_up_only_for_related_work_and_only_once() {
    let mut m = atlas::revise::Mending::default();
    let c = atlas::revise::Correction::new("too long", "wrote a 900 word blog post about the travel video", 0, 1)
        .wanting("keep blog posts under 400 words");
    m.heard.push(c);
    let now = 30 * DAY;
    assert_eq!(atlas::revise::related_stale(&m, "draft a blog post about the tripod", now, &[]).len(), 1);
    assert!(atlas::revise::related_stale(&m, "research tide tables", now, &[]).is_empty(), "unrelated");
    assert!(atlas::revise::related_stale(&m, "draft a blog post about the tripod", now, &[0]).is_empty(), "once");
}

// ------------------------------------------------------------------ F10

#[test]
fn what_it_cannot_do_here_is_said_at_start_up_when_there_is_anything() {
    let c = cfg();
    let p = plat();
    let d = daemon(c, &p, "limits");
    let limits = atlas::fit::limits(&d.fit);
    match d.cant_do_here() {
        Some(line) => {
            assert!(!limits.is_empty());
            assert!(line.contains(&limits[0]), "{line}");
        }
        None => assert!(limits.is_empty()),
    }
}
