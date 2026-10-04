//! Call notes, from noticing the call to the notes file.
//!
//! Eric, 24 Sep 2026: "call: yes notes, voices ask then record". The
//! recording itself (`callrec`) needs Windows' sound devices, so here it is
//! exercised where it can be — the resampler, the file format, keeping both
//! sides on one clock — and on the laptop by `atlas call check`. Everything
//! else runs for real: reading Windows' microphone list, the consent steps,
//! and writing up a call from two timed transcripts.

use atlas::callnotes::{notes_name, notes_text, who_said_what, write_up, Finished, Notes};
use atlas::callrec::{wav_header, silence_due, To16k, WavOut, RATE};
use atlas::callwatch::{mic_users_from, call_from, Change, MicUser, Watch};
use atlas::consent::ConsentConfig;
use atlas::viewing::Spoken;

/// Real `reg query` output from Eric's laptop (24 Sep 2026), with Zoom added
/// holding the microphone now (a stop time of zero).
const REG: &str = r"
HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone
    Value    REG_SZ    Allow
    LastSetTime    REG_QWORD    0x1dc61768fdffa30

HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone\Microsoft.WindowsCamera_8wekyb3d8bbwe
    Value    REG_SZ    Allow
    LastSetTime    REG_QWORD    0x1dc923100cdad02
    LastUsedTimeStart    REG_QWORD    0x1dc92310fa80349
    LastUserAnnotatedLabel    REG_DWORD    0x2
    LastUsedTimeStop    REG_QWORD    0x1dc9231139d0676
    PersistedInDatabase    REG_DWORD    0x1

HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone\NonPackaged\C:#Program Files (x86)#Steam#steamapps#common#Lethal Company#Lethal Company.exe
    LastUsedTimeStart    REG_QWORD    0x1dca3cb5ed1112c
    LastUsedTimeStop    REG_QWORD    0x1dca3cb6ed1112c

HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone\NonPackaged\C:#Users#erics#AppData#Roaming#Zoom#bin#Zoom.exe
    LastUsedTimeStart    REG_QWORD    0x1dd4c0000000000
    LastUsedTimeStop    REG_QWORD    0x0
";

#[test]
fn windows_microphone_list_is_read_and_a_call_is_recognised() {
    let users = mic_users_from(REG);
    assert_eq!(users.len(), 3, "{users:?}");
    assert!(users.iter().filter(|u| u.now).count() == 1);
    assert_eq!(call_from(&users), Some("Zoom"));
    // Holding the microphone isn't a call if it isn't a call app.
    let game = vec![MicUser { who: "C:#Games#Lethal Company.exe".into(), now: true }];
    assert_eq!(call_from(&game), None);
    // Atlas's own listening never counts.
    let own = vec![MicUser { who: r"C:#Atlas#tools#ffmpeg#ffmpeg.exe".into(), now: true }];
    assert_eq!(call_from(&own), None);
    // A browser holding the microphone is a call in the browser.
    let meet = vec![MicUser { who: r"C:#Program Files#Google#Chrome#Application#chrome.exe".into(), now: true }];
    assert_eq!(call_from(&meet), Some("a call in Chrome"));
}

#[test]
fn a_call_starting_and_ending_is_seen_once_each() {
    let mut w = Watch::default();
    assert_eq!(w.saw(None), Change::Same);
    assert_eq!(w.saw(Some("Zoom")), Change::Started("Zoom"));
    assert_eq!(w.saw(Some("Zoom")), Change::Same);
    assert_eq!(w.saw(None), Change::Ended("Zoom"));
}

#[test]
fn any_device_becomes_16k_mono() {
    // 48 kHz stereo, one second of a constant level: 16,000 samples at that level.
    let mut d = To16k::new(48_000, 2);
    let got = d.feed(&vec![0.5f32; 48_000 * 2]);
    assert!((got.len() as i64 - 16_000).abs() <= 1, "{}", got.len());
    assert!(got.iter().all(|s| (*s as i32 - (i16::MAX as i32 / 2)).abs() < 2));
    // Left and right averaged, not one channel dropped.
    let mut d = To16k::new(16_000, 2);
    assert_eq!(d.feed(&[1.0, 0.0]), vec![i16::MAX / 2]);
}

