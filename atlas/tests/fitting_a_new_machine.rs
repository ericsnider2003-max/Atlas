//! `adapt.rs`, reachable.
//!
//! The module describes a two-layer config split: `config/*.yaml` is the
//! recipe that ships to anyone and uses `%LOCALAPPDATA%` placeholders,
//! `config/machine.yaml` is what Atlas found on *this* computer and is never
//! shared. Every piece of it was written, tested and unreachable — nothing
//! ever wrote the second file and nothing ever read it, so the "generic"
//! layer was carrying a literal `C:/Program Files/Google/Chrome/...` and a
//! Realtek microphone name and was simply wrong on any machine but one.
//!
//! That matters more here than it would in most projects, because the stated
//! goal is handing Atlas to friends who run their own instance on their own
//! hardware.

use atlas::adapt::{apply, detect, leaks_a_username, portable_with, AppFound, Machine, MonitorFact};
use atlas::config::Config;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-adapt-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn desk() -> MockPlatform {
    MockPlatform::new(vec![
        Monitor { id: 1, x: 0, y: 0, width: 2560, height: 1440, primary: true },
        Monitor { id: 2, x: 2560, y: 0, width: 1920, height: 1080, primary: false },
    ])
}

// ---------- the shipped layer is safe to hand to someone ----------

