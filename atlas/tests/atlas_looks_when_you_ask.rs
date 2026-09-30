//! Eric, 29 Sep 2026, 23:07-23:09: "Atlas, can you see me?" -- "I don't have
//! a camera". "Please use my camera and look at me." -- "I can't -- I'm not
//! supposed to." "Why, I gave you permission." -- "not *my* permission".
//!
//! **Root cause.** None of those sentences matched a command, so each went to
//! the language model, which answered from its idea of itself. The camera
//! path existed (`capture_webcam`: one frame, read by the local picture
//! reader, deleted), reachable only by the model choosing its tool. And had
//! it been reached: the camera named in tools.yaml was the shipped
//! "Integrated Camera" -- inside his shut laptop -- and "what am I holding"
//! answered "seeing is switched off".
//!
//! **Now:** those sentences are the camera (`camera_ask`); the first look is
//! asked about once, in plain words, and a yes is kept; every look says
//! "Looking now"; the frame is read by the picture reader when it is there
//! and by the detectors otherwise, and never kept; and the camera is the one
//! pointed at you (the C920 whose microphone hears you), never the one under
//! a shut lid.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::intent::{Intent, Parser};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-cam-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn parser() -> Parser {
    Parser::new(&Config::load(Path::new("config")).unwrap().commands)
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

#[test]
fn erics_questions_reach_the_camera() {
    let p = parser();
    for s in [
        "Atlas, can you see me?",
        "Are you using my camera? Can you see me?",
        "Please use my camera and look at me.",
        "look at me",
        "can you see me",
    ] {
        assert_eq!(p.parse(s), Intent::CaptureWebcam, "{s}");
    }
    // A refusal is not a request, and the screen is not the camera.
    for s in ["don't look at me", "stop using my camera", "look at my screen"] {
        assert_ne!(p.parse(s), Intent::CaptureWebcam, "{s}");
    }
    // "What am I holding" stays with the thing-in-front-of-the-camera
    // command, which looks through the camera the same way.
    assert_eq!(p.parse("what am I holding"), Intent::WhatsThis);
}

