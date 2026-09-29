//! Answering a time someone proposed, driven through the daemon.
//!
//! `booking` does the tedious half — read what was proposed, check it against
//! your calendar, lay out what fits and what to offer instead — and then stops.
//! Only an explicit accept writes it to your calendar, and nothing is ever sent
//! on your behalf. These drive that through a whole daemon.

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
    let p = std::env::temp_dir().join(format!("atlas-booking-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
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

fn cfg_on() -> Config {
    let mut c = daytime(Config::load(Path::new("config")).unwrap());
    // Booking ships off; turn it on for the test the way a person would.
    c.tools.as_mut().unwrap().booking.enabled = true;
    c
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

#[test]
fn it_is_off_by_default_and_says_so() {
    let c = Config::load(Path::new("config")).unwrap(); // shipped: off
    let p = plat();
    let mut d = daemon(&c, &p, "off");
    let reply = d.turn("log a proposal from Sam tomorrow at 2pm for a review", NOON);
    assert!(reply.to_lowercase().contains("switched off"), "off by default: {reply}");
}

#[test]
fn a_proposal_is_read_checked_and_nothing_is_booked_yet() {
    let (c, p) = (cfg_on(), plat());
    let mut d = daemon(&c, &p, "log");
    let reply = d.turn("log a proposal from Sam tomorrow at 2pm for the review", NOON);
    // It names who, and — the load-bearing honesty — books nothing yet.
    assert!(reply.contains("Sam"), "should name who proposed: {reply}");
    assert!(
        reply.to_lowercase().contains("nothing's booked") || reply.to_lowercase().contains("say yes"),
        "must make clear nothing is booked: {reply}"
    );
    // Nothing on the calendar until you accept.
    assert_eq!(d.calendar.len(), 0, "a proposal must not write to the calendar on its own");
}

#[test]
fn accepting_writes_it_to_the_calendar_but_sends_nothing() {
    let (c, p) = (cfg_on(), plat());
    let mut d = daemon(&c, &p, "accept");
    d.turn("log a proposal from Sam tomorrow at 2pm for the review", NOON);
    let reply = d.turn("accept the meeting", NOON);
    // It's on the calendar now.
    assert_eq!(d.calendar.len(), 1, "accepting should write the event: {reply}");
    // And it's honest that it hasn't replied for you.
    assert!(
        reply.to_lowercase().contains("haven't replied") || reply.to_lowercase().contains("don't send"),
        "must not claim to have sent a reply: {reply}"
    );
}

#[test]
fn declining_marks_it_and_writes_nothing() {
    let (c, p) = (cfg_on(), plat());
    let mut d = daemon(&c, &p, "decline");
    d.turn("log a proposal from Sam tomorrow at 2pm for the review", NOON);
    let reply = d.turn("decline the meeting", NOON);
    assert!(reply.to_lowercase().contains("declined"), "should confirm the decline: {reply}");
    assert_eq!(d.calendar.len(), 0, "declining writes nothing to the calendar");
}

#[test]
fn offering_another_time_lays_out_alternatives_and_books_nothing() {
    let (c, p) = (cfg_on(), plat());
    let mut d = daemon(&c, &p, "counter");
    d.turn("log a proposal from Sam tomorrow at 2pm for the review", NOON);
    let reply = d.turn("offer another time", NOON);
    assert!(
        reply.to_lowercase().contains("offer") || reply.to_lowercase().contains("instead"),
        "should lay out alternatives: {reply}"
    );
    assert_eq!(d.calendar.len(), 0, "a counter-offer books nothing");
}
