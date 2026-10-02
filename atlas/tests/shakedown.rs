//! The commissioning shakedown, end to end.
//!
//! Walk the never-run capabilities and verify what can be verified without the
//! person: read-only checks now, visible ones on a go-ahead, data ones on real
//! use, camera ones flagged for their eyes. The point is that the human part is
//! a short guided list, not every feature by hand.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::shakedown::{self, Outcome, Step};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn dir(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-shakedown-{tag}-{}", std::process::id()));
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

#[test]
fn the_report_leads_with_what_was_verified_and_names_your_part() {
    let steps = vec![
        Step { capability: "read the screen", outcome: Outcome::Verified("read the active window".into()) },
        Step { capability: "arrange windows", outcome: Outcome::OnYourGo("I move a window and read it back".into()) },
        Step { capability: "read the market", outcome: Outcome::OnRealData("on your files".into()) },
        Step { capability: "name what the camera sees", outcome: Outcome::NeedsYou("you confirm".into()) },
    ];
    assert!(shakedown::all_clear(&steps), "nothing failed");
    let r = shakedown::report(&steps);
    assert!(r.contains("verified 1"), "leads with what's confirmed: {r}");
    assert!(r.to_lowercase().contains("your go-ahead"), "names the one-tap set: {r}");
    assert!(r.to_lowercase().contains("need your eyes"), "names your part: {r}");
}

#[test]
fn a_failed_check_is_not_all_clear_and_is_named() {
    let steps = vec![
        Step { capability: "read the screen", outcome: Outcome::Failed("no screen".into()) },
    ];
    assert!(!shakedown::all_clear(&steps), "a failure is not clear");
    let r = shakedown::report(&steps);
    assert!(r.contains("no screen"), "the failure reason is surfaced: {r}");
}

#[test]
fn running_a_shakedown_through_the_daemon_covers_every_never_run_thing() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(dir("run")), Proactive::new(ProactiveConfig::default()));
    let reply = d.turn("run a shakedown", 100);
    assert!(reply.to_lowercase().contains("shakedown of"), "it ran the shakedown: {reply}");
    // The verified-now count is real — it read the active window off the mock
    // platform for the read-only capabilities — so the number is not canned.
    assert!(reply.to_lowercase().contains("verified"), "it reports what it verified: {reply}");
    // And it's a real answer, not the catch-all.
    assert_ne!(reply.to_lowercase(), "i don't have anything for that.", "the phrase routed: {reply}");
}

#[test]
fn the_shakedown_says_only_a_couple_need_your_eyes() {
    // The whole reassurance: of everything never run, the human part is tiny.
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(dir("eyes")), Proactive::new(ProactiveConfig::default()));
    let reply = d.turn("commission yourself", 100);
    // Your part: the two camera ones; since round 5 the three that need your
    // voice or your hand on the key (the built-in voice encoder, your wake
    // phrase, the push-to-talk key); and, from the third chat's line, one
    // real call for call notes. Merged 26 Sep 2026. And from the Atlas
    // Project chat's 25j, the Windows key hook (hold to talk, press to type),
    // which only a person at the keyboard can confirm.
    let (_c, _r, eyes_n, _names) = atlas::capability::commissioning();
    assert!(reply.contains(&format!("{eyes_n} need your eyes")), "your part is the camera, voice, key and call ones: {reply}");
    // And the number is the real classification, not a canned phrase.
    // Eight since 28 Sep 2026: `audio` -- choosing the microphone and
    // speakers, and not opening a Bluetooth headset's microphone for nothing
    // -- was built and unlisted until the catalogue covered the whole tree.
    // Only your ears can confirm the headset still sounds right.
    // Ten on 1 Oct 2026: `camwatch` and `callmute` came in (the camera kept
    // to its word, Atlas on a call) -- both only your eyes and ears can check.
    assert_eq!(eyes_n, 10, "the classification really is those ten");
    assert!(reply.contains(&eyes_n.to_string()), "the reported count is the real one");
}