#[test]
fn the_file_is_a_real_wav_with_its_sizes_filled_in() {
    let p = std::env::temp_dir().join(format!("atlas-callrec-{}.wav", std::process::id()));
    let mut w = WavOut::create(&p).unwrap();
    w.write(&vec![1000i16; 1600]).unwrap();
    assert_eq!(w.close().unwrap(), 1600);
    let b = std::fs::read(&p).unwrap();
    assert_eq!(b.len(), 44 + 3200);
    assert_eq!(&b[..44], &wav_header(3200));
    assert_eq!(u32::from_le_bytes([b[24], b[25], b[26], b[27]]), RATE);
    // Readable by Atlas's own WAV reader, the one the moving line uses.
    assert!(atlas::speaking::levels_of_wav(&b, 30).is_some());
    let _ = std::fs::remove_file(p);
}

#[test]
fn a_quiet_stretch_is_kept_so_both_sides_stay_in_step() {
    let s = std::time::Duration::from_secs;
    assert_eq!(silence_due(s(10), 10 * RATE as u64), 0);
    assert_eq!(silence_due(s(10), 10 * RATE as u64 - 100), 0, "a small lag is left alone");
    assert_eq!(silence_due(s(10), 4 * RATE as u64), 6 * RATE as usize);
}

#[test]
fn who_said_what_in_the_order_it_was_said() {
    let you = vec![Spoken { at: 1.0, words: "Can we move the launch?".into() }, Spoken { at: 9.0, words: "Great, thanks.".into() }];
    let them = vec![Spoken { at: 4.5, words: "Yes, to Friday.".into() }, Spoken { at: 6.0, words: "[BLANK_AUDIO]".into() }];
    assert_eq!(
        who_said_what(&you, &them),
        "[00:01] You: Can we move the launch?\n[00:04] Them: Yes, to Friday.\n[00:09] You: Great, thanks."
    );
}

#[test]
fn the_notes_say_whose_voices_are_in_them() {
    let both = notes_text("Zoom", 12, Some("Launch moves to Friday."), "[00:01] You: hi", true);
    assert!(both.starts_with("# Call notes — Zoom\n\n12 minutes. Both sides were recorded, after everyone said yes."));
    assert!(both.contains("## Summary\n\nLaunch moves to Friday."));
    let mine = notes_text("Zoom", 3, None, "", false);
    assert!(mine.contains("Only your side was recorded.") && !mine.contains("## Summary"));
    assert!(mine.contains("(nothing was said that I could make out)"));
    let name = notes_name("a call in Chrome", 1_790_000_000);
    assert!(name.starts_with("Call notes 2026-") && name.ends_with(" a call in Chrome.md"), "{name}");
    assert!(!name.contains(':'), "Windows won't have a colon in a file name: {name}");
}

fn on() -> ConsentConfig {
    ConsentConfig { enabled: true, ..ConsentConfig::default() }
}

