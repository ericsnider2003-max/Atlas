//! A mode you turn on, you can turn off again.
//!
//! `modes::enter` was wired into `Intent::SetMode` and `modes::leave` was not,
//! so a mode could be entered and never cleanly left -- the exact thing the
//! function's own doc warns about: "a mode you cannot get out of cleanly is a
//! mode you stop using." `leave` restores what was open before entering, drops
//! the mode's rules and clears the active mode. It was proven by the modes
//! tests and reached by nothing in production until now.
//!
//! Wired into the existing `SetMode` handler rather than a new intent: "mode
//! off", "go into normal mode", "mode normal" carry a leaving word as the
//! argument, and the handler leaves the current mode instead of trying to enter
//! one by that name -- but only when a mode is actually on, so someone who
//! genuinely built a "normal" mode can still turn it on.
//!
//! Driven end to end through a real parser and a real Daemon with plain-string
//! utterances, and checked by state (`modes.active()`), not only by text.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::intent::{Intent, Parser};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-leave-mode-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

#[test]
fn a_leaving_word_routes_to_set_mode() {
    let c = Config::load(Path::new("config")).unwrap();
    let parser = Parser::new(&c.commands);
    // The argument, not a standalone phrase: "mode" is the base and "off" is
    // what follows, so the parser keeps a non-empty argument to discriminate on.
    assert_eq!(parser.parse("mode off"), Intent::SetMode("off".into()));
    assert_eq!(parser.parse("go into normal mode"), Intent::SetMode("normal mode".into()));
}

#[test]
fn a_mode_can_be_left_once_it_is_entered() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = daemon(&c, &p, "round-trip");
    // The shipped starter modes, so there is a real "focus" mode to enter.
    for m in atlas::modes::suggested() {
        d.modes.add(m);
    }

    // Enter, through the front door.
    let entered = d.turn("go into focus mode", 100);
    assert_eq!(
        d.modes.active().map(|m| m.name.clone()),
        Some("focus".into()),
        "entering a mode should make it the active mode, got reply: {entered}"
    );

    // Leave, through the front door, with a plain-string leaving word.
    let left = d.turn("mode off", 200);
    assert!(
        d.modes.active().is_none(),
        "\"mode off\" should clear the active mode, leaving none; reply was: {left}"
    );
    assert!(
        left.contains("focus"),
        "leaving should name the mode it left, got: {left}"
    );
}

#[test]
fn leaving_when_no_mode_is_on_says_so_and_does_not_enter_one() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = daemon(&c, &p, "nothing-on");
    for m in atlas::modes::suggested() {
        d.modes.add(m);
    }

    // A contrast run: the very same utterance that leaves a mode when one is on
    // must NOT invent a mode named "off" when none is on. Before, the handler
    // only ever tried `enter`, so this would have answered "I don't have a off
    // mode." Now it distinguishes leaving from entering by whether a mode is on.
    assert!(d.modes.active().is_none(), "no mode should be on to start with");
    let reply = d.turn("mode off", 100);
    // The daemon's phrasing pass may personalise the line, so the state check
    // below is the load-bearing assertion: no mode was entered.
    assert!(
        reply.to_lowercase().contains("not in a mode"),
        "leaving with nothing on should say there is no mode, got: {reply}"
    );
    assert!(
        d.modes.active().is_none(),
        "a leaving word with no mode on must not enter a mode"
    );
}
