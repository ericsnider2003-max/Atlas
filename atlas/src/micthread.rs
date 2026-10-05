//! The microphone on its own thread.
//!
//! **Why.** With the wake word on, the run loop used to record a three-second
//! clip (and transcribe it, or match it against your taught phrase) on every
//! pass. The loop is also what answers the hub, the typing box and the icon
//! by the clock, so for those three seconds everything else waited: Atlas
//! running in the background with no window open was, most of the time, a
//! hub that didn't answer (Eric, 28 Sep 2026).
//!
//! Now the recording happens here. The loop polls a channel that never blocks
//! (`MicThread::poll`); when the wake word is heard this thread also records
//! what you say next and sends both at once, so the loop only stops for the
//! turn itself.
//!
//! **The name and the request in one breath** (29 Sep 2026). Where the name
//! is heard by speech-to-text (no wake-word program of its own), the
//! microphone is one running stream, cut where you pause (`utterance`): the
//! request is what you said after the name in the same breath, a long one is
//! recorded until you stop, and the name on its own waits a moment for the
//! rest before `Heard::Named` asks the loop to say "Yes?". Until then the
//! words the name was found in were thrown away and a second recording
//! started -- after you had already asked -- and Atlas said "I heard my name
//! but nothing after it".
//!
//! **Cutting in by voice** (`barge_in` in settings, off by default). While
//! Atlas is speaking a reply, this thread can watch the microphone for your
//! voice: sustained speech (300 ms by default) stops the playback and what
//! you say becomes the next turn. Speech is told from noise by Silero VAD
//! (MIT, Silero Team) when its model file is in the models folder, run by
//! `tract` — the ONNX engine already built into Atlas, pure Rust, so nothing
//! native is needed on Windows — and otherwise by Atlas's own detector
//! (`vad`), which learns the room. A missing or unreadable model is never an
//! error: it is said once in the log and the in-house detector is used.
//!
//! **Echo.** Without echo cancellation the microphone also hears Atlas
//! itself through the speakers, and Atlas's voice *is* speech. What is done
//! about it: the level a window must reach is measured against a floor that
//! follows everything that isn't you — Atlas's own voice through the
//! speakers included — and is raised further while the reply is loud
//! (`speaking`'s envelope says how loud each moment of the reply is). That
//! helps; it does not replace a headset. With speakers, expect Atlas to cut
//! itself off now and then, which is why this is off until you turn it on.
//!
//! **Pause.** Pausing Atlas stops this thread listening: no wake word, no
//! watching for your voice, no recording. It picks up again on resume.
//! Holding the talk key still works while paused — that is you deliberately
//! pressing a key, and it is one way to say "carry on".

use crate::error::Result;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use crate::doorbell::{channel, Sender};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// What the microphone thread tells the loop.
#[derive(Debug, Clone, PartialEq)]
pub enum Heard {
    /// The wake word, then what you said after it — or why that couldn't be
    /// heard (the recorder or speech-to-text failed).
    Wake(std::result::Result<String, String>),
    /// Listening for the wake word failed (no microphone, no recorder).
    Trouble(String),
    /// What was said in the open floor after a reply (`MicThread::follow_up`,
    /// by its number): words, silence (`None`), or why it couldn't be heard.
    FollowUp(u64, std::result::Result<Option<String>, String>),
    /// What was said while the talk key was held, and for how long it was
    /// held (30 Sep 2026: recorded here, not on the loop, so the hub and
    /// everything else keep going while you talk).
    Talk(std::result::Result<Option<String>, String>, f32),
    /// Your name and nothing after it, even after a moment's wait (29 Sep
    /// 2026). The loop answers "Yes?" and listens for the rest.
    Named,
}

/// Whether a stretch of audio holds the wake word (`MicWork::name_in`).
#[derive(Debug, Clone, PartialEq)]
pub enum NameCheck {
    /// The name, and the words said after it (empty: nothing after it).
    Named(String),
    /// Not the name: all the words heard.
    NotNamed(String),
}

/// What the thread does with the microphone. `Voice` in the running Atlas;
/// a stand-in in tests.
pub trait MicWork: Send {
    /// One bounded attempt at the wake word. `stop` turning true means give
    /// up now (paused, shutting down, a turn started): stop recording and
    /// return `Ok(false)`.
    fn wake_once(&mut self, stop: &dyn Fn() -> bool) -> Result<bool>;
    /// What you say after the wake word, as words.
    fn listen(&mut self) -> Result<String>;
    /// Words heard in the same clip as the wake word, after it ("Atlas, can
    /// you see me?" said in one breath), taken once. `None` by default.
    fn take_said_with_wake(&mut self) -> Option<String> {
        None
    }
    /// The microphone as a stream of 16 kHz samples, for watching while
    /// Atlas speaks. `None` when this machine can't stream the microphone
    /// (no device named for it).
    fn open_stream(&mut self) -> Option<Box<dyn MicStream>>;
    /// Words for audio already recorded.
    fn transcribe(&mut self, samples: &[i16]) -> Result<String>;
    /// Recording while the talk key is held, as words. `Ok(None)`: a tap,
    /// or nothing said. The default listens once, for stand-ins.
    fn listen_while(&mut self, held: &dyn Fn() -> bool) -> Result<Option<String>> {
        let _ = held;
        self.listen().map(Some)
    }
    /// The open floor after a reply: up to `secs` of recording, as words.
    /// `Ok(None)` is silence. `stop` turning true (paused, Atlas closing,
    /// the loop no longer waiting) means give up now and return `Ok(None)`.
    /// Here rather than on the loop (28 Sep 2026): six to twenty seconds of
    /// recording after every spoken reply held the hub for all of it.
    fn follow_up(&mut self, secs: u32, stop: &dyn Fn() -> bool) -> Result<Option<String>>;
    /// Get the microphone running while a reply plays, so the open floor
    /// after it starts on a recorder that's already going (`follow_up`).
    /// The default does nothing.
    fn warm_up(&mut self) {}
    /// How loud Atlas's own voice is right now, 0..1, while it speaks.
    fn playing_level(&mut self) -> Option<f32> {
        None
    }
    /// Can the wake word be listened for on the microphone's stream -- the
    /// name found in what you said, and the request taken from the same
    /// breath (`utterance::wait_for_name`) -- rather than with `wake_once`
    /// and a second recording? False for a wake-word program of its own,
    /// which gives no words, and for stand-ins that don't say.
    fn hears_name_in_audio(&self) -> bool {
        false
    }
    /// Listening for the name by its sound first (`wake.listen_first`, the
    /// spotter in `kws`): `Some(false)` when it surely isn't in this audio,
    /// so it needn't be written out at all. `None`, the default: not
    /// listening that way, so `name_in` decides.
    fn name_by_sound(&mut self, samples: &[i16]) -> Option<bool> {
        let _ = samples;
        None
    }
    /// Is the wake word in this audio, and what was said after it?
    fn name_in(&mut self, samples: &[i16]) -> Result<NameCheck> {
        self.transcribe(samples).map(NameCheck::NotNamed)
    }
    /// When you've stopped talking: `endpoint` in settings.
    fn endpointing(&self) -> crate::endpoint::EndpointConfig {
        crate::endpoint::EndpointConfig::default()
    }
    /// How much of a long stretch of speech is looked through for the name
    /// at once (`wake.clip_seconds`).
    fn clip_secs(&self) -> u32 {
        3
    }
    /// Where the Silero model would be.
    fn models_dir(&self) -> PathBuf {
        crate::roots::models_dir()
    }
}

