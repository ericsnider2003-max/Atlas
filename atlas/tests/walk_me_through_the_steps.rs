//! "Walk me through freeing up memory" reads a procedure back as its steps.
//!
//! `knowhow::as_plan` turns a shipped procedure into something to actually
//! follow -- each step, with the "until" check folded in where it has one. It
//! was built, tested in `fit_knowhow.rs`, and reached by nothing in
//! production. The daemon already matched a task to a procedure in
//! `known_procedure` (the `Unknown` fallback), but only ever said
//! `announce` -- "I know this one, 3 steps" -- and stopped. So "I know how" was
//! never followed by "here's how".
//!
//! `Intent::WalkThrough` is the question that wanted it. Distinct from
//! `Diagnose`, which starts from a symptom and reads `for_symptom`: this starts
//! from a task, reads `for_request` + `as_plan`, and reads the steps out. It
//! ships compiled in, so it works with the network unplugged. The whole path is
//! driven through a real parser and a real Daemon with a plain-string utterance.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::intent::{Intent, Parser};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-walkthrough-{tag}"));
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
fn the_plain_sentence_routes_to_walk_through() {
    let c = Config::load(Path::new("config")).unwrap();
    let parser = Parser::new(&c.commands);
    assert_eq!(
        parser.parse("walk me through freeing up memory"),
        Intent::WalkThrough("freeing up memory".into()),
        "naming a task must reach the walk-through handler"
    );
}

#[test]
fn walk_through_reads_the_steps_out_through_the_daemon() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = daemon(&c, &p, "front-door");

    // A plain-string utterance, through the front door, so the real parser
    // routes it and the real handler answers. The spoken pass flattens the
    // numbered layout, so the front-door check is on the step text that
    // survives it, lower-cased; the numbered structure is asserted on
    // `execute` below, which returns the handler's own words.
    let reply = d.turn("walk me through freeing up memory", 100).to_lowercase();

    assert!(
        reply.contains("room when memory is tight"),
        "walk-through should name the procedure's goal, got: {reply}"
    );
    assert!(
        reply.contains("find what's holding memory"),
        "walk-through should read the first step out, got: {reply}"
    );
    // The `until` check is folded in by `as_plan`, which is the difference
    // between reading the steps and just listing their titles.
    assert!(
        reply.contains("(until something over 200mb and untouched)"),
        "walk-through should fold each step's check in, got: {reply}"
    );
}

#[test]
fn a_known_task_and_an_unknown_one_answer_differently() {
    // A contrast run: a task Atlas has a procedure for and one it does not must
    // give genuinely different answers, which is the difference between reading
    // `knowhow::as_plan` and printing one canned line either way.
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = daemon(&c, &p, "contrast");

    let known = d.execute(&Intent::WalkThrough("freeing up memory".into()));
    let unknown = d.execute(&Intent::WalkThrough(
        "the whatsit flurbled sideways and quibbled".into(),
    ));

    assert_ne!(
        known, unknown,
        "a matched task and an unmatched one must not give the same answer"
    );
    // A non-`.contains` assertion: the shipped procedure has exactly three
    // steps, so a real numbered plan carries the markers "1.", "2." and "3."
    // and no "4." A canned "I know this one" line would carry none of them.
    let numbered = |n: u32| known.contains(&format!("{n}. "));
    assert!(
        numbered(1) && numbered(2) && numbered(3) && !numbered(4),
        "the plan should number exactly the three steps this procedure has, got: {known}"
    );
    assert!(
        unknown.contains("don't have steps for"),
        "an unknown task should say so honestly rather than inventing steps, got: {unknown}"
    );
    // And the empty argument is turned away with a prompt, not an empty plan.
    let nothing = d.execute(&Intent::WalkThrough(String::new()));
    assert!(
        nothing.contains("Walk you through what?"),
        "an empty task should ask for one, got: {nothing}"
    );
}