/// The shipped config, with the camera replaced by ffmpeg's own test
/// picture (so real frames come off a real ffmpeg, the way they come off the
/// C920) and the picture reader absent.
fn config_with_a_camera(dir: &Path) -> Config {
    let mut c = Config::load(Path::new("config")).unwrap();
    if let Some(t) = c.tools.as_mut() {
        let cam = t.capture_webcam.as_mut().expect("the shipped config has a camera capture");
        cam.args = ["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "testsrc=size=160x120:rate=5", "-frames:v", "1", "-y", "{out_png}"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        t.work_dir = dir.join("work").display().to_string();
        t.models.dir = dir.join("no-models").display().to_string();
        t.picture_talk.enabled = false;
        t.vision.enabled = false;
    }
    c
}

#[test]
fn it_asks_once_then_looks_says_so_and_keeps_no_picture() {
    let dir = tmp("ask-once");
    let c = config_with_a_camera(&dir);
    let p = plat();
    let store = Store::new(dir.join("state"));
    let mut d = Daemon::new(&c, &p, None, store.clone(), Proactive::new(ProactiveConfig::default()));

    let first = d.turn("Atlas, can you see me?", 100);
    assert_eq!(first, atlas::camera_ask::ALLOW, "the first look is asked about");
    assert!(!first.contains("atlas "), "plain words: {first}");

    let second = d.turn("yes", 105);
    assert!(second.starts_with(atlas::camera_ask::LOOKING), "a look is said as it happens: {second}");
    // With no picture reader and no detector models here, it says so rather
    // than pretending it saw nothing -- after really taking a frame.
    assert!(!second.contains("couldn't get a picture"), "the frame should have come off the camera: {second}");
    assert!(second.contains("can't make sense of the picture"), "{second}");

    // The yes is kept, for good: the question was "may I look when you ask".
    let perms: atlas::grants::Permissions = store.load("permissions");
    assert!(
        perms.grants.iter().any(|g| g.app == "camera" && g.span == atlas::grants::Span::Always),
        "{:?}",
        perms.grants
    );
    // Asked again: no question this time.
    let third = d.turn("look at me", 200);
    assert!(third.starts_with(atlas::camera_ask::LOOKING), "{third}");

    // No picture of him is left behind.
    let left: Vec<_> = std::fs::read_dir(dir.join("work")).map(|r| r.flatten().map(|e| e.file_name()).collect()).unwrap_or_default();
    assert!(left.iter().all(|n| !n.to_string_lossy().ends_with(".png")), "{left:?}");
}

#[test]
fn saying_use_my_camera_is_the_permission() {
    let dir = tmp("named");
    let c = config_with_a_camera(&dir);
    let p = plat();
    let mut d = Daemon::new(&c, &p, None, Store::new(dir.join("state")), Proactive::new(ProactiveConfig::default()));
    let r = d.turn("Please use my camera and look at me.", 100);
    assert!(r.starts_with(atlas::camera_ask::LOOKING), "naming the camera is the yes: {r}");
}

#[test]
fn no_means_no_look() {
    let dir = tmp("no");
    let c = config_with_a_camera(&dir);
    let p = plat();
    let store = Store::new(dir.join("state"));
    let mut d = Daemon::new(&c, &p, None, store.clone(), Proactive::new(ProactiveConfig::default()));
    assert_eq!(d.turn("can you see me", 100), atlas::camera_ask::ALLOW);
    let r = d.turn("no", 101);
    assert!(!r.starts_with(atlas::camera_ask::LOOKING), "{r}");
    let perms: atlas::grants::Permissions = store.load("permissions");
    assert!(perms.grants.iter().all(|g| g.app != "camera"));
}

#[test]
fn what_am_i_holding_looks_rather_than_refusing() {
    let dir = tmp("holding");
    let c = config_with_a_camera(&dir);
    let p = plat();
    let mut d = Daemon::new(&c, &p, None, Store::new(dir.join("state")), Proactive::new(ProactiveConfig::default()));
    let r = d.turn("what am I holding", 100);
    assert!(!r.contains("switched off"), "{r}");
    assert_eq!(r, atlas::camera_ask::ALLOW);
}

#[test]
fn the_camera_is_the_one_pointed_at_you() {
    let cams = vec!["Integrated Camera".to_string(), "HD Pro Webcam C920".to_string(), "OBS Virtual Camera".to_string()];
    // Lid shut behind the monitors, hearing him through the C920's mic, and
    // tools.yaml still naming the shipped "Integrated Camera".
    assert_eq!(
        atlas::audio::pick_camera_for(&cams, "Integrated Camera", "Microphone (HD Pro Webcam C920)", false).as_deref(),
        Some("HD Pro Webcam C920")
    );
    // Lid shut, a laptop mic in use: still never the camera under the lid.
    assert_eq!(
        atlas::audio::pick_camera_for(&cams, "Integrated Camera", "Microphone Array (Intel® Smart Sound Technology for Digital Microphones)", false).as_deref(),
        Some("HD Pro Webcam C920")
    );
    // Lid open, laptop mic: the configured one stands.
    assert_eq!(
        atlas::audio::pick_camera_for(&cams, "Integrated Camera", "Microphone Array (Realtek(R) Audio)", true).as_deref(),
        Some("Integrated Camera")
    );
    // Only the built-in camera, lid shut: nothing better, so it (a black
    // frame is still an honest answer, and the lid may be read wrong).
    assert_eq!(atlas::audio::pick_camera_for(&["Integrated Camera".to_string()], "x", "", false).as_deref(), Some("Integrated Camera"));
}

#[test]
fn the_windows_capture_line_opens_the_c920_by_name_and_takes_one_frame() {
    let c = Config::load(Path::new("config")).unwrap();
    let t = c.tools.as_ref().unwrap();
    let cam = t.capture_webcam.as_ref().unwrap();
    let mut vars = t.vars.clone();
    vars.insert("webcam_device".into(), "HD Pro Webcam C920".into());
    vars.insert("out_png".into(), r"C:\Atlas\data\tmp\webcam_1.png".into());
    let (cmd, args) = cam.resolved(&vars);
    assert_eq!(cmd, "ffmpeg");
    let at = |a: &str| args.iter().position(|x| x == a).unwrap_or_else(|| panic!("no {a} in {args:?}"));
    assert_eq!(args[at("-f") + 1], "dshow");
    assert_eq!(args[at("-i") + 1], "video=HD Pro Webcam C920", "one argument, spaces and all, as dshow wants it");
    assert_eq!(args[at("-frames:v") + 1], "1", "one frame: nothing is recorded");
    assert!(args.last().unwrap().ends_with("webcam_1.png"));
    // The same device, opened as a stream of raw frames for the detectors.
    let feed = atlas::frames::from_capture_args(&args);
    assert!(feed.iter().any(|a| a == "video=HD Pro Webcam C920"));
    assert!(!feed.iter().any(|a| a == "-frames:v" || a.ends_with(".png")));
}

#[test]
fn the_picture_reader_is_asked_about_you_from_the_frame() {
    let cfg = atlas::picture_talk::PictureTalkConfig::default();
    let q = atlas::camera_ask::question("Atlas, can you see me?");
    assert!(q.contains("webcam") && q.contains("\"you\""), "{q}");
    let held = atlas::camera_ask::question("can you see me? what am I holding up?");
    assert!(held.contains("holding"), "{held}");
    let line = atlas::picture_talk::command_line(&cfg, Path::new("/opt/atlas"), Path::new("/tmp/webcam_1.png"), &q);
    let at = |a: &str| line.iter().position(|x| x == a).unwrap();
    assert_eq!(line[at("--image") + 1], "/tmp/webcam_1.png");
    assert_eq!(line[at("-p") + 1], q);
}
