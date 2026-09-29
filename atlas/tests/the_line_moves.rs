//! The line moves when Atlas speaks, and the words show on the desktop.
//!
//! Eric, 24 Sep 2026, item 4. The loudness comes from the reply's own WAV
//! (`speaking::levels_of_wav`), written beside Atlas's data with the moment
//! playback starts; the Atlas window and the desktop overlay both read it.

use atlas::overlay::{Element, OverlayConfig};
use atlas::overlaywin::Stage;
use atlas::speaking::{self, Speaking};
use std::path::PathBuf;

/// A 16-bit mono WAV: 300 ms silent, 300 ms loud, 300 ms quiet.
fn wav() -> Vec<u8> {
    wav_of(&[(0.0, 300), (20_000.0, 300), (2_000.0, 300)])
}

fn wav_of(parts: &[(f32, u32)]) -> Vec<u8> {
    let rate = 16_000u32;
    let mut samples: Vec<i16> = Vec::new();
    for &(amp, ms) in parts {
        for n in 0..(rate * ms / 1000) {
            samples.push((amp * (n as f32 * 0.3).sin()) as i16);
        }
    }
    let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    let mut w = Vec::new();
    w.extend(b"RIFF");
    w.extend((36 + data.len() as u32).to_le_bytes());
    w.extend(b"WAVEfmt ");
    w.extend(16u32.to_le_bytes());
    w.extend(1u16.to_le_bytes()); // PCM
    w.extend(1u16.to_le_bytes()); // mono
    w.extend(rate.to_le_bytes());
    w.extend((rate * 2).to_le_bytes());
    w.extend(2u16.to_le_bytes());
    w.extend(16u16.to_le_bytes());
    // A chunk piper doesn't write but other tools do, to prove it's skipped.
    w.extend(b"LIST");
    w.extend(3u32.to_le_bytes());
    w.extend([1, 2, 3, 0]); // odd length, padded
    w.extend(b"data");
    w.extend((data.len() as u32).to_le_bytes());
    w.extend(data);
    w
}

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-line-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[test]
fn the_loudness_follows_the_speech_itself() {
    let levels = speaking::levels_of_wav(&wav(), 30).expect("16-bit PCM is read");
    assert_eq!(levels.len(), 30, "900 ms in 30 ms frames");
    assert!(levels[..9].iter().all(|l| *l == 0), "silence stays flat: {:?}", &levels[..10]);
    assert_eq!(*levels.iter().max().unwrap(), 255, "the loudest moment uses the full height");
    let loud = levels[12] as i32;
    let quiet = levels[25] as i32;
    assert!(loud > quiet && quiet > 0, "quiet speech is lower but still moves: loud {loud}, quiet {quiet}");
}

#[test]
fn anything_that_isnt_pcm_speech_gives_no_level_rather_than_a_wrong_one() {
    assert_eq!(speaking::levels_of_wav(b"not a wav at all", 30), None);
    let mut float = wav();
    float[20] = 3; // IEEE float, not 16-bit PCM
    assert_eq!(speaking::levels_of_wav(&float, 30), None);
}

#[test]
fn the_file_says_what_is_being_said_and_is_gone_when_it_ends() {
    let d = dir("file");
    speaking::begin(&d, "Hello Eric.", &wav(), 1_000).unwrap();
    let s = speaking::now_saying(&d).expect("written");
    assert_eq!(s.text, "Hello Eric.");
    assert_eq!(s.level_at(1_000 + 400), Some(s.levels[13] as f32 / 255.0));
    assert_eq!(s.level_at(999), None, "not before it starts");
    assert_eq!(s.level_at(1_000 + s.levels.len() as u64 * s.frame_ms as u64), None, "a file left by a crash can't keep the line moving");
    speaking::end(&d);
    assert!(speaking::now_saying(&d).is_none());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_watch_reads_the_voice_level_now() {
    let d = dir("watch");
    let mut w = speaking::Watch::new(d.clone());
    assert_eq!(w.level(), None, "nothing said, nothing moves");
    // The loud part runs 300 ms to 3.3 s, so a machine busy with thousands
    // of other tests (a native Windows run read it 150 ms late and caught
    // the quiet tail of a 300 ms loud part) still reads it mid-word.
    let long = wav_of(&[(0.0, 300), (20_000.0, 3_000), (2_000.0, 300)]);
    speaking::begin(&d, "Loud part.", &long, speaking::now_ms() - 400).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(60));
    let level = w.level().expect("mid-speech");
    assert!(level > 0.5, "400 ms in is the loud part: {level}");
    let _ = std::fs::remove_dir_all(&d);
}

