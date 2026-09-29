//! The gaps an audit of the 24 September work found, each shut and held
//! shut by a test.
//!
//! Eric, 24 Sep 2026: "Can we make it better, identify and fix gaps." Three
//! reviews read the new call notes, delegation, security switch, captions and
//! picture reader line by line. What they found that was real is below, one
//! test (or a few) per gap, driven through the real code wherever it can be.

use atlas::callnotes::{free_name, transcribe_timeout_secs, write_up, Finished, Notes};
use atlas::callrec::{self, To16k};
use atlas::config::Config;
use atlas::confirmed::{before_pressing, host_of, site_of, Pressed};
use atlas::consent::ConsentConfig;
use atlas::daemon::Daemon;
use atlas::delegate::{after_reply, is_screen_noise, something_new};
use atlas::intent::Intent;
use atlas::platform::mock::{Action, MockPlatform};
use atlas::platform::Monitor;
use atlas::preferences::Preferences;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::speaking::{caption, levels_of_wav, Speaking, CAPTION_CHARS};
use atlas::store::Store;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-gaps-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

// ---------------------------------------------------------------- speech in

/// Every spoken turn passed the literal text `{task_opt}` to whisper: the
/// microphone path never filled in the language variables the shipped
/// command carries. One function now fills them for every caller.
#[test]
fn whispers_command_has_no_placeholders_left_in_it() {
    let cfg = Config::load(Path::new("config")).unwrap();
    let tools = cfg.tools.clone().unwrap();
    let mut vars = tools.vars.clone();
    for k in ["in_wav", "stem", "work_dir", "transcript", "srt"] {
        vars.insert(k.into(), format!("/tmp/{k}"));
    }
    let model = atlas::language::model_facts(vars.get("stt_model").map(String::as_str).unwrap_or(""));
    atlas::language::insert_whisper_vars(&tools.language, &model, &mut vars);
    let (_, resolved) = tools.stt.resolved(&vars);
    let joined = resolved.join(" ");
    assert!(!joined.contains('{'), "left unfilled: {joined}");
    // The shipped English-only model gets no language flags at all.
    assert!(!resolved.iter().any(|a| a == "-l" || a == "-tr"), "{resolved:?}");

    // A multilingual model with translation on gets both.
    let mut lang = tools.language.clone();
    lang.multilingual = true;
    lang.translate_others = true;
    let mut v = vars.clone();
    let small = atlas::language::model_facts("models/ggml-small.bin");
    atlas::language::insert_whisper_vars(&lang, &small, &mut v);
    assert_eq!(v.get("task_opt").map(String::as_str), Some("-tr"));
    assert_eq!(v.get("lang_opt").map(String::as_str), Some("-l"));
}

// ---------------------------------------------------------------- call notes

/// An 8 kHz headset: one input sample becomes two output ones. It used to
/// become one, so the file played at double speed.
#[test]
fn a_slow_device_is_brought_up_to_16k_not_left_at_its_own_rate() {
    let mut up = To16k::new(8_000, 1);
    let out = up.feed(&vec![0.5f32; 8_000]);
    assert!((15_990..=16_010).contains(&out.len()), "a second at 8 kHz gave {} samples", out.len());
    assert!(out.iter().all(|s| (*s - 16_383).abs() < 2), "the level changed on the way up");
    // And the usual 48 kHz stereo still comes down to one second's worth.
    let mut down = To16k::new(48_000, 2);
    let n = down.feed(&vec![0.1f32; 96_000]).len();
    assert!((15_990..=16_010).contains(&n), "{n}");
}

fn consent_on() -> ConsentConfig {
    let mut c = ConsentConfig::default();
    c.enabled = true;
    c
}

fn notes(tag: &str) -> Notes {
    let mut n = Notes::new(consent_on(), scratch(tag));
    n.starter = callrec::silent;
    n
}

