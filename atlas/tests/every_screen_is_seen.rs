//! Eric, 29 Sep 2026, a laptop and two external monitors: "I think Atlas is
//! only seeing one of my monitors."
//!
//! **Root causes, three of them.**
//! 1. Atlas never declared itself DPI-aware, so Windows scaled every
//!    coordinate it saw to the primary monitor's scale: with the laptop's
//!    screen at 150% or 200% and the monitors at 100%, a capture of a
//!    window's rectangle grabbed the wrong part of the desktop
//!    (`platform::win::become_dpi_aware`, proved on Windows only).
//! 2. "Look at my screen" without the picture reader read the words off
//!    the *window in front* only -- one window, on one screen -- and the
//!    picture reader got ffmpeg's capture of the whole desktop, which isn't
//!    DPI-aware either and shrinks three screens into one small picture.
//!    Now the screen you're working on is read whole (or every screen, or
//!    the one you name), and the answer says which.
//! 3. The model was told about the displays through the layout roles only:
//!    "laptop" was the *primary* display, claimed last, so a third monitor
//!    no role claimed was never mentioned, and with an external monitor set
//!    as primary the laptop's own screen was called something else.
//!
//! Here the mock platform plays three monitors; the arithmetic the Windows
//! capture relies on (the virtual screen, stitching, which monitor a window
//! is on, which monitor is the laptop's) is proved on it.

use atlas::brain::Llm;
use atlas::daemon::Daemon;
use atlas::intent::Intent;
use atlas::platform::mock::MockPlatform;
use atlas::platform::{describe_screen, monitor_under, screens_asked_for, stitch, virtual_screen, Grab, Monitor, PixelRect, Platform, ScreenPick};
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

fn mon(id: u32, x: i32, y: i32, w: i32, h: i32, primary: bool) -> Monitor {
    Monitor { id, x, y, width: w, height: h, primary }
}

/// Eric's desk as it might be: two monitors, the left one primary, and the
/// laptop's own screen on the right, smaller.
fn desk() -> Vec<Monitor> {
    vec![mon(11, 0, 0, 192, 108, true), mon(12, -192, 0, 192, 108, false), mon(13, 192, 20, 144, 90, false)]
}

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-screens-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn cfg() -> atlas::config::Config {
    atlas::config::Config::load(Path::new("config")).unwrap()
}

#[test]
fn each_screen_has_the_name_you_would_use() {
    let m = desk();
    assert_eq!(describe_screen(&m, 13, Some(13)), "your laptop screen");
    assert_eq!(describe_screen(&m, 12, Some(13)), "the left screen");
    assert_eq!(describe_screen(&m, 11, Some(13)), "the right screen", "the laptop is named for itself, not counted among the monitors");
    // Not known which is the laptop's: three across.
    assert_eq!(describe_screen(&m, 12, None), "the left screen");
    assert_eq!(describe_screen(&m, 11, None), "the middle screen");
    assert_eq!(describe_screen(&m, 13, None), "the right screen");
    // A laptop and one monitor.
    let two = vec![mon(1, 0, 0, 1920, 1080, true), mon(2, 1920, 0, 2560, 1440, false)];
    assert_eq!(describe_screen(&two, 2, Some(1)), "your monitor");
    assert_eq!(describe_screen(&[mon(1, 0, 0, 1, 1, true)], 1, None), "your screen");
}

#[test]
fn which_screens_a_request_means() {
    let m = desk();
    assert_eq!(screens_asked_for("look at my screen", &m, Some(13)), ScreenPick::Active);
    assert_eq!(screens_asked_for("look at all my screens", &m, Some(13)), ScreenPick::All);
    assert_eq!(screens_asked_for("what's on my monitors", &m, Some(13)), ScreenPick::All);
    assert_eq!(screens_asked_for("look at both screens", &m, Some(13)), ScreenPick::All);
    assert_eq!(screens_asked_for("look at my laptop screen", &m, Some(13)), ScreenPick::This(13));
    assert_eq!(screens_asked_for("what's on the left screen", &m, Some(13)), ScreenPick::This(12));
}

