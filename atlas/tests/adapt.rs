use atlas::adapt::{
    apply, detect, first_run_message, leaks_a_username, monitor_fixture, AppFound, Machine,
};
use atlas::config::Config;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use std::path::{Path, PathBuf};

fn plat() -> MockPlatform {
    MockPlatform::new(vec![
        Monitor { id: 66629, x: 0, y: 0, width: 2560, height: 1392, primary: true },
        Monitor { id: 65677, x: 2560, y: 0, width: 2560, height: 1392, primary: false },
    ])
}

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-adapt-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

// ================= nothing shipped is personal =================

#[test]
fn no_shipped_config_contains_anyones_home_directory() {
    // The check that stops one person's paths reaching a friend. If this ever
    // fails, someone hard-coded a machine into the generic layer.
    for f in ["apps.yaml", "indexing.yaml", "layouts.yaml", "commands.yaml", "tools.yaml", "policy.yaml"] {
        let text = std::fs::read_to_string(Path::new("config").join(f)).unwrap();
        assert_eq!(
            leaks_a_username(&text),
            None,
            "config/{f} still contains a username — use %USERPROFILE% instead"
        );
    }
}

#[test]
fn a_personal_path_is_detected_and_the_generic_form_is_not() {
    assert_eq!(leaks_a_username("C:/Users/erics/Desktop"), Some("erics".into()));
    assert_eq!(leaks_a_username(r"C:\Users\Someone\AppData"), Some("someone".into()));
    assert_eq!(leaks_a_username("%USERPROFILE%/Desktop"), None);
    assert_eq!(leaks_a_username("C:/Users/Public/Documents"), None, "Public is on every machine");
}

#[test]
fn the_dry_run_fixture_carries_nobodys_real_monitor_ids() {
    let src = crate::common::source_of("main");
    let fixture = src.split("fn fake_monitors").nth(1).unwrap();
    for real in ["1247003", "106696887", "66629", "65677", "1508383", "17892425"] {
        assert!(!fixture.contains(real), "a real monitor id leaked into the fixture: {real}");
    }
}

// ================= working out where it is =================

fn found(name: &str) -> Option<AppFound> {
    match name {
        "chrome" => Some(AppFound { launch: "%PROGRAMFILES%/Google/Chrome/chrome.exe".into(), store: false }),
        "claude" => Some(AppFound { launch: "Claude_abc!Claude".into(), store: true }),
        _ => None,
    }
}

fn machine() -> Machine {
    detect(
        &plat(),
        &["chrome".into(), "claude".into(), "discord".into()],
        &found,
        vec!["Microphone (HD Webcam)".into()],
        vec!["Headphones (AirPods)".into()],
        &|t| (t == "ffmpeg").then(|| "C:/tools/ffmpeg.exe".to_string()),
        &["ffmpeg".into(), "piper".into()],
        1000,
    )
}

#[test]
fn setup_records_what_this_computer_actually_has() {
    let m = machine();
    assert_eq!(m.monitors.len(), 2);
    assert_eq!(m.apps.len(), 2);
    assert!(m.apps["claude"].store, "a Store app is recorded as one");
    assert_eq!(m.tools.len(), 1);
    assert_eq!(m.audio_inputs.len(), 1);
}

#[test]
fn what_it_could_not_find_is_recorded_rather_than_silently_dropped() {
    let m = machine();
    assert!(m.notes.iter().any(|n| n.contains("discord")));
    assert!(m.summary().contains("2 apps"));
}

#[test]
fn the_machine_layer_overrides_the_generic_one() {
    let mut cfg = Config::load(Path::new("config")).unwrap();
    let before = cfg.apps.get("chrome").unwrap().launch.clone();
    let n = apply(&mut cfg, &machine());
    assert!(n >= 1);
    assert_ne!(cfg.apps.get("chrome").unwrap().launch, before);
    assert!(cfg.apps.get("claude").unwrap().store, "store flag comes across too");
}

#[test]
fn the_machine_file_round_trips() {
    let d = tmp("save");
    machine().save(&d).unwrap();
    let back = Machine::load(&d).unwrap();
    assert_eq!(back.apps.len(), 2);
    assert_eq!(back.monitors.len(), 2);
}

#[test]
fn the_machine_file_warns_against_sharing_itself() {
    let d = tmp("warn");
    machine().save(&d).unwrap();
    let text = std::fs::read_to_string(d.join("machine.yaml")).unwrap();
    assert!(text.contains("Do not share"), "it holds paths and device names");
    // This asserted "run setup again" until the module was wired, at which
    // point the command turned out to be `atlas adapt` -- there is no `atlas
    // setup`. The file was telling you to run something that does not exist,
    // and the test was holding it there.
    assert!(text.contains("atlas adapt"), "the file names a command that does not exist: {text}");
}

#[test]
fn changing_your_monitors_is_noticed_so_setup_can_be_rerun() {
    let m = machine();
    let same = plat().monitors().unwrap();
    assert!(m.matches(&same));

    let one_unplugged = vec![Monitor { id: 66629, x: 0, y: 0, width: 2560, height: 1392, primary: true }];
    assert!(!m.matches(&one_unplugged));
}

#[test]
fn partial_detection_is_measurable_so_atlas_knows_to_ask_for_help() {
    let m = machine();
    let wanted: Vec<String> = vec!["chrome".into(), "claude".into(), "discord".into(), "notepad".into()];
    assert_eq!(m.coverage(&wanted), 0.5);
    assert_eq!(m.coverage(&[]), 1.0);
}

#[test]
fn first_run_says_what_it_found_and_what_it_missed() {
    let wanted: Vec<String> = vec!["chrome".into(), "claude".into(), "discord".into()];
    let said = first_run_message(&machine(), &wanted);
    assert!(said.contains("2 apps") && said.contains("2 displays"));
    assert!(said.contains("discord"), "names what it couldn't find: {said}");
}

#[test]
fn with_no_microphone_it_says_we_are_typing_for_now() {
    let mut m = machine();
    m.audio_inputs.clear();
    let said = first_run_message(&m, &[]);
    assert!(said.contains("typing for now"), "got: {said}");
}

#[test]
fn the_generated_fixture_matches_the_real_desk() {
    let f = monitor_fixture(&machine());
    assert!(f.contains("x: 2560"));
    assert!(f.contains("primary: true"));
    assert!(f.starts_with("fn fake_monitors"));
}

use atlas::platform::Platform;
