//! Eric, 29 Sep 2026, on his laptop: "I use the wake word and Atlas says 'I
//! heard my name but nothing after it'." And: he shouldn't need the
//! push-to-talk key at all.
//!
//! **The root cause.** The wake word was heard by recording a three-second
//! clip, transcribing it and looking for "atlas" in the words -- and then
//! the words were thrown away and a second recording was started for the
//! request. "Atlas, what time is it" said in one breath fits in the first
//! clip; the second recording, started after speech-to-text on the first
//! and the recorder starting up again, heard the silence after it. Its
//! speech detector also learned "the room" from its first third of a
//! second, which -- if you were still talking -- was you.
//!
//! **Now** (`utterance`, `micthread::wake_on_stream`) the microphone is one
//! running stream cut where you pause. The name is looked for in what you
//! said, and the request is what came after it in the same breath; a long
//! request is recorded until you stop and heard whole; the name on its own
//! waits a moment for the rest on the same stream, and only then does Atlas
//! say "Yes?" and listen. After a reply the conversation stays open, no key
//! and no wake word, until you've said nothing for a while or said "that's
//! all".
//!
//! The clips are real speech, synthesized (espeak-ng,
//! tests/fixtures/speech/wake/make_wake.sh). The speech engine is scripted
//! except in the last test, which runs the real whisper when it's on this
//! machine (ATLAS_TEST_WHISPER and ATLAS_TEST_WHISPER_MODEL), and says so
//! when it isn't.

use atlas::config::Config;
use atlas::daemon::{Daemon, Ears, Mouth};
use atlas::micthread::{Heard, MicStream, MicThread, MicWork, NameCheck};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use atlas::utterance::{Seg, Segmenter};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const RATE: usize = 16_000;

fn wake_clip(name: &str) -> Vec<i16> {
    let (s, r) = atlas::diarize::read_wav(&std::fs::read(format!("tests/fixtures/speech/wake/{name}.wav")).unwrap()).unwrap();
    assert_eq!(r, 16000);
    s
}

fn corpus(name: &str) -> Vec<i16> {
    let (s, r) = atlas::diarize::read_wav(&std::fs::read(format!("tests/fixtures/speech/corpus/{name}.wav")).unwrap()).unwrap();
    assert_eq!(r, 16000);
    s
}

/// A quiet room: low, slightly coloured noise, about -60 dBFS.
fn room(ms: usize, seed: u64) -> Vec<i16> {
    let mut x = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    let mut lp = 0f32;
    (0..ms * RATE / 1000)
        .map(|_| {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let w = ((x >> 33) as f32 / (1u64 << 31) as f32) - 1.0;
            lp = 0.9 * lp + 0.1 * w;
            (lp * 90.0) as i16
        })
        .collect()
}

fn then(parts: &[Vec<i16>]) -> Vec<i16> {
    parts.iter().flatten().copied().collect()
}

fn secs(samples: usize) -> f32 {
    samples as f32 / RATE as f32
}

/// Audio handed over as fast as it's read; `None` at the end.
struct Script {
    samples: Vec<i16>,
    at: usize,
}

impl MicStream for Script {
    fn read(&mut self, n: usize) -> Option<Vec<i16>> {
        if self.at + n > self.samples.len() {
            return None;
        }
        let w = self.samples[self.at..self.at + n].to_vec();
        self.at += n;
        Some(w)
    }
}

// ================= cutting real speech where you pause =================

#[test]
fn two_things_said_with_a_pause_between_are_two_utterances_each_whole() {
    let (a, b) = (corpus("A_s3"), corpus("A_s8"));
    let s = then(&[room(800, 1), a.clone(), room(1600, 2), b.clone(), room(1600, 3)]);
    let mut seg = Segmenter::new(&atlas::endpoint::EndpointConfig::default());
    let mut done = Vec::new();
    for w in s.chunks_exact(atlas::utterance::WINDOW) {
        if let Seg::Done(u) = seg.feed(w) {
            done.push(u.len());
        }
    }
    assert_eq!(done.len(), 2, "two sentences with a pause between: {:?}", done.iter().map(|n| secs(*n)).collect::<Vec<_>>());
    // Each whole (espeak's own lead-in and tail silence aside) and neither
    // swallowing the pause.
    for (got, said) in done.iter().zip([&a, &b]) {
        assert!(secs(*got) >= secs(said.len()) - 0.35, "cut short: {} of {}", secs(*got), secs(said.len()));
        assert!(secs(*got) <= secs(said.len()) + 0.8, "the pause went with it: {} for {}", secs(*got), secs(said.len()));
    }
}

