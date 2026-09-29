//! "How much do you know?" reads the size of the knowledge store back to you.
//!
//! `consolidate.rs` is built around one promise: the store's cost is value
//! density, not count, and it does not grow while you are not asking.
//! `consolidate::size_note` states that promise in numbers -- how many things
//! are known and roughly what they cost -- and its own doc says it is worth
//! stating "because the fear is that it grows without bound". Every caller of
//! it lived in a test; nothing a person could say reached it, so the
//! reassurance the module exists to give could never be asked for.
//!
//! `Intent::KnowledgeSize` is the question that wanted it: "how much do you
//! know", "how big is your memory". The whole thing is driven through a real
//! parser and a real Daemon -- the plain sentence routes to the intent, and
//! the answer is built from the store's actual count, not a fixed string.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::intent::{Intent, Parser};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-knowsize-{tag}"));
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
fn the_plain_sentence_routes_to_knowledge_size() {
    let c = Config::load(Path::new("config")).unwrap();
    let parser = Parser::new(&c.commands);
    assert_eq!(
        parser.parse("how much do you know"),
        Intent::KnowledgeSize,
        "the spoken sentence must reach the knowledge-size handler"
    );
    // And it is not swallowed by `what_i_have`, which owns the nearby
    // "what do you know about X" phrase -- that one opens a subject, this one
    // asks the size of the whole store.
    assert_eq!(
        parser.parse("how big is your memory"),
        Intent::KnowledgeSize,
        "asking how big the memory is is a size question, not a subject lookup"
    );
    assert_eq!(
        parser.parse("what do you know about the wash sale rule"),
        Intent::WhatIHave("the wash sale rule".into()),
        "a subject lookup must still reach what_i_have, not the size handler"
    );
}

#[test]
fn asking_the_size_reads_the_real_store_count_through_the_daemon() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = daemon(&c, &p, "front-door");

    // The plain sentence, driven through the real parser and daemon. A fresh
    // Atlas has researched nothing, so the store holds nothing -- and the
    // answer is the exact reassurance `size_note` gives for an empty store,
    // not a variant name or a canned line.
    let reply = d.turn("how much do you know", 100);
    assert!(
        reply.contains(&atlas::consolidate::size_note(0)),
        "the size answer must be built from size_note on the real count, got: {reply}"
    );
}

#[test]
fn the_answer_tracks_the_store_rather_than_a_fixed_string() {
    // A contrast run: the same intent gives genuinely different answers for an
    // empty store and one with something folded into it, which is the
    // difference between reading real state and printing a canned line.
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();

    let mut empty = daemon(&c, &p, "empty");
    let nothing = empty.execute(&Intent::KnowledgeSize);
    // Exact equality, not a substring: an empty store answers with size_note(0)
    // to the byte.
    assert_eq!(
        nothing,
        atlas::consolidate::size_note(0),
        "an empty store must answer with size_note(0) exactly"
    );

    // Fold one thing in through the real `learned` path -- the same seam a
    // finished research errand uses -- then ask again. `learned` returns false
    // for a brand-new claim (it wasn't a merge into an existing one), so the
    // signal that it landed is the store count, which is exactly what the
    // handler reads.
    let mut used = daemon(&c, &p, "used");
    used.learned("the wash sale rule bars a repurchase within thirty days", "test", 100);
    let filled = used.execute(&Intent::KnowledgeSize);

    assert_eq!(
        filled,
        atlas::consolidate::size_note(1),
        "a store with one claim must answer with size_note(1)"
    );
    assert_ne!(
        filled, nothing,
        "the size answer must move when the store grows, not return a fixed string"
    );
}
