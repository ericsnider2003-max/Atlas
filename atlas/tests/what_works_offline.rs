//! "What works offline?" is answered with a count, not a guess.
//!
//! Offline-first is the primary promise of this tree, so asking Atlas what it
//! can do without the internet is a first-class question. The spoken
//! `Intent::Capabilities` handler had branches for "what's new" and "what are
//! you waiting on" but none for offline, so a query like "can you work
//! offline" fell through to the catch-all and was matched against a single
//! capability by keyword — the wrong shape of answer entirely.
//!
//! `capability::offline_count` was built and tested and never called. This
//! binds it to the handler that needs it: the answer must be the count of
//! capabilities that survive the network being unplugged.

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
    let p = std::env::temp_dir().join(format!("atlas-wwo-{tag}"));
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
fn asking_what_works_offline_answers_with_the_offline_count() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = daemon(&c, &p, "count");

    let (offline, total) = capability::offline_count();
    // The catalogue is not empty and not everything leaves the machine, so the
    // fixture is a real one, not a degenerate 0-of-0 that any string passes.
    assert!(offline > 0 && offline <= total && total > 0);

    let reply = d.execute(&Intent::Capabilities("without internet".into()));
    assert!(
        reply.contains(&offline.to_string()) && reply.contains(&total.to_string()),
        "offline question should report {offline} of {total}, got: {reply}"
    );
    assert!(
        reply.to_lowercase().contains("unplug") || reply.to_lowercase().contains("network"),
        "the answer should be about working without the network, got: {reply}"
    );
}

#[test]
fn the_offline_branch_is_not_the_generic_capability_summary() {
    // Regression guard on the gap this closed: before the offline branch, an
    // offline query fell through and produced something else entirely. The
    // offline answer and the plain "what can you do" answer must differ.
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = daemon(&c, &p, "distinct");

    let offline = d.execute(&Intent::Capabilities("can you work offline".into()));
    let summary = d.execute(&Intent::Capabilities(String::new()));
    assert_ne!(
        offline, summary,
        "an offline question got the generic summary rather than the offline count"
    );
}