/// A "no", "nobody answered" or "I couldn't ask" after a yes: their side
/// stops and what was recorded of it is deleted. It used to keep recording.
#[test]
fn a_no_after_a_yes_stops_their_side_and_deletes_it() {
    for (how, step) in [
        ("no", Notes::they_declined as fn(&mut Notes) -> atlas::callnotes::Said),
        ("nobody", Notes::nobody_answered),
        ("couldnt", Notes::couldnt_ask),
    ] {
        let mut n = notes(&format!("later-no-{how}"));
        let _ = n.begin("Zoom", 1_000);
        let _ = n.everyone();
        let _ = n.they_agreed();
        let theirs = n.call.as_ref().and_then(|c| c.theirs.as_ref()).map(|r| r.path.clone());
        let theirs = theirs.unwrap_or_else(|| panic!("{how}: their side wasn't recording after a yes"));
        assert!(theirs.exists());
        let said = step(&mut n);
        let call = n.call.as_ref().expect("the call goes on");
        assert!(call.theirs.is_none(), "{how}: still recording them: {:?}", said.lines);
        assert!(!theirs.exists(), "{how}: their recording was kept");
        assert!(call.yours.is_some(), "{how}: your side should carry on");
        let _ = n.end(2_000);
    }
}

/// "Stop taking notes" while the call app still holds the microphone: the
/// watch next looks and must not start the call again.
#[test]
fn stop_is_not_undone_by_the_next_look() {
    let mut n = notes("stop-sticks");
    let _ = n.look(1_000, Some("Zoom"));
    assert!(n.call.is_some());
    let _ = n.end(1_010);
    for t in [1_015, 1_020, 1_025] {
        let _ = n.look(t, Some("Zoom"));
        assert!(n.call.is_none(), "the call restarted at {t} after you said stop");
    }
    // Once that call ends, the next one is noticed.
    let _ = n.look(1_030, None);
    let _ = n.look(1_035, Some("Teams"));
    assert!(n.call.is_some());
    let _ = n.end(1_040);
}

/// "Take notes on this call" with no call app seen: it lasts until you say
/// stop, not until the watch looks five seconds later.
#[test]
fn a_call_you_started_by_hand_lasts_until_you_stop_it() {
    let mut n = notes("by-hand");
    let _ = n.begin_by_hand("this call", 1_000);
    for t in [1_005, 1_010, 1_060] {
        let _ = n.look(t, None);
        assert!(n.call.is_some(), "ended by the watch at {t}");
    }
    // Seen by the watch after all: from then on the watch ends it.
    let _ = n.look(1_065, Some("Zoom"));
    assert!(!n.call.as_ref().unwrap().manual);
    let _ = n.look(1_070, None);
    assert!(n.call.is_none());
}

fn call_daemon<'a>(p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    let mut cfg = Config::load(Path::new("config")).unwrap();
    cfg.tools.as_mut().unwrap().call_notes.enabled = true;
    let store = Store::new(scratch(&format!("store-{tag}")));
    let cfg: &'static Config = Box::leak(Box::new(cfg));
    let mut d = Daemon::new(cfg, p, None, store, Proactive::new(ProactiveConfig::default()));
    d.call_notes.starter = callrec::silent;
    d.call_notes.dir = scratch(&format!("calls-{tag}"));
    d
}

/// Paused mid-call: the recording holds (nothing captured) and the call's
/// notes carry on when you're back. Pausing holds; it doesn't erase.
#[test]
fn pausing_atlas_holds_a_call_recording_rather_than_ending_it() {
    let p = plat();
    let mut d = call_daemon(&p, "pause");
    let said = d.execute_timed(&Intent::CallNotes("start".into()), "take notes on this call");
    assert!(d.call_notes.call.is_some(), "{said}");
    let _ = d.attention.pause(None, atlas::store::now());
    let out = d.tick(atlas::store::now() + 10);
    let call = d.call_notes.call.as_ref().expect("a pause ended the call's notes");
    assert!(call.yours.as_ref().unwrap().is_held(), "still recording while paused");
    assert!(out.iter().any(|l| l.contains("stopped recording")), "{out:?}");
    assert!(d.call_notes.status().contains("paused"), "{}", d.call_notes.status());
    let _ = d.attention.resume(atlas::store::now());
    let out = d.tick(atlas::store::now() + 20);
    assert!(!d.call_notes.call.as_ref().unwrap().yours.as_ref().unwrap().is_held());
    assert!(out.iter().any(|l| l.contains("again")), "{out:?}");
}

