//! Your after-me arrangement is read back when you ask, and never announced.
//!
//! Eric, 24 Sep 2026: "Not sure I want it to announce where it's at unless
//! asked." Atlas holds no passphrase — only the kind of place and who was
//! told — and says even that only in answer to you.

use atlas::afterme::{Arrangement, Where, RECORD};
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::intent::Intent;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::Path;

#[test]
fn asked_it_says_what_you_arranged() {
    let cfg = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let dir = std::env::temp_dir().join(format!("atlas-envelope-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::new(dir.clone());
    let a = Arrangement { kind: Some(Where::YoursAlone), ..Default::default() };
    store.save(RECORD, &a).unwrap();
    let mut d = Daemon::new(&cfg, &p, None, store, Proactive::new(ProactiveConfig::default()));
    let said = d.execute_timed(&Intent::AfterMe, "where's my envelope");
    assert!(said.starts_with("The envelope:"), "{said}");
    assert!(!said.to_lowercase().contains("passphrase is"), "{said}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_question_reaches_it() {
    let cfg = Config::load(Path::new("config")).unwrap();
    let p = atlas::intent::Parser::new(&cfg.commands);
    assert_eq!(p.parse("where's my envelope"), Intent::AfterMe);
}

#[test]
fn it_is_never_said_unasked() {
    // The only unasked line about it is the review reminder, which names no
    // place: "It's been N days since you checked the envelope arrangement".
    let src = crate::common::source_of("daemon");
    let unasked: Vec<&str> = src.lines().filter(|l| l.contains(".spoken(&") && l.contains("after_me")).collect();
    assert_eq!(unasked.len(), 1, "the arrangement is read out somewhere other than when asked: {unasked:?}");
}
