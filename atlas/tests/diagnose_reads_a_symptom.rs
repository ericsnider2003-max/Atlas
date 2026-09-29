//! "Troubleshoot the app that closes itself" reads a symptom back as a cause.
//!
//! `knowhow::for_symptom` scores what you describe against every shipped
//! procedure's *snags* -- the things that usually go wrong -- and returns the
//! matching snag with its likely cause and fix. Its sibling `for_request`
//! (which answers "how do I do X") was wired into the daemon as
//! `known_procedure`; `for_symptom` was built, tested in `fit_knowhow.rs`, and
//! reached by nothing in production. The daemon only ever asked "how do I do
//! X", never "here's what went wrong".
//!
//! `Intent::Diagnose` is the question that wanted it. Distinct from
//! `MachineHealth`, which reads the live vitals: this starts from a symptom
//! the person names and matches it against knowledge that ships compiled in,
//! so it works with the network unplugged. The whole path is driven through a
//! real parser and a real Daemon with a plain-string utterance.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::intent::{Intent, Parser};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-diagnose-{tag}"));
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
fn the_plain_sentence_routes_to_diagnose() {
    let c = Config::load(Path::new("config")).unwrap();
    let parser = Parser::new(&c.commands);
    assert_eq!(
        parser.parse("troubleshoot an app that launches and closes immediately"),
        Intent::Diagnose("an app that launches and closes immediately".into()),
        "a described symptom must reach the diagnose handler"
    );
}

#[test]
fn diagnose_names_the_cause_and_fix_through_the_daemon() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = daemon(&c, &p, "front-door");

    // A plain-string utterance, through the front door, so the real parser
    // routes it and the real handler answers.
    let reply = d.turn("troubleshoot an app that launches and closes immediately", 100);

    assert!(
        reply.contains("wait two seconds and look for the real window"),
        "diagnose should hand back the known fix for this snag, got: {reply}"
    );
    assert!(
        reply.contains("updater stub"),
        "diagnose should name the likely cause, got: {reply}"
    );
}

#[test]
fn diagnose_distinguishes_a_known_snag_from_an_unknown_one() {
    // A contrast run: a symptom Atlas has a snag for and one it does not must
    // give genuinely different answers, which is the difference between reading
    // the shipped procedures and printing one canned line either way.
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = daemon(&c, &p, "contrast");

    let known = d.execute(&Intent::Diagnose(
        "it launches and closes immediately".into(),
    ));
    let unknown = d.execute(&Intent::Diagnose(
        "the whatsit flurbled sideways and quibbled".into(),
    ));

    assert_ne!(
        known, unknown,
        "a matched symptom and an unmatched one must not give the same answer"
    );
    assert!(
        unknown.contains("can't match"),
        "an unrecognised symptom should say so honestly rather than inventing a cause, got: {unknown}"
    );
}
