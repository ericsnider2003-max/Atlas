use atlas::awareness::Signals;
use atlas::config::Config;
use atlas::index::Changes;
use atlas::input::{Tier, Tiers};
use atlas::log::Log;
use atlas::perf::{PerfConfig, Power, Throttle};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::probe::{app_named, target_for, Probe, Target};
use std::cell::Cell;
use std::path::Path;

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}
fn mon(id: u32, x: i32, primary: bool) -> Monitor {
    Monitor { id, x, y: 0, width: 1920, height: 1040, primary }
}

// ================= three tiers of input =================

#[test]
fn voice_is_the_default_and_survives_the_occasional_failure() {
    let mut t = Tiers::default();
    assert_eq!(t.tier, Tier::Voice);
    assert!(t.failed().is_none());
    assert!(t.failed().is_none(), "two misses is a noisy room, not a broken mic");
    assert_eq!(t.tier, Tier::Voice);
}

#[test]
fn repeated_voice_failure_drops_to_push_to_talk_and_says_so() {
    let mut t = Tiers::default();
    t.failed();
    t.failed();
    let msg = t.failed().expect("should announce the switch");
    assert_eq!(t.tier, Tier::PushToTalk);
    assert!(msg.contains("push-to-talk"), "got: {msg}");
}

#[test]
fn repeated_push_to_talk_failure_drops_to_typing() {
    let mut t = Tiers::default();
    for _ in 0..3 {
        t.failed();
    }
    for _ in 0..3 {
        t.failed();
    }
    assert_eq!(t.tier, Tier::Typed);
}

#[test]
fn typing_is_the_floor_and_never_degrades_further() {
    let mut t = Tiers::default();
    for _ in 0..30 {
        t.failed();
    }
    assert_eq!(t.tier, Tier::Typed);
    assert!(t.failed().is_none(), "nothing below typing to fall to");
}

#[test]
fn a_temporarily_noisy_room_does_not_demote_you_permanently() {
    let mut t = Tiers::default();
    for _ in 0..3 {
        t.failed();
    }
    assert_eq!(t.tier, Tier::PushToTalk);
    for _ in 0..4 {
        assert!(t.succeeded().is_none());
    }
    let msg = t.succeeded().expect("should climb back");
    assert_eq!(t.tier, Tier::Voice);
    assert!(msg.contains("wake word"), "got: {msg}");
}

#[test]
fn one_success_resets_the_failure_run() {
    let mut t = Tiers::default();
    t.failed();
    t.failed();
    t.succeeded();
    assert!(t.failed().is_none(), "counter restarted");
    assert_eq!(t.tier, Tier::Voice);
}

#[test]
fn broken_audio_skips_straight_to_typing_instead_of_failing_six_times() {
    let mut t = Tiers::default();
    let msg = t.audio_unavailable().expect("should announce");
    assert_eq!(t.tier, Tier::Typed);
    assert!(msg.contains("typing"), "got: {msg}");
    assert!(t.audio_unavailable().is_none(), "already there, stay quiet");
}

// ================= staying out of the way =================

fn idle() -> Signals {
    Signals { active: None, dwell_secs: 600, idle_secs: 600, recent_changes: Changes::default(), in_conversation: false, ..Default::default() }
}
fn active() -> Signals {
    Signals { dwell_secs: 1, idle_secs: 1, ..idle() }
}

#[test]
fn idle_polling_backs_off_toward_once_a_minute() {
    let mut th = Throttle::new(PerfConfig::default());
    let mut last = 0;
    for _ in 0..20 {
        last = th.next_interval(&idle(), Power::default());
    }
    assert_eq!(last, 60, "settles at the idle ceiling");
}

#[test]
fn it_snaps_back_to_responsive_the_moment_you_do_something() {
    let mut th = Throttle::new(PerfConfig::default());
    for _ in 0..20 {
        th.next_interval(&idle(), Power::default());
    }
    assert_eq!(th.next_interval(&active(), Power::default()), 2, "instantly attentive again");
}

#[test]
fn a_conversation_keeps_it_fast_even_without_other_activity() {
    let mut th = Throttle::new(PerfConfig::default());
    for _ in 0..20 {
        th.next_interval(&idle(), Power::default());
    }
    let mut s = idle();
    s.in_conversation = true;
    assert_eq!(th.next_interval(&s, Power::default()), 2);
}

#[test]
fn new_files_count_as_activity() {
    let mut th = Throttle::new(PerfConfig::default());
    for _ in 0..20 {
        th.next_interval(&idle(), Power::default());
    }
    let mut s = idle();
    s.recent_changes = Changes { added: vec!["/x".into()], ..Default::default() };
    assert_eq!(th.next_interval(&s, Power::default()), 2);
}

#[test]
fn battery_stretches_every_interval() {
    let mut a = Throttle::new(PerfConfig::default());
    let mut b = Throttle::new(PerfConfig::default());
    let plugged = a.next_interval(&active(), Power { on_battery: false, percent: 100 });
    let unplugged = b.next_interval(&active(), Power { on_battery: true, percent: 80 });
    assert!(unplugged > plugged, "{unplugged} should exceed {plugged}");
}