#[test]
fn no_shipped_config_carries_somebody_s_home_directory() {
    // The concrete form of "you can hand this to a friend". `adapt.rs`
    // provides exactly this check and nothing called it, which is how a
    // personal path ships: not by anyone deciding to, but by nobody looking.
    let mut checked = 0;
    for entry in std::fs::read_dir("config").expect("config/ exists") {
        let path = entry.unwrap().path();
        if path.extension().map(|e| e != "yaml").unwrap_or(true) {
            continue;
        }
        // machine.yaml is the layer that is *supposed* to be personal, and
        // is never shipped. Everything else must be generic.
        if path.file_name().map(|n| n == "machine.yaml").unwrap_or(false) {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        checked += 1;
        assert_eq!(
            leaks_a_username(&text),
            None,
            "{} contains someone's home directory. Use a %LOCALAPPDATA% style \
             placeholder, or move the value into config/machine.yaml, which is \
             written per machine and never shared.",
            path.display()
        );
    }
    assert!(checked >= 5, "only {checked} config files were checked -- has the layout moved?");
}

#[test]
fn the_shared_windows_folders_are_not_mistaken_for_a_person() {
    // C:/Users/Public exists identically on every Windows machine, so a path
    // through it is generic. Getting this wrong would make the guard above
    // unusable and it would get deleted rather than fixed.
    assert_eq!(leaks_a_username("C:/Users/Public/Documents/thing.txt"), None);
    assert_eq!(leaks_a_username("C:\\Users\\Default\\AppData"), None);
    assert_eq!(leaks_a_username("C:/Users/erics/AppData/Local"), Some("erics".into()));
}

#[test]
fn a_found_path_is_made_portable_before_it_is_written_down() {
    // Whatever was found on disk is an absolute path with a username in it.
    // Storing it verbatim is how the leak happens in the first place.
    // The environment is passed in, not set: this file shares a process with
    // the rest of the suite, and a variable set here is read by every test
    // running beside it.
    let env = |n: &str| (n == "LOCALAPPDATA").then(|| "C:/Users/erics/AppData/Local".to_string());
    let out = portable_with("C:/Users/erics/AppData/Local/Discord/app.exe", &env);
    assert_eq!(out, "%LOCALAPPDATA%/Discord/app.exe");
    assert_eq!(leaks_a_username(&out), None, "a portable path still names someone");
}

// ---------- detection actually runs ----------

#[test]
fn detection_records_what_is_here_and_says_what_is_not() {
    let found = |name: &str| -> Option<AppFound> {
        (name == "editor").then(|| AppFound { launch: "/usr/bin/editor".into(), store: false })
    };
    let tool = |t: &str| -> Option<String> { (t == "ffmpeg").then(|| "/usr/bin/ffmpeg".into()) };

    let m = detect(
        &desk(),
        &["editor".to_string(), "chrome".to_string()],
        &found,
        vec!["Built-in Microphone".into()],
        vec!["Speakers".into()],
        &tool,
        &["ffmpeg".to_string(), "tesseract".to_string()],
        1000,
    );

    assert_eq!(m.apps.len(), 1);
    assert_eq!(m.monitors.len(), 2);
    assert_eq!(m.tools.get("ffmpeg").map(|s| s.as_str()), Some("/usr/bin/ffmpeg"));
    assert!(!m.tools.contains_key("tesseract"), "a tool that is not here was recorded as here");
    // The point of the notes: a missing app is said out loud rather than
    // discovered later as a launch failure.
    assert!(m.notes.iter().any(|n| n.contains("chrome")), "the missing app was not mentioned");
    assert_eq!(m.detected_at, 1000);
}

#[test]
fn what_it_found_survives_being_written_and_read_back() {
    let dir = tmp("roundtrip");
    let m = Machine {
        detected_at: 7,
        monitors: vec![MonitorFact { id: 1, x: 0, y: 0, width: 2560, height: 1440, primary: true }],
        audio_inputs: vec!["Built-in Microphone".into()],
        ..Default::default()
    };
    m.save(&dir).expect("written");
    let back = Machine::load(&dir).expect("read back");
    assert_eq!(back.detected_at, 7);
    assert_eq!(back.monitors.len(), 1);
    assert_eq!(back.audio_inputs, vec!["Built-in Microphone".to_string()]);
}

#[test]
fn the_machine_file_says_out_loud_that_it_is_not_for_sharing() {
    // It is the one file here that legitimately holds personal paths, so the
    // warning has to be in the file rather than only in the docs.
    let dir = tmp("header");
    Machine::default().save(&dir).unwrap();
    let text = std::fs::read_to_string(dir.join("machine.yaml")).unwrap();
    assert!(text.contains("don't share") || text.contains("Do not share"), "no warning: {text}");
    assert!(text.contains("atlas adapt"), "the file names a command that does not exist: {text}");
}

// ---------- and it is applied, which is the part that was missing ----------

#[test]
fn loading_the_config_applies_the_machine_layer_over_the_generic_one() {
    // The whole wiring, end to end: a machine file next to the shipped
    // config changes what `Config::load` hands back. Without this the file
    // is written and then read by nobody.
    let dir = tmp("applied");
    for f in ["apps.yaml", "layouts.yaml", "commands.yaml", "tools.yaml", "policy.yaml"] {
        std::fs::copy(Path::new("config").join(f), dir.join(f)).unwrap();
    }

    let generic = Config::load(&dir).expect("loads without a machine file");
    let name = generic.apps.apps.keys().next().expect("at least one app").clone();
    let before = generic.apps.apps[&name].launch.clone();

    let mut m = Machine::default();
    m.apps.insert(name.clone(), AppFound { launch: "/opt/found/here".into(), store: false });
    m.save(&dir).unwrap();

    let adapted = Config::load(&dir).expect("loads with a machine file");
    assert_eq!(adapted.apps.apps[&name].launch, "/opt/found/here");
    assert_ne!(adapted.apps.apps[&name].launch, before, "the machine layer changed nothing");
}

#[test]
fn a_machine_file_naming_an_app_the_config_does_not_have_is_ignored() {
    // Uninstall an app from apps.yaml and a stale machine.yaml still names
    // it. That must not resurrect it, and must not be an error either.
    let mut cfg = Config::load(Path::new("config")).unwrap();
    let mut m = Machine::default();
    m.apps.insert("nothing-like-this".into(), AppFound { launch: "/x".into(), store: false });
    assert_eq!(apply(&mut cfg, &m), 0, "an app the config never had was applied");
}

#[test]
fn no_machine_file_leaves_the_shipped_config_exactly_as_it_ships() {
    // The normal case: a fresh clone, and every other test in this tree.
    let a = Config::load(Path::new("config")).unwrap();
    let b = Config::load(Path::new("config")).unwrap();
    assert_eq!(a.apps.apps.len(), b.apps.apps.len());
    assert!(!Path::new("config/machine.yaml").exists(), "a machine file was committed");
}

// ---------- knowing when it has gone stale ----------

#[test]
fn plugging_in_a_different_monitor_is_noticed() {
    let m = Machine {
        monitors: vec![MonitorFact { id: 1, x: 0, y: 0, width: 2560, height: 1440, primary: true }],
        ..Default::default()
    };
    let same = vec![Monitor { id: 1, x: 0, y: 0, width: 2560, height: 1440, primary: true }];
    let more = vec![
        Monitor { id: 1, x: 0, y: 0, width: 2560, height: 1440, primary: true },
        Monitor { id: 2, x: 2560, y: 0, width: 1920, height: 1080, primary: false },
    ];
    assert!(m.matches(&same));
    assert!(!m.matches(&more), "a second display went unnoticed");
}

#[test]
fn coverage_answers_whether_it_is_worth_running_again() {
    let mut m = Machine::default();
    m.apps.insert("editor".into(), AppFound { launch: "/x".into(), store: false });
    let wanted = vec!["editor".to_string(), "chrome".to_string()];
    assert!((m.coverage(&wanted) - 0.5).abs() < 1e-6);
    // No apps configured is complete coverage, not a division by zero.
    assert!((m.coverage(&[]) - 1.0).abs() < 1e-6);
}