#[test]
fn nobody_speaking_is_nothing_and_costs_no_transcription() {
    let mut s = Script { samples: room(6000, 4), at: 0 };
    let got = atlas::utterance::next_utterance(&mut s, &atlas::endpoint::EndpointConfig::default(), 5000, &|| false);
    assert!(got.is_none());
}

#[test]
fn the_open_floor_waits_for_you_to_start_and_takes_all_you_say() {
    // Silence for four seconds, then a sentence: the old open floor recorded
    // six seconds and would have cut this one off.
    let said = corpus("B_s4");
    let mut s = Script { samples: then(&[room(4000, 5), said.clone(), room(2000, 6)]), at: 0 };
    let got = atlas::utterance::next_utterance(&mut s, &atlas::endpoint::EndpointConfig::default(), 25_000, &|| false).expect("heard");
    assert!(secs(got.len()) >= secs(said.len()) - 0.35, "{} of {}", secs(got.len()), secs(said.len()));
}

// ================= the wake word, on the microphone's thread =================

/// A microphone whose first stream is `audio`, then a quiet room; the speech
/// engine gives `answers` in turn, and every stretch it was asked about is
/// kept.
struct Scripted {
    audio: Mutex<Option<Vec<i16>>>,
    answers: Mutex<VecDeque<NameCheck>>,
    asked: Arc<Mutex<Vec<usize>>>,
    listened: Arc<AtomicUsize>,
    clips: Arc<AtomicUsize>,
    follow: Arc<Mutex<VecDeque<Option<String>>>>,
    follow_secs: Arc<Mutex<Vec<u32>>>,
    /// The microphone can't be opened while this is true.
    broken: Arc<AtomicBool>,
}

impl Scripted {
    fn new(audio: Vec<i16>, answers: Vec<NameCheck>) -> Scripted {
        Scripted {
            audio: Mutex::new(Some(audio)),
            answers: Mutex::new(answers.into()),
            asked: Arc::default(),
            listened: Arc::default(),
            clips: Arc::default(),
            follow: Arc::default(),
            follow_secs: Arc::default(),
            broken: Arc::default(),
        }
    }
    fn handles(&self) -> Scripted {
        Scripted {
            audio: Mutex::new(None),
            answers: Mutex::new(VecDeque::new()),
            asked: self.asked.clone(),
            listened: self.listened.clone(),
            clips: self.clips.clone(),
            follow: self.follow.clone(),
            follow_secs: self.follow_secs.clone(),
            broken: self.broken.clone(),
        }
    }
}

/// A microphone that can't be opened: no sound, and ffmpeg's reason.
struct Unopenable;
impl MicStream for Unopenable {
    fn read(&mut self, _n: usize) -> Option<Vec<i16>> {
        None
    }
    fn why_stopped(&mut self) -> Option<String> {
        Some("Could not find audio only device with name [nothing] among source devices of type audio.".into())
    }
}

/// A quiet room, handed over in real time (so a thread reading it doesn't spin).
struct QuietRoom(Instant, usize);
impl MicStream for QuietRoom {
    fn read(&mut self, n: usize) -> Option<Vec<i16>> {
        let due = Duration::from_micros(((self.1 + n) as u64 * 1_000_000) / RATE as u64);
        if let Some(w) = due.checked_sub(self.0.elapsed()) {
            std::thread::sleep(w);
        }
        self.1 += n;
        Some(room(n * 1000 / RATE + 1, self.1 as u64)[..n].to_vec())
    }
}