/// Switched off in Settings mid-call: the recording stops at the next tick;
/// and "stop" works even once the setting is off.
#[test]
fn switching_call_notes_off_mid_call_stops_the_recording() {
    for how in ["tick", "stop"] {
        let conf = scratch(&format!("conf-off-{how}"));
        for f in ["tools.yaml", "apps.yaml", "layouts.yaml", "commands.yaml", "policy.yaml", "indexing.yaml"] {
            let from = Path::new("config").join(f);
            if from.is_file() {
                std::fs::copy(&from, conf.join(f)).unwrap();
            }
        }
        let mut prefs = Preferences::load(&conf);
        prefs.set("call_notes.enabled", "true");
        prefs.save(&conf).unwrap();
        let cfg = Config::load(&conf).unwrap();
        let p = plat();
        let mut d = Daemon::new(&cfg, &p, None, Store::new(scratch(&format!("store-off-{how}"))), Proactive::new(ProactiveConfig::default()))
            .watch_settings(conf.clone());
        d.call_notes.starter = callrec::silent;
        d.call_notes.dir = scratch(&format!("calls-off-{how}"));
        let _ = d.execute_timed(&Intent::CallNotes("start".into()), "take notes on this call");
        assert!(d.call_notes.call.is_some(), "{how}: didn't start");
        let mut prefs = Preferences::load(&conf);
        prefs.set("call_notes.enabled", "false");
        prefs.save(&conf).unwrap();
        if how == "tick" {
            let out = d.tick(atlas::store::now() + 10);
            assert!(d.call_notes.call.is_none(), "still recording with call notes off: {out:?}");
        } else {
            let reply = d.execute_timed(&Intent::CallNotes("stop".into()), "stop taking notes");
            assert!(d.call_notes.call.is_none(), "stop didn't work with the setting off: {reply}");
        }
    }
}

/// A transcriber that fails is reported, not written up as "nothing was
/// said"; and the time allowed grows with the recording.
#[test]
fn a_failed_transcription_is_reported_not_written_as_silence() {
    let dir = scratch("stt-fails");
    let wav = dir.join("call-1-you.wav");
    let mut w = callrec::WavOut::create(&wav).unwrap();
    w.write(&vec![0i16; 16_000 * 3]).unwrap();
    w.close().unwrap();
    let broken: atlas::tools::ExternalTool = serde_yaml::from_str("command: sh\nargs: [\"-c\", \"exit 3\"]\n").unwrap();
    let done = Finished { app: "Zoom".into(), started: 1, ended: 200, yours: Some(wav.clone()), theirs: None };
    let r = write_up(&done, &broken, &Default::default(), None, &dir.join("notes"));
    assert!(r.is_err(), "a failed transcription was written up: {r:?}");
    assert!(transcribe_timeout_secs(&wav) >= 600);
    // An hour of audio gets well over the default two minutes.
    let hour = dir.join("hour.wav");
    std::fs::File::create(&hour).unwrap().set_len(32_000 * 3_600).unwrap();
    assert!(transcribe_timeout_secs(&hour) >= 7_000, "{}", transcribe_timeout_secs(&hour));
}

/// Two calls in the same minute don't write over each other's notes.
#[test]
fn two_calls_in_one_minute_get_two_notes_files() {
    let dir = scratch("same-minute");
    let a = free_name(&dir, "Call notes 2026-09-24 14-05 Zoom.md");
    std::fs::write(&a, "first").unwrap();
    let b = free_name(&dir, "Call notes 2026-09-24 14-05 Zoom.md");
    assert_ne!(a, b);
    assert!(b.to_string_lossy().ends_with("Zoom (2).md"), "{}", b.display());
}

// ---------------------------------------------------------------- delegation