/// A running recording. Dropping it stops the recorder.
pub trait MicStream: Send {
    /// The next `n` samples; `None` when the recorder has stopped.
    fn read(&mut self, n: usize) -> Option<Vec<i16>>;
    /// Why it stopped, when the recorder said (a microphone that couldn't
    /// be opened). Asked after `read` gives `None`.
    fn why_stopped(&mut self) -> Option<String> {
        None
    }
}

/// Cutting in by voice: settings.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct BargeInConfig {
    /// Off unless you turn it on: without a headset the microphone hears
    /// Atlas too.
    pub enabled: bool,
    /// How sure the detector must be that a moment is speech, 0..1.
    pub threshold: f32,
    /// How long you must speak over Atlas before it stops.
    pub min_ms: u32,
    /// How far above the room (and Atlas's own voice, as the microphone
    /// hears it) your voice must be, in dB.
    pub margin_db: f32,
    /// Extra dB asked for while Atlas's reply is at its loudest.
    pub echo_db: f32,
    /// The Silero VAD model, in the models folder. Optional: without it
    /// Atlas's own detector is used.
    pub model: String,
    /// Cutting in by voice while you're listening through a headset, even
    /// with `enabled` off (29 Sep 2026). A headset's microphone doesn't hear
    /// Atlas's own voice, so the echo that makes cutting in unreliable on
    /// speakers isn't there. On speakers it stays as `enabled` says.
    pub headset: bool,
}

impl BargeInConfig {
    /// What's in force for this microphone: `headset` turns it on for one.
    pub fn for_microphone(&self, mic_name: &str) -> BargeInConfig {
        let mut c = self.clone();
        if !c.enabled && c.headset && crate::hearing::is_headset(mic_name) {
            c.enabled = true;
        }
        c
    }
}

impl Default for BargeInConfig {
    fn default() -> Self {
        BargeInConfig {
            enabled: false,
            threshold: 0.5,
            min_ms: 300,
            margin_db: 5.0,
            echo_db: 3.0,
            model: SILERO_FILE.into(),
            headset: true,
        }
    }
}

/// The Silero file Atlas looks for: the 16 kHz, opset-15 export from the
/// silero-vad package (MIT), about 1.3 MB.
pub const SILERO_FILE: &str = "silero_vad_16k_op15.onnx";

/// Samples per window the detector judges: 32 ms at 16 kHz, Silero's own
/// window.
pub const WINDOW: usize = 512;
const RATE: u32 = 16_000;
const WINDOW_MS: u32 = 32;

// ---------------------------------------------------------------------------
// Stopping Atlas's own playback
// ---------------------------------------------------------------------------

static CUT: AtomicBool = AtomicBool::new(false);

/// Stop what Atlas is saying: the player is ended and the rest of the reply
/// isn't started. Public (28 Sep 2026) because the talk key cuts the reply
/// too, now that the reply plays on its own thread (`speakthread`) and the
/// key is watched while it plays rather than between sentences.
pub fn cut_playback() {
    CUT.store(true, Ordering::SeqCst);
}

/// Has the reply been cut?
pub fn playback_cut() -> bool {
    CUT.load(Ordering::SeqCst)
}

/// A new reply starts uncut.
pub fn clear_cut() {
    CUT.store(false, Ordering::SeqCst);
}

// ---------------------------------------------------------------------------
// Telling a voice from the room
// ---------------------------------------------------------------------------

/// Is this window speech? 0..1, `None` while it is still learning.
pub trait VoiceDetector: Send {
    fn speech(&mut self, window: &[i16]) -> Option<f32>;
    /// Which detector, in plain words, for the log.
    fn name(&self) -> &'static str;
    /// Forget the last utterance (a new reply is starting).
    fn reset(&mut self) {}
}

/// Atlas's own detector (`vad`): the share of 10 ms frames in the window
/// that are voice, against a room it learns in the first third of a second.
pub struct RoomVad(crate::vad::Vad);

impl RoomVad {
    pub fn new() -> RoomVad {
        RoomVad(crate::vad::Vad::new(RATE))
    }
}

impl Default for RoomVad {
    fn default() -> Self {
        RoomVad::new()
    }
}

impl VoiceDetector for RoomVad {
    fn speech(&mut self, window: &[i16]) -> Option<f32> {
        self.0.window(window)
    }
    fn name(&self) -> &'static str {
        "Atlas's own voice detector"
    }
    /// Nothing to forget between replies: what it knows is the room, which
    /// is measured once while nothing is going on (`MicThread`) and followed
    /// from then on. Starting again here (as it did until 28 Sep 2026) put
    /// the first third of a second of every reply beyond cutting into.
    fn reset(&mut self) {}
}

/// Silero VAD through tract.
///
/// Run as tract's plain (unoptimised) plan: the export's `If` nodes — which
/// sample rate, whether there is a state — can't be turned into tract's
/// optimised form, but they run as they are, and the conditions are the same
/// every window. About a third of a millisecond of model per millisecond of
/// audio in a debug build, far less optimised.
#[cfg(feature = "onnx")]
pub struct SileroVad {
    plan: std::sync::Arc<tract_onnx::prelude::InferenceSimplePlan>,
    /// Input order, by name: "input", "state", "sr".
    names: Vec<String>,
    state: Vec<f32>,
    context: Vec<f32>,
}

#[cfg(feature = "onnx")]
const CONTEXT: usize = 64;

#[cfg(feature = "onnx")]
impl SileroVad {
    pub fn load(path: &Path) -> std::result::Result<SileroVad, String> {
        use tract_onnx::prelude::*;
        if !path.exists() {
            return Err(format!("{} isn't in the models folder", path.display()));
        }
        let go = || -> TractResult<SileroVad> {
            let mut m = tract_onnx::onnx().model_for_path(path)?;
            let names: Vec<String> =
                m.input_outlets()?.iter().map(|o| m.node(o.node).name.clone()).collect();
            for (i, n) in names.iter().enumerate() {
                let f: InferenceFact = match n.as_str() {
                    "input" => f32::fact([1usize, WINDOW + CONTEXT]).into(),
                    "state" => f32::fact([2usize, 1, 128]).into(),
                    "sr" => InferenceFact::from(tensor0(RATE as i64)),
                    other => return Err(TractError::msg(format!("an input I don't know: {other}"))),
                };
                m.set_input_fact(i, f)?;
            }
            m.analyse(false)?;
            let plan = tract_onnx::tract_core::plan::SimplePlan::new(m)?;
            Ok(SileroVad { plan, names, state: vec![0.0; 256], context: vec![0.0; CONTEXT] })
        };
        let mut v = go().map_err(|e| format!("couldn't read {}: {e}", path.display()))?;
        // One window of silence, so a model that loads but can't run is
        // found now rather than while you're talking.
        v.run(&[0i16; WINDOW]).map_err(|e| format!("{} doesn't run: {e}", path.display()))?;
        v.reset();
        Ok(v)
    }

