//! The app-permission gate, end to end.
//!
//! `Daemon.permissions` was constructed empty and referenced nowhere — the
//! whole gate was dead. An unknown app was never asked about, and an
//! "always allow X" could not have been remembered because nothing was ever
//! written or read. These drive the daemon and check the gate actually gates.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-grants-{tag}"));
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

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

#[test]
fn a_configured_app_is_not_gated() {
    // The gate is not a nag. An app Atlas already knows (chrome is in the
    // shipped apps.yaml) opens without a permission question.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "known");
    let reply = d.turn("open chrome", 100);
    assert!(
        !reply.to_lowercase().contains("i don't know"),
        "a configured app should not be questioned: {reply}"
    );
}

#[test]
fn an_unknown_app_is_asked_about_before_use() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "unknown");
    let reply = d.turn("open frobnicator", 100);
    assert!(
        reply.to_lowercase().contains("don't know") || reply.to_lowercase().contains("use it"),
        "an unknown app must be asked about, got: {reply}"
    );
}

#[test]
fn yes_always_records_a_standing_grant_that_survives_a_restart() {
    let dir = tmp("always");
    let c = cfg();
    let p = plat();
    {
        let mut d = Daemon::new(&c, &p, None, Store::new(dir.clone()), Proactive::new(ProactiveConfig::default()));
        d.turn("open frobnicator", 100); // parks the question
        let reply = d.turn("yes, always", 101); // grant Always + act
        assert!(
            !reply.to_lowercase().contains("don't know"),
            "after 'yes always' it should act, not re-ask: {reply}"
        );
        d.persist();
    }
    // A brand-new daemon on the same store: the Always grant survived, so the
    // app is no longer questioned.
    let d2 = Daemon::new(&c, &p, None, Store::new(dir), Proactive::new(ProactiveConfig::default()));
    assert!(
        d2.permissions.granted_apps().iter().any(|a| a.eq_ignore_ascii_case("frobnicator")),
        "an 'always' grant must survive a restart"
    );
}

#[test]
fn a_session_grant_does_not_survive_a_restart() {
    let dir = tmp("session");
    let c = cfg();
    let p = plat();
    {
        let mut d = Daemon::new(&c, &p, None, Store::new(dir.clone()), Proactive::new(ProactiveConfig::default()));
        d.turn("open frobnicator", 100);
        d.turn("yes, just this session", 101);
        d.persist();
    }
    let d2 = Daemon::new(&c, &p, None, Store::new(dir), Proactive::new(ProactiveConfig::default()));
    assert!(
        !d2.permissions.granted_apps().iter().any(|a| a.eq_ignore_ascii_case("frobnicator")),
        "a session grant must be dropped on restart — only 'always' survives"
    );
}

#[test]
fn naming_an_app_to_use_grants_it_for_the_task() {
    // "use frobnicator to ..." — naming the tool IS the permission, so the
    // gate does not then ask about it.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "instruction");
    d.turn("use frobnicator to sort my files", 100);
    // The grant is recorded even though the instruction wasn't an OpenApp.
    let apps = d.permissions.granted_apps();
    assert!(
        apps.iter().any(|a| a.eq_ignore_ascii_case("frobnicator")),
        "naming a tool in an instruction should grant it: {apps:?}"
    );
}

#[test]
fn a_refusal_does_not_grant() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "refusal");
    d.turn("don't use frobnicator for anything", 100);
    assert!(
        d.permissions.granted_apps().is_empty(),
        "a refusal must grant nothing"
    );
}
