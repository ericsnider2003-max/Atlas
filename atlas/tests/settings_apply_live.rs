//! A setting changed while Atlas runs applies without a restart.
//!
//! Eric, 24 Sep 2026, item 3 of the list: settings that apply live. Before
//! this, the daemon held the `Config` it was started with for its whole life,
//! so a switch moved in the settings window or the hub was kept on disk and
//! did nothing until the next start — and the window said so, and offered a
//! restart for every change.
//!
//! Now the running Atlas watches its two settings files and picks a change up
//! on the next tick or the next request. The few settings that set something
//! up at the start (`settings::NEEDS_A_RESTART`) still wait, and say so.
//!
//! These drive the real `Daemon`: write a preference the way the settings
//! window does, and check what the running daemon now reads.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::preferences::Preferences;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::settings;
use atlas::store::Store;
use std::fs;
use std::path::{Path, PathBuf};

fn fresh(dir: &Path) -> PathBuf {
    let _ = fs::remove_dir_all(dir);
    fs::create_dir_all(dir).unwrap();
    dir.to_path_buf()
}

/// A copy of the shipped config, so a test can change it under a running
/// daemon.
fn install(tag: &str) -> PathBuf {
    let dir = fresh(&std::env::temp_dir().join(format!("atlas-live-settings-{tag}-{}", std::process::id())));
    for f in ["tools.yaml", "apps.yaml", "layouts.yaml", "commands.yaml", "policy.yaml", "indexing.yaml"] {
        let from = Path::new("config").join(f);
        if from.is_file() {
            fs::copy(&from, dir.join(f)).unwrap();
        }
    }
    dir
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn store(tag: &str) -> Store {
    Store::new(fresh(&std::env::temp_dir().join(format!("atlas-live-store-{tag}-{}", std::process::id()))))
}

fn change(dir: &Path, key: &str, raw: &str) {
    let mut p = Preferences::load(dir);
    p.set(key, raw);
    p.save(dir).unwrap();
}

#[test]
fn a_switch_moved_while_atlas_runs_applies_without_a_restart() {
    let dir = install("switch");
    let cfg = Config::load(&dir).unwrap();
    let p = plat();
    let mut d = Daemon::new(&cfg, &p, None, store("switch"), Proactive::new(ProactiveConfig::default()))
        .watch_settings(dir.clone());
    assert!(d.tools_cfg().research.enabled, "the shipped config ships web lookup on");
    assert!(!d.proactive.cfg.enabled, "the shipped config ships speaking first off");

    change(&dir, "research.enabled", "off");
    change(&dir, "proactive.enabled", "on");
    let said = d.pick_up_settings();

    assert!(!d.tools_cfg().research.enabled, "a setting read at the moment of use didn't change");
    assert!(d.proactive.cfg.enabled, "the part that keeps its own copy of a setting wasn't refreshed");
    assert_eq!(said.len(), 2, "one line per changed setting, and only those: {said:?}");
    assert!(said.iter().any(|l| l == "Web research is now off."), "{said:?}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_tone_it_talks_in_changes_mid_run() {
    let dir = install("tone");
    let cfg = Config::load(&dir).unwrap();
    let p = plat();
    let mut d = Daemon::new(&cfg, &p, None, store("tone"), Proactive::new(ProactiveConfig::default()))
        .watch_settings(dir.clone());
    let before = format!("{:?}", d.persona.tone).to_lowercase();
    let other = if before == "warm" { "dry" } else { "warm" };

    change(&dir, "persona.tone", other);
    d.pick_up_settings();
    assert_eq!(format!("{:?}", d.persona.tone).to_lowercase(), other);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_next_tick_or_request_picks_it_up_on_its_own() {
    let dir = install("tick");
    let cfg = Config::load(&dir).unwrap();
    let p = plat();
    let mut d = Daemon::new(&cfg, &p, None, store("tick"), Proactive::new(ProactiveConfig::default()))
        .watch_settings(dir.clone());

    change(&dir, "research.enabled", "off");
    d.tick(1_700_000_000);
    assert!(!d.tools_cfg().research.enabled, "the heartbeat tick didn't look");

    // The voice loop and the typed prompt don't tick; a request looks instead.
    change(&dir, "research.enabled", "on");
    let intent = d.parser.parse("what time is it");
    d.execute_timed(&intent, "what time is it");
    assert!(d.tools_cfg().research.enabled, "a request didn't look");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn one_that_needs_a_restart_is_kept_and_says_so() {
    let dir = install("restart");
    let cfg = Config::load(&dir).unwrap();
    let p = plat();
    let mut d = Daemon::new(&cfg, &p, None, store("restart"), Proactive::new(ProactiveConfig::default()))
        .watch_settings(dir.clone());

    // 29 Sep 2026: the wake word ships on now (hands-free by default), so
    // the change that needs a restart is turning it off.
    change(&dir, "wake.enabled", "off");
    let said = d.pick_up_settings();
    assert_eq!(said, vec!["Wake word will be off when I next start.".to_string()]);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn nothing_changed_means_nothing_said_and_nothing_reloaded() {
    let dir = install("quiet");
    let cfg = Config::load(&dir).unwrap();
    let p = plat();
    let mut d = Daemon::new(&cfg, &p, None, store("quiet"), Proactive::new(ProactiveConfig::default()))
        .watch_settings(dir.clone());
    assert!(d.pick_up_settings().is_empty());
    assert!(d.pick_up_settings().is_empty());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_daemon_not_told_where_its_settings_are_watches_nothing() {
    // Every other test in the tree builds a daemon without `watch_settings`;
    // none of them may start reading some other folder's settings mid-test.
    let dir = install("unwatched");
    let cfg = Config::load(&dir).unwrap();
    let p = plat();
    let mut d = Daemon::new(&cfg, &p, None, store("unwatched"), Proactive::new(ProactiveConfig::default()));
    change(&dir, "research.enabled", "off");
    assert!(d.pick_up_settings().is_empty());
    assert!(d.tools_cfg().research.enabled);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_broken_settings_file_keeps_the_settings_it_had() {
    let dir = install("broken");
    let cfg = Config::load(&dir).unwrap();
    let p = plat();
    let mut d = Daemon::new(&cfg, &p, None, store("broken"), Proactive::new(ProactiveConfig::default()))
        .watch_settings(dir.clone());
    change(&dir, "research.enabled", "on");
    d.pick_up_settings();

    fs::write(dir.join("tools.yaml"), "enabled: [this is not\n").unwrap();
    let said = d.pick_up_settings();
    assert_eq!(said.len(), 1);
    assert!(said[0].starts_with("I couldn't read my changed settings"), "{said:?}");
    assert!(d.tools_cfg().research.enabled, "a broken file threw away the working settings");
    // Said once, not on every tick while it stays broken.
    assert!(d.pick_up_settings().is_empty());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn what_atlas_worked_out_at_the_start_survives_a_reload() {
    // The microphone the voice loop picks at start-up lives in `vars`, not in
    // any file. A reload that dropped it would leave the recorder and the
    // daemon disagreeing about which microphone is in use.
    let dir = install("mic");
    let mut cfg = Config::load(&dir).unwrap();
    cfg.tools.as_mut().unwrap().vars.insert("mic_device".into(), "Desk Mic".into());
    let p = plat();
    let mut d = Daemon::new(&cfg, &p, None, store("mic"), Proactive::new(ProactiveConfig::default()))
        .watch_settings(dir.clone());
    change(&dir, "research.enabled", "on");
    d.pick_up_settings();
    assert_eq!(d.tools_cfg().vars.get("mic_device").map(String::as_str), Some("Desk Mic"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn every_setting_that_waits_for_a_restart_is_a_real_setting() {
    let all = settings::registry(&Default::default());
    for key in settings::NEEDS_A_RESTART {
        assert!(all.get(key).is_some(), "{key} is listed as needing a restart and isn't a setting");
    }
    assert!(settings::needs_a_restart("wake.enabled"));
    assert!(!settings::needs_a_restart("research.enabled"));
}

#[test]
fn the_settings_window_says_when_a_change_takes_hold() {
    use atlas::settingswin::when_it_applies;
    assert_eq!(
        when_it_applies("research.enabled", "Web research is now on"),
        "Web research is now on. Atlas picks it up straight away."
    );
    assert_eq!(
        when_it_applies("wake.enabled", "Wake word is now on"),
        "Wake word is now on. That one takes effect when Atlas restarts."
    );
}

#[test]
fn differences_names_only_what_moved() {
    let a = settings::registry(&Default::default());
    let mut b = a.clone();
    b.set("research.enabled", "off").unwrap();
    let d = a.differences(&b);
    assert_eq!(d.iter().map(|s| s.key.as_str()).collect::<Vec<_>>(), vec!["research.enabled"]);
    assert!(a.differences(&a).is_empty());
}