    fn run(&mut self, window: &[i16]) -> std::result::Result<f32, String> {
        use tract_onnx::prelude::*;
        let mut x = self.context.clone();
        x.extend(window.iter().map(|s| *s as f32 / 32768.0));
        x.resize(WINDOW + CONTEXT, 0.0);
        self.context = x[WINDOW..].to_vec();
        let input = Tensor::from_shape(&[1, WINDOW + CONTEXT], &x).map_err(|e| e.to_string())?;
        let state = Tensor::from_shape(&[2, 1, 128], &self.state).map_err(|e| e.to_string())?;
        let mut ins: TVec<TValue> = tvec!();
        for n in &self.names {
            ins.push(match n.as_str() {
                "input" => input.clone().into(),
                "state" => state.clone().into(),
                _ => tensor0(RATE as i64).into(),
            });
        }
        let out = self.plan.run(ins).map_err(|e| e.to_string())?;
        let p: &Tensor = &out[0];
        let p = p.view().as_slice::<f32>().map_err(|e| e.to_string())?.first().copied().unwrap_or(0.0);
        let s: &Tensor = &out[1];
        let s = s.view().as_slice::<f32>().map_err(|e| e.to_string())?.to_vec();
        if s.len() == self.state.len() {
            self.state = s;
        }
        Ok(p)
    }
}

#[cfg(feature = "onnx")]
impl VoiceDetector for SileroVad {
    fn speech(&mut self, window: &[i16]) -> Option<f32> {
        self.run(window).ok()
    }
    fn name(&self) -> &'static str {
        "Silero VAD"
    }
    fn reset(&mut self) {
        self.state = vec![0.0; 256];
        self.context = vec![0.0; CONTEXT];
    }
}

/// The best detector this machine has: Silero when its model is there and
/// runs, Atlas's own otherwise. The second value says why Silero wasn't
/// used, when it wasn't.
pub fn detector_for(model: Option<&Path>) -> (Box<dyn VoiceDetector>, Option<String>) {
    let Some(path) = model else {
        return (Box::new(RoomVad::new()), Some("no Silero model named".into()));
    };
    #[cfg(feature = "onnx")]
    {
        match SileroVad::load(path) {
            Ok(v) => (Box::new(v), None),
            Err(why) => (Box::new(RoomVad::new()), Some(why)),
        }
    }
    #[cfg(not(feature = "onnx"))]
    {
        let _ = path;
        (Box::new(RoomVad::new()), Some("this build has no ONNX engine".into()))
    }
}

// ---------------------------------------------------------------------------
// Deciding that you are speaking over Atlas
// ---------------------------------------------------------------------------

/// Sustained voice above the room and above Atlas's own voice.
///
/// Fed one 32 ms window at a time. Two things your voice must rise above:
///
/// * **the room**, measured once while nothing is going on (`learn_room`,
///   at start; again now and then while the wake word listens anyway) and
///   then following every quiet window — quickly down, slowly up. Until
///   28 Sep 2026 it was measured in the first quarter-second of each reply,
///   during which nothing you said could count;
/// * **Atlas itself through the speakers.** How loud Atlas's reply is at each
///   moment is known before it plays (`speaking`'s envelope). Two things are
///   learned from it: how late the microphone hears the reply (the recorder
///   buffers; up to `MAX_LAG` windows, found by lining the reply's loudness
///   up with the microphone's), and how much of it arrives (the gap in dB
///   between the two). Each window is then expected to hold that much of
///   Atlas, and only a voice clearly above it counts. If the microphone
///   hears nothing of the reply (a headset), that is learned instead.
///   Until one or the other is known, nothing said over the reply counts —
///   so the first second or so of the very first reply can't be cut into.
///   Both are kept from reply to reply, and from one run to the next
///   (`Learned`, in Atlas's state): they belong to the speakers and the
///   microphone, not to what was said.
///
/// Your voice counts once it has been above all that, and judged speech by
/// the detector, for `min_ms` of the last half-second.
///
/// This is not echo cancellation — Atlas's sound isn't subtracted, only
/// out-shouted — so with speakers your voice must be clearly louder than
/// Atlas at the microphone, and a headset still works best. Measured
/// (`tests/the_microphone_has_its_own_thread.rs`) on synthesized voices with
/// the echo a clean copy of the reply, delayed by up to a third of a second.
#[derive(Debug, Clone)]
pub struct BargeGate {
    cfg: BargeInConfig,
    // This reply.
    floor_db: Option<f32>,
    learned: u32,
    recent: std::collections::VecDeque<bool>,
    playing: Vec<f32>,
    levels: Vec<f32>,
    counted: Vec<bool>,
    // Kept across replies: how Atlas reaches the microphone.
    lag: Option<usize>,
    coupling_db: Option<f32>,
    coupling_seen: u32,
}

/// Windows the floor is learned from before anything is judged.
const LEARN_WINDOWS: u32 = 8;
/// The longest the microphone may hear the reply after it plays: 640 ms
/// (a Windows recorder buffers up to about half a second).
pub const MAX_LAG: usize = 20;
/// Windows of reply needed to say how late and how loud it arrives.
const LAG_WINDOWS: usize = 12;
/// Windows of Atlas alone before its echo's strength is trusted.
const COUPLING_WINDOWS: u32 = 4;
/// Louder than this, the reply counts as playing.
const PLAYING: f32 = 0.3;
/// Any of the reply audible at all.
const AUDIBLE: f32 = 0.05;
/// The last half-second, over which `min_ms` of voice is counted.
const RECENT_WINDOWS: usize = 16;
/// What "the microphone hears none of Atlas" is recorded as.
const NO_ECHO_DB: f32 = -200.0;
/// History kept for finding the lag (about 13 s).
const HISTORY: usize = 400;

impl BargeGate {
    pub fn new(cfg: &BargeInConfig) -> BargeGate {
        BargeGate {
            cfg: cfg.clone(),
            floor_db: None,
            learned: 0,
            recent: std::collections::VecDeque::new(),
            playing: Vec::new(),
            levels: Vec::new(),
            counted: Vec::new(),
            lag: None,
            coupling_db: None,
            coupling_seen: 0,
        }
    }

    /// A new reply: the counting starts again; what was learned about the
    /// speakers and the microphone is kept, and so is the room once it has
    /// been measured (28 Sep 2026: it was measured again in the first
    /// quarter-second of every reply, during which nothing you said could
    /// count).
    fn new_reply(&mut self, cfg: &BargeInConfig) {
        let keep = (self.lag, self.coupling_db, self.coupling_seen);
        let room = (self.learned >= LEARN_WINDOWS).then_some(self.floor_db).flatten();
        *self = BargeGate::new(cfg);
        (self.lag, self.coupling_db, self.coupling_seen) = keep;
        if let Some(f) = room {
            self.floor_db = Some(f);
            self.learned = LEARN_WINDOWS;
        }
    }

    /// How late the microphone hears the reply, in windows, once known.
    pub fn lag_windows(&self) -> Option<usize> {
        self.lag
    }

    /// One window of the room with Atlas silent (`MicThread` measures it
    /// once, while nothing is going on): the floor, learned before any reply
    /// rather than in the first quarter-second of one.
    pub fn learn_room(&mut self, level_db: f32) {
        self.learned = (self.learned + 1).min(LEARN_WINDOWS);
        self.floor_db = Some(match self.floor_db {
            None => level_db,
            Some(f) => f + (level_db - f) / self.learned as f32,
        });
    }