#[test]
fn the_steps_follow_the_rulings() {
    let dir = std::env::temp_dir().join(format!("atlas-calls-{}", std::process::id()));
    let mut n = Notes::new(on(), dir.clone());
    // Off: nothing starts.
    let mut off = Notes::new(ConsentConfig::default(), dir.clone());
    assert!(off.begin("Zoom", 100).lines.is_empty() && off.call.is_none());

    let said = n.begin("Zoom", 100);
    assert!(said.lines[0].starts_with("On Zoom: I'm noting your side."), "{:?}", said.lines);
    assert!(n.call.is_some());
    // "They said yes" before anything was asked is not a yes.
    assert!(n.they_agreed().lines[0].contains("record everyone"));
    let ask = n.everyone();
    assert!(ask.lines[0].starts_with("Put this in the call's chat: \u{201c}Is it alright if my assistant takes notes"), "{:?}", ask.lines);
    let no = n.they_declined();
    assert!(no.lines.iter().any(|l| l.contains("your side")), "{:?}", no.lines);
    assert!(n.call.as_ref().map(|c| c.theirs.is_none()).unwrap_or(false), "their side was recorded after a no");
    let _ = n.end(700);
    assert!(n.call.is_none());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_finished_call_is_written_up_from_both_transcripts() {
    let dir = std::env::temp_dir().join(format!("atlas-writeup-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let you = dir.join("call-1-you.wav");
    let them = dir.join("call-1-them.wav");
    std::fs::write(&you, wav_header(0)).unwrap();
    std::fs::write(&them, wav_header(0)).unwrap();
    std::fs::write(dir.join("call-1-you.fixture"), "1\n00:00:01,000 --> 00:00:03,000\nShall we ship Friday?\n\n").unwrap();
    std::fs::write(dir.join("call-1-them.fixture"), "1\n00:00:04,000 --> 00:00:05,000\nFriday works.\n\n").unwrap();
    // Stands in for whisper: writes the timed transcript it was asked for.
    let timed: atlas::tools::ExternalTool =
        if cfg!(windows) {
            // cmd's copy, since Windows has neither sh nor cp.
            serde_yaml::from_str("command: cmd\nargs: [\"/c\", \"copy /y {stem}.fixture {srt} >nul\"]\nresult_file: \"{srt}\"\n").unwrap()
        } else {
            serde_yaml::from_str("command: sh\nargs: [\"-c\", \"cp {stem}.fixture {srt}\"]\nresult_file: \"{srt}\"\n").unwrap()
        };
    let done = Finished { app: "Zoom".into(), started: 1_790_000_000, ended: 1_790_000_600, yours: Some(you), theirs: Some(them) };
    let notes_dir = dir.join("notes");
    let path = write_up(&done, &timed, &Default::default(), None, &notes_dir).expect("written");
    let text = std::fs::read_to_string(&path.path).unwrap();
    assert!(text.contains("10 minutes. Both sides were recorded"), "{text}");
    assert!(text.contains("[00:01] You: Shall we ship Friday?\n[00:04] Them: Friday works."), "{text}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn old_audio_goes_and_new_audio_stays() {
    let dir = std::env::temp_dir().join(format!("atlas-oldaudio-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("call-1-you.wav"), b"x").unwrap();
    std::fs::write(dir.join("notes.md"), b"x").unwrap();
    let now = atlas::store::now();
    assert!(atlas::callnotes::audio_to_delete(&dir, now, 7).is_empty(), "a fresh recording was marked old");
    let later = atlas::callnotes::audio_to_delete(&dir, now + 8 * 86_400, 7);
    assert_eq!(later.len(), 1, "only the audio, never anything else: {later:?}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_phrases_reach_call_notes() {
    let cfg = atlas::config::Config::load(std::path::Path::new("config")).unwrap();
    let p = atlas::intent::Parser::new(&cfg.commands);
    use atlas::intent::Intent::CallNotes;
    assert_eq!(p.parse("take notes on this call"), CallNotes("start".into()));
    assert_eq!(p.parse("record everyone"), CallNotes("everyone".into()));
    assert_eq!(p.parse("they said yes"), CallNotes("agreed".into()));
    assert_eq!(p.parse("they said no"), CallNotes("declined".into()));
    assert_eq!(p.parse("stop taking notes"), CallNotes("stop".into()));
}

#[test]
fn every_answer_to_the_question_but_yes_means_your_side_only() {
    let dir = std::env::temp_dir().join(format!("atlas-calls2-{}", std::process::id()));
    for (label, run) in [
        ("couldn't ask", Notes::couldnt_ask as fn(&mut Notes) -> atlas::callnotes::Said),
        ("nobody answered", Notes::nobody_answered),
        ("they said no", Notes::they_declined),
    ] {
        let mut n = Notes::new(on(), dir.clone());
        n.begin("Zoom", 100);
        n.everyone();
        let said = run(&mut n);
        assert!(said.lines.iter().any(|l| l.contains("your side")), "{label}: {:?}", said.lines);
        assert!(n.call.as_ref().map(|c| c.theirs.is_none() && !c.recorder.capturing_others()).unwrap_or(false), "{label}");
        // And a yes after that doesn't start recording them.
        let _ = n.they_agreed();
        assert!(!n.call.as_ref().unwrap().recorder.capturing_others(), "{label}: a late yes started it");
        let _ = n.end(200);
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn asked_whether_its_recording_it_says_what_and_who_knows() {
    let dir = std::env::temp_dir().join(format!("atlas-calls3-{}", std::process::id()));
    let mut n = Notes::new(on(), dir.clone());
    let idle = n.status();
    assert!(idle.starts_with("I'm not recording anything.") && idle.contains("wait for a yes"), "{idle}");
    n.begin("Zoom", 100);
    let s = n.status();
    // On Linux the microphone can't open, so it may not be recording; either
    // way the answer names the call and never claims others are recorded.
    assert!(s.starts_with("On Zoom"), "{s}");
    assert!(!s.contains("everyone on the call"), "{s}");
    let mine = n.just_mine();
    assert!(mine.lines.iter().any(|l| l == "Just your side."));
    let _ = std::fs::remove_dir_all(dir);
}