#[test]
fn a_window_is_on_the_monitor_it_mostly_covers() {
    let m = desk();
    assert_eq!(monitor_under(&m, PixelRect { x: -150, y: 10, width: 100, height: 50 }), Some(12));
    // Straddling: more of it on the laptop's screen.
    assert_eq!(monitor_under(&m, PixelRect { x: 180, y: 30, width: 100, height: 40 }), Some(13));
    // Off every screen: the nearest.
    assert_eq!(monitor_under(&m, PixelRect { x: 400, y: 30, width: 10, height: 10 }), Some(13));
}

#[test]
fn every_screen_together_is_the_desktop_as_the_screens_sit() {
    let p = MockPlatform::new(desk());
    *p.screen_pictures.borrow_mut() = true;
    let all = p.grab_all_screens().unwrap().expect("a picture");
    // From the left screen's left edge (-192) to the laptop's right (336);
    // from the top (0) to the laptop's bottom (110).
    let v = virtual_screen(&desk().iter().map(|m| PixelRect { x: m.x, y: m.y, width: m.width, height: m.height }).collect::<Vec<_>>()).unwrap();
    assert_eq!((v.x, v.y, v.width, v.height), (-192, 0, 528, 110));
    assert_eq!((all.width, all.height), (528, 110));
    assert_eq!(all.rgb.len(), 528 * 110 * 3);
    let px = |x: i32, y: i32| {
        let i = (((y - v.y) as usize) * 528 + (x - v.x) as usize) * 3;
        [all.rgb[i], all.rgb[i + 1], all.rgb[i + 2]]
    };
    let colour = |id: u32| [(id * 40 % 256) as u8, (id * 90 % 256) as u8, (id * 150 % 256) as u8];
    assert_eq!(px(-192, 0), colour(12), "the left screen at the left");
    assert_eq!(px(0, 107), colour(11));
    assert_eq!(px(335, 109), colour(13), "the laptop's bottom-right corner");
    assert_eq!(px(200, 5), [0, 0, 0], "above the lower laptop screen is no screen at all");
}

#[test]
fn stitching_one_picture_is_that_picture() {
    let g = Grab { width: 2, height: 1, rgb: vec![1, 2, 3, 4, 5, 6], title: "x".into() };
    assert_eq!(stitch(&[(PixelRect { x: 5, y: 5, width: 2, height: 1 }, g.clone())]), Some(g));
    assert_eq!(stitch(&[]), None);
}

#[test]
fn the_laptop_screen_is_found_by_its_device_name() {
    let mons = vec![(11, "\\\\.\\DISPLAY2".to_string()), (12, "\\\\.\\DISPLAY3".to_string()), (13, "\\\\.\\DISPLAY1".to_string())];
    assert_eq!(atlas::platform::builtin_among(&mons, &["\\\\.\\DISPLAY1".into()]), Some(13));
    assert_eq!(atlas::platform::builtin_among(&mons, &[]), None, "a desktop, or the lid shut");
}

#[test]
fn the_laptop_role_is_the_laptops_own_screen_not_the_primary() {
    let c = cfg();
    let m = desk();
    let roles = atlas::layout::resolve_roles_with(&c.layouts, &m, Some(13));
    assert_eq!(roles["laptop"].id, 13);
    assert_eq!(roles["main"].id, 11, "rightmost of the monitors");
    assert_eq!(roles["side"].id, 12);
    // Not known: the primary, as before.
    let roles = atlas::layout::resolve_roles_with(&c.layouts, &m, None);
    assert_eq!(roles["laptop"].id, 11);
    // Every monitor claimed by some role: none left out of the description.
    let claimed: std::collections::BTreeSet<u32> = atlas::layout::resolve_roles_with(&c.layouts, &m, Some(13)).values().map(|m| m.id).collect();
    assert_eq!(claimed.len(), 3);
}

