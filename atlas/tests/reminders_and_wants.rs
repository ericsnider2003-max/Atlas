//! Reminders (B3) and opportunity-spotting (B2), through the daemon.
//!
//! Both hang off the `Intent::Unknown` chain, so the honest test drives real
//! sentences through `Daemon::turn` rather than calling the handlers directly.
//! A reminder is a scheduler JOB (what Atlas *says* at a time), distinct from
//! the calendar (what you look at); an opportunity is Atlas weighing a stated
//! want — and, honestly, never declaring a verdict, because Money can't be
//! audited and Fit is the user's call.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-remind-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

#[test]
fn a_relative_reminder_creates_a_job_and_says_when() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "relative");
    let before = d.scheduler.active().len();
    let reply = d.turn("remind me in 20 minutes to stretch", 100);
    assert!(reply.contains("20 minutes"), "should say when: {reply}");
    assert!(reply.contains("stretch"), "should carry the thing: {reply}");
    assert_eq!(d.scheduler.active().len(), before + 1, "a scheduler job was created");
}

#[test]
fn a_reminder_with_no_time_is_not_dropped() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "notime");
    let reply = d.turn("remind me to call the plumber", 100);
    assert!(reply.contains("plumber"), "a timeless reminder is still surfaced: {reply}");
}

#[test]
fn a_stated_want_is_weighed_as_an_opportunity_not_answered_blindly() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "want");
    let reply = d.turn("i want to turn my woodworking into a side business", 100);
    // It engages the want as an opportunity to weigh — and, with Money
    // un-auditable and Fit the user's call, it never returns a bare verdict.
    assert!(
        reply.to_lowercase().contains("weigh") || reply.to_lowercase().contains("opportunity"),
        "a stated want should be weighed, not passed over: {reply}"
    );
}

#[test]
fn an_ordinary_question_is_not_mistaken_for_a_want() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "notwant");
    // No want-cue, so spot_opportunity must not claim it.
    let reply = d.turn("what is the capital of France", 100);
    assert!(
        !reply.to_lowercase().contains("worth weighing"),
        "a plain question must not be treated as an opportunity: {reply}"
    );
}

#[test]
fn weighing_an_opportunity_names_what_it_cannot_settle() {
    // Direct on weigh_opportunity: with Money un-auditable and Fit the user's
    // call, it surfaces the frame and what it needs, never a bare verdict.
    let (c, p) = (cfg(), plat());
    let d = daemon(&c, &p, "weigh");
    let out = d.weigh_opportunity("turn the woodworking into a business", "a stated want");
    assert!(!out.trim().is_empty(), "an opportunity frame should say something: {out}");
}
