//! Knowing when you've stopped talking.
//!
//! Recording for a fixed eight seconds is the most-copied mistake in voice
//! software. It cuts you off mid-word when you have more to say, and makes you
//! wait seven seconds after "yes". On this machine it is also the single
//! biggest avoidable cost: every turn transcribes eight seconds of audio
//! whether you spoke for one or for seven.
//!
//! So Atlas listens for the silence instead. The whole problem is choosing how
//! long a silence has to be — too short and it cuts you off while you think,
//! too long and it feels slow. The answer is that it depends on what you were
//! saying, so that's what this measures.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct EndpointConfig {
    pub enabled: bool,
    /// Anything quieter than this is silence, in dBFS.
    pub silence_below_db: f32,
    /// Silence needed after a short reply — "yes", "the second one".
    pub short_reply_ms: u64,
    /// Silence needed after a full sentence, where you may still be thinking.
    pub sentence_ms: u64,
    /// Silence needed mid-phrase, where you're clearly not finished.
    pub mid_phrase_ms: u64,
    /// Give up if nothing is said at all.
    pub no_speech_after_ms: u64,
    /// Stop regardless after this, so a stuck microphone can't run forever.
    pub hard_stop_ms: u64,
    /// Ignore everything before this — the first moments catch the room, not
    /// you.
    pub warmup_ms: u64,
    /// The speech detector's thresholds (`vad::VadParams`), in dB. Shipped
    /// at the measured values; here so a room that proves them wrong can be
    /// tuned without a rebuild.
    pub vad_energy_db: f64,
    pub vad_flatness_db: f64,
    pub vad_loud_db: f64,
}

impl EndpointConfig {
    pub fn vad_params(&self) -> crate::vad::VadParams {
        crate::vad::VadParams { energy_db: self.vad_energy_db, flatness_db: self.vad_flatness_db, loud_db: self.vad_loud_db }
    }
}

