//! The microphone on its own thread (28 Sep 2026).
//!
//! 1. The wake word's recording used to block the run loop -- and with it
//!    the hub, the typing box and the icon's Pause -- for three seconds at a
//!    time. A pass of the loop now returns at once while the clip records,
//!    and the wake (with what was said after it) arrives over a channel.
//! 2. Cutting in by voice while Atlas speaks, with the real Silero model
//!    (tests/fixtures/silero, MIT) run by tract, on synthesized speech from
//!    the round-5 corpus; and Atlas's own detector when the model is absent.
//! 3. Pause turns the microphone off: nothing recorded until resume.
//!
//! The recorders here are stand-ins for ffmpeg (there is no microphone in a
//! test), but everything they drive is the real thread, the real loop pass,
//! the real detector and the real reply path.

use atlas::config::Config;
use atlas::daemon::{Daemon, Ears, Mouth};
use atlas::micthread::{self, BargeGate, BargeInConfig, Heard, MicStream, MicThread, MicWork, VoiceDetector};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::server::Action;
use atlas::store::Store;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Setting up
// ---------------------------------------------------------------------------

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-micthread-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn cfg(barge: bool) -> Config {
    let mut c = Config::load(Path::new("config")).unwrap();
    let t = c.tools.get_or_insert_with(Default::default);
    t.wake = Some(atlas::voice::WakeConfig { enabled: true, phrase: "atlas".into(), clip_seconds: 3, detector: None, listen_first: false });
    t.barge_in = BargeInConfig { enabled: barge, ..BargeInConfig::default() };
    c
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

fn wav(name: &str) -> Vec<i16> {
    let (s, r) = atlas::diarize::read_wav(&std::fs::read(format!("tests/fixtures/speech/corpus/{name}.wav")).unwrap()).unwrap();
    assert_eq!(r, 16000);
    s
}

fn silero() -> PathBuf {
    PathBuf::from("tests/fixtures/silero").join(micthread::SILERO_FILE)
}

/// Pink-ish room noise at about `db` dBFS.
fn room(n: usize, db: f32, seed: u64) -> Vec<f32> {
    let mut x = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    let mut lp = 0f32;
    let amp = 32768.0 * 10f32.powf(db / 20.0) * 3.0;
    (0..n)
        .map(|_| {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let w = ((x >> 33) as f32 / (1u64 << 31) as f32) - 1.0;
            lp = 0.9 * lp + 0.1 * w;
            lp * amp
        })
        .collect()
}

fn gain(s: &[i16], db: f32) -> Vec<f32> {
    let k = 10f32.powf(db / 20.0);
    s.iter().map(|v| *v as f32 * k).collect()
}

fn mix(parts: &[(&[f32], usize)], len: usize) -> Vec<i16> {
    let mut out = vec![0f32; len];
    for (p, at) in parts {
        for (i, v) in p.iter().enumerate() {
            if at + i < len {
                out[at + i] += v;
            }
        }
    }
    out.iter().map(|v| v.clamp(-32767.0, 32767.0) as i16).collect()
}

// ---------------------------------------------------------------------------
// Stand-ins for the recorder
// ---------------------------------------------------------------------------

#[derive(Default)]
struct MicState {
    recording: AtomicBool,
    attempts: AtomicUsize,
    streams: AtomicUsize,
    transcribed: AtomicUsize,
}

/// A wake-word clip that takes `clip_ms` to record, as ffmpeg's does, and
/// hears the wake word on attempt `fire_at`. The stream numbered
/// `speech_on` carries `speech` (paced in real time); the others are quiet
/// room. (28 Sep 2026: with cutting in by voice on, the thread's first
/// stream measures the room while nothing is going on, so the one watched
/// while Atlas speaks is the second.)
struct FakeMic {
    st: Arc<MicState>,
    clip_ms: u64,
    fire_at: usize,
    said: String,
    speech: Arc<Vec<i16>>,
    speech_on: usize,
    heard_words: String,
}

impl MicWork for FakeMic {
    fn wake_once(&mut self, stop: &dyn Fn() -> bool) -> atlas::error::Result<bool> {
        let n = self.st.attempts.fetch_add(1, Ordering::SeqCst) + 1;
        self.st.recording.store(true, Ordering::SeqCst);
        let until = Instant::now() + Duration::from_millis(self.clip_ms);
        while Instant::now() < until {
            if stop() {
                self.st.recording.store(false, Ordering::SeqCst);
                return Ok(false);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        self.st.recording.store(false, Ordering::SeqCst);
        Ok(n >= self.fire_at)
    }
    fn listen(&mut self) -> atlas::error::Result<String> {
        Ok(self.said.clone())
    }
    fn open_stream(&mut self) -> Option<Box<dyn MicStream>> {
        let n = self.st.streams.fetch_add(1, Ordering::SeqCst);
        let samples = if n == self.speech_on { (*self.speech).clone() } else { mix(&[(&room(16000 * 4, -60.0, 9), 0)], 16000 * 4) };
        Some(Box::new(Paced { samples, at: 0, started: Instant::now() }))
    }
    fn transcribe(&mut self, samples: &[i16]) -> atlas::error::Result<String> {
        assert!(samples.len() > 16000 / 2, "what was kept of your words is too short: {}", samples.len());
        self.st.transcribed.fetch_add(1, Ordering::SeqCst);
        Ok(self.heard_words.clone())
    }
    fn follow_up(&mut self, _secs: u32, _stop: &dyn Fn() -> bool) -> atlas::error::Result<Option<String>> {
        Ok(None)
    }
    fn models_dir(&self) -> PathBuf {
        PathBuf::from("tests/fixtures/silero")
    }
}

/// Audio handed over no faster than it would be recorded.
struct Paced {
    samples: Vec<i16>,
    at: usize,
    started: Instant,
}

impl MicStream for Paced {
    fn read(&mut self, n: usize) -> Option<Vec<i16>> {
        if self.at + n > self.samples.len() {
            return None;
        }
        let due = Duration::from_micros(((self.at + n) as u64 * 1_000_000) / 16000);
        if let Some(wait) = due.checked_sub(self.started.elapsed()) {
            std::thread::sleep(wait);
        }
        let w = self.samples[self.at..self.at + n].to_vec();
        self.at += n;
        Some(w)
    }
}

struct FakeEars {
    st: Arc<MicState>,
    clip_ms: u64,
    fire_at: usize,
    speech: Arc<Vec<i16>>,
    speech_on: usize,
}

impl Ears for FakeEars {
    fn wait_for_wake(&self) -> atlas::error::Result<()> {
        Ok(())
    }
    fn listen(&self) -> atlas::error::Result<String> {
        Ok(String::new())
    }
    fn listen_briefly(&self, _secs: u32) -> atlas::error::Result<Option<String>> {
        Ok(None)
    }
    fn mic_work(&self) -> Option<Box<dyn MicWork>> {
        Some(Box::new(FakeMic {
            st: self.st.clone(),
            clip_ms: self.clip_ms,
            fire_at: self.fire_at,
            said: "what time is it".into(),
            speech: self.speech.clone(),
            speech_on: self.speech_on,
            heard_words: "what day is it".into(),
        }))
    }
}

fn ears(clip_ms: u64, fire_at: usize) -> (FakeEars, Arc<MicState>) {
    let st = Arc::new(MicState::default());
    (FakeEars { st: st.clone(), clip_ms, fire_at, speech: Arc::new(Vec::new()), speech_on: 0 }, st)
}

/// A speaker whose first line plays for up to `first_ms` (long enough, in
/// the cutting-in test, for a debug-build Silero sharing two CPUs with the
/// rest of the suite to catch up with the microphone) and the rest for a
/// tenth of a second, each stopped early when the reply is cut -- as the
/// real player is killed.
struct Speaker {
    first_ms: u64,
    said: Mutex<Vec<(String, bool)>>,
}

impl Default for Speaker {
    fn default() -> Self {
        Speaker { first_ms: 100, said: Mutex::new(Vec::new()) }
    }
}

impl Mouth for Speaker {
    fn speak(&self, text: &str) -> atlas::error::Result<()> {
        let first = self.said.lock().unwrap().is_empty();
        let until = Instant::now() + Duration::from_millis(if first { self.first_ms } else { 100 });
        let mut cut = false;
        while Instant::now() < until {
            if micthread::playback_cut() {
                cut = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        self.said.lock().unwrap().push((text.to_string(), cut));
        Ok(())
    }
}

/// The two tests that speak a reply with the player's stop switch (one
/// switch for the whole process, as there is one player) take turns -- with
/// each other and with `speaking_off_the_loop`'s.
pub(crate) static ONE_PLAYER: Mutex<()> = Mutex::new(());

fn wait_until(what: &str, secs: u64, mut f: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(secs);
    while !f() {
        assert!(Instant::now() < until, "waited {secs}s for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

// ================= 1. the wake word off the loop =================

#[test]
fn a_pass_of_the_loop_returns_at_once_while_the_wake_clip_records_and_the_wake_arrives_by_channel() {
    let (c, p) = (cfg(false), plat());
    let mut d = daemon(&c, &p, "fast");
    let (e, st) = ears(1500, 1);
    let mouth = Speaker::default();
    let clock = || 100u64;

    let t = Instant::now();
    assert!(!d.listen_pass(&e, &mouth, &clock), "nothing heard yet");
    crate::common::assert_prompt(t.elapsed(), Duration::from_millis(200), "starting the microphone took");
    wait_until("the wake clip to start recording", 2, || st.recording.load(Ordering::SeqCst));

    // Mid-recording: a pass takes no time, and the hub is answered.
    for _ in 0..5 {
        let t = Instant::now();
        assert!(!d.listen_pass(&e, &mouth, &clock));
        crate::common::assert_prompt(t.elapsed(), Duration::from_millis(50), "a pass waited on the microphone");
        assert!(d.mic_recording_for_test(), "the clip should still be recording");
        std::thread::sleep(Duration::from_millis(20));
    }
    let t = Instant::now();
    let r = atlas::hublive::reply(&mut d, Action::GlanceJson);
    assert_eq!(r.status, 200);
    assert!(st.recording.load(Ordering::SeqCst), "the hub was answered while the clip recorded");
    crate::common::assert_prompt(t.elapsed(), Duration::from_millis(1500), "the hub took");

    // The wake, and what was said after it, arrive through the channel and
    // are answered.
    let t = Instant::now();
    wait_until("the wake word to be taken", 5, || d.listen_pass(&e, &mouth, &clock));
    assert!(t.elapsed() >= Duration::from_millis(500), "the wake came before the clip had finished");
    assert!(!mouth.said.lock().unwrap().is_empty(), "the wake's turn wasn't answered");
    // Listening again for the next one.
    wait_until("the next wake clip", 3, || st.attempts.load(Ordering::SeqCst) >= 2);
}

#[test]
fn the_thread_hands_over_the_wake_with_what_was_said_after_it() {
    let e = FakeMic {
        st: Arc::new(MicState::default()),
        clip_ms: 50,
        fire_at: 2,
        said: "open my notes".into(),
        speech: Arc::new(Vec::new()),
        speech_on: 0,
        heard_words: String::new(),
    };
    let m = MicThread::start(Box::new(e));
    m.set_wake(true);
    let until = Instant::now() + crate::common::allowed(Duration::from_secs(3));
    let got = loop {
        if let Some(h) = m.poll() {
            break h;
        }
        assert!(Instant::now() < until, "no wake came through the channel");
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(got, Heard::Wake(Ok("open my notes".into())));
    // Not listening again until the loop has dealt with it.
    std::thread::sleep(Duration::from_millis(200));
    assert!(m.poll().is_none());
}

#[test]
fn stopping_atlas_stops_the_microphone_thread_mid_clip() {
    let (c, p) = (cfg(false), plat());
    let mut d = daemon(&c, &p, "stop");
    let (e, st) = ears(10_000, 99);
    let mouth = Speaker::default();
    d.listen_pass(&e, &mouth, &|| 100);
    wait_until("the clip to start", 2, || st.recording.load(Ordering::SeqCst));
    let t = Instant::now();
    let _ = d.shut_down();
    crate::common::assert_prompt(t.elapsed(), Duration::from_secs(2), "shutting down waited");
    assert!(d.mic_stopped_for_test(), "the microphone's thread outlived Atlas");
    assert!(!st.recording.load(Ordering::SeqCst), "the ten-second clip was still recording");
}

// ================= 3. Pause mutes the microphone =================

#[test]
fn pause_stops_the_recording_and_resume_starts_it_again() {
    let (c, p) = (cfg(false), plat());
    let mut d = daemon(&c, &p, "pause");
    let (e, st) = ears(10_000, 99);
    let mouth = Speaker::default();
    let clock = || 100u64;
    d.listen_pass(&e, &mouth, &clock);
    wait_until("the clip to start", 2, || st.recording.load(Ordering::SeqCst));

    // The hub's Pause, the same path as the icon's.
    let _ = atlas::hublive::reply(&mut d, Action::Pause(true));
    d.listen_pass(&e, &mouth, &clock);
    wait_until("the clip to stop on Pause", 1, || !st.recording.load(Ordering::SeqCst));
    let tries = st.attempts.load(Ordering::SeqCst);
    for _ in 0..20 {
        d.listen_pass(&e, &mouth, &clock);
        std::thread::sleep(Duration::from_millis(25));
        assert!(!st.recording.load(Ordering::SeqCst), "recorded while paused");
    }
    assert_eq!(st.attempts.load(Ordering::SeqCst), tries, "a clip was started while paused");
    assert!(!d.mic_recording_for_test());

    let _ = atlas::hublive::reply(&mut d, Action::Pause(false));
    d.listen_pass(&e, &mouth, &clock);
    wait_until("listening again after resume", 2, || st.recording.load(Ordering::SeqCst));
    let _ = d.shut_down();
}

#[test]
fn the_icon_and_the_hub_say_pause_turns_the_microphone_off() {
    use atlas::notifyicon as tray;
    assert!(tray::RUNNING_ENTRY.contains("stop listening"), "{}", tray::RUNNING_ENTRY);
    assert!(tray::PAUSED_ENTRY.contains("listen again"), "{}", tray::PAUSED_ENTRY);
    assert!(tray::tray_tooltip(true).contains("not listening"));
    let (c, p) = (cfg(false), plat());
    let mut d = daemon(&c, &p, "words");
    let r = atlas::hublive::reply(&mut d, Action::Pause(true));
    assert!(r.body.contains("microphone") || r.body.contains("said="), "{}", r.body);
}

// ================= 2. cutting in by voice =================

fn probs(det: &mut dyn VoiceDetector, s: &[i16]) -> Vec<f32> {
    s.chunks_exact(micthread::WINDOW).map(|w| det.speech(w).unwrap_or(0.0)).collect()
}

#[test]
fn silero_runs_through_tract_and_tells_speech_from_noise() {
    let (mut det, why) = micthread::detector_for(Some(&silero()));
    assert_eq!(why, None, "the Silero model should load here");
    assert_eq!(det.name(), "Silero VAD");
    let speech = probs(&mut *det, &wav("A_s1"));
    let loud = speech.iter().filter(|p| **p >= 0.5).count() as f32 / speech.len() as f32;
    assert!(loud > 0.5, "Silero heard speech in only {:.0}% of a spoken sentence", loud * 100.0);
    det.reset();
    let noise = mix(&[(&room(16000 * 3, -30.0, 3), 0)], 16000 * 3);
    let fooled = probs(&mut *det, &noise).iter().filter(|p| **p >= 0.5).count();
    assert_eq!(fooled, 0, "Silero took room noise for a voice");
}

#[test]
fn a_missing_or_broken_model_falls_back_to_atlas_own_detector() {
    let (det, why) = micthread::detector_for(Some(Path::new("tests/fixtures/silero/not-here.onnx")));
    assert_eq!(det.name(), "Atlas's own voice detector");
    assert!(why.unwrap().contains("isn't in the models folder"));
    let junk = tmp("junk").join("junk.onnx");
    std::fs::write(&junk, b"not a model").unwrap();
    let (det, why) = micthread::detector_for(Some(&junk));
    assert_eq!(det.name(), "Atlas's own voice detector");
    assert!(why.is_some());
    let (det, _) = micthread::detector_for(None);
    assert_eq!(det.name(), "Atlas's own voice detector");
}

/// Fed through the gate a window at a time, with Atlas's reply envelope when
/// there is one. Returns when it fired (ms) and the gate, for what it learned.
fn fires_at(det: &mut dyn VoiceDetector, s: &[i16], envelope: Option<&[u8]>) -> (Option<u32>, BargeGate) {
    let mut gate = BargeGate::new(&BargeInConfig { enabled: true, ..BargeInConfig::default() });
    for (i, w) in s.chunks_exact(micthread::WINDOW).enumerate() {
        let playing = envelope.map(|e| e.get(i).copied().unwrap_or(0) as f32 / 255.0);
        if gate.feed(det.speech(w), atlas::audio::level_db(w), playing) {
            return (Some((i as u32 + 1) * 32), gate);
        }
    }
    (None, gate)
}

/// Atlas's reply `echo` as its envelope says it plays (starting 250 ms in),
/// and as the microphone hears it from the speakers: `echo_db` down, `lag`
/// windows late, over a quiet room -- and, if given, you starting at
/// `you_at` samples, `you_db` louder than the echo.
fn scene(echo: &[i16], echo_db: Option<f32>, lag: usize, you: Option<(&[i16], usize, f32)>, seed: u64) -> (Vec<i16>, Vec<u8>) {
    let start = 4000;
    let mut padded = vec![0i16; start];
    padded.extend_from_slice(echo);
    let env = atlas::speaking::levels_of_wav(&atlas::audio::wav_bytes(&padded, 16000), 32).unwrap();
    let late = start + lag * micthread::WINDOW;
    let len = (late + echo.len()).max(you.map(|(y, at, _)| at + y.len()).unwrap_or(0)) + 4000;
    let r = room(len, -55.0, seed);
    let e = gain(echo, echo_db.unwrap_or(-200.0));
    let y = you.map(|(y, _, db)| gain(y, echo_db.unwrap_or(-12.0) + db)).unwrap_or_default();
    let mut parts: Vec<(&[f32], usize)> = vec![(&r, 0), (&e, late)];
    if let Some((_, at, _)) = you {
        parts.push((&y, at));
    }
    (mix(&parts, len), env)
}

#[test]
fn you_speaking_over_a_quiet_room_is_heard_in_about_a_third_of_a_second() {
    let you = wav("B_s3");
    let len = 16000 + you.len();
    let s = mix(&[(&room(len, -55.0, 1), 0), (&gain(&you, 0.0), 16000)], len);
    for (name, mut det) in [
        ("Silero", micthread::detector_for(Some(&silero())).0),
        ("Atlas's own", micthread::detector_for(None).0),
    ] {
        let at = fires_at(&mut *det, &s, None).0.unwrap_or_else(|| panic!("{name}: never heard you"));
        assert!(at >= 1000 + 300, "{name}: fired at {at} ms, before you had said 300 ms");
        assert!(at <= 1000 + 1500, "{name}: took until {at} ms");
    }
}

#[test]
fn atlas_hearing_its_own_voice_through_the_speakers_does_not_cut_it_off_with_silero() {
    // Atlas's voice from the speakers, 12 dB down and heard 192 ms late (a
    // recorder's buffering): never taken for you.
    let echo = wav("A_s1");
    let (alone, env) = scene(&echo, Some(-12.0), 6, None, 2);
    let mut det = micthread::detector_for(Some(&silero())).0;
    let (at, gate) = fires_at(&mut *det, &alone, Some(&env));
    assert_eq!(at, None, "Atlas cut itself off on its own voice");
    let lag = gate.lag_windows().expect("how late the microphone hears the reply wasn't found");
    assert!((5..=7).contains(&lag), "found the microphone {lag} windows late; it was 6");

    // You, 9 dB louder than that echo, a second and a half into the reply:
    // heard.
    let you = wav("B_s3");
    let (over, env) = scene(&echo, Some(-12.0), 6, Some((&you, 24000, 9.0)), 2);
    let mut det = micthread::detector_for(Some(&silero())).0;
    let at = fires_at(&mut *det, &over, Some(&env)).0.expect("you, speaking over Atlas, weren't heard");
    assert!(at > 1500, "fired at {at} ms, before you started");
}

#[test]
fn with_a_headset_the_reply_is_not_heard_and_you_can_cut_in_once_it_is_under_way() {
    // The reply plays (the envelope says so) but the microphone hears only
    // the room, then you.
    let echo = wav("C_s2");
    let you = wav("D_s5");
    let (s, env) = scene(&echo, None, 0, Some((&you, 24000, -6.0)), 5);
    let mut det = micthread::detector_for(Some(&silero())).0;
    let (at, gate) = fires_at(&mut *det, &s, Some(&env));
    let at = at.expect("with a headset, you weren't heard over the reply");
    assert!(at > 1500, "fired at {at} ms, before you started");
    assert_eq!(gate.lag_windows(), Some(0), "no echo should have been learned as 'no echo'");
}

#[test]
fn speaking_over_a_reply_stops_it_and_what_you_said_is_answered_next() {
    let (c, p) = (cfg(true), plat());
    let mut d = daemon(&c, &p, "barge");
    // What the microphone hears while Atlas speaks: half a second of room,
    // then you.
    let you = wav("B_s3");
    let len = 8000 + you.len() + 16000;
    let speech = mix(&[(&room(len, -55.0, 4), 0), (&gain(&you, 0.0), 8000)], len);
    let st = Arc::new(MicState::default());
    // Stream 0 measures the room when the thread starts; stream 1 is the
    // one watched while Atlas speaks.
    let e = FakeEars { st: st.clone(), clip_ms: 10_000, fire_at: 99, speech: Arc::new(speech), speech_on: 1 };
    let _one = ONE_PLAYER.lock().unwrap_or_else(|e| e.into_inner());
    let mouth = Speaker { first_ms: 30_000, ..Speaker::default() };
    let clock = || 100u64;
    d.listen_pass(&e, &mouth, &clock);
    micthread::clear_cut();

    d.converse("what time is it", &e, &mouth, &clock);

    let said = mouth.said.lock().unwrap().clone();
    assert!(said.first().is_some_and(|(_, cut)| *cut), "the first line wasn't cut off: {said:?}");
    assert_eq!(st.transcribed.load(Ordering::SeqCst), 1, "what you said over it wasn't taken");
    // The room and the first reply were watched; and the answer to what you
    // said was spoken. 29 Sep 2026: this counted a third microphone stream,
    // for the answer -- which opened only because "Paused." was said first
    // and gave the microphone's thread time to finish with your words.
    // Taking the turn is answered without "Paused." now (`speech::YOUR_TURN`),
    // and this test's answer (the time) comes back instantly, so it can be
    // said before the thread is watching again; a real answer waits on the
    // model. What the test is about -- you were heard over the reply, it
    // stopped, and what you said was answered -- is asserted directly.
    assert!(st.streams.load(Ordering::SeqCst) >= 2, "the reply wasn't watched: {said:?}");
    assert!(said.len() >= 2, "no second reply was spoken (the cut-in wasn't answered): {said:?}");
    // Both chats' version of this test (merged 30 Sep 2026): no "Paused."
    // before the answer to words said over a reply.
    assert!(!said.iter().any(|(l, _)| l == "Paused."), "\"Paused.\" was said before the answer: {said:?}");
    assert!(said.iter().skip(1).any(|(_, cut)| !cut), "the answer to what you said was cut too: {said:?}");
    assert!(!st.recording.load(Ordering::SeqCst) || st.attempts.load(Ordering::SeqCst) > 0);
    let _ = d.shut_down();
    micthread::clear_cut();
}

#[test]
fn cutting_in_by_voice_is_off_unless_turned_on() {
    assert!(!BargeInConfig::default().enabled);
    let (c, p) = (cfg(false), plat());
    let mut d = daemon(&c, &p, "off");
    let st = Arc::new(MicState::default());
    let you = wav("B_s3");
    let e = FakeEars { st: st.clone(), clip_ms: 10_000, fire_at: 99, speech: Arc::new(you), speech_on: 0 };
    let _one = ONE_PLAYER.lock().unwrap_or_else(|e| e.into_inner());
    let mouth = Speaker::default();
    let clock = || 100u64;
    d.listen_pass(&e, &mouth, &clock);
    micthread::clear_cut();
    d.converse("what time is it", &e, &mouth, &clock);
    assert_eq!(st.streams.load(Ordering::SeqCst), 0, "the microphone was opened while Atlas spoke with cutting in off");
    assert!(mouth.said.lock().unwrap().iter().all(|(_, cut)| !cut));
    let _ = d.shut_down();
}

#[test]
fn the_setting_is_in_the_list_and_applies_without_a_restart() {
    let s = atlas::settings::registry(&atlas::voice::ToolsConfig::default());
    let item = s.items.iter().find(|i| i.key == "barge_in.enabled").expect("no setting for cutting in by voice");
    assert!(!atlas::settings::needs_a_restart(&item.key));
}


/// Measured across the corpus: Atlas's reply in each of six voices through
/// the speakers at three strengths and three delays, alone (must never cut
/// itself off) and with you -- another voice, 9 dB louder than the echo,
/// starting 1.5 s into the reply -- speaking over it. Synthesized voices, a
/// steady room and an echo that is a clean delayed copy: the real thing adds
/// reverberation and a speaker's colouring, so these numbers are a ceiling.
///
/// Ignored in the ordinary run because Silero in a debug build takes about
/// ten minutes over the lot; run it with
/// `-- --ignored across_voices --nocapture`. Measured 28 Sep 2026: cut
/// itself off 0/54, heard you 54/54 (21 minutes on a loaded two-CPU box).
#[test]
#[ignore]
fn across_voices_echo_strengths_and_delays_atlas_does_not_cut_itself_off_and_you_are_heard() {
    let voices = ["A", "B", "C", "D", "E", "F"];
    let (mut false_cuts, mut heard, mut tries) = (0, 0, 0);
    for (i, v) in voices.iter().enumerate() {
        let echo = wav(&format!("{v}_s{}", i % 10));
        let you = wav(&format!("{}_s{}", voices[(i + 1) % 6], (i + 3) % 10));
        for echo_db in [-6.0f32, -12.0, -20.0] {
            for lag in [0usize, 4, 10] {
                let (alone, env) = scene(&echo, Some(echo_db), lag, None, i as u64);
                let mut det = micthread::detector_for(Some(&silero())).0;
                if let (Some(at), _) = fires_at(&mut *det, &alone, Some(&env)) {
                    println!("{v} at {echo_db} dB, {} ms late: cut itself off at {at} ms", lag * 32);
                    false_cuts += 1;
                }
                let (over, env) = scene(&echo, Some(echo_db), lag, Some((&you, 24000, 9.0)), i as u64);
                let mut det = micthread::detector_for(Some(&silero())).0;
                match fires_at(&mut *det, &over, Some(&env)).0 {
                    Some(at) if at > 1500 => heard += 1,
                    other => println!("{v} at {echo_db} dB, {} ms late: you not heard ({other:?})", lag * 32),
                }
                tries += 1;
            }
        }
    }
    println!("Silero: cut itself off {false_cuts}/{tries}, heard you over it {heard}/{tries}");
    assert_eq!(false_cuts, 0, "Atlas cut itself off on its own voice");
    assert!(heard * 10 >= tries * 9, "you were heard over Atlas only {heard}/{tries} times");
}