impl MicWork for Scripted {
    fn wake_once(&mut self, _stop: &dyn Fn() -> bool) -> atlas::error::Result<bool> {
        self.clips.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(20));
        Ok(false)
    }
    fn listen(&mut self) -> atlas::error::Result<String> {
        self.listened.fetch_add(1, Ordering::SeqCst);
        Ok(String::new())
    }
    fn open_stream(&mut self) -> Option<Box<dyn MicStream>> {
        if self.broken.load(Ordering::SeqCst) {
            return Some(Box::new(Unopenable));
        }
        match self.audio.lock().unwrap().take() {
            Some(a) => Some(Box::new(Script { samples: a, at: 0 })),
            None => Some(Box::new(QuietRoom(Instant::now(), 0))),
        }
    }
    fn transcribe(&mut self, samples: &[i16]) -> atlas::error::Result<String> {
        self.name_in(samples).map(|c| match c {
            NameCheck::Named(r) | NameCheck::NotNamed(r) => r,
        })
    }
    fn follow_up(&mut self, secs: u32, _stop: &dyn Fn() -> bool) -> atlas::error::Result<Option<String>> {
        self.follow_secs.lock().unwrap().push(secs);
        Ok(self.follow.lock().unwrap().pop_front().flatten())
    }
    fn hears_name_in_audio(&self) -> bool {
        true
    }
    fn name_in(&mut self, samples: &[i16]) -> atlas::error::Result<NameCheck> {
        self.asked.lock().unwrap().push(samples.len());
        Ok(self.answers.lock().unwrap().pop_front().unwrap_or(NameCheck::NotNamed(String::new())))
    }
    fn models_dir(&self) -> PathBuf {
        PathBuf::from("tests/fixtures/silero")
    }
}

fn first_heard(m: &MicThread, within: Duration) -> Option<Heard> {
    let until = Instant::now() + within;
    while Instant::now() < until {
        if let Some(h) = m.poll() {
            return Some(h);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    None
}

#[test]
fn atlas_what_time_is_it_in_one_breath_is_the_name_and_the_request() {
    let audio = then(&[room(700, 7), wake_clip("atlas_what_time"), room(3000, 8)]);
    let mic = Scripted::new(audio, vec![NameCheck::Named("what time is it?".into())]);
    let h = mic.handles();
    let m = MicThread::start(Box::new(mic));
    m.set_wake(true);
    let got = first_heard(&m, Duration::from_secs(5)).expect("the name was heard");
    assert_eq!(got, Heard::Wake(Ok("what time is it?".into())));
    assert_eq!(h.listened.load(Ordering::SeqCst), 0, "a second recording was made for a request already said");
    let asked = h.asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 1, "one look at one stretch: {asked:?}");
    // The stretch the engine heard is the whole of what was said -- name and
    // request -- with a little room either side, not a three-second slice.
    let clip = secs(wake_clip("atlas_what_time").len());
    assert!(secs(asked[0]) >= clip - 0.2 && secs(asked[0]) <= clip + 1.0, "heard {}s of a {clip}s clip", secs(asked[0]));
}

#[test]
fn a_long_request_is_heard_to_the_end_not_cut_at_the_clip() {
    let long = wake_clip("atlas_long");
    assert!(secs(long.len()) > 4.0);
    let audio = then(&[room(700, 9), long.clone(), room(3000, 10)]);
    let whole = "remind me to call the bank about the transfer tomorrow morning at nine";
    let mic = Scripted::new(audio, vec![NameCheck::Named("remind me to".into()), NameCheck::Named(whole.into())]);
    let h = mic.handles();
    let m = MicThread::start(Box::new(mic));
    m.set_wake(true);
    let got = first_heard(&m, Duration::from_secs(8)).expect("the name was heard");
    assert_eq!(got, Heard::Wake(Ok(whole.into())));
    let asked = h.asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 2, "{asked:?}");
    assert!((secs(asked[0]) - 3.0).abs() < 0.1, "the first look is at the first three seconds: {}", secs(asked[0]));
    assert!(secs(asked[1]) >= secs(long.len()) - 0.3, "the request was cut: {} of {}", secs(asked[1]), secs(long.len()));
}

#[test]
fn the_name_then_a_pause_then_the_request_is_one_request() {
    let audio = then(&[room(700, 11), wake_clip("atlas_alone"), room(1500, 12), wake_clip("what_time"), room(3000, 13)]);
    let mic = Scripted::new(audio, vec![NameCheck::Named(String::new()), NameCheck::NotNamed("What time is it?".into())]);
    let h = mic.handles();
    let m = MicThread::start(Box::new(mic));
    m.set_wake(true);
    let got = first_heard(&m, Duration::from_secs(8)).expect("heard");
    assert_eq!(got, Heard::Wake(Ok("What time is it?".into())));
    assert_eq!(h.listened.load(Ordering::SeqCst), 0);
}

#[test]
fn the_name_on_its_own_is_named_so_atlas_can_ask() {
    let audio = then(&[room(700, 14), wake_clip("atlas_alone"), room(4000, 15)]);
    let mic = Scripted::new(audio, vec![NameCheck::Named(String::new())]);
    let m = MicThread::start(Box::new(mic));
    m.set_wake(true);
    assert_eq!(first_heard(&m, Duration::from_secs(8)), Some(Heard::Named));
}

