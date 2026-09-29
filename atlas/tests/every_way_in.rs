//! "What are all the ways you could get me that?"
//!
//! `route.rs` was written so a closed path moves straight on to the next
//! rather than starting the thinking again: `plan` picks the single cheapest
//! way in that could work, `another_way` names the next when the first fails,
//! and `all_routes` lays the whole ordered menu out in advance. For weeks
//! `all_routes` was proven by `stance_route.rs` and reached by nothing — the
//! daemon only ever surfaced one way at a time, so a person who wanted to see
//! the options before committing had no way to ask.
//!
//! This drives the wire that fixed that. A line asking for *all* the ways now
//! reaches `all_routes` through `ways_in_help`, in the `Unknown` chain beside
//! `decision_help`, and the reply is the ordered menu rather than a shrug.
//!
//! What it can prove: the daemon turns "what are all the ways you could get the spreadsheet numbers"
//! into a numbered list of several approaches, in order, and an ordinary
//! line does not trip it. What it cannot: whether those are the *best* routes
//! for the specific task — that depends on what is actually installed, which
//! `offline_coverage` answers separately.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-ways-{tag}"));
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

/// The number a menu reply opens with — "3 ways I'd try, in order — ...".
///
/// Parsed rather than matched as a substring, so the assertion is about how
/// many ways were actually laid out, not about the wording around them.
fn ways_counted(reply: &str) -> usize {
    reply
        .split_whitespace()
        .next()
        .and_then(|w| w.parse::<usize>().ok())
        .unwrap_or(0)
}

#[test]
fn asking_for_all_the_ways_lays_the_menu_out_in_order() {
    let c = cfg();
    let p = plat();
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("menu")), Proactive::new(ProactiveConfig::default()));

    // Not a command — it lands as an unrecognised line, the same door
    // `decision_help` sits behind, and `ways_in_help` sits just above it.
    let reply = d.turn("what are all the ways you could get the spreadsheet numbers", 1_000);

    // A non-`.contains` assertion: the reply must open by naming a real count
    // of routes, and a menu is more than one way. This is the whole point of
    // `all_routes` over `plan` — several, in order, not a single pick.
    let n = ways_counted(&reply);
    assert!(
        n >= 3,
        "asking for all the ways should lay out several, got {n}: {reply}"
    );

    // The order is planned in advance so a failure moves straight on, so the
    // list is numbered and the count in the opening line matches the number of
    // numbered steps that follow it.
    let numbered = (1..=n).all(|i| reply.contains(&format!("{i}. ")));
    assert!(numbered, "the menu should be numbered 1..={n}: {reply}");
}

#[test]
fn an_ordinary_line_does_not_trip_the_menu() {
    // Contrast run through the same real daemon: a plain factual line is not a
    // request for the ways, so it must not come back as a route menu. The two
    // replies being different is the state check — `ways_in_help` returns
    // `None` here and the ordinary path answers instead.
    let c = cfg();
    let p = plat();
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("contrast")), Proactive::new(ProactiveConfig::default()));

    let menu = d.turn("what are all the ways you could get the spreadsheet numbers", 1_000);
    let plain = d.turn("the capital of France is Paris", 2_000);

    assert_ne!(
        menu, plain,
        "an ordinary line should not produce the route menu"
    );
    assert_eq!(
        ways_counted(&plain),
        0,
        "a plain line opens with no route count: {plain}"
    );
}