#[test]
fn the_model_is_told_about_every_screen_and_which_one_you_are_on() {
    let p = MockPlatform::new(desk());
    *p.laptop_screen.borrow_mut() = Some(13);
    *p.active_screen.borrow_mut() = Some(12);
    let s = atlas::brain::context(&cfg(), &p);
    assert!(s.contains("Displays: 3"), "{s}");
    for name in ["your laptop screen", "the left screen", "the right screen"] {
        assert!(s.contains(&format!("screen: {name} =")), "{name} missing: {s}");
    }
    assert!(s.lines().any(|l| l.contains("the left screen") && l.contains("the window in front is here")), "{s}");
}

struct Reader(Mutex<Vec<(String, String)>>);
impl Llm for Reader {
    fn complete(&self, system: &str, user: &str) -> atlas::error::Result<String> {
        self.0.lock().unwrap().push((system.into(), user.into()));
        Ok("Your left screen shows the build log.".into())
    }
}

fn three_screens_with_words() -> MockPlatform {
    let p = MockPlatform::new(desk());
    *p.screen_pictures.borrow_mut() = true;
    *p.laptop_screen.borrow_mut() = Some(13);
    *p.active_screen.borrow_mut() = Some(12);
    let mut w = p.ocr_by_title.borrow_mut();
    w.insert("screen 11".into(), "Inbox\nMaya: lunch on Friday?\nNorthwind invoice attached".into());
    w.insert("screen 12".into(), "cargo build\nerror: expected semicolon\nbuild failed".into());
    w.insert("screen 13".into(), "Discord\n# general\nJordan: anyone around tonight?".into());
    drop(w);
    p
}

#[test]
fn look_at_my_screen_reads_the_whole_screen_you_are_on_and_says_which() {
    let c = cfg();
    let p = three_screens_with_words();
    let llm = Arc::new(Reader(Mutex::new(Vec::new())));
    let mut d = Daemon::new(&c, &p, Some(llm.clone()), Store::new(tmp("active")), Proactive::new(ProactiveConfig::default()));
    let said = d.execute(&Intent::ViewDisplay);
    assert_eq!(said, "Reading your screen -- one moment.", "{said}");
    let _ = d.errands_done_for_test();
    let asked = llm.0.lock().unwrap().clone();
    let (_, user) = &asked[0];
    assert!(user.contains("expected semicolon"), "the screen you're on wasn't read: {user}");
    assert!(user.contains("the left screen"), "which screen isn't said: {user}");
    assert!(!user.contains("Jordan"), "another screen was read: {user}");
}

#[test]
fn where_the_platform_cannot_say_the_window_s_place_tells_which_screen() {
    let c = cfg();
    let p = three_screens_with_words();
    *p.active_screen.borrow_mut() = None;
    *p.front.borrow_mut() = Some(atlas::platform::WindowId(5));
    let front = PixelRect { x: 200, y: 30, width: 120, height: 60 };
    p.window_rects.borrow_mut().insert(5, front);
    // The window sits on the laptop's screen (13), not the primary (11).
    assert_eq!(monitor_under(&desk(), front), Some(13));
    let llm = Arc::new(Reader(Mutex::new(Vec::new())));
    let mut d = Daemon::new(&c, &p, Some(llm.clone()), Store::new(tmp("rect")), Proactive::new(ProactiveConfig::default()));
    d.execute(&Intent::ViewDisplay);
    let _ = d.errands_done_for_test();
    let asked = llm.0.lock().unwrap().clone();
    assert_eq!(asked.len(), 1, "one screen read, one question: {asked:?}");
    let (_, user) = asked[0].clone();
    assert!(user.contains("Jordan") && user.contains("your laptop screen"), "{user}");
    assert!(!user.contains("Maya") && !user.contains("expected semicolon"), "another screen was read: {user}");
}

