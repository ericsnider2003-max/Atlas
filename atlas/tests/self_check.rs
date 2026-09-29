//! The fast, on-device self-check.
//!
//! The thousands of development tests are a build-machine gate, not something a
//! person runs to use Atlas. What a fresh install needs is this: a few fast
//! probes of the parts everything rests on, and an honest count of what's ready
//! on this machine. These prove the check runs end to end through a real daemon
//! and reports what it found.

use atlas::checkup::{self, Check, Outcome};
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn dir(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-selfcheck-{tag}-{}", std::process::id()));
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

// --- the report ------------------------------------------------------------

#[test]
fn a_clean_check_reads_as_passed() {
    let checks = vec![Check::pass("saving state"), Check::note("memory", "3 facts remembered")];
    assert!(checkup::all_clear(&checks), "no failures means all clear");
    let report = checkup::report(&checks);
    assert!(report.to_lowercase().contains("passed"), "leads with the verdict: {report}");
}

#[test]
fn a_failure_is_counted_and_named() {
    let checks = vec![
        Check::pass("saving state"),
        Check::fail("displays", "no screen found"),
    ];
    assert!(!checkup::all_clear(&checks), "a failure is not all clear");
    let report = checkup::report(&checks);
    assert!(report.contains("1 problem"), "the count is surfaced: {report}");
    assert!(report.contains("no screen found"), "the reason is surfaced: {report}");
}

#[test]
fn a_note_is_neither_pass_nor_fail() {
    let c = Check::note("model", "configured, on this machine");
    assert!(!c.failed(), "a note is informational, not a failure");
    assert_eq!(c.outcome, Outcome::Note("configured, on this machine".into()));
}

// --- end to end through the daemon -----------------------------------------

#[test]
fn asking_atlas_to_check_itself_runs_the_probes() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(dir("run")), Proactive::new(ProactiveConfig::default()));
    let reply = d.turn("check yourself", 100);
    // The store probe ran and passed on a working store.
    assert!(
        reply.to_lowercase().contains("saving and reading state"),
        "it actually probed the store: {reply}"
    );
    // It reports the honest completion picture, and it saw the one display.
    assert!(reply.to_lowercase().contains("what's ready"), "it reports what's ready here: {reply}");
    assert!(reply.contains("1 screen"), "it saw the one display: {reply}");
    // It's a real answer, not the catch-all miss.
    assert_ne!(reply.to_lowercase(), "i don't have anything for that.", "the phrase routed: {reply}");
}

#[test]
fn the_self_check_is_a_read_it_doesnt_need_approval() {
    // A self-check writes only its own probe key; it should answer directly, not
    // park behind a "go ahead?" the way an action does.
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(dir("read")), Proactive::new(ProactiveConfig::default()));
    let reply = d.turn("run a self check", 100);
    assert!(
        reply.to_lowercase().contains("self-check"),
        "it ran the check rather than asking permission: {reply}"
    );
}