fn said(text: &str, started_ms: u64) -> Speaking {
    Speaking { text: text.into(), started_ms, frame_ms: 30, levels: vec![200; 100] }
}

#[test]
fn the_overlay_shows_a_reply_once_then_leaves_the_desktop_alone() {
    let cfg = OverlayConfig::default();
    let mut stage = Stage::default();
    assert!(!stage.step(None, 0, &cfg), "nothing to show before anything is said");

    let s = said("Your two o'clock moved to three.", 10_000);
    assert!(stage.step(Some(&s), 10_010, &cfg));
    let (first, _) = stage.frame(2560, 1440, &cfg);
    assert!(first.iter().any(|e| matches!(e, Element::Mark { .. })), "the mark arrives first");

    // Long after: typed, held, faded, gone — even though the file still
    // names the same reply, it isn't shown a second time.
    let mut t = 10_010;
    while stage.step(Some(&s), t, &cfg) {
        t += 50;
        assert!(t < 60_000, "never went away");
    }
    assert!(stage.frame(2560, 1440, &cfg).0.is_empty());
    assert!(!stage.step(Some(&s), t + 1_000, &cfg), "the same reply came back");

    // The next thing said is shown.
    assert!(stage.step(Some(&said("Done.", t + 2_000)), t + 2_010, &cfg));
}

#[test]
fn the_words_type_in_over_the_desktop() {
    let cfg = OverlayConfig::default();
    let mut stage = Stage::default();
    let s = said("Hello Eric.", 0);
    stage.step(Some(&s), 1, &cfg);
    stage.step(Some(&s), 1_000, &cfg); // the mark has arrived; typing begins
    stage.step(Some(&s), 1_200, &cfg);
    let (frame, opacity) = stage.frame(1920, 1080, &cfg);
    let typed = frame.iter().find_map(|e| match e {
        Element::Typed { text, .. } => Some(text.clone()),
        _ => None,
    });
    assert!(typed.map(|t| !t.is_empty() && "Hello Eric.".starts_with(&t)).unwrap_or(false));
    assert!(frame.iter().any(|e| matches!(e, Element::Shade { .. })), "the shade that keeps it readable over a white page");
    assert_eq!(opacity, 1.0);
}

#[test]
fn switched_off_it_shows_nothing_and_fades_what_it_was_showing() {
    let off = OverlayConfig { enabled: false, ..OverlayConfig::default() };
    let mut stage = Stage::default();
    assert!(!stage.step(Some(&said("Hi.", 0)), 10, &off), "switched off, nothing appears");

    let on = OverlayConfig::default();
    let mut stage = Stage::default();
    stage.step(Some(&said("A longer sentence to type.", 0)), 10, &on);
    stage.stand_down(20);
    let mut t = 20;
    while stage.step(None, t, &on) {
        t += 50;
    }
    assert!(t <= 20 + on.fade_ms + 100, "it faded instead of holding: {t}");
}

#[test]
fn the_overlay_only_runs_while_the_background_atlas_does() {
    let d = dir("alive");
    std::fs::create_dir_all(&d).unwrap();
    let now = atlas::store::now();
    // 28 Sep 2026: asked through a `Watching`, which keeps the answer over
    // time (a lock only just gone quiet may be an Atlas waking from sleep).
    let mut w = atlas::onlyone::Watching::default();
    assert!(!atlas::overlaywin::atlas_is_up(&mut w, &d, now));
    atlas::onlyone::OnlyOne::at(&d).take(now).unwrap();
    assert!(atlas::overlaywin::atlas_is_up(&mut w, &d, now));
    // The laptop slept overnight: on waking, the lock reads abandoned until
    // Atlas beats. The overlay stays for that moment...
    let morning = now + 8 * 3600;
    assert!(atlas::overlaywin::atlas_is_up(&mut w, &d, morning), "the overlay closed itself on waking from sleep");
    // ...and goes once Atlas really hasn't come back.
    assert!(!atlas::overlaywin::atlas_is_up(&mut w, &d, morning + atlas::onlyone::WOKE_GRACE_SECS));
    // Quit properly: gone at once.
    let mut w = atlas::onlyone::Watching::default();
    atlas::onlyone::OnlyOne::at(&d).release();
    assert!(!atlas::overlaywin::atlas_is_up(&mut w, &d, now));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_desktop_captions_are_a_setting_that_applies_without_a_restart() {
    let s = atlas::settings::registry(&Default::default());
    let it = s.get("overlay.enabled").expect("in the settings list");
    assert_eq!(it.group, "How it talks back");
    assert!(!atlas::settings::needs_a_restart("overlay.enabled"));
}