#[test]
fn look_at_all_my_screens_reads_every_one_each_named() {
    let c = cfg();
    let p = three_screens_with_words();
    let llm = Arc::new(Reader(Mutex::new(Vec::new())));
    let mut d = Daemon::new(&c, &p, Some(llm.clone()), Store::new(tmp("all")), Proactive::new(ProactiveConfig::default()));
    let said = d.turn("look at all my screens", 1_790_735_149);
    assert_eq!(said, "Reading your screen -- one moment.", "{said}");
    let _ = d.errands_done_for_test();
    let asked = llm.0.lock().unwrap().clone();
    let (_, user) = &asked[0];
    for (words, name) in [("Maya", "--- the right screen ---"), ("expected semicolon", "--- the left screen ---"), ("Jordan", "--- your laptop screen ---")] {
        assert!(user.contains(words) && user.contains(name), "{name} / {words} missing: {user}");
    }
}

#[test]
fn a_platform_that_cannot_capture_screens_reads_the_window_in_front_as_before() {
    let c = cfg();
    let p = MockPlatform::new(desk());
    *p.grab.borrow_mut() = Some(Grab { width: 2, height: 1, rgb: vec![0; 6], title: "Build output".into() });
    *p.ocr.borrow_mut() = Some("error: expected semicolon\nbuild failed\nsee above".into());
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("window")), Proactive::new(ProactiveConfig::default()));
    let said = d.execute(&Intent::ViewDisplay);
    // No screen capture here: nothing whole to read, so the window in front.
    assert_eq!(p.grab_screen(11).unwrap(), None);
    assert!(said.starts_with(atlas::screentext::WORDS_ONLY), "the words weren't read: {said}");
    assert!(said.contains("\u{201c}Build output\u{201d}"), "{said}");
}

/// Run on Windows itself (Eric's laptop): nothing is moved or shown.
#[cfg(windows)]
mod on_windows {
    use atlas::platform::win::WindowsPlatform;
    use atlas::platform::Platform;

    #[test]
    fn windows_every_monitor_is_listed_in_real_pixels() {
        let p = WindowsPlatform;
        let m = p.monitors().unwrap();
        let bounds: Vec<_> = m.iter().map(|x| p.monitor_bounds(x.id).unwrap()).collect();
        println!("LIVE [screens] {} monitor(s): {m:?}", m.len());
        println!("LIVE [screens] whole: {bounds:?}; laptop's own: {:?}; window in front on: {:?}", p.built_in_monitor(), p.active_monitor());
        assert!(!m.is_empty());
        for (w, b) in m.iter().zip(&bounds) {
            // The work area sits inside the whole screen.
            assert!(w.x >= b.x && w.y >= b.y && w.x + w.width <= b.x + b.width && w.y + w.height <= b.y + b.height, "{w:?} outside {b:?}");
        }
        if let Some(id) = p.built_in_monitor() {
            assert!(m.iter().any(|x| x.id == id), "the laptop's screen isn't one of the monitors");
        }
    }

    #[test]
    fn windows_each_screen_and_all_of_them_can_be_captured() {
        let p = WindowsPlatform;
        for m in p.monitors().unwrap() {
            let g = p.grab_screen(m.id).unwrap().expect("a screen can be captured");
            let b = p.monitor_bounds(m.id).unwrap();
            println!("LIVE [screens] monitor {} captured {}x{}", m.id, g.width, g.height);
            assert_eq!((g.width as i32, g.height as i32), (b.width, b.height), "captured at the wrong scale");
            assert_eq!(g.rgb.len(), (g.width * g.height * 3) as usize);
        }
        let all = p.grab_all_screens().unwrap().expect("every screen");
        println!("LIVE [screens] all together {}x{}", all.width, all.height);
    }
}