#[test]
fn indexing_stops_entirely_on_a_low_battery() {
    let th = Throttle::new(PerfConfig::default());
    assert!(th.may_scan(Power { on_battery: false, percent: 5 }, 100));
    assert!(!th.may_scan(Power { on_battery: true, percent: 10 }, 100));
}

#[test]
fn an_oversized_index_stops_rescanning_rather_than_thrashing() {
    let th = Throttle::new(PerfConfig::default());
    assert!(!th.may_scan(Power::default(), 500_000));
}

// ================= gathering context by moving windows =================

fn plat3() -> MockPlatform {
    MockPlatform::new(vec![mon(1, 0, true), mon(2, -1920, false), mon(3, 1920, false)])
}

#[test]
fn probing_the_active_window_does_not_move_anything() {
    let (c, p) = (cfg(), plat3());
    p.focus_on("chrome.exe", "some page");
    let shots = Cell::new(0);
    let shoot = || {
        shots.set(shots.get() + 1);
        Ok("/tmp/a.png".to_string())
    };
    let out = Probe::default().gather(&c, &p, &shoot, &Target::Active).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(shots.get(), 1);
    assert!(p.actions().is_empty(), "nothing was launched or focused");
}

#[test]
fn probing_another_app_brings_it_forward_then_puts_you_back() {
    use atlas::platform::mock::Action;
    let (c, p) = (cfg(), plat3());
    atlas::workspace::workspace_on(&c, &p).unwrap();
    p.focus_on("chrome.exe", "where you were");
    let shoot = || Ok("/tmp/a.png".to_string());

    let out = Probe::default()
        .gather(&c, &p, &shoot, &Target::App("discord".into()))
        .unwrap();
    assert_eq!(out[0].app, "discord");

    let focuses: Vec<String> = p.actions().iter().filter_map(|a| match a {
        Action::Focus(n) => Some(n.clone()),
        _ => None,
    }).collect();
    assert_eq!(focuses, vec!["discord.exe", "chrome.exe"], "went and looked, then came back");
}

#[test]
fn probing_the_whole_workspace_visits_every_running_app() {
    let (c, p) = (cfg(), plat3());
    atlas::workspace::workspace_on(&c, &p).unwrap();
    let shoot = || Ok("/tmp/a.png".to_string());
    let out = Probe::default().gather(&c, &p, &shoot, &Target::Workspace).unwrap();
    assert_eq!(out.len(), 4, "one capture per open app");
}

#[test]
fn focus_is_restored_even_when_a_capture_fails() {
    use atlas::platform::mock::Action;
    let (c, p) = (cfg(), plat3());
    atlas::workspace::workspace_on(&c, &p).unwrap();
    p.focus_on("chrome.exe", "where you were");
    let shoot = || Err(atlas::error::AtlasError::Platform("screenshot died".into()));

    let r = Probe::default().gather(&c, &p, &shoot, &Target::App("discord".into()));
    assert!(r.is_err());
    let last_focus = p.actions().iter().rev().find_map(|a| match a {
        Action::Focus(n) => Some(n.clone()),
        _ => None,
    });
    assert_eq!(last_focus.as_deref(), Some("chrome.exe"), "losing your place is worse");
}

#[test]
fn a_probe_only_reads_it_never_clicks_or_types() {
    use atlas::platform::mock::Action;
    let (c, p) = (cfg(), plat3());
    atlas::workspace::workspace_on(&c, &p).unwrap();
    let before = p.actions().len();
    let shoot = || Ok("/tmp/a.png".to_string());
    Probe::default().gather(&c, &p, &shoot, &Target::Workspace).unwrap();
    let all = p.actions();
    let after: Vec<&Action> = all[before..].iter().collect();
    assert!(
        after.iter().all(|a| matches!(a, Action::Focus(_) | Action::Sleep(_))),
        "probe must only focus and wait: {after:?}"
    );
}

#[test]
fn asking_about_a_named_app_targets_that_app() {
    let c = cfg();
    assert_eq!(target_for("what does discord say", &c), Target::App("discord".into()));
    assert_eq!(target_for("summarize my whole workspace", &c), Target::Workspace);
    assert_eq!(target_for("what am I looking at", &c), Target::Active);
}

#[test]
fn processes_map_back_to_configured_app_names() {
    let c = cfg();
    assert_eq!(app_named(&c, "Discord.exe").as_deref(), Some("discord"));
    assert_eq!(app_named(&c, "notepad++.exe"), None);
}

// ================= bounded logging =================

#[test]
fn logs_rotate_instead_of_filling_the_disk() {
    let d = std::env::temp_dir().join("atlas-log-test");
    let _ = std::fs::remove_dir_all(&d);
    let l = Log::new(&d, 512);
    for i in 0..200 {
        l.info(&format!("line number {i} with some padding to grow the file"));
    }
    assert!(l.size() < 512 * 2, "current file stayed bounded: {}", l.size());
    assert!(d.join("atlas.log.1").exists(), "one previous file is kept");
    let count = std::fs::read_dir(&d).unwrap().count();
    assert_eq!(count, 2, "exactly two files, ever");
}
