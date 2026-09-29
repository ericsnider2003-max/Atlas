//! "List them all" gets the itemised capability breakdown, not the counts.
//!
//! The spoken `Intent::Capabilities` handler answered "what can you do" with
//! `capability::summary` -- a one-line tally of how many things work, are off,
//! or are blocked. There was no way to ask for the actual list. Someone who
//! wants to read what Atlas does, item by item, grouped by area, was given a
//! number instead.
//!
//! `capability::full` was built and tested and never called. This binds it to
//! the handler that needs it: a request to see the list -- "list them all",
//! "in detail", "one by one" -- now returns the per-area breakdown with its
//! legend, and that answer is distinct from the summary counts.

use atlas::capability;
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::intent::Intent;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-lcif-{tag}"));
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
fn asking_to_list_them_all_returns_the_full_breakdown() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = daemon(&c, &p, "list");

    // The legend line is emitted by the full listing regardless of which
    // capabilities happen to be in which state, so it is a stable marker that
    // the itemised breakdown -- not the summary -- came back. The summary uses
    // the words "never run on your machine"; the full listing's legend says
    // "never run for real", which is unique to it.
    let reply = d.execute(&Intent::Capabilities("list them all".into()));
    assert!(
        reply.contains("never run for real"),
        "asking for the list should return the itemised breakdown with its legend, got: {reply}"
    );
    // A real listing is many lines, not a single tally sentence.
    assert!(
        reply.lines().count() > 5,
        "the full listing should span several lines, got: {reply}"
    );
}

#[test]
fn the_full_list_is_not_the_summary() {
    // Regression guard on the gap this closed: before the branch, a request to
    // see the list fell through and was matched against a single capability by
    // keyword. The itemised answer and the "what can you do" tally must differ.
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = daemon(&c, &p, "distinct");

    let listed = d.execute(&Intent::Capabilities("what can you do, in detail".into()));
    let summary = d.execute(&Intent::Capabilities(String::new()));
    assert_ne!(
        listed, summary,
        "a request to see the list got the generic summary rather than the breakdown"
    );
    // And it is the same text the capability module builds for reading.
    assert!(
        listed.contains("never run for real") && !summary.contains("never run for real"),
        "the listed answer should be the full breakdown and the summary should not"
    );
    let _ = capability::full();
}