impl Default for EndpointConfig {
    fn default() -> Self {
        EndpointConfig {
            enabled: true,
            silence_below_db: -38.0,
            // Production voice systems aim for a turn gap under half a second;
            // these are chosen against that.
            short_reply_ms: 380,
            sentence_ms: 700,
            mid_phrase_ms: 1100,
            no_speech_after_ms: 2500,
            hard_stop_ms: 20_000,
            warmup_ms: 120,
            vad_energy_db: crate::vad::DEFAULT_MEASURED.energy_db,
            vad_flatness_db: crate::vad::DEFAULT_MEASURED.flatness_db,
            vad_loud_db: crate::vad::DEFAULT_MEASURED.loud_db,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Listening {
    /// Nothing yet.
    Waiting,
    /// You're talking.
    Speaking,
    /// You've paused. Might be finished, might be thinking.
    Pausing,
    /// Done — send it.
    Finished(Why),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// You stopped and stayed stopped.
    YouFinished,
    /// You never said anything.
    Nothing,
    /// Something is wrong with the microphone.
    RanTooLong,
}

/// How complete what you've said sounds so far.
///
/// "Yes" is finished the moment you stop. "I want you to" plainly is not, and
/// cutting it off there is worse than waiting an extra half second.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Short,
    Sentence,
    MidPhrase,
}

/// Speech this short, with no words yet, is taken for a short reply ("yes",
/// "the second one", "stop"): about two or three words at a normal pace.
pub const SHORT_SPEECH_MS: u64 = 900;

/// Words that mean more is coming, however long the pause.
const HANGING: &[&str] = &[
    "and", "but", "or", "so", "then", "because", "which", "that", "the", "a", "an",
    "to", "for", "with", "of", "in", "on", "at", "if", "when", "my", "your",
    "is", "was", "will", "can", "could", "would", "should", "i", "we", "it",
];

pub fn shape_of(text: &str) -> Shape {
    let t = text.trim().to_lowercase();
    if t.is_empty() {
        return Shape::MidPhrase;
    }
    let words: Vec<&str> = t.split_whitespace().collect();
    let last = words.last().copied().unwrap_or("");
    // A dangling connective means you're mid-thought whatever the silence.
    if HANGING.contains(&last.trim_matches(|c: char| !c.is_alphanumeric())) {
        return Shape::MidPhrase;
    }
    if words.len() <= 3 {
        return Shape::Short;
    }
    Shape::Sentence
}

impl Shape {
    pub fn silence_needed(&self, cfg: &EndpointConfig) -> u64 {
        match self {
            Shape::Short => cfg.short_reply_ms,
            Shape::Sentence => cfg.sentence_ms,
            Shape::MidPhrase => cfg.mid_phrase_ms,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Endpointer {
    pub state: Listening,
    started_ms: u64,
    /// When the current silence began.
    quiet_since: Option<u64>,
    pub heard_anything: bool,
    /// Total time you were actually speaking.
    pub speech_ms: u64,
    last_ms: u64,
}

impl Default for Endpointer {
    fn default() -> Self {
        Endpointer {
            state: Listening::Waiting,
            started_ms: 0,
            quiet_since: None,
            heard_anything: false,
            speech_ms: 0,
            last_ms: 0,
        }
    }
}

impl Endpointer {
    pub fn start(now_ms: u64) -> Endpointer {
        Endpointer { started_ms: now_ms, last_ms: now_ms, ..Default::default() }
    }

    /// Feed in one short window of audio.
    ///
    /// `text_so_far` is whatever has been transcribed already, which is what
    /// makes the silence threshold depend on what you were saying.
    pub fn feed(
        &mut self,
        level_db: f32,
        text_so_far: &str,
        now_ms: u64,
        cfg: &EndpointConfig,
    ) -> Listening {
        if let Listening::Finished(_) = self.state {
            return self.state;
        }
        let since_start = now_ms.saturating_sub(self.started_ms);
        let step = now_ms.saturating_sub(self.last_ms);
        self.last_ms = now_ms;

        // A stuck microphone must not record forever.
        if since_start >= cfg.hard_stop_ms {
            self.state = Listening::Finished(Why::RanTooLong);
            return self.state;
        }
        // The first moments catch the room settling, not you.
        if since_start < cfg.warmup_ms {
            return self.state;
        }

        let loud = level_db > cfg.silence_below_db;
        if loud {
            self.heard_anything = true;
            self.speech_ms += step;
            self.quiet_since = None;
            self.state = Listening::Speaking;
            return self.state;
        }

        // Silence.
        if !self.heard_anything {
            if since_start >= cfg.no_speech_after_ms {
                self.state = Listening::Finished(Why::Nothing);
            }
            return self.state;
        }

        let began = *self.quiet_since.get_or_insert(now_ms);
        let quiet_for = now_ms.saturating_sub(began);
        // Nothing transcribed yet -- the usual case, since the words come
        // after the recording -- read as mid-phrase, so every short "yes"
        // waited out the longest gap (1.1 s) before anything happened
        // (research report, Stage 1 item 9). With no words to go on, how long
        // you spoke is the shape: a breath of speech is a short reply.
        let needed = if text_so_far.trim().is_empty() {
            if self.speech_ms <= SHORT_SPEECH_MS { Shape::Short } else { Shape::Sentence }
        } else {
            shape_of(text_so_far)
        }
        .silence_needed(cfg);

        self.state = if quiet_for >= needed {
            Listening::Finished(Why::YouFinished)
        } else {
            Listening::Pausing
        };
        self.state
    }

    pub fn finished(&self) -> bool {
        matches!(self.state, Listening::Finished(_))
    }

    /// How long the clip is, which is what transcription will cost.
    pub fn clip_ms(&self, now_ms: u64) -> u64 {
        now_ms.saturating_sub(self.started_ms)
    }
}

// The per-stage turn timing that used to live here was a dead duplicate of
// `timing::Turn`, which is the one the daemon actually builds and reads
// (`daemon.rs` `self.timing`). It was constructed only in a test. Removed so
// there is one answer to "how long did the turn take", not two.