#[test]
fn talk_without_the_name_wakes_nothing() {
    let audio = then(&[room(700, 16), wake_clip("what_time"), room(2000, 17)]);
    let mic = Scripted::new(audio, vec![NameCheck::NotNamed("what time is it".into())]);
    let h = mic.handles();
    let m = MicThread::start(Box::new(mic));
    m.set_wake(true);
    assert_eq!(first_heard(&m, Duration::from_secs(2)), None);
    assert_eq!(h.asked.lock().unwrap().len(), 1, "the speech was looked at");
    // Nobody talking: nothing more is transcribed.
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(h.asked.lock().unwrap().len(), 1, "the quiet room went through the speech engine");
}

// ================= the daemon: "Yes?", hands-free, coming back =================

fn cfg() -> Config {
    let mut c = Config::load(Path::new("config")).unwrap();
    let t = c.tools.get_or_insert_with(Default::default);
    t.wake = Some(atlas::voice::WakeConfig { enabled: true, phrase: "atlas".into(), clip_seconds: 3, detector: None });
    c
}

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-onebreath-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

#[derive(Default)]
struct Speaker(Mutex<Vec<String>>);
impl Mouth for Speaker {
    fn speak(&self, text: &str) -> atlas::error::Result<()> {
        self.0.lock().unwrap().push(text.to_string());
        Ok(())
    }
}

/// Ears whose microphone thread is a `Scripted`, and whose own listening
/// (no thread) gives `briefly` in turn.
struct ScriptedEars {
    mic: Mutex<Option<Scripted>>,
    briefly: Mutex<VecDeque<Option<String>>>,
    briefly_secs: Mutex<Vec<u32>>,
}

impl Ears for ScriptedEars {
    fn wait_for_wake(&self) -> atlas::error::Result<()> {
        Ok(())
    }
    fn listen(&self) -> atlas::error::Result<String> {
        Ok(String::new())
    }
    fn listen_briefly(&self, secs: u32) -> atlas::error::Result<Option<String>> {
        self.briefly_secs.lock().unwrap().push(secs);
        Ok(self.briefly.lock().unwrap().pop_front().flatten())
    }
    fn mic_work(&self) -> Option<Box<dyn MicWork>> {
        self.mic.lock().unwrap().take().map(|m| Box::new(m) as Box<dyn MicWork>)
    }
}

#[test]
fn the_name_alone_gets_yes_and_what_you_say_next_is_answered() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("yes")), Proactive::new(ProactiveConfig::default()));
    let mic = Scripted::new(then(&[room(700, 18), wake_clip("atlas_alone"), room(4000, 19)]), vec![NameCheck::Named(String::new())]);
    // After "Yes?": the time asked for, then nothing more.
    mic.follow.lock().unwrap().extend([Some("what time is it".to_string()), None]);
    let h = mic.handles();
    let ears = ScriptedEars { mic: Mutex::new(Some(mic)), briefly: Mutex::default(), briefly_secs: Mutex::default() };
    let mouth = Speaker::default();
    let clock = || 1_790_735_149u64;
    let until = Instant::now() + Duration::from_secs(10);
    while !d.listen_pass(&ears, &mouth, &clock) {
        assert!(Instant::now() < until, "the name never came through");
        std::thread::sleep(Duration::from_millis(10));
    }
    let said = mouth.0.lock().unwrap().clone();
    assert_eq!(said.first().map(String::as_str), Some("Yes?"), "{said:?}");
    assert!(!said.iter().any(|s| s.contains("nothing after it")), "{said:?}");
    assert!(said.len() >= 2 && said[1].chars().any(|c| c.is_ascii_digit()), "the time wasn't told: {said:?}");
    // The open floor after the answer is the hands-free one.
    let asked = h.follow_secs.lock().unwrap().clone();
    assert_eq!(asked.get(1).copied(), Some(atlas::utterance::ConversationConfig::default().quiet_secs), "{asked:?}");
}

