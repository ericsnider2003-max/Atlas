//! "What can you do on your own?" reads the autonomy ledger back to you.
//!
//! `earned::Record` scores how often each kind of work turns out right and
//! decides, per kind, whether Atlas may act alone (`may_act_alone`) and how
//! much rope it has earned (`rope`). It is written on every turn -- `note`
//! after a run, `taken_back` after an undo -- and read back by nothing a
//! person could reach: `how_am_i_doing` reports *corrections*, not trust. So
//! the two methods that decide whether Atlas acts on its own were built,
//! proven by their own tests, and answerable by no question.
//!
//! `Intent::ActAlone` is that question: "what can you do on your own", "where
//! do you still ask me first". Driven here through a real parser and a real
//! Daemon: the plain sentence routes to the intent, and the answer is built
//! from the record actually seeded -- an empty record and one with a clean run
//! give genuinely different replies, which is the difference between reading
//! state and printing a canned line.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::earned::Kind;
use atlas::intent::{Intent, Parser};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-act-alone-{tag}"));
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
fn the_plain_sentence_routes_to_act_alone() {
    let c = Config::load(Path::new("config")).unwrap();
    let parser = Parser::new(&c.commands);
    assert_eq!(
        parser.parse("what can you do on your own"),
        Intent::ActAlone,
        "asking what Atlas does unattended must reach the autonomy handler"
    );
    assert_eq!(
        parser.parse("where do you still ask me first"),
        Intent::ActAlone,
        "the other side of the same question is still the autonomy handler"
    );
    // The longer, more specific phrase must win over `capabilities`, which owns
    // the shorter "what can you do". If it lost, this would come back as a
    // capabilities question with the tail as its argument.
    assert_ne!(
        parser.parse("what can you do"),
        Intent::ActAlone,
        "the bare capabilities phrase is not the autonomy question"
    );
}

#[test]
fn act_alone_reads_the_earned_record_through_the_daemon() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = daemon(&c, &p, "front-door");

    // A clean run of a low-cost kind: eight right answers is enough to earn the
    // top rope for `Answering`, whose cost of being wrong is lowest.
    for i in 0..8 {
        d.earned.note(Kind::Answering, true, "answered something", 100 + i);
    }

    // The plain sentence, driven through the real parser and daemon.
    let reply = d.turn("what can you do on your own", 300);

    assert!(
        reply.contains("Answering"),
        "the kind that earned its rope should be named, got: {reply}"
    );
    assert!(
        reply.contains("gets on with it"),
        "a kind with a clean run should read as self-acting, got: {reply}"
    );
    assert!(
        reply.contains("Still checking with you first"),
        "the kinds with no record yet must still be listed as asking first, got: {reply}"
    );
}

#[test]
fn act_alone_reflects_the_record_rather_than_a_fixed_string() {
    // A contrast run: the same intent gives a genuinely different answer for an
    // empty record and one with a clean run in it. That is the difference
    // between reading the live ledger and printing a canned line.
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();

    let mut fresh = daemon(&c, &p, "fresh");
    let nothing = fresh.execute(&Intent::ActAlone);
    assert!(
        nothing.contains("check with you first about everything"),
        "a record with no runs in it earns nothing, got: {nothing}"
    );

    let mut earned = daemon(&c, &p, "earned");
    for i in 0..8 {
        earned.earned.note(Kind::Answering, true, "answered something", 100 + i);
    }
    let filled = earned.execute(&Intent::ActAlone);

    assert_ne!(
        filled, nothing,
        "the autonomy answer must reflect the record, not return a fixed string"
    );
    assert!(
        filled.contains("What I'll do on my own"),
        "a record that earned some rope should lead with what Atlas will do alone, got: {filled}"
    );
    // Money and secrets can never be earned into acting alone, so the full
    // answer must still list the sensitive kind on the asking side however
    // clean the rest of the record reads.
    assert!(
        filled.contains("Money and secrets"),
        "the sensitive kind must always be listed as asking first, got: {filled}"
    );
}