    /// Is the room known (measured, or followed through a reply)?
    pub fn knows_the_room(&self) -> bool {
        self.learned >= LEARN_WINDOWS && self.floor_db.is_some()
    }

    /// How Atlas reaches the microphone, once both halves are known: kept in
    /// Atlas's state so the next start doesn't learn it all again.
    pub fn learned(&self) -> Option<Learned> {
        let (lag, coupling_db) = (self.lag?, self.coupling_db?);
        (self.coupling_seen >= COUPLING_WINDOWS).then_some(Learned { lag, coupling_db })
    }

    /// Start from what an earlier run learned: the first reply can be cut
    /// into from its first moment rather than once the echo has been
    /// measured again (a second or more).
    pub fn seed(&mut self, l: &Learned) {
        if l.lag > MAX_LAG || !l.coupling_db.is_finite() {
            return;
        }
        self.lag = Some(l.lag);
        self.coupling_db = Some(l.coupling_db);
        self.coupling_seen = COUPLING_WINDOWS;
    }

    /// One window: how sure the detector is it's speech, how loud it is
    /// (dBFS), and how loud Atlas's reply is at this moment (0..1, if
    /// known). True once you have been speaking over Atlas for `min_ms`.
    pub fn feed(&mut self, speech: Option<f32>, level_db: f32, playing: Option<f32>) -> bool {
        self.playing.push(playing.unwrap_or(0.0).clamp(0.0, 1.0));
        self.levels.push(level_db);
        self.counted.push(false);
        if self.playing.len() > HISTORY {
            self.playing.remove(0);
            self.levels.remove(0);
            self.counted.remove(0);
        }
        let n = self.playing.len();
        let lately = self.playing[n.saturating_sub(MAX_LAG + 1)..].iter().copied().fold(0f32, f32::max);
        if self.lag.is_none() && n % 4 == 0 {
            self.find_lag();
        }
        let env = match self.lag {
            Some(lag) => self.playing_at(lag),
            None => lately,
        };
        // The envelope is the square root of loudness relative to the
        // reply's peak (`speaking::levels_of_wav`), so dB is 40·log10.
        let env_db = 40.0 * env.max(1e-3).log10();
        if self.learned < LEARN_WINDOWS {
            self.learned += 1;
            self.floor_db = Some(match self.floor_db {
                None => level_db,
                Some(f) => f + (level_db - f) / self.learned as f32,
            });
            return false;
        }
        let floor = self.floor_db.unwrap_or(level_db);
        // Any of the reply audible and its echo not yet known: nothing
        // counts yet (a quiet stretch of Atlas is still Atlas).
        let unknown = lately > AUDIBLE && (self.lag.is_none() || self.coupling_seen < COUPLING_WINDOWS);
        let echo = if env > AUDIBLE { self.coupling_db.map(|c| c + env_db) } else { None };
        let need = floor.max(echo.unwrap_or(f32::MIN)) + self.cfg.margin_db + self.cfg.echo_db * env;
        let voiced = !unknown && speech.map(|p| p >= self.cfg.threshold).unwrap_or(false) && level_db >= need;
        if let Some(c) = self.counted.last_mut() {
            *c = voiced;
        }
        self.recent.push_back(voiced);
        if self.recent.len() > RECENT_WINDOWS {
            self.recent.pop_front();
        }
        if !voiced {
            if lately < AUDIBLE {
                let rate = if level_db < floor { 0.3 } else { 0.05 };
                self.floor_db = Some(floor + (level_db - floor) * rate);
            }
            if self.lag.is_some() {
                self.learn_coupling(env, level_db - env_db);
            }
        }
        self.recent.iter().filter(|v| **v).count() as u32 * WINDOW_MS >= self.cfg.min_ms
    }

    /// The reply's loudness `lag` windows ago, give or take one (the lag is
    /// found to the nearest window).
    fn playing_at(&self, lag: usize) -> f32 {
        let n = self.playing.len();
        let Some(i) = (n - 1).checked_sub(lag) else { return 0.0 };
        let lo = i.saturating_sub(1);
        let hi = (i + 1).min(n - 1);
        self.playing[lo..=hi].iter().copied().fold(0f32, f32::max)
    }

    /// How late the microphone hears the reply: the delay at which the
    /// microphone's level follows the reply's loudness best (correlation of
    /// at least 0.5). Or, if a good while into the reply the microphone
    /// hears nothing of it at all, that there is no echo to allow for.
    fn find_lag(&mut self) {
        let n = self.playing.len();
        let mut best: Option<(f32, usize)> = None;
        for lag in 0..=MAX_LAG {
            let (mut xs, mut ys) = (Vec::new(), Vec::new());
            for t in lag..n {
                let e = self.playing[t - lag];
                if e > AUDIBLE && !self.counted[t] {
                    xs.push(40.0 * e.log10());
                    ys.push(self.levels[t]);
                }
            }
            if xs.len() < LAG_WINDOWS {
                continue;
            }
            if let Some(r) = correlation(&xs, &ys) {
                if best.map(|b| r > b.0).unwrap_or(true) {
                    best = Some((r, lag));
                }
            }
        }
        if let Some((r, lag)) = best {
            if r >= 0.5 {
                self.lag = Some(lag);
                return;
            }
        }
        // No echo: well into the reply (past the longest lag), the
        // microphone stayed at the room while Atlas played.
        let Some(first) = self.playing.iter().position(|e| *e > AUDIBLE) else { return };
        let floor = self.floor_db.unwrap_or(0.0);
        let heard: Vec<f32> = (first + MAX_LAG..n)
            .filter(|t| self.playing[t.saturating_sub(MAX_LAG)..=*t].iter().any(|e| *e > PLAYING) && !self.counted[*t])
            .map(|t| self.levels[t])
            .take(LAG_WINDOWS)
            .collect();
        if heard.len() >= LAG_WINDOWS && heard.iter().filter(|l| **l < floor + 3.0).count() * 10 >= heard.len() * 8 {
            self.lag = Some(0);
            self.coupling_db = Some(NO_ECHO_DB);
            self.coupling_seen = COUPLING_WINDOWS;
        }
    }

    fn learn_coupling(&mut self, env: f32, gap_db: f32) {
        if env < PLAYING {
            return;
        }
        self.coupling_seen += 1;
        self.coupling_db = Some(match self.coupling_db {
            None => gap_db,
            // The first few windows of Atlas alone: the loudest of them, so
            // the echo is never thought quieter than it is.
            Some(c) if self.coupling_seen <= COUPLING_WINDOWS => c.max(gap_db),
            // Then followed upward only by windows that could still be Atlas
            // alone -- up to 6 dB above what was expected -- and let down
            // slowly. Upward, because the reply's loudness is taken as the
            // loudest of three windows (the lag is known to a window), which
            // makes the gap read low but at the start of a word. Bounded,
            // because your own voice, in the moment before it counts, must
            // not raise the bar it has to clear -- the same reason `vad`
            // re-learns the room only from the room.
            Some(c) if gap_db > c && gap_db <= c + 6.0 => c + (gap_db - c) * 0.3,
            Some(c) if gap_db <= c => c + (gap_db - c) * 0.01,
            Some(c) => c,
        });
    }
}