#[test]
fn hands_free_the_conversation_stays_open_until_you_say_thats_all() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("handsfree")), Proactive::new(ProactiveConfig::default()));
    let ears = ScriptedEars {
        mic: Mutex::new(None),
        briefly: Mutex::new(VecDeque::from([Some("what day is it".to_string()), Some("thanks Atlas".to_string()), Some("what time is it".to_string())])),
        briefly_secs: Mutex::default(),
    };
    let mouth = Speaker::default();
    let clock = || 1_790_735_149u64;
    d.converse("what time is it", &ears, &mouth, &clock);
    let said = mouth.0.lock().unwrap().clone();
    assert_eq!(said.last().map(String::as_str), Some("Anytime."), "{said:?}");
    // Two follow-ups taken, no key and no wake word; the third never asked.
    let secs = ears.briefly_secs.lock().unwrap().clone();
    assert_eq!(secs.len(), 2, "{secs:?}");
    assert!(secs.iter().all(|s| *s == 25), "the floor wasn't the hands-free one: {secs:?}");
    assert_eq!(ears.briefly.lock().unwrap().len(), 1);
}

#[test]
fn hands_free_ends_on_silence() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("silence")), Proactive::new(ProactiveConfig::default()));
    let ears = ScriptedEars { mic: Mutex::new(None), briefly: Mutex::new(VecDeque::from([None])), briefly_secs: Mutex::default() };
    let mouth = Speaker::default();
    d.converse("what time is it", &ears, &mouth, &|| 1_790_735_149u64);
    assert_eq!(ears.briefly_secs.lock().unwrap().len(), 1);
    assert_ne!(mouth.0.lock().unwrap().last().map(String::as_str), Some("Anytime."));
}

#[test]
fn off_hands_free_the_floor_is_the_short_one_again() {
    let mut c = cfg();
    c.tools.as_mut().unwrap().conversation.hands_free = false;
    let p = plat();
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("short")), Proactive::new(ProactiveConfig::default()));
    let ears = ScriptedEars { mic: Mutex::new(None), briefly: Mutex::new(VecDeque::from([None])), briefly_secs: Mutex::default() };
    d.converse("what time is it", &ears, &Speaker::default(), &|| 1_790_735_149u64);
    let secs = ears.briefly_secs.lock().unwrap().clone();
    assert!(secs[0] < 25, "{secs:?}");
}

#[test]
fn push_to_talk_after_a_missing_microphone_comes_back_to_the_wake_word_by_itself() {
    // The log, 29 Sep: "listening for the wake word: ... Could not find audio
    // only device with name [nothing]" three times, "Switching to
    // push-to-talk -- wake word isn't working", and never back.
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("comeback")), Proactive::new(ProactiveConfig::default()));
    let mic = Scripted::new(Vec::new(), Vec::new());
    mic.audio.lock().unwrap().take();
    mic.broken.store(true, Ordering::SeqCst);
    let h = mic.handles();
    let ears = ScriptedEars { mic: Mutex::new(Some(mic)), briefly: Mutex::default(), briefly_secs: Mutex::default() };
    let mouth = Speaker::default();
    let clock = || 100u64;
    let until = Instant::now() + Duration::from_secs(10);
    while d.tiers.tier != atlas::input::Tier::PushToTalk {
        d.listen_pass(&ears, &mouth, &clock);
        assert!(Instant::now() < until, "a microphone that can't be opened never dropped the wake word");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(mouth.0.lock().unwrap().iter().any(|s| s.contains("push-to-talk")));
    // Still broken: tried now and then, and nothing changes.
    let mut t = 1_000u64;
    for _ in 0..3 {
        d.back_to_the_wake_word(&mouth, t);
        t += 30;
        std::thread::sleep(Duration::from_millis(200));
    }
    assert_eq!(d.tiers.tier, atlas::input::Tier::PushToTalk);
    // The microphone works again (the right one picked, plugged back in).
    h.broken.store(false, Ordering::SeqCst);
    let until = Instant::now() + Duration::from_secs(10);
    while d.tiers.tier != atlas::input::Tier::Voice {
        d.back_to_the_wake_word(&mouth, t);
        t += 30;
        assert!(Instant::now() < until, "a working microphone never brought the wake word back");
        std::thread::sleep(Duration::from_millis(100));
    }
    let said = mouth.0.lock().unwrap().clone();
    assert!(said.iter().any(|s| s.contains("The microphone works again")), "{said:?}");
}

#[test]
fn a_headset_can_be_cut_into_by_voice_and_speakers_stay_as_set() {
    let b = atlas::micthread::BargeInConfig::default();
    assert!(!b.enabled);
    assert!(b.for_microphone("Headset (Jordan's AirPods)").enabled);
    assert!(b.for_microphone("Microphone (AirPods Hands-Free AG Audio)").enabled);
    assert!(!b.for_microphone("Microphone Array (Intel\u{ae} Smart Sound Technology for Digital Microphones)").enabled);
    let off = atlas::micthread::BargeInConfig { headset: false, ..b };
    assert!(!off.for_microphone("Headset (AirPods)").enabled);
}

