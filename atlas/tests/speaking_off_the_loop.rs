//! Speaking off the loop (28 Sep 2026, round four).
//!
//! 1. A sentence being said held the run loop -- and the hub, the typing box
//!    and the icon's Pause with it -- for as long as it took to say. Now the
//!    reply plays on its own thread (`speakthread`) and the loop answers the
//!    hub while it plays.
//! 2. The open floor after a spoken reply (six to twenty seconds of
//!    recording) held the loop the same way. Now the microphone's thread
//!    records it and the loop answers the hub meanwhile.
//! 3. Watching for your voice started again for every sentence of a reply
//!    the model was still writing (a new recorder and a quarter-second of
//!    room each time). Now it is one watch for the whole reply, the room is
//!    measured while nothing is going on, and what was learned about the
//!    speakers and the microphone is kept for the next start.
//! 4. "Carry on" after a cut counted the sentence you heard half of as said.
//!    Now it starts with that sentence.
//!
//! The players and recorders are stand-ins (no speakers or microphone in a
//! test), but everything they drive is the real loop, the real threads and
//! the real hub.

use atlas::brain::{ChatReply, ChatRequest, Llm};
use atlas::daemon::{Daemon, Ears, Mouth};
use atlas::micthread::{self, BargeInConfig, MicStream, MicWork};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::speakthread::SpeakWork;
use atlas::store::Store;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-speaking-off-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn cfg(barge: bool) -> atlas::config::Config {
    let mut c = atlas::config::Config::load(Path::new("config")).unwrap();
    let t = c.tools.get_or_insert_with(Default::default);
    t.barge_in = BargeInConfig { enabled: barge, ..BargeInConfig::default() };
    c
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

/// The player's stop switch is one for the whole process (there is one
/// player), so the tests here that speak take turns with each other and
/// with the microphone tests.
use crate::the_microphone_has_its_own_thread::ONE_PLAYER;

// ---------------------------------------------------------------------------
// A model that writes its reply a word at a time
// ---------------------------------------------------------------------------

struct Talker {
    reply: String,
    word_ms: u64,
}

impl Llm for Talker {
    fn complete(&self, _: &str, _: &str) -> atlas::error::Result<String> {
        Ok(r#"{"action":"say","arg":null,"say":"(the one-prompt path)"}"#.into())
    }
    fn native_chat(&self) -> bool {
        true
    }
    fn chat(&self, req: &ChatRequest, on_text: &mut dyn FnMut(&str) -> bool) -> atlas::error::Result<ChatReply> {
        let about = req.messages.last().map(|m| m.content.rsplit("User said: ").next().unwrap_or("").to_string()).unwrap_or_default();
        let reply = format!("On {about}: {}", self.reply);
        for w in reply.split_inclusive(' ') {
            if !on_text(w) {
                break;
            }
            std::thread::sleep(Duration::from_millis(self.word_ms));
        }
        Ok(ChatReply { text: reply, tool_calls: vec![] })
    }
}

// ---------------------------------------------------------------------------
// A slow voice that can be played on a thread
// ---------------------------------------------------------------------------

/// What the player did: each sentence, and whether it was cut short.
#[derive(Default)]
struct Played {
    lines: Mutex<Vec<(String, bool)>>,
    /// The sentence playing right now, if any.
    now: Mutex<Option<String>>,
    threads: Mutex<Vec<String>>,
}

/// Each sentence takes `ms` to say (the first `first_ms`), stopped early when
/// the reply is cut -- as the real player is killed.
struct SlowWork {
    played: Arc<Played>,
    first_ms: u64,
    ms: u64,
}

impl SpeakWork for SlowWork {
    fn speak(&mut self, text: &str) -> atlas::error::Result<()> {
        let first = self.played.lines.lock().unwrap().is_empty();
        self.played.threads.lock().unwrap().push(std::thread::current().name().unwrap_or("").to_string());
        *self.played.now.lock().unwrap() = Some(text.to_string());
        let until = Instant::now() + Duration::from_millis(if first { self.first_ms } else { self.ms });
        let mut cut = false;
        while Instant::now() < until {
            if micthread::playback_cut() {
                cut = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        *self.played.now.lock().unwrap() = None;
        self.played.lines.lock().unwrap().push((text.to_string(), cut));
        Ok(())
    }
}

struct SlowMouth {
    played: Arc<Played>,
    first_ms: u64,
    ms: u64,
}

impl SlowMouth {
    fn new(first_ms: u64, ms: u64) -> SlowMouth {
        SlowMouth { played: Arc::new(Played::default()), first_ms, ms }
    }
    fn lines(&self) -> Vec<(String, bool)> {
        self.played.lines.lock().unwrap().clone()
    }
}

impl Mouth for SlowMouth {
    /// Only reached for lines said outside a reply (`Daemon::say`).
    fn speak(&self, text: &str) -> atlas::error::Result<()> {
        SlowWork { played: self.played.clone(), first_ms: self.ms, ms: self.ms }.speak(text)
    }
    fn speak_work(&self) -> Option<Box<dyn SpeakWork>> {
        Some(Box::new(SlowWork { played: self.played.clone(), first_ms: self.first_ms, ms: self.ms }))
    }
}

// ---------------------------------------------------------------------------
// The hub, asked for a page every 100 ms
// ---------------------------------------------------------------------------

const TOKEN: &str = "abcdef-ghjkmn-pqrstu-vwxyz2";

struct Asker {
    stop: Arc<AtomicBool>,
    handle: std::thread::JoinHandle<Vec<(Instant, Duration)>>,
}

fn hub_on(d: &mut Daemon) -> Asker {
    let server = atlas::server::Server::bind(&atlas::server::ServerConfig { enabled: true, port: 0, ..Default::default() }, TOKEN).expect("bind");
    let port = server.port();
    d.hub_server = Some(server.threaded().expect("threaded"));
    let stop = Arc::new(AtomicBool::new(false));
    let s2 = stop.clone();
    let handle = std::thread::spawn(move || {
        let mut waits = Vec::new();
        while !s2.load(Ordering::SeqCst) {
            let started = Instant::now();
            let mut conn = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            conn.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
            conn.write_all(format!("GET /hub/glance.json HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Atlas-Token: {TOKEN}\r\n\r\n").as_bytes()).unwrap();
            let mut out = Vec::new();
            let _ = conn.read_to_end(&mut out);
            waits.push((started, started.elapsed()));
            std::thread::sleep(Duration::from_millis(100));
        }
        waits
    });
    std::thread::sleep(Duration::from_millis(150));
    Asker { stop, handle }
}

/// Stop asking, answering what is still waiting as `Daemon::run` would.
fn hub_off(d: &mut Daemon, a: Asker) -> Vec<(Instant, Duration)> {
    a.stop.store(true, Ordering::SeqCst);
    let door = d.hub_server.take().unwrap();
    while !a.handle.is_finished() {
        door.wait_and_answer(20, &mut |x| atlas::hublive::reply(d, x));
    }
    a.handle.join().unwrap()
}

struct NoEars;

impl Ears for NoEars {
    fn wait_for_wake(&self) -> atlas::error::Result<()> {
        Ok(())
    }
    fn listen(&self) -> atlas::error::Result<String> {
        Ok(String::new())
    }
    fn listen_briefly(&self, _secs: u32) -> atlas::error::Result<Option<String>> {
        Ok(None)
    }
}

// ================= 1. the hub while a long sentence plays =================

#[test]
fn the_hub_is_answered_while_a_long_sentence_is_being_said() {
    let _one = ONE_PLAYER.lock().unwrap_or_else(|e| e.into_inner());
    micthread::clear_cut();
    let (c, p) = (cfg(false), plat());
    let llm = Arc::new(Talker { reply: "This first sentence is a long one and takes four seconds to say. Then a short one.".into(), word_ms: 0 });
    let mut d = Daemon::new(&c, &p, Some(llm as Arc<dyn Llm>), Store::new(tmp("long")), Proactive::new(ProactiveConfig::default()));
    let mouth = SlowMouth::new(4_000, 300);
    let asker = hub_on(&mut d);
    let started = Instant::now();
    d.converse("tell me something", &NoEars, &mouth, &|| 1_790_000_000);
    let talked = started.elapsed();
    let waits = hub_off(&mut d, asker);

    let lines = mouth.lines();
    assert!(lines.len() >= 2 && lines.iter().all(|(_, cut)| !cut), "{lines:?}");
    assert!(talked >= Duration::from_millis(4_000), "the long sentence wasn't waited for: {talked:?}");
    // Played on its own thread, not the loop's.
    assert!(mouth.played.threads.lock().unwrap().iter().all(|t| t == "atlas-speaking"), "{:?}", mouth.played.threads.lock().unwrap());
    // Pages asked for while the long sentence played were answered then,
    // not when it ended.
    let during: Vec<Duration> = waits.iter().filter(|(at, _)| at.duration_since(started) < Duration::from_millis(3_500)).map(|(_, w)| *w).collect();
    let worst = during.iter().max().copied().unwrap_or_default();
    println!("said {} sentence(s) in {talked:?}; {} page(s) asked for during the long one, the slowest answered in {worst:?}", lines.len(), during.len());
    assert!(during.len() >= 10, "only {} pages were answered in three and a half seconds of speaking", during.len());
    assert!(worst < Duration::from_millis(1_500), "a page waited {worst:?} while a four-second sentence played");
}

// ================= 4. carry on starts with the sentence that was cut =================

#[test]
fn carrying_on_after_a_cut_starts_with_the_sentence_that_was_cut() {
    let _one = ONE_PLAYER.lock().unwrap_or_else(|e| e.into_inner());
    micthread::clear_cut();
    let (c, p) = (cfg(false), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("carry")), Proactive::new(ProactiveConfig::default()));
    let mouth = SlowMouth::new(400, 1_500);
    let played = mouth.played.clone();
    // "Stop", said half-way through the second sentence.
    let mut listen = || {
        let now = played.now.lock().unwrap().clone();
        now.filter(|s| s.starts_with("Second")).map(|_| {
            std::thread::sleep(Duration::from_millis(300));
            "stop".to_string()
        })
    };
    let delivery = d.say_interruptibly(&mouth, "First, the short part. Second, the part you missed. Third, the end.", &mut listen);
    assert!(delivery.was_interrupted());
    assert_eq!(delivery.spoken, vec!["First, the short part.".to_string()], "the cut sentence was counted as said");
    assert_eq!(delivery.unspoken.first().map(String::as_str), Some("Second, the part you missed."));
    let lines = mouth.lines();
    assert!(lines.iter().any(|(s, cut)| s.starts_with("Second") && *cut), "the second sentence wasn't cut off mid-way: {lines:?}");
    assert!(!lines.iter().any(|(s, _)| s.starts_with("Third")), "the third was played after the stop: {lines:?}");

    // "Carry on" picks up at the start of the sentence that was cut.
    let rest = d.turn("carry on", 1_790_000_000);
    assert!(rest.starts_with("Second, the part you missed."), "carry on said: {rest:?}");
    assert!(rest.contains("Third, the end."), "{rest:?}");
    micthread::clear_cut();
}

// ================= 2. the open floor after a reply =================

#[derive(Default)]
struct FloorState {
    asked: AtomicUsize,
    recording: AtomicBool,
    streams: AtomicUsize,
}

/// The microphone's work: no wake word, and the open floor after a reply
/// takes `floor_ms` of recording before the first one hears `then`.
struct FloorMic {
    st: Arc<FloorState>,
    floor_ms: u64,
    then: String,
}

impl MicWork for FloorMic {
    fn wake_once(&mut self, _stop: &dyn Fn() -> bool) -> atlas::error::Result<bool> {
        std::thread::sleep(Duration::from_millis(20));
        Ok(false)
    }
    fn listen(&mut self) -> atlas::error::Result<String> {
        Ok(String::new())
    }
    fn open_stream(&mut self) -> Option<Box<dyn MicStream>> {
        self.st.streams.fetch_add(1, Ordering::SeqCst);
        None
    }
    fn transcribe(&mut self, _samples: &[i16]) -> atlas::error::Result<String> {
        Ok(String::new())
    }
    fn follow_up(&mut self, _secs: u32, stop: &dyn Fn() -> bool) -> atlas::error::Result<Option<String>> {
        let n = self.st.asked.fetch_add(1, Ordering::SeqCst);
        self.st.recording.store(true, Ordering::SeqCst);
        let until = Instant::now() + Duration::from_millis(self.floor_ms);
        while Instant::now() < until {
            if stop() {
                self.st.recording.store(false, Ordering::SeqCst);
                return Ok(None);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        self.st.recording.store(false, Ordering::SeqCst);
        Ok((n == 0).then(|| self.then.clone()))
    }
}

struct FloorEars {
    st: Arc<FloorState>,
    floor_ms: u64,
    on_the_loop: AtomicUsize,
}

impl Ears for FloorEars {
    fn wait_for_wake(&self) -> atlas::error::Result<()> {
        Ok(())
    }
    fn listen(&self) -> atlas::error::Result<String> {
        Ok(String::new())
    }
    fn listen_briefly(&self, _secs: u32) -> atlas::error::Result<Option<String>> {
        self.on_the_loop.fetch_add(1, Ordering::SeqCst);
        Ok(None)
    }
    fn mic_work(&self) -> Option<Box<dyn MicWork>> {
        Some(Box::new(FloorMic { st: self.st.clone(), floor_ms: self.floor_ms, then: "and what day is it".into() }))
    }
}

#[test]
fn the_hub_is_answered_while_the_floor_is_open_after_a_reply() {
    let _one = ONE_PLAYER.lock().unwrap_or_else(|e| e.into_inner());
    micthread::clear_cut();
    let (c, p) = (cfg(false), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("floor")), Proactive::new(ProactiveConfig::default()));
    let st = Arc::new(FloorState::default());
    let ears = FloorEars { st: st.clone(), floor_ms: 3_000, on_the_loop: AtomicUsize::new(0) };
    let mouth = SlowMouth::new(100, 100);
    let clock = || 1_790_000_000u64;
    // The microphone's thread starts with the first pass of the loop.
    d.listen_pass(&ears, &mouth, &clock);
    let asker = hub_on(&mut d);
    let started = Instant::now();
    d.converse("what time is it", &ears, &mouth, &clock);
    let took = started.elapsed();
    let waits = hub_off(&mut d, asker);
    let _ = d.shut_down();

    assert_eq!(ears.on_the_loop.load(Ordering::SeqCst), 0, "the open floor was recorded on the loop");
    assert!(st.asked.load(Ordering::SeqCst) >= 2, "the floor was opened {} time(s)", st.asked.load(Ordering::SeqCst));
    assert!(took >= Duration::from_millis(6_000), "two open floors of three seconds each took {took:?}");
    // What was said in the open floor was answered, as the next turn.
    assert!(d.thread.recent.iter().any(|e| e.said.contains("what day is it")), "the follow-up wasn't answered");
    let worst = waits.iter().map(|(_, w)| *w).max().unwrap_or_default();
    println!("{} page(s) asked for over {took:?} of conversation, the slowest answered in {worst:?}", waits.len());
    assert!(waits.len() >= 20, "only {} pages answered in {took:?}", waits.len());
    assert!(worst < Duration::from_millis(1_500), "a page waited {worst:?} while the floor was open");
}

// ================= 3. one watch per reply, and what it learns kept =================

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

/// A quiet room at about `db` dBFS.
fn room(n: usize, db: f32, seed: u64) -> Vec<i16> {
    let mut x = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    let mut lp = 0f32;
    let amp = 32768.0 * 10f32.powf(db / 20.0) * 3.0;
    (0..n)
        .map(|_| {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let w = ((x >> 33) as f32 / (1u64 << 31) as f32) - 1.0;
            lp = 0.9 * lp + 0.1 * w;
            (lp * amp) as i16
        })
        .collect()
}

fn wav(name: &str) -> Vec<i16> {
    atlas::diarize::read_wav(&std::fs::read(format!("tests/fixtures/speech/corpus/{name}.wav")).unwrap()).unwrap().0
}

#[derive(Default)]
struct EchoState {
    streams: AtomicUsize,
    /// Windows read from the stream being watched: the moment of the reply
    /// the microphone is at.
    read: Arc<AtomicUsize>,
}

/// `Paced`, counting the windows read.
struct Counted {
    inner: Paced,
    read: Arc<AtomicUsize>,
}

impl MicStream for Counted {
    fn read(&mut self, n: usize) -> Option<Vec<i16>> {
        let w = self.inner.read(n)?;
        self.read.fetch_add(1, Ordering::SeqCst);
        Some(w)
    }
}

/// The microphone while Atlas speaks through the speakers: after the first
/// stream (the room), each carries `echo` -- Atlas's own reply, 12 dB down
/// and 192 ms late -- and `playing_level` says how loud the reply is at each
/// moment of it, as `speaking`'s envelope does.
struct EchoMic {
    st: Arc<EchoState>,
    stream: Vec<i16>,
    envelope: Vec<u8>,
}

impl MicWork for EchoMic {
    fn wake_once(&mut self, _stop: &dyn Fn() -> bool) -> atlas::error::Result<bool> {
        std::thread::sleep(Duration::from_millis(20));
        Ok(false)
    }
    fn listen(&mut self) -> atlas::error::Result<String> {
        Ok(String::new())
    }
    fn open_stream(&mut self) -> Option<Box<dyn MicStream>> {
        let n = self.st.streams.fetch_add(1, Ordering::SeqCst);
        let samples = if n == 0 { room(16000 * 2, -60.0, 7) } else { self.stream.clone() };
        self.st.read.store(0, Ordering::SeqCst);
        Some(Box::new(Counted { inner: Paced { samples, at: 0, started: Instant::now() }, read: self.st.read.clone() }))
    }
    fn transcribe(&mut self, _samples: &[i16]) -> atlas::error::Result<String> {
        Ok(String::new())
    }
    fn follow_up(&mut self, _secs: u32, _stop: &dyn Fn() -> bool) -> atlas::error::Result<Option<String>> {
        Ok(None)
    }
    /// Read the moment the window was: in the real thing the envelope is
    /// looked up by the clock, and a recorder a little behind shows up as a
    /// little more lag.
    fn playing_level(&mut self) -> Option<f32> {
        let i = self.st.read.load(Ordering::SeqCst).saturating_sub(1);
        Some(self.envelope.get(i).copied().unwrap_or(0) as f32 / 255.0)
    }
    /// No Silero here: Atlas's own detector, which keeps up with the
    /// microphone in a debug build sharing two CPUs with the rest of the
    /// suite (Silero managed about 45 windows in four seconds there). How
    /// late and how loud the echo is doesn't depend on which one is used.
    fn models_dir(&self) -> PathBuf {
        PathBuf::from("tests/fixtures/no-models-here")
    }
}

struct EchoEars {
    st: Arc<EchoState>,
}

impl EchoEars {
    fn new() -> EchoEars {
        EchoEars { st: Arc::new(EchoState::default()) }
    }
}

impl Ears for EchoEars {
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
        // Atlas's reply twice over (about five seconds), 12 dB down, 6
        // windows late, over a quiet room; the envelope starts a quarter of
        // a second in.
        let mut reply = wav("A_s1");
        reply.extend(wav("C_s2"));
        let start = 4000;
        let mut padded = vec![0i16; start];
        padded.extend_from_slice(&reply);
        let envelope = atlas::speaking::levels_of_wav(&atlas::audio::wav_bytes(&padded, 16000), 32).unwrap();
        let late = start + 6 * micthread::WINDOW;
        let len = late + reply.len() + 16000 * 2;
        let mut stream = room(len, -55.0, 3);
        let k = 10f32.powf(-12.0 / 20.0);
        for (i, v) in reply.iter().enumerate() {
            stream[late + i] = (stream[late + i] as f32 + *v as f32 * k).clamp(-32767.0, 32767.0) as i16;
        }
        Some(Box::new(EchoMic { st: self.st.clone(), stream, envelope }))
    }
}

#[test]
fn a_reply_the_model_is_still_writing_is_watched_once_and_what_was_learned_is_kept() {
    let _one = ONE_PLAYER.lock().unwrap_or_else(|e| e.into_inner());
    micthread::clear_cut();
    let (c, p) = (cfg(true), plat());
    let dir = tmp("one-watch");
    // Four sentences, written a word every 60 ms: the model is still writing
    // while the first ones are said.
    let llm = Arc::new(Talker {
        reply: "Octopuses have three hearts. Two pump blood through the gills. The third serves the body. It stops when they swim.".into(),
        word_ms: 60,
    });
    let mut d = Daemon::new(&c, &p, Some(llm as Arc<dyn Llm>), Store::new(dir.clone()), Proactive::new(ProactiveConfig::default()));
    let ears = EchoEars::new();
    let mouth = SlowMouth::new(700, 700);
    let clock = || 1_790_000_000u64;
    d.listen_pass(&ears, &mouth, &clock);
    d.converse("tell me about octopus hearts", &ears, &mouth, &clock);

    let lines = mouth.lines();
    assert!(lines.len() >= 4, "{lines:?}");
    assert!(lines.iter().all(|(_, cut)| !cut), "Atlas cut itself off on its own voice: {lines:?}");
    // The room once, while nothing was going on; then one watch for the
    // whole reply (it was one per sentence).
    assert_eq!(ears.st.streams.load(Ordering::SeqCst), 2, "the microphone was opened {} times for one reply", ears.st.streams.load(Ordering::SeqCst));

    // What the watch learned about the speakers and the microphone is kept
    // in Atlas's state for the next start.
    let until = Instant::now() + Duration::from_secs(10);
    let kept = loop {
        d.listen_pass(&ears, &mouth, &clock);
        if let Some(l) = Store::new(dir.clone()).load::<Option<micthread::Learned>>("cut_in") {
            break l;
        }
        assert!(Instant::now() < until, "nothing learned about the echo was kept");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!((5..=7).contains(&kept.lag), "kept a lag of {} windows; it was 6", kept.lag);
    println!("kept: {kept:?}");
    let _ = d.shut_down();
    micthread::clear_cut();
}

// ================= 3. seeded, the first reply can be cut into at once =================

/// Fed through a gate a window at a time: when it fired, in ms.
fn fires_at(gate: &mut micthread::BargeGate, det: &mut dyn micthread::VoiceDetector, s: &[i16], envelope: &[u8]) -> Option<u32> {
    for (i, w) in s.chunks_exact(micthread::WINDOW).enumerate() {
        let playing = envelope.get(i).copied().unwrap_or(0) as f32 / 255.0;
        if gate.feed(det.speech(w), atlas::audio::level_db(w), Some(playing)) {
            return Some((i as u32 + 1) * 32);
        }
    }
    None
}

#[test]
fn seeded_with_what_the_last_run_learned_the_first_reply_can_be_cut_into_from_its_start() {
    let silero = PathBuf::from("tests/fixtures/silero").join(micthread::SILERO_FILE);
    let cfg = BargeInConfig { enabled: true, ..BargeInConfig::default() };
    // Atlas's reply from the speakers (12 dB down, 6 windows late), and you,
    // 15 dB over it, from 300 ms into the reply. (Measured: with the echo
    // learned, you are heard about 450 ms after you start; learning it from
    // nothing, not until the reply has ended. Quieter than that, over this
    // echo, neither hears you until the reply ends -- the gate's margin, not
    // what this test is about.)
    let echo = wav("A_s1");
    let you = wav("B_s3");
    let start = 4000;
    let mut padded = vec![0i16; start];
    padded.extend_from_slice(&echo);
    let env = atlas::speaking::levels_of_wav(&atlas::audio::wav_bytes(&padded, 16000), 32).unwrap();
    let late = start + 6 * micthread::WINDOW;
    let you_at = start + 16000 * 300 / 1000;
    let len = (late + echo.len()).max(you_at + you.len()) + 4000;
    let mut s: Vec<f32> = room(len, -55.0, 2).iter().map(|v| *v as f32).collect();
    for (i, v) in echo.iter().enumerate() {
        s[late + i] += *v as f32 * 10f32.powf(-12.0 / 20.0);
    }
    for (i, v) in you.iter().enumerate() {
        s[you_at + i] += *v as f32 * 10f32.powf(3.0 / 20.0);
    }
    let s: Vec<i16> = s.iter().map(|v| v.clamp(-32767.0, 32767.0) as i16).collect();

    // What a first run learns from a reply said with nobody talking over it.
    let alone: Vec<i16> = {
        let mut a: Vec<f32> = room(len, -55.0, 5).iter().map(|v| *v as f32).collect();
        for (i, v) in echo.iter().enumerate() {
            a[late + i] += *v as f32 * 10f32.powf(-12.0 / 20.0);
        }
        a.iter().map(|v| *v as i16).collect()
    };
    let mut first_run = micthread::BargeGate::new(&cfg);
    let mut det = micthread::detector_for(Some(&silero)).0;
    assert_eq!(fires_at(&mut first_run, &mut *det, &alone, &env), None, "cut itself off on its own voice");
    let learned = first_run.learned().expect("the first run learned nothing about the echo");
    assert!((5..=7).contains(&learned.lag), "{learned:?}");
    // Kept as Atlas keeps it, and read back.
    let back: micthread::Learned = serde_json::from_str(&serde_json::to_string(&learned).unwrap()).unwrap();
    assert_eq!(back, learned);

    // The next start: seeded, and the room measured before any reply.
    let mut seeded = micthread::BargeGate::new(&cfg);
    seeded.seed(&back);
    let mut det = micthread::detector_for(Some(&silero)).0;
    for w in room(16000, -55.0, 11).chunks_exact(micthread::WINDOW).take(11) {
        let _ = det.speech(w);
        seeded.learn_room(atlas::audio::level_db(w));
    }
    assert!(seeded.knows_the_room());
    let seeded_at = fires_at(&mut seeded, &mut *det, &s, &env).expect("seeded, you weren't heard over the reply");

    // Not seeded: the echo has to be learned again first.
    let mut cold = micthread::BargeGate::new(&cfg);
    let mut det = micthread::detector_for(Some(&silero)).0;
    let cold_at = fires_at(&mut cold, &mut *det, &s, &env);
    let you_ms = (you_at * 1000 / 16000) as u32;
    println!("you started at {you_ms} ms: heard at {seeded_at} ms seeded, {cold_at:?} learning from nothing");
    assert!(seeded_at > you_ms, "fired at {seeded_at} ms, before you started");
    assert!(seeded_at <= you_ms + 1_000, "seeded, you were heard only at {seeded_at} ms");
    assert!(cold_at.is_none_or(|c| c > seeded_at), "seeding made no difference: {cold_at:?} vs {seeded_at}");
}


