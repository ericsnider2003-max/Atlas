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

/// "What do you see" (`whats_there`) is still the room question, driven
/// through the daemon (30 Sep 2026, on merging r8-brain and r8-senses: the
/// camera requests moved to `capture_webcam`, and this branch lost the only
/// test that drove it). It looks the same way the camera command does --
/// asking first -- rather than refusing.
#[test]
fn what_do_you_see_is_the_room_and_asks_before_looking() {
    let dir = tmp("whats-there");
    let c = config_with_a_camera(&dir);
    let p = plat();
    assert_eq!(parser().parse("what do you see"), Intent::WhatsThere);
    let mut d = Daemon::new(&c, &p, None, Store::new(dir.join("state")), Proactive::new(ProactiveConfig::default()));
    let r = d.execute(&Intent::WhatsThere);
    assert!(!r.contains("don't have a camera"), "{r}");
    assert!(!r.to_lowercase().contains("not supposed to"), "{r}");
    assert!(!r.trim().is_empty());
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

// ---------------------------------------------------------------------------
// Eric, 1 Oct 2026, about 4:30-5:00 pm: "Look at me." -- "Allow the camera?"
// -- "I allow the camera." (seen) -- "Can you see me?" -- "Allow the camera?"
// again, and again, six times in sixteen minutes. permissions.json held no
// camera grant at all: "I allow the camera" didn't open with a yes, so it was
// taken as a new request and nothing was kept.
// ---------------------------------------------------------------------------

#[test]
fn erics_own_answer_is_kept_and_not_asked_again() {
    for answer in ["I allow the camera.", "I Allow The Camera.", "I love the camera."] {
        let dir = tmp(&format!("erics-answer-{}", answer.len() + answer.chars().filter(|c| c.is_uppercase()).count()));
        let c = config_with_a_camera(&dir);
        let p = plat();
        let store = Store::new(dir.join("state"));
        let mut d = Daemon::new(&c, &p, None, store.clone(), Proactive::new(ProactiveConfig::default()));
        assert_eq!(d.turn("Look at me.", 100), atlas::camera_ask::ALLOW);
        let yes = d.turn(answer, 110);
        assert!(yes.starts_with(atlas::camera_ask::LOOKING), "{answer} is a yes: {yes}");
        let perms: atlas::grants::Permissions = store.load("permissions");
        assert!(perms.grants.iter().any(|g| g.app == "camera" && g.span == atlas::grants::Span::Always), "{answer}: {:?}", perms.grants);
        let again = d.turn("Can you see me?", 130);
        assert_ne!(again, atlas::camera_ask::ALLOW, "{answer}: asked again");
        assert!(again.starts_with(atlas::camera_ask::LOOKING), "{again}");
    }
}

/// "What do you see?" with Recognising things on went straight to a frame:
/// no question, no "Looking now" (1 Oct 2026: "I can see someone I don't
/// recognise, a person, a bed" came with neither).
#[test]
fn every_way_of_looking_goes_through_the_one_gate() {
    let dir = tmp("one-gate");
    let mut c = config_with_a_camera(&dir);
    if let Some(t) = c.tools.as_mut() {
        t.vision.enabled = true;
    }
    let p = plat();
    let mut d = Daemon::new(&c, &p, None, Store::new(dir.join("state")), Proactive::new(ProactiveConfig::default()));
    assert_eq!(d.turn("what do you see", 100), atlas::camera_ask::ALLOW, "not allowed yet: asked first");
    let r = d.turn("yes", 101);
    assert!(r.starts_with(atlas::camera_ask::LOOKING), "{r}");
    let r = d.turn("what do you see", 120);
    assert!(r.starts_with(atlas::camera_ask::LOOKING), "allowed now, and said: {r}");
}

/// "Can you watch me for a five minute period?" -- "I can't continuously
/// watch for five minutes with the current setup." Now it can: asked for the
/// camera first, started on the yes, asked about, and stopped.
#[test]
fn watching_for_a_while() {
    let dir = tmp("watching");
    let mut c = config_with_a_camera(&dir);
    if let Some(t) = c.tools.as_mut() {
        // A camera that keeps sending frames, like the real one.
        let cam = t.capture_webcam.as_mut().unwrap();
        cam.args = ["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "testsrc=size=160x120:rate=5", "-frames:v", "1", "-y", "{out_png}"]
            .iter()
            .map(|s| s.to_string())
            .collect();
    }
    let p = plat();
    let mut d = Daemon::new(&c, &p, None, Store::new(dir.join("state")), Proactive::new(ProactiveConfig::default()));
    let asked = d.turn("Atlas, can you watch me for a five minute period?", 100);
    assert_eq!(asked, atlas::camera_ask::ALLOW, "the camera is asked about first");
    let started = d.turn("I allow the camera.", 105);
    assert!(started.starts_with("Watching now, for 5 minutes"), "{started}");
    let status = d.turn("are you watching me?", 110);
    assert!(status.starts_with("Yes"), "{status}");
    std::thread::sleep(std::time::Duration::from_millis(500));
    let stopped = d.turn("stop watching", 120);
    assert!(stopped.starts_with("Stopped watching"), "{stopped}");
    assert!(stopped.contains("nothing was kept"), "{stopped}");
    assert_eq!(d.turn("are you watching me?", 130), "No, I'm not watching. Say \"watch me for five minutes\" and I will.");
}

/// "Ok so can you work that so you have this capability." -- "I can't add
/// that capability with the current setup." A request for an ability is
/// written down for Eric's yes.
#[test]
fn asking_for_an_ability_is_written_down_for_your_yes() {
    let dir = tmp("ability");
    let c = config_with_a_camera(&dir);
    let p = plat();
    let store = Store::new(dir.join("state"));
    let mut d = Daemon::new(&c, &p, None, store.clone(), Proactive::new(ProactiveConfig::default()));
    let r = d.turn("Give yourself the ability to read my texts out loud", 100);
    assert!(r.contains("written it down"), "{r}");
    let w: atlas::growth::WantedAbilities = store.load(atlas::growth::STORE);
    assert_eq!(w.waiting().map(|x| x.what.as_str()), Some("read my texts out loud"));
    let r = d.turn("yes, build that ability", 110);
    assert!(r.starts_with("Approved"), "{r}");
    let w: atlas::growth::WantedAbilities = store.load(atlas::growth::STORE);
    assert_eq!(w.items[0].state, atlas::growth::State::Approved);
}

/// "Do some research on things that would allow you to advance your own
/// capabilities, then you can present them to me for approval" -- "I can't
/// tell what you mean -- nothing copied, nothing selected, nothing open."
#[test]
fn a_research_request_that_names_its_subject_is_not_asked_what_it_means() {
    let dir = tmp("research-them");
    let c = config_with_a_camera(&dir);
    let p = plat();
    let mut d = Daemon::new(&c, &p, None, Store::new(dir.join("state")), Proactive::new(ProactiveConfig::default()));
    let r = d.turn("research ways an assistant can safely add new abilities, then present them to me for approval", 100);
    assert!(!r.contains("nothing copied, nothing selected"), "{r}");
}