#[test]
fn the_words_after_the_name_are_the_request() {
    use atlas::voice::words_after_name;
    assert_eq!(words_after_name("Atlas, what time is it?", "atlas").as_deref(), Some("what time is it?"));
    assert_eq!(words_after_name("Hey, Atlas! Open my notes.", "hey atlas").as_deref(), Some("Open my notes."));
    assert_eq!(words_after_name("Atlas.", "atlas").as_deref(), Some(""));
    assert_eq!(words_after_name("so I told Atlas to wait", "atlas").as_deref(), Some("to wait"));
    assert_eq!(words_after_name("open Atlassian", "atlas"), None);
    assert_eq!(words_after_name("what time is it", "atlas"), None);
}

// ================= the real speech engine, when it's here =================

/// whisper-cli and a model, from ATLAS_TEST_WHISPER / ATLAS_TEST_WHISPER_MODEL.
fn whisper() -> Option<(String, String)> {
    let w = std::env::var("ATLAS_TEST_WHISPER").ok()?;
    let m = std::env::var("ATLAS_TEST_WHISPER_MODEL").ok()?;
    (Path::new(&w).exists() && Path::new(&m).exists()).then_some((w, m))
}

/// The real `VoiceWork`: its recorder is a script that plays `audio` as the
/// microphone would stream it, its speech engine the real whisper with the
/// arguments tools.yaml gives it.
fn real_ears(dir: &Path, audio: &[i16], w: &str, m: &str) -> atlas::voice::ToolsConfig {
    let raw = dir.join("mic.raw");
    let bytes: Vec<u8> = audio.iter().flat_map(|s| s.to_le_bytes()).collect();
    std::fs::write(&raw, bytes).unwrap();
    let rec = dir.join("mic.sh");
    std::fs::write(&rec, format!("#!/bin/sh\ncat '{}'\n", raw.display())).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&rec, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let yaml = format!(
        "enabled: true\nwork_dir: '{work}'\nvars:\n  mic_device: 'the test microphone'\n  whisper: '{w}'\n  stt_model: '{m}'\n\
         record:\n  command: '{rec}'\n  args: []\n\
         stt:\n  command: '{{whisper}}'\n  args: ['-m', '{{stt_model}}', '-f', '{{in_wav}}', '-otxt', '-of', '{{stem}}', '-nt', '{{hint_opt}}', '{{hint_val}}']\n  result_file: '{{transcript}}'\n\
         wake:\n  enabled: true\n  phrase: atlas\n  clip_seconds: 3\n",
        work = dir.join("work").display(),
        rec = rec.display(),
    );
    serde_yaml::from_str(&yaml).unwrap()
}

#[test]
fn with_the_real_speech_engine_atlas_what_time_is_it_is_heard_as_a_request() {
    let Some((w, m)) = whisper() else {
        eprintln!("SKIPPED: no whisper here (set ATLAS_TEST_WHISPER and ATLAS_TEST_WHISPER_MODEL to run it)");
        return;
    };
    let dir = tmp("whisper");
    let audio = then(&[room(700, 20), wake_clip("atlas_what_time"), room(3000, 21)]);
    let t = real_ears(&dir, &audio, &w, &m);
    let work = atlas::voice::Voice::new(&t).mic_work();
    let mic = MicThread::start(Box::new(work));
    mic.set_wake(true);
    let got = first_heard(&mic, Duration::from_secs(60)).expect("nothing came through");
    let Heard::Wake(Ok(said)) = got else { panic!("{got:?}") };
    println!("LIVE [whisper] the request heard: {said:?}");
    assert!(atlas::voice::loose(&said).contains("whattimeisit"), "{said:?}");

    let dir = tmp("whisper-alone");
    let audio = then(&[room(700, 22), wake_clip("atlas_alone"), room(4000, 23)]);
    let t = real_ears(&dir, &audio, &w, &m);
    let mic = MicThread::start(Box::new(atlas::voice::Voice::new(&t).mic_work()));
    mic.set_wake(true);
    assert_eq!(first_heard(&mic, Duration::from_secs(60)), Some(Heard::Named));
}

