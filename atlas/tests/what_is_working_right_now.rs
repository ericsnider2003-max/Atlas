//! "What can you do right now" names the usable set, not the counts.
//!
//! The spoken `Intent::Capabilities` handler had two itemised answers and a
//! tally: `summary` for "what can you do" (counts), `full` for "list them all"
//! (the whole catalogue, blocked and off included), and a keyword match for
//! "can you read my email". The one question it had nothing for was the most
//! ordinary one -- "what can you do *right now*", "what's *working*" -- which
//! wants only the things usable this moment, by name. It fell through to the
//! keyword branch and came back "I don't have anything for that."
//!
//! `capability::working` is exactly that filter -- `all()` minus everything not
//! usable -- and it was built, tested and reached by nothing. This binds it to
//! the handler that needed it, and drives the whole thing through a real parser
//! and a real Daemon: the plain sentence routes to `Intent::Capabilities`, and
//! the reply is the usable list rather than the summary or the full catalogue.

use atlas::capability;
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::intent::{Intent, Parser};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-wiwrn-{tag}"));
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
fn the_plain_sentence_routes_to_capabilities() {
    let c = Config::load(Path::new("config")).unwrap();
    let parser = Parser::new(&c.commands);
    // The phrase "what can you do" is matched and the remainder -- "right now"
    // -- is carried into the intent, which is what the handler keys off.
    assert_eq!(
        parser.parse("what can you do right now"),
        Intent::Capabilities("right now".into()),
        "the ordinary spoken question must reach the capabilities handler"
    );
}

#[test]
fn asking_what_works_right_now_names_the_usable_set() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = daemon(&c, &p, "usable");

    let reply = d.execute(&Intent::Capabilities("right now".into()));

    // It leads with the count of usable things, then names them -- the same
    // list `working` builds. Every usable capability's `what` must appear.
    let working = capability::working();
    assert!(!working.is_empty(), "the tree should have some usable capabilities");
    assert!(
        reply.starts_with(&format!("{} things work right now:", working.len())),
        "the answer should lead with the usable count and a colon, got: {reply}"
    );
    for cap in &working {
        assert!(
            reply.contains(cap.what),
            "the usable capability {:?} should be named in the answer, got: {reply}",
            cap.what
        );
    }
}

#[test]
fn what_works_is_neither_the_summary_nor_the_full_list() {
    // The gap this closed: three questions, three answers, and this was the one
    // with no home. It must differ from both the tally ("what can you do") and
    // the whole catalogue ("list them all") -- the summary omits the names, the
    // full listing includes what is blocked or off, and this is only the usable
    // set, named.
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = daemon(&c, &p, "distinct");

    let works = d.execute(&Intent::Capabilities("what's working".into()));
    let summary = d.execute(&Intent::Capabilities(String::new()));
    let full = d.execute(&Intent::Capabilities("list them all".into()));

    assert_ne!(works, summary, "the working list must not be the summary tally");
    assert_ne!(works, full, "the working list must not be the full catalogue");
    // The full listing carries its own legend line; the working answer does not.
    assert!(
        !works.contains("never run for real"),
        "the working answer should not carry the full listing's legend, got: {works}"
    );
}