struct Writer(Mutex<Vec<(String, String)>>, &'static str);
impl atlas::brain::Llm for Writer {
    fn complete(&self, system: &str, user: &str) -> atlas::error::Result<String> {
        self.0.lock().unwrap().push((system.to_string(), user.to_string()));
        Ok(self.1.into())
    }
}

fn show(p: &MockPlatform, id: u64, process: &str, text: &str) {
    p.focus_on(process, "");
    *p.front.borrow_mut() = Some(atlas::platform::WindowId(id));
    p.set_window_text(id, text);
}

fn typed(p: &MockPlatform) -> Vec<String> {
    p.actions().into_iter().filter_map(|a| match a { Action::Type(t) => Some(t), _ => None }).collect()
}

/// Tick until every window job has nothing being written or waiting to be
/// typed — the model writes on the crew's threads now, so a reply lands a
/// tick or two after it's asked for. Gives up (without failing) after a
/// few hundred ticks, which is what a job that's paused or waiting for a
/// gap in your typing looks like. Returns what was said.
fn settle(d: &mut atlas::daemon::Daemon, from: u64) -> Vec<String> {
    let mut out = Vec::new();
    for k in 0..300u64 {
        out.extend(d.tick(from + k));
        if k > 0 && d.working_for_you.iter().all(|w| w.composing.is_none() && w.ready.is_none()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    out
}

fn delegate_daemon<'a>(p: &'a MockPlatform, tag: &str, reply: &'static str) -> (Daemon<'a>, Arc<Writer>) {
    let cfg = Config::load(Path::new("config")).unwrap();
    let cfg: &'static Config = Box::leak(Box::new(cfg));
    let mut d = Daemon::new(cfg, p, None, Store::new(scratch(&format!("dstore-{tag}"))), Proactive::new(ProactiveConfig::default()));
    let w = Arc::new(Writer(Mutex::new(Vec::new()), reply));
    d.llm = Some(w.clone());
    (d, w)
}

/// What you asked for is in the system prompt, where it carries authority;
/// the model's quoted material is the screen and nothing else.
#[test]
fn your_goal_is_an_instruction_and_the_screen_is_only_quoted() {
    let p = plat();
    let (mut d, w) = delegate_daemon(&p, "goal", "Friday works.");
    show(&p, 7, "SomeChat.exe", "Sam: Friday?");
    let _ = d.execute_timed(&Intent::Delegate("draft a polite no to this".into()), "draft a polite no to this");
    let settled = settle(&mut d, atlas::store::now() + 50);
    let _ = &settled;
    let calls = w.0.lock().unwrap();
    let (system, user) = &calls[0];
    assert!(system.contains("draft a polite no to this"), "{system}");
    assert!(!user.contains("draft a polite no"), "your words were quoted as screen text: {user}");
    assert!(user.contains("Sam: Friday?"));
    assert_eq!(typed(&p), vec!["Friday works.".to_string()], "the draft wasn't placed");
}

/// Paused, a conversation being carried on holds — nothing is typed and
/// nothing is lost.
#[test]
fn pausing_holds_a_conversation_being_carried_on() {
    let p = plat();
    let (mut d, _) = delegate_daemon(&p, "paused", "ok");
    show(&p, 7, "SomeChat.exe", "Sam: hi");
    let _ = d.execute_timed(&Intent::Delegate("take over this conversation".into()), "take over this conversation");
    let settled = settle(&mut d, atlas::store::now() + 50);
    let _ = &settled;
    let _ = d.attention.pause(None, atlas::store::now());
    p.set_window_text(7, "Sam: hi. Me: ok. Sam: still there?");
    let _ = settle(&mut d, atlas::store::now() + 100);
    assert_eq!(d.working_for_you.len(), 1, "a pause ended it");
    assert_eq!(typed(&p).len(), 1, "wrote while paused");
}

/// With the cursor somewhere you can't type, nothing is typed.
#[test]
fn nothing_is_typed_when_the_cursor_is_not_in_a_text_box() {
    let p = plat();
    let (mut d, _) = delegate_daemon(&p, "no-box", "Friday works.");
    show(&p, 7, "SomeChat.exe", "Sam: Friday?");
    *p.focus_editable.borrow_mut() = Some(false);
    let said = d.execute_timed(&Intent::Delegate("draft a reply to this".into()), "draft a reply to this");
    let settled = settle(&mut d, atlas::store::now() + 50);
    let _ = &settled;
    assert!(typed(&p).is_empty(), "typed with no text box focused");
    assert!(settled.join(" ").contains("click into the reply box"), "{said} {settled:?}");
}

/// An empty reply from the model is it choosing to wait, not a failure.
#[test]
fn a_model_with_nothing_to_say_waits_rather_than_quitting() {
    let p = plat();
    let (mut d, _) = delegate_daemon(&p, "wait", "   ");
    show(&p, 7, "SomeChat.exe", "Sam: ok thanks, bye for now");
    let _ = d.execute_timed(&Intent::Delegate("take over this conversation".into()), "take over this conversation");
    let settled = settle(&mut d, atlas::store::now() + 50);
    let _ = &settled;
    assert!(typed(&p).is_empty());
    assert_eq!(d.working_for_you.len(), 1, "gave up when the model chose to wait");
}

/// An app you confirm every message in: carrying on becomes one draft you
/// send yourself. It used to stop dead at the first message.
#[test]
fn an_app_that_confirms_every_message_gets_a_draft_not_a_dead_end() {
    let mut cfg = Config::load(Path::new("config")).unwrap();
    let (name, spec) = cfg.apps.apps.iter().next().map(|(n, s)| (n.clone(), s.clone())).expect("an app in apps.yaml");
    let mut spec = spec;
    spec.no_input = true;
    cfg.apps.apps.insert(name.clone(), spec);
    let p = plat();
    let mut d = Daemon::new(&cfg, &p, None, Store::new(scratch("dstore-confirm")), Proactive::new(ProactiveConfig::default()));
    d.llm = Some(Arc::new(Writer(Mutex::new(Vec::new()), "On my way.")));
    show(&p, 7, &format!("{name}.exe"), "Sam: where are you?");
    let said = d.execute_timed(&Intent::Delegate("take over this conversation".into()), "take over this conversation");
    let settled = settle(&mut d, atlas::store::now() + 50);
    let _ = &settled;
    assert!(said.contains("leave my reply in the box"), "{said}");
    assert_eq!(typed(&p), vec!["On my way.".to_string()]);
    assert!(!p.actions().iter().any(|a| matches!(a, Action::Press(k) if k == "enter")), "sent in a confirm-every-time app");
}

/// Whether something new has arrived is judged after Atlas's own last
/// reply, with times and "seen" left out — so it doesn't answer itself or a
/// clock ticking over.
#[test]
fn only_something_new_after_its_own_reply_counts() {
    let base = "Sam: Friday?\nMe: Friday works for me.\n10:42\nSeen";
    // The clock and "seen" changing isn't news.
    assert!(!something_new("Sam: Friday?\nMe: Friday works for me.\n10:43\nDelivered", Some(base), Some("Friday works for me.")));
    // The window's other parts changing isn't either.
    assert!(!something_new("Sam: Friday? (edited)\nMe: Friday works for me.\n10:43", Some(base), Some("Friday works for me.")));
    // A new line after the reply is.
    assert!(something_new("Sam: Friday?\nMe: Friday works for me.\nSam: 3pm?", Some(base), Some("Friday works for me.")));
    // No baseline yet: look.
    assert!(something_new("anything", None, None));
    assert_eq!(after_reply("a b Friday works for me. Sam: 3pm?", "Friday works for me.").as_deref(), Some(" Sam: 3pm?"));
    assert!(is_screen_noise("10:42 am") && is_screen_noise("Sam is typing…") && is_screen_noise("2 min ago") && !is_screen_noise("see you at 10:42"));
}

// ---------------------------------------------------------- security switch

/// Before pressing: the page must be on the site you said yes to, and not a
/// sign-in page.
#[test]
fn the_switch_is_only_pressed_on_the_site_you_said_yes_to() {
    let want = "https://myaccount.google.com/signinoptions/two-step-verification";
    assert_eq!(host_of(want), "myaccount.google.com");
    assert_eq!(site_of("myaccount.google.com"), "google.com");
    assert_eq!(site_of("www.bbc.co.uk"), "bbc.co.uk");
    assert_eq!(before_pressing(want, "myaccount.google.com"), None);
    assert_eq!(before_pressing(want, "accounts.google.com"), Some(Pressed::WantsYouToSignIn));
    assert_eq!(before_pressing(want, "login.example.com"), Some(Pressed::WantsYouToSignIn));
    assert!(matches!(before_pressing(want, "myaccount.google.com.evil.net"), Some(Pressed::SomewhereElse(_))));
    assert!(matches!(before_pressing(want, ""), Some(Pressed::SomewhereElse(_))));
    // A local test page with no address is judged against an expected
    // address that has none either.
    assert_eq!(before_pressing("data:text/html,x", ""), None);
}

// ---------------------------------------------------------------- settings

/// A settings file edited by hand into something that won't read is kept
/// beside the new one, not written over; and saving is all-or-nothing.
#[test]
fn a_settings_file_that_wont_read_is_kept_not_overwritten() {
    let dir = scratch("prefs");
    std::fs::write(Preferences::file(&dir), "voice.enabled: [unclosed\n").unwrap();
    assert!(Preferences::load_checked(&dir).is_err());
    let mut p = Preferences::load(&dir);
    p.set("wake.enabled", "true");
    p.save(&dir).unwrap();
    let kept = std::fs::read_to_string(dir.join("settings.unreadable.yaml")).expect("the unreadable file was lost");
    assert!(kept.contains("[unclosed"));
    assert_eq!(Preferences::load_checked(&dir).unwrap().chosen.get("wake.enabled").map(String::as_str), Some("true"));
    assert!(!dir.join("settings.yaml.part").exists());
}

// ---------------------------------------------------------------- captions

fn wav(format: u16, bits: u16, data: &[u8], extensible: bool) -> Vec<u8> {
    let fmt_len: u32 = if extensible { 40 } else { 16 };
    let mut v = b"RIFF".to_vec();
    v.extend((4 + 8 + fmt_len + 8 + data.len() as u32).to_le_bytes());
    v.extend(b"WAVEfmt ");
    v.extend(fmt_len.to_le_bytes());
    v.extend((if extensible { 0xFFFEu16 } else { format }).to_le_bytes());
    v.extend(1u16.to_le_bytes());
    v.extend(16_000u32.to_le_bytes());
    v.extend((16_000 * bits as u32 / 8).to_le_bytes());
    v.extend((bits / 8).to_le_bytes());
    v.extend(bits.to_le_bytes());
    if extensible {
        v.extend(22u16.to_le_bytes());
        v.extend(bits.to_le_bytes());
        v.extend(4u32.to_le_bytes());
        v.extend(format.to_le_bytes());
        v.extend([0u8; 14]);
    }
    v.extend(b"data");
    v.extend((data.len() as u32).to_le_bytes());
    v.extend(data);
    v
}

/// Speech written as 32-bit floats, or with the "extensible" header, still
/// moves the line. Only 16-bit PCM used to.
#[test]
fn the_line_moves_for_float_and_extensible_wavs_too() {
    let loud_then_quiet: Vec<f32> = (0..16_000).map(|i| if i < 8_000 { 0.8 } else { 0.05 }).collect();
    let bytes: Vec<u8> = loud_then_quiet.iter().flat_map(|s| s.to_le_bytes()).collect();
    for ext in [false, true] {
        let levels = levels_of_wav(&wav(3, 32, &bytes, ext), 30).expect("float speech wasn't read");
        assert!(levels[0] > levels[levels.len() - 1], "{ext}: {levels:?}");
    }
    let pcm: Vec<u8> = (0..16_000i32).flat_map(|i| (if i < 8_000 { 20_000i16 } else { 500 }).to_le_bytes()).collect();
    assert!(levels_of_wav(&wav(1, 16, &pcm, true), 30).is_some(), "extensible 16-bit wasn't read");
}

/// With no levels (a format that gave none), captions still show, and a
/// file left behind still expires.
#[test]
fn captions_show_even_when_the_voice_gave_no_levels() {
    let s = Speaking { text: "Your nine o'clock moved to ten.".into(), started_ms: 1_000, frame_ms: 30, levels: vec![] };
    assert!(s.still_going(1_500));
    assert!(!s.still_going(1_000 + 200_000));
    let mut stage = atlas::overlaywin::Stage::default();
    let mut cfg = atlas::overlay::OverlayConfig::default();
    cfg.enabled = true;
    assert!(stage.step(Some(&s), 1_500, &cfg), "nothing shown for speech without levels");
}

/// A long reply is spoken whole, but the desktop shows its start.
#[test]
fn a_long_caption_is_cut_at_a_word() {
    let long = "word ".repeat(200);
    let c = caption(&long);
    assert!(c.chars().count() <= CAPTION_CHARS + 1, "{}", c.len());
    assert!(c.ends_with("word…"), "{c}");
    assert_eq!(caption("short"), "short");
}

// ---------------------------------------------------------------- pictures

/// A big screenshot is made smaller before the picture reader sees it.
#[test]
fn a_big_screenshot_is_made_smaller_first() {
    if std::process::Command::new("ffmpeg").arg("-version").output().is_err() {
        eprintln!("skipped: no ffmpeg here");
        return;
    }
    let dir = scratch("smaller");
    let big = dir.join("screen.png");
    let ok = std::process::Command::new("ffmpeg")
        .args(["-y", "-loglevel", "error", "-f", "lavfi", "-i", "color=c=gray:s=3840x2160", "-frames:v", "1"])
        .arg(&big)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(ok);
    let small = atlas::picture_talk::smaller(&big).expect("not made smaller");
    let probe = std::process::Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width", "-of", "csv=p=0"])
        .arg(&small)
        .output();
    if let Ok(o) = probe {
        let w: u32 = String::from_utf8_lossy(&o.stdout).trim().parse().unwrap_or(0);
        assert_eq!(w, atlas::picture_talk::MAX_WIDTH);
    }
}
