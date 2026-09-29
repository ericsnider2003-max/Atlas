//! "What did you do on your own?" — the review of the unprompted actions.
//!
//! `undo.rs` keeps one list of everything Atlas did, each entry marked with
//! whether you asked for it or Atlas decided. `History::on_its_own` returns
//! only the ones you did not ask for — its own doc calls them "what you'd want
//! to review" — and for weeks nothing reached it. "What did you do" answered
//! from the whole log: it mentions how many were unprompted but gives no way
//! to see *only* those, and the argument remainder ("on your own") that would
//! narrow it fell through `understand` to "Did you mean...".
//!
//! This drives the wire that fixed that through the daemon: a history question
//! carrying "on your own" (or "without asking") is now `Asking::WhatOnYourOwn`
//! and is answered from `on_its_own`, while a plain "what did you do" still
//! returns the whole list.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use atlas::undo::Undo;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The handler reads the wall clock to decide the window, so the seeded
/// actions have to sit just behind "now" to fall inside the default hour.
fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-unprompted-{tag}"));
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

/// Two things Atlas did on its own and one you asked for, all within the last
/// minute so the default hour window reaches them.
fn seed(d: &mut Daemon) {
    let now = now_secs();
    // you_asked = false: Atlas decided.
    d.history.note("archived 12 newsletters", "files", Undo::Atlas("put them back".into()), false, now - 30);
    d.history.note("dimmed your screen", "settings", Undo::Atlas("brighten it".into()), false, now - 20);
    // you_asked = true: you told it to.
    d.history.note("opened Chrome", "apps", Undo::You("close it yourself".into()), true, now - 10);
}

#[test]
fn asking_what_atlas_did_on_its_own_answers_from_the_unprompted_actions_alone() {
    let c = cfg();
    let p = plat();
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("own")), Proactive::new(ProactiveConfig::default()));
    seed(&mut d);
    // `now` small enough that the default hour window reaches all three.
    let reply = d.turn("what did you do on your own", 1_500);

    // Both unprompted actions are in scope, and only those.
    assert!(
        reply.contains("2 things I did on my own"),
        "the answer should count only the unprompted actions, got: {reply}"
    );
    // The most recent unprompted action is named.
    assert!(
        reply.contains("dimmed your screen"),
        "the answer should name the most recent unprompted action, got: {reply}"
    );
    // The thing you asked for is not part of this review.
    assert!(
        !reply.contains("opened Chrome"),
        "an action you asked for must not appear in the on-your-own review, got: {reply}"
    );
}

#[test]
fn without_asking_is_the_same_question_worded_differently() {
    let c = cfg();
    let p = plat();
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("without")), Proactive::new(ProactiveConfig::default()));
    seed(&mut d);
    let reply = d.turn("what have you done without asking", 1_500);
    assert!(
        reply.contains("2 things I did on my own") && reply.contains("dimmed your screen"),
        "\"without asking\" should reach the same review, got: {reply}"
    );
}

#[test]
fn the_plain_question_still_returns_the_whole_log_not_just_the_unprompted() {
    let c = cfg();
    let p = plat();
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("plain")), Proactive::new(ProactiveConfig::default()));
    seed(&mut d);
    let reply = d.turn("what did you do", 1_500);
    // The whole list includes the thing you asked for, which the on-your-own
    // review omits -- the two questions genuinely differ.
    assert!(
        reply.contains("opened Chrome"),
        "the plain question should return the whole log including asked actions, got: {reply}"
    );
    // And it still says how many of those were unprompted, as it always has.
    assert!(
        reply.contains("I did on my own"),
        "the whole-log answer still flags the unprompted count, got: {reply}"
    );
}

#[test]
fn nothing_unprompted_is_said_plainly_rather_than_faked() {
    let c = cfg();
    let p = plat();
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("none")), Proactive::new(ProactiveConfig::default()));
    // Only things you asked for.
    d.history.note("opened Chrome", "apps", Undo::You("close it".into()), true, now_secs() - 10);
    let reply = d.turn("what did you do on your own", 1_500);
    assert!(
        reply.contains("Nothing on my own"),
        "with no unprompted actions the review should say so, got: {reply}"
    );
}