/// What cutting in by voice learned about this machine's speakers and
/// microphone: how many windows late the microphone hears Atlas, how much of
/// Atlas it hears (dB relative to the reply's loudness; -200 for a headset).
/// Saved in Atlas's state (`Daemon`, as "cut_in") and handed to the next
/// start (`MicThread::seed`). The room isn't kept: it is measured again at
/// each start, while nothing is going on, because rooms change.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, Deserialize)]
pub struct Learned {
    pub lag: usize,
    pub coupling_db: f32,
}

/// Pearson correlation; `None` when either side doesn't vary.
fn correlation(x: &[f32], y: &[f32]) -> Option<f32> {
    let n = x.len().min(y.len()) as f32;
    if n < 2.0 {
        return None;
    }
    let (mx, my) = (x.iter().sum::<f32>() / n, y.iter().sum::<f32>() / n);
    let (mut sxy, mut sxx, mut syy) = (0f32, 0f32, 0f32);
    for (a, b) in x.iter().zip(y) {
        sxy += (a - mx) * (b - my);
        sxx += (a - mx) * (a - mx);
        syy += (b - my) * (b - my);
    }
    (sxx > 1e-6 && syy > 1e-6).then(|| sxy / (sxx * syy).sqrt())
}

// ---------------------------------------------------------------------------
// The thread
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Shared {
    stop: AtomicBool,
    paused: AtomicBool,
    /// The wake word is on and the loop is in the tier that uses it.
    wake: AtomicBool,
    /// A turn is going on (typed, the talk key, a conversation): the wake
    /// word isn't listened for, so two recorders never share a microphone.
    busy: AtomicBool,
    /// Not listening again until the loop has taken the last wake.
    taken: AtomicBool,
    /// Atlas is speaking a reply: watch for your voice.
    watching: AtomicBool,
    /// The microphone is open right now.
    recording: AtomicBool,
    barge: Mutex<BargeInConfig>,
    /// You cut in: set when your voice is heard, before the words are.
    cut_pending: AtomicBool,
    cut_words: Mutex<Option<String>>,
    /// Said once in the log, not every reply.
    notes: Mutex<Vec<String>>,
    lag_said: AtomicBool,
    /// The open floor after a reply: which one (its number) and how long.
    follow: Mutex<Option<(u64, u32)>>,
    follow_seq: std::sync::atomic::AtomicU64,
    /// The loop has stopped waiting for the follow-up it asked for.
    follow_cancel: AtomicBool,
    /// What an earlier run learned about the speakers and the microphone,
    /// to start from.
    seed: Mutex<Option<Learned>>,
    /// What has been learned and not yet taken by the loop to keep.
    learned: Mutex<Option<Learned>>,
    /// Recordings that gave sound: proof the microphone works, so a wake
    /// word dropped for push-to-talk can come back (`MicThread::probe`).
    heard_audio: std::sync::atomic::AtomicU64,
    /// The talk key is down: record while this says it's held.
    talk: Mutex<Option<Arc<dyn Fn() -> bool + Send + Sync>>>,
    /// The loop asks whether the microphone works now (push-to-talk after
    /// the wake word failed): one short recording, when nothing else is.
    probe: AtomicBool,
}

impl Shared {
    fn wake_should_stop(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
            || self.paused.load(Ordering::SeqCst)
            || !self.wake.load(Ordering::SeqCst)
            || self.busy.load(Ordering::SeqCst)
            || self.watching.load(Ordering::SeqCst)
    }
    fn follow_should_stop(&self) -> bool {
        self.stop.load(Ordering::SeqCst) || self.paused.load(Ordering::SeqCst) || self.follow_cancel.load(Ordering::SeqCst)
    }
    fn watch_should_stop(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
            || self.paused.load(Ordering::SeqCst)
            || !self.watching.load(Ordering::SeqCst)
            || !self.barge.lock().or_else(crate::crash::unpoison).map(|b| b.enabled).unwrap_or(false)
    }
    fn note(&self, line: String) {
        if let Ok(mut n) = self.notes.lock().or_else(crate::crash::unpoison) {
            n.push(line);
        }
    }
}

/// The microphone's thread, owned by the run loop. Dropping it stops it.
pub struct MicThread {
    shared: Arc<Shared>,
    rx: Receiver<Heard>,
    handle: Option<std::thread::JoinHandle<()>>,
}

/// The part of the thread a reply needs while it is being spoken: start and
/// stop watching, and take what you said over it.
#[derive(Clone)]
pub struct MicLink(Arc<Shared>);

