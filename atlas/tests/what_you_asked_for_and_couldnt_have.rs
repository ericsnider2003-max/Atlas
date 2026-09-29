//! "What am I missing?" — and the answer is the thing you actually asked for.
//!
//! `wants::recommend` was built to turn a request Atlas couldn't carry out into
//! a concrete suggestion: *"you asked me to X and I couldn't — I'd want the
//! ability to X."* The mechanism was complete and proven, and its input was
//! always empty. The `Intent::Recommend` handler said so in a comment:
//! `unsupported_requests` "has no real source yet — nothing in the daemon
//! currently tracks it", so every "what am I missing?" answered from timings
//! and missing tools alone and never from what you had actually tried to do.
//!
//! `wants::asked_for_something_missing` is the method that records one. The
//! wiring: a turn that lands as an unanswerable `Intent::Unknown` — nothing in
//! your notes, no decision to help with, not an expression of feeling — is a
//! request Atlas has no way to do, so it is written onto `Daemon::wants_seen`,
//! and the Recommend handler reads it back. This proves the loop end to end,
//! through the daemon's own turn entrypoint.

use atlas::config::Config;
use atlas::daemon::{Autonomy, Daemon};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

const NOW: u64 = 1_700_000_000;

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-wants-seen-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}

#[test]
fn a_request_atlas_cannot_do_becomes_something_it_says_it_is_missing() {
    let c = cfg();
    let p = plat();
    // Unattended so the unknown request parks rather than leaving a pending
    // approval question in the way of the next turn. The recording happens
    // before that gate regardless, which is the point.
    let mut d = Daemon::new(
        &c,
        &p,
        None,
        Store::new(tmp("does")),
        Proactive::new(ProactiveConfig::default()),
    );
    d.autonomy = Autonomy::Unattended;

    // A plain request that matches no command and no note, so it lands as
    // Unknown and answers from nothing. Phrased as a question so it reads as a
    // request for something rather than as an ambiguous mood to check on.
    let _ = d.turn("will you juggle three chainsaws for me?", NOW);

    // Now ask what Atlas is missing. The reply must name the thing that was
    // asked for, sourced from the turn above rather than invented.
    let missing = d.turn("what am i missing", NOW + 1);
    assert!(
        missing.to_lowercase().contains("chainsaw"),
        "Recommend should surface the request Atlas couldn't do, got: {missing:?}"
    );
    assert!(
        missing.to_lowercase().contains("couldn't") || missing.to_lowercase().contains("want"),
        "and it should frame it as a want it hasn't got, got: {missing:?}"
    );
}

#[test]
fn a_stray_word_is_not_recorded_as_a_want() {
    let c = cfg();
    let p = plat();
    let mut d = Daemon::new(
        &c,
        &p,
        None,
        Store::new(tmp("stray")),
        Proactive::new(ProactiveConfig::default()),
    );
    d.autonomy = Autonomy::Unattended;

    // A single unrecognised token is a mishear, not a request for a
    // capability. It must not turn into a standing "you're missing this".
    let _ = d.turn("juggling?", NOW);
    let missing = d.turn("what am i missing", NOW + 1);
    assert!(
        !missing.to_lowercase().contains("juggling"),
        "a one-word mishear should not become a recommendation, got: {missing:?}"
    );
}
