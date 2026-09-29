//! "Recap our conversation" reads this session's turns back to you.
//!
//! `session::transcript` assembles the recent turns of a session as
//! `you: ...\natlas: ...` lines. Its own doc says it is "for handing to the
//! model", and nothing ever handed it anywhere -- the daemon builds model
//! context from `thread` instead, so the one function that turns the session's
//! own turns into a readable transcript was built, tested, and reached by
//! nothing.
//!
//! `Intent::Recap` is the question that wanted it: "recap our conversation",
//! "what have we been talking about". Distinct from `History`, which is the
//! log of what Atlas *did* across files, mail and settings; this is what was
//! said. The whole thing is driven through a real parser and a real Daemon:
//! the plain sentence routes to `Intent::Recap`, and the answer is built from
//! the turns actually taken this session -- not a fixed string.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::intent::{Intent, Parser};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-recap-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor {
        id: 1,
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
        primary: true,
    }])
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

#[test]
fn the_plain_sentence_routes_to_recap() {
    let c = Config::load(Path::new("config")).unwrap();
    let parser = Parser::new(&c.commands);
    assert_eq!(
        parser.parse("recap our conversation"),
        Intent::Recap,
        "the spoken sentence must reach the recap handler"
    );
    // And it is not swallowed by `history`, which owns nearby "what" phrases.
    assert_eq!(
        parser.parse("what have we been talking about"),
        Intent::Recap,
        "asking what we've been talking about is a recap, not the action log"
    );
}

#[test]
fn recap_reads_this_sessions_turns_back_through_the_daemon() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = daemon(&c, &p, "front-door");

    // An ordinary turn, through the front door, so the session records it.
    // Deliberately a capability question -- already driven through the daemon
    // elsewhere -- so this test adds no new coverage to any other intent.
    d.turn("what can you do", 100);

    // The plain sentence, driven through the real parser and daemon.
    let reply = d.turn("recap our conversation", 300);

    assert!(
        reply.contains("Here's our conversation so far"),
        "recap should lead with the conversation, got: {reply}"
    );
    assert!(
        reply.contains("what can you do"),
        "recap should read back what you said earlier, got: {reply}"
    );
    // The current recap turn is recorded only after the handler returns, so it
    // never appears inside its own answer.
    assert!(
        !reply.contains("recap our conversation"),
        "the recap turn should not be inside its own transcript, got: {reply}"
    );
}

#[test]
fn recap_reflects_the_session_rather_than_a_fixed_string() {
    // A contrast run: the same intent gives genuinely different answers for an
    // empty session and one with a turn in it, which is the difference between
    // reading real state and printing a canned line.
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();

    let mut empty = daemon(&c, &p, "empty");
    let nothing = empty.execute(&Intent::Recap);
    assert_eq!(
        nothing, "We haven't said anything yet this session.",
        "an empty session has nothing to recap"
    );

    let mut used = daemon(&c, &p, "used");
    used.turn("open chrome", 100);
    let filled = used.execute(&Intent::Recap);

    assert_ne!(
        filled, nothing,
        "recap must reflect what happened this session, not return a fixed string"
    );
    assert!(
        filled.contains("open chrome"),
        "recap of a session that opened chrome should say so, got: {filled}"
    );
}