impl MicThread {
    pub fn start(work: Box<dyn MicWork>) -> MicThread {
        let shared = Arc::new(Shared::default());
        let (tx, rx) = channel();
        let s = shared.clone();
        let handle = std::thread::Builder::new()
            .name("atlas-microphone".into())
            .spawn(move || {
                // Caught: a panic here must not leave the loop waiting on a
                // thread that is gone. It is said, and the loop degrades.
                let tx2 = tx.clone();
                let s2 = s.clone();
                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || listen_loop(s2, work, tx2))).is_err() {
                    s.recording.store(false, Ordering::SeqCst);
                    let _ = tx.send(Heard::Trouble("the microphone's thread stopped unexpectedly".into()));
                }
            })
            .ok();
        MicThread { shared, rx, handle }
    }

    /// The wake word on (and the loop in the tier that uses it) or off.
    pub fn set_wake(&self, on: bool) {
        self.shared.wake.store(on, Ordering::SeqCst);
    }
    /// Atlas paused: nothing is recorded until it isn't.
    pub fn set_paused(&self, on: bool) {
        self.shared.paused.store(on, Ordering::SeqCst);
    }
    /// A turn going on elsewhere: the wake word waits.
    pub fn set_busy(&self, on: bool) {
        self.shared.busy.store(on, Ordering::SeqCst);
    }
    /// Cutting in by voice, as the settings say now.
    pub fn set_barge(&self, cfg: &BargeInConfig) {
        if let Ok(mut b) = self.shared.barge.lock().or_else(crate::crash::unpoison) {
            if *b != *cfg {
                *b = cfg.clone();
            }
        }
    }
    /// What the microphone heard since the last look. Never waits.
    pub fn poll(&self) -> Option<Heard> {
        match self.rx.try_recv() {
            Ok(h) => Some(h),
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
        }
    }
    /// The last wake has been dealt with: listen for the next one.
    pub fn rearm(&self) {
        self.shared.taken.store(false, Ordering::SeqCst);
    }
    /// Is the microphone open right now?
    pub fn is_recording(&self) -> bool {
        self.shared.recording.load(Ordering::SeqCst)
    }
    pub fn link(&self) -> MicLink {
        MicLink(self.shared.clone())
    }
    /// Lines worth a place in the log (which detector, why not Silero).
    pub fn take_notes(&self) -> Vec<String> {
        self.shared.notes.lock().or_else(crate::crash::unpoison).map(|mut n| std::mem::take(&mut *n)).unwrap_or_default()
    }
    /// Stop the thread and wait for it — briefly. Every recording it starts
    /// is stopped by the same flag, so this takes a few tens of
    /// milliseconds; only a speech-to-text run already under way can hold it
    /// longer, and that is let finish on its own rather than waited on.
    pub fn stop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            let until = Instant::now() + Duration::from_secs(2);
            while !h.is_finished() && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(10));
            }
            if h.is_finished() {
                let _ = h.join();
            }
        }
    }
    /// Has the thread ended?
    pub fn is_stopped(&self) -> bool {
        self.handle.as_ref().map(|h| h.is_finished()).unwrap_or(true)
    }
    /// Listen for up to `secs` in the open floor after a reply, here rather
    /// than on the loop. The words come back through `poll` as
    /// `Heard::FollowUp` with the number returned.
    pub fn follow_up(&self, secs: u32) -> u64 {
        let id = self.shared.follow_seq.fetch_add(1, Ordering::SeqCst) + 1;
        self.shared.follow_cancel.store(false, Ordering::SeqCst);
        if let Ok(mut f) = self.shared.follow.lock().or_else(crate::crash::unpoison) {
            *f = Some((id, secs));
        }
        id
    }
    /// The talk key went down: record on this thread while `held` says so,
    /// and hand back `Heard::Talk`. The wake word gives way at once.
    pub fn talk(&self, held: Arc<dyn Fn() -> bool + Send + Sync>) {
        if let Ok(mut t) = self.shared.talk.lock().or_else(crate::crash::unpoison) {
            *t = Some(held);
        }
        self.shared.busy.store(true, Ordering::SeqCst);
    }
    /// The loop has stopped waiting: stop the recording now.
    pub fn cancel_follow_up(&self) {
        if let Ok(mut f) = self.shared.follow.lock().or_else(crate::crash::unpoison) {
            *f = None;
        }
        self.shared.follow_cancel.store(true, Ordering::SeqCst);
    }
    /// Start from what an earlier run learned about the speakers and the
    /// microphone.
    pub fn seed(&self, l: Learned) {
        if let Ok(mut s) = self.shared.seed.lock().or_else(crate::crash::unpoison) {
            *s = Some(l);
        }
    }
    /// What has been learned about them since the last look, if it has
    /// changed: for the loop to keep in Atlas's state.
    pub fn take_new_learned(&self) -> Option<Learned> {
        self.shared.learned.lock().or_else(crate::crash::unpoison).ok().and_then(|mut l| l.take())
    }
    /// How many recordings have given sound since the thread started. Goes
    /// up while the microphone works; stays put while it doesn't.
    pub fn audio_heard(&self) -> u64 {
        self.shared.heard_audio.load(Ordering::SeqCst)
    }
    /// Try the microphone once, when it isn't otherwise in use: a third of
    /// a second of recording, counted in `audio_heard` if it gave sound.
    /// For coming back from push-to-talk once the microphone works again
    /// (29 Sep 2026: Atlas switched to push-to-talk when the microphone
    /// failed and never switched back).
    pub fn probe(&self) {
        self.shared.probe.store(true, Ordering::SeqCst);
    }
}

impl Drop for MicThread {
    fn drop(&mut self) {
        self.stop();
    }
}

impl MicLink {
    /// Atlas starts (true) or stops (false) speaking a reply.
    pub fn watch(&self, on: bool) {
        if on {
            self.0.cut_pending.store(false, Ordering::SeqCst);
            if let Ok(mut w) = self.0.cut_words.lock().or_else(crate::crash::unpoison) {
                *w = None;
            }
        }
        self.0.watching.store(on, Ordering::SeqCst);
    }
    /// Is cutting in by voice on?
    pub fn barge_on(&self) -> bool {
        self.0.barge.lock().or_else(crate::crash::unpoison).map(|b| b.enabled).unwrap_or(false)
    }
    /// What you said over Atlas, if you did, without waiting: your voice
    /// heard and the words still being made out is `Waiting`; `Said("")`
    /// when they couldn't be. Until 28 Sep 2026 this waited (up to twenty
    /// seconds) for the words, and the hub with it.
    pub fn try_cut_in(&self) -> CutIn {
        if !self.0.cut_pending.load(Ordering::SeqCst) {
            return CutIn::No;
        }
        if let Some(w) = self.0.cut_words.lock().or_else(crate::crash::unpoison).ok().and_then(|mut w| w.take()) {
            self.0.cut_pending.store(false, Ordering::SeqCst);
            return CutIn::Said(w);
        }
        if self.0.stop.load(Ordering::SeqCst) {
            self.0.cut_pending.store(false, Ordering::SeqCst);
            return CutIn::Said(String::new());
        }
        CutIn::Waiting
    }
    /// Has your voice stopped the reply (words or not yet)? Doesn't take
    /// anything.
    pub fn cut_pending(&self) -> bool {
        self.0.cut_pending.load(Ordering::SeqCst)
    }
    /// The words took too long: stop waiting for them.
    pub fn give_up_cut_in(&self) {
        self.0.cut_pending.store(false, Ordering::SeqCst);
    }
}

/// Whether you cut in by voice (`MicLink::try_cut_in`).
#[derive(Debug, Clone, PartialEq)]
pub enum CutIn {
    No,
    /// Your voice stopped the reply; the words aren't in yet.
    Waiting,
    Said(String),
}

fn idle(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

/// The detector, loaded the first time it's wanted and kept.
fn detector<'a>(s: &Shared, work: &dyn MicWork, slot: &'a mut Option<Box<dyn VoiceDetector>>) -> &'a mut Box<dyn VoiceDetector> {
    slot.get_or_insert_with(|| {
        let model = s.barge.lock().or_else(crate::crash::unpoison).map(|b| b.model.clone()).unwrap_or_default();
        let path = (!model.trim().is_empty()).then(|| work.models_dir().join(model.trim()));
        let (d, why) = detector_for(path.as_deref());
        match why {
            None => s.note(format!("cutting in by voice: using {}", d.name())),
            Some(why) => s.note(format!("cutting in by voice: using {} ({why})", d.name())),
        }
        d
    })
}

/// Windows of room measured while nothing is going on: about a third of a
/// second, enough for both the gate's floor and Atlas's own detector.
const ROOM_WINDOWS: usize = 11;
/// With the wake word listening anyway, the room is measured again this
/// often between its clips. Without it the microphone isn't opened just for
/// this: the room is followed through each reply instead.
const ROOM_AGAIN: Duration = Duration::from_secs(600);

