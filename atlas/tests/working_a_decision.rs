//! A decision, worked instead of answered.
//!
//! `decide.rs` was written on the principle that an assistant should not just
//! pick for you -- it should work the decision, and the first move of working
//! one is naming the question underneath. `daemon::work_a_decision` was the
//! method that did this, and for weeks nothing reached it: a line like "should
//! I take the contract or keep the retainer" is not a command, so it fell
//! through to the unrecognised path and came back as "I didn't catch that".
//!
//! This drives the wire that fixed that. A deciding phrase in an otherwise
//! unrecognised line now reaches `work_a_decision` through `decision_help`,
//! and the reply is the first move rather than a shrug.
//!
//! What it can prove: the daemon turns a deciding sentence into the framing
//! move, and the cheap-and-reversible branch says so and stops. What it
//! cannot: whether the framing is the *right* question -- that is judgement,
//! and `decide.rs` deliberately refuses to fake it.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

/// Words that name a winner. Checked against the laid-out text, because the
/// refusal is the product: an assistant that says "I'd go with the second one"
/// has decided, whatever it adds afterwards about it being your call.
///
/// Lived in `src/decide.rs` as `names_a_winner` until 25 Sep 2026; nothing in
/// production called it, so it is kept in the tests that ask (this file and tests/decide.rs).
fn names_a_winner(text: &str) -> bool {
    const RECOMMENDS: &[&str] = &[
        "i'd go with", "i would go with", "i recommend", "my recommendation",
        "the best option", "the right choice", "you should pick", "you should choose",
        "go with the", "the winner", "clearly better", "obviously better",
        "i'd pick", "i would pick", "the answer is", "definitely the",
    ];
    let lower = text.to_lowercase();
    RECOMMENDS.iter().any(|r| lower.contains(r))
}

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-decide-{tag}"));
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
fn a_deciding_line_is_worked_not_shrugged_off() {
    let c = cfg();
    let p = plat();
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("worked")), Proactive::new(ProactiveConfig::default()));

    // Not a command, so it lands as an unrecognised line -- exactly the door
    // work_a_decision's own doc pointed at.
    let reply = d.turn("should I take the contract or keep the retainer", 1_000);

    // The first move of working a decision is naming the question underneath,
    // not picking one. Match the framing question's own words.
    assert!(
        reply.to_lowercase().contains("question you should be asking"),
        "a deciding line should be worked into the framing move, got: {reply}"
    );
    // And it must not have quietly decided for you.
    assert!(
        !names_a_winner(&reply),
        "working a decision must not name a winner, got: {reply}"
    );
}

#[test]
fn a_cheap_reversible_decision_is_told_to_just_pick() {
    let c = cfg();
    let p = plat();
    let d = Daemon::new(&c, &p, None, Store::new(tmp("cheap")), Proactive::new(ProactiveConfig::default()));

    // The one exception the module keeps: a cheap, reversible choice is not
    // worth an evening of analysis, and saying so is the useful answer.
    let reply = d.work_a_decision("which phone case to buy", true);
    assert!(
        reply.to_lowercase().contains("cheap") && reply.to_lowercase().contains("pick one"),
        "a cheap reversible decision should be told to just pick, got: {reply}"
    );
}

#[test]
fn an_ordinary_line_is_not_mistaken_for_a_decision() {
    // The detector is narrow on purpose: an ordinary question must not be
    // answered with "what's the question underneath".
    assert!(!atlas::decide::wants_working("what time is it in Tokyo"));
    assert!(!atlas::decide::wants_working("remind me to call the plumber"));
    assert!(atlas::decide::wants_working("should I take the job or stay put"));
    assert!(atlas::decide::wants_working("help me decide whether to hire now"));
}