fn listen_loop(s: Arc<Shared>, mut work: Box<dyn MicWork>, tx: Sender<Heard>) {
    let mut detector_slot: Option<Box<dyn VoiceDetector>> = None;
    // Kept from reply to reply: how Atlas reaches the microphone.
    let mut gate = BargeGate::new(&BargeInConfig::default());
    let mut failures: u32 = 0;
    let mut room_at: Option<Instant> = None;
    while !s.stop.load(Ordering::SeqCst) {
        if s.paused.load(Ordering::SeqCst) {
            idle(30);
            continue;
        }
        if let Some(l) = s.seed.lock().or_else(crate::crash::unpoison).ok().and_then(|mut l| l.take()) {
            gate.seed(&l);
        }
        // The talk key first: you're holding it now.
        if let Some(held) = s.talk.lock().or_else(crate::crash::unpoison).ok().and_then(|mut t| t.take()) {
            let started = Instant::now();
            let stop_s = s.clone();
            let still = move || held() && !stop_s.stop.load(Ordering::SeqCst);
            // On a call: muted there while you talk to Atlas (`callmute`).
            crate::callmute::addressed();
            s.recording.store(true, Ordering::SeqCst);
            let got = work.listen_while(&still).map_err(|e| e.to_string());
            s.recording.store(false, Ordering::SeqCst);
            if matches!(got, Ok(Some(_))) {
                s.heard_audio.fetch_add(1, Ordering::SeqCst);
            }
            if tx.send(Heard::Talk(got, started.elapsed().as_secs_f32())).is_err() {
                return;
            }
            continue;
        }
        // The open floor after a reply comes first: the loop is waiting on it.
        if let Some((id, secs)) = s.follow.lock().or_else(crate::crash::unpoison).ok().and_then(|mut f| f.take()) {
            let stop_s = s.clone();
            let stop = move || stop_s.follow_should_stop();
            s.recording.store(true, Ordering::SeqCst);
            let got = work.follow_up(secs, &stop).map_err(|e| e.to_string());
            s.recording.store(false, Ordering::SeqCst);
            if got.is_ok() {
                s.heard_audio.fetch_add(1, Ordering::SeqCst);
            }
            if tx.send(Heard::FollowUp(id, got)).is_err() {
                return;
            }
            continue;
        }
        let barge = s.barge.lock().or_else(crate::crash::unpoison).map(|b| b.enabled).unwrap_or(false);
        // The room, measured while nothing is going on -- once at the start,
        // and again now and then while the wake word has the microphone
        // anyway -- so no reply spends its first moments measuring it.
        let wake_armed = s.wake.load(Ordering::SeqCst) && !s.busy.load(Ordering::SeqCst);
        let room_due = match room_at {
            None => true,
            Some(t) => wake_armed && !s.watching.load(Ordering::SeqCst) && t.elapsed() >= ROOM_AGAIN,
        };
        // Not while the loop itself is recording you (a turn going on, and
        // not a reply being spoken): two recorders on one microphone.
        let loop_recording = s.busy.load(Ordering::SeqCst) && !s.watching.load(Ordering::SeqCst);
        if barge && room_due && !loop_recording {
            room_at = Some(Instant::now());
            let det = detector(&s, &*work, &mut detector_slot);
            measure_room(&s, &mut *work, &mut **det, &mut gate);
            continue;
        }
        if s.watching.load(Ordering::SeqCst) {
            if !s.watch_should_stop() {
                let det = detector(&s, &*work, &mut detector_slot);
                let before = gate.learned();
                watch(&s, &mut *work, &mut **det, &mut gate);
                if let Some(l) = gate.learned().filter(|l| Some(*l) != before) {
                    if let Ok(mut k) = s.learned.lock().or_else(crate::crash::unpoison) {
                        *k = Some(l);
                    }
                }
            }
            // The recorder for the open floor after this reply, started
            // now so your first words aren't lost to it starting up.
            work.warm_up();
            // Once per reply: after you've cut in (or the microphone
            // couldn't be streamed) nothing more is recorded until the
            // reply is over.
            while s.watching.load(Ordering::SeqCst) && !s.stop.load(Ordering::SeqCst) {
                idle(20);
            }
            continue;
        }
        let armed = s.wake.load(Ordering::SeqCst) && !s.busy.load(Ordering::SeqCst) && !s.taken.load(Ordering::SeqCst);
        if !armed {
            // Asked whether the microphone works now: push-to-talk after
            // the wake word failed, waiting to come back (`probe`).
            if s.probe.swap(false, Ordering::SeqCst) && !s.busy.load(Ordering::SeqCst) {
                probe_once(&s, &mut *work);
            }
            idle(30);
            continue;
        }
        // The name heard in what you said, on one running stream, and the
        // request taken from the same breath (29 Sep 2026). A recorder that
        // can't stream falls through to the clip-at-a-time way below.
        if work.hears_name_in_audio() {
            match wake_on_stream(&s, &mut *work) {
                StreamWake::Heard(h) => {
                    failures = 0;
                    s.taken.store(true, Ordering::SeqCst);
                    if tx.send(h).is_err() {
                        return;
                    }
                    continue;
                }
                StreamWake::Ended => {
                    failures = 0;
                    continue;
                }
                StreamWake::Failed(e) => {
                    failures += 1;
                    if tx.send(Heard::Trouble(e)).is_err() {
                        return;
                    }
                    let pause = (200u64 * u64::from(failures)).min(2_000);
                    let until = Instant::now() + Duration::from_millis(pause);
                    while Instant::now() < until && !s.wake_should_stop() {
                        idle(20);
                    }
                    continue;
                }
                StreamWake::NoStream => {}
            }
        }
        let stop_s = s.clone();
        let stop = move || stop_s.wake_should_stop();
        s.recording.store(true, Ordering::SeqCst);
        let got = work.wake_once(&stop);
        s.recording.store(false, Ordering::SeqCst);
        match got {
            Ok(true) if !s.wake_should_stop() => {
                failures = 0;
                s.heard_audio.fetch_add(1, Ordering::SeqCst);
                s.taken.store(true, Ordering::SeqCst);
                // The name was heard: on a call, muted there for the rest.
                crate::callmute::addressed();
                s.recording.store(true, Ordering::SeqCst);
                // Merged 30 Sep 2026: both chats fixed "I heard my name but
                // nothing after it". The stream above (`wake_on_stream`) is the
                // main way -- one running stream, cut where you pause. This
                // clip-at-a-time way is kept for where the microphone can't be
                // streamed or a wake-word program listens, and keeps the words
                // from the name's own clip (the other chat's fix) rather than
                // throwing them away.
                let with_name = work.take_said_with_wake();
                // A sentence already finished in the clip ("Atlas, can you see
                // me?") is answered now, not after waiting on a silence.
                let said = if with_name.as_deref().is_some_and(|w| sentence_finished(w)) {
                    with_wake_word(with_name, Ok(String::new()))
                } else {
                    with_wake_word(with_name, work.listen().map_err(|e| e.to_string()))
                };
                s.recording.store(false, Ordering::SeqCst);
                if tx.send(Heard::Wake(said)).is_err() {
                    return;
                }
            }
            Ok(_) => {
                failures = 0;
                s.heard_audio.fetch_add(1, Ordering::SeqCst);
            }
            Err(e) => {
                failures += 1;
                if tx.send(Heard::Trouble(e.to_string())).is_err() {
                    return;
                }
                // Backing off, as `Voice::wait_for_wake` does: a device busy
                // for a moment recovers in a second or two, and a missing one
                // should not be asked ten times in ten milliseconds.
                let pause = (200u64 * u64::from(failures)).min(2_000);
                let until = Instant::now() + Duration::from_millis(pause);
                while Instant::now() < until && !s.wake_should_stop() {
                    idle(20);
                }
            }
        }
    }
}

/// How listening for the name on a stream ended.
enum StreamWake {
    Heard(Heard),
    /// Stopped (paused, a turn elsewhere) or the recorder ran its course.
    Ended,
    /// No sound at all: why.
    Failed(String),
    /// This recorder can't stream.
    NoStream,
}

/// Listen for the name on one running stream (`utterance::wait_for_name`).
fn wake_on_stream(s: &Arc<Shared>, work: &mut dyn MicWork) -> StreamWake {
    let Some(raw) = work.open_stream() else { return StreamWake::NoStream };
    // Read on a thread of its own: while the first seconds of a long request
    // are being transcribed, the rest of it keeps arriving.
    let mut stream = crate::utterance::pumped(raw);
    s.recording.store(true, Ordering::SeqCst);
    let cfg = work.endpointing();
    let clip = work.clip_secs();
    let stop_s = s.clone();
    let stop = move || stop_s.wake_should_stop();
    let count_s = s.clone();
    let heard = move || {
        count_s.heard_audio.fetch_add(1, Ordering::SeqCst);
    };
    let got = crate::utterance::wait_for_name(&mut stream, work, &cfg, clip, &stop, &heard);
    drop(stream);
    s.recording.store(false, Ordering::SeqCst);
    match got {
        Ok(Some(crate::utterance::Woke::Request(r))) => StreamWake::Heard(Heard::Wake(Ok(r))),
        Ok(Some(crate::utterance::Woke::NameOnly)) => StreamWake::Heard(Heard::Named),
        Ok(None) => StreamWake::Ended,
        Err(e) => StreamWake::Failed(e),
    }
}

/// A third of a second from the microphone, to see whether it works.
fn probe_once(s: &Shared, work: &mut dyn MicWork) {
    let Some(mut stream) = work.open_stream() else { return };
    s.recording.store(true, Ordering::SeqCst);
    let mut n = 0;
    while n < ROOM_WINDOWS && !s.stop.load(Ordering::SeqCst) && !s.paused.load(Ordering::SeqCst) && !s.busy.load(Ordering::SeqCst) {
        if stream.read(WINDOW).is_none() {
            break;
        }
        n += 1;
    }
    drop(stream);
    s.recording.store(false, Ordering::SeqCst);
    if n > 0 {
        s.heard_audio.fetch_add(1, Ordering::SeqCst);
    }
}

/// Does this end like a whole sentence (whisper punctuates what it hears)?
pub fn sentence_finished(words: &str) -> bool {
    let t = words.trim();
    t.split_whitespace().count() >= 2 && t.ends_with(['.', '?', '!'])
}

/// What was said with the name, joined to what the listen after it heard.
///
/// 29 Sep 2026: the words in the wake word's own clip were thrown away and a
/// fresh recording started once whisper had found the name -- by then "Atlas,
/// can you see me?" was over, and Atlas said "I heard my name but nothing
/// after it" five times in an evening. Now those words are the start of what
/// you said; a listen that hears nothing more leaves them as the whole of it.
pub fn with_wake_word(with_name: Option<String>, then: std::result::Result<String, String>) -> std::result::Result<String, String> {
    let first = with_name.map(|w| w.trim().to_string()).filter(|w| !w.is_empty());
    match (first, then) {
        (None, then) => then,
        (Some(w), Ok(more)) if !more.trim().is_empty() => Ok(format!("{w} {}", more.trim())),
        (Some(w), Ok(_)) => Ok(w),
        (Some(w), Err(why)) if why.contains(crate::voice::HEARD_NOTHING) => Ok(w),
        (Some(_), Err(why)) => Err(why),
    }
}

/// The room, with Atlas silent: a third of a second of it, for the gate's
/// floor and the detector's.
fn measure_room(s: &Shared, work: &mut dyn MicWork, det: &mut dyn VoiceDetector, gate: &mut BargeGate) {
    let Some(mut stream) = work.open_stream() else {
        s.note("cutting in by voice: the microphone can't be streamed here (no microphone named for it)".into());
        return;
    };
    s.recording.store(true, Ordering::SeqCst);
    let mut n = 0;
    while n < ROOM_WINDOWS && !s.stop.load(Ordering::SeqCst) && !s.paused.load(Ordering::SeqCst) {
        let Some(w) = stream.read(WINDOW) else { break };
        let _ = det.speech(&w);
        gate.learn_room(crate::audio::level_db(&w));
        n += 1;
    }
    drop(stream);
    s.recording.store(false, Ordering::SeqCst);
    if gate.knows_the_room() {
        s.note("cutting in by voice: measured the room while nothing was going on".into());
    }
}

/// While Atlas speaks: read the microphone a window at a time until you
/// speak over it (then take your words) or the reply ends.
fn watch(s: &Shared, work: &mut dyn MicWork, det: &mut dyn VoiceDetector, gate: &mut BargeGate) {
    let cfg = s.barge.lock().or_else(crate::crash::unpoison).map(|b| b.clone()).unwrap_or_default();
    let Some(mut stream) = work.open_stream() else {
        s.note("cutting in by voice: the microphone can't be streamed here (no microphone named for it)".into());
        return;
    };
    s.recording.store(true, Ordering::SeqCst);
    det.reset();
    gate.new_reply(&cfg);
    // The last 600 ms, so the start of what you said isn't lost to the time
    // it took to be sure it was you.
    let pre_windows = 600 / WINDOW_MS as usize;
    let mut recent: std::collections::VecDeque<Vec<i16>> = std::collections::VecDeque::new();
    let mut heard = false;
    while !s.watch_should_stop() {
        let Some(w) = stream.read(WINDOW) else { break };
        let p = det.speech(&w);
        let level = crate::audio::level_db(&w);
        let playing = work.playing_level();
        recent.push_back(w);
        if recent.len() > pre_windows {
            recent.pop_front();
        }
        if gate.feed(p, level, playing) {
            heard = true;
            break;
        }
    }
    if let Some(lag) = gate.lag_windows() {
        if !s.lag_said.swap(true, Ordering::SeqCst) {
            let line = if gate.coupling_db == Some(NO_ECHO_DB) {
                "cutting in by voice: the microphone doesn't hear Atlas's own voice (a headset?)".to_string()
            } else {
                format!("cutting in by voice: the microphone hears Atlas about {} ms after it plays", lag as u32 * WINDOW_MS)
            };
            s.note(line);
        }
    }
    if !heard {
        drop(stream);
        s.recording.store(false, Ordering::SeqCst);
        return;
    }
    s.cut_pending.store(true, Ordering::SeqCst);
    cut_playback();
    // Keep recording until you stop: 700 ms without speech, or 15 s.
    let mut kept: Vec<i16> = recent.into_iter().flatten().collect();
    let mut quiet_ms = 0u32;
    let started = Instant::now();
    while quiet_ms < 700 && started.elapsed() < Duration::from_secs(15) && !s.stop.load(Ordering::SeqCst) && !s.paused.load(Ordering::SeqCst) {
        let Some(w) = stream.read(WINDOW) else { break };
        let speaking = det.speech(&w).map(|p| p >= cfg.threshold).unwrap_or(true);
        quiet_ms = if speaking { 0 } else { quiet_ms + WINDOW_MS };
        kept.extend_from_slice(&w);
    }
    drop(stream);
    // A failure is written down (29 Sep 2026): it used to become "no words",
    // the reply stopped, and nothing said why.
    let words = match work.transcribe(&kept) {
        Ok(t) => t.trim().to_string(),
        Err(e) => {
            s.note(format!("couldn't make out what you said over me: {e}"));
            String::new()
        }
    };
    s.recording.store(false, Ordering::SeqCst);
    if let Ok(mut slot) = s.cut_words.lock().or_else(crate::crash::unpoison) {
        *slot = Some(words);
    }
}
