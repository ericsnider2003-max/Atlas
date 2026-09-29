//! Turning a recorded turn into something `voiceid` can compare.
//!
//! `voiceid.rs` was written complete — the comparison, the grey band, the
//! policy that decides whether Atlas listens rather than whether Atlas is
//! allowed — and it has never once run. Its own doc comment says embeddings
//! come from "an external speaker-encoder"; no such encoder existed anywhere
//! in the tree, no tool was configured for one, and `VoiceId::check` had no
//! caller. The capability was reachable, tested, and did nothing.
//!
//! It is worth being precise about why the wiring guard missed it, because it
//! is the sharpest example of that blind spot in this codebase: `voiceid` left
//! `UNWIRED_BASELINE` when `recall.rs` started calling `voiceid::cosine` to
//! compare *notes*. One vector-maths helper, borrowed for an unrelated job,
//! made the whole module count as wired while the speaker identification it
//! exists for still had no path to it. Reachability and "actually runs" are
//! different questions.
//!
//! This module is the missing half, and it is deliberately the smaller half:
//! the encoder itself is an external program, the same way speech-to-text and
//! text-to-speech are. What lives here is the seam — running it over the wav
//! the turn was already recorded into, and reading the numbers back.

use crate::error::{AtlasError, Result};
use crate::tools::{ExternalTool, Vars};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct SpeakerConfig {
    /// The program that turns `{in_wav}` into an embedding.
    ///
    /// Absent means Atlas cannot tell voices apart on this machine. That is
    /// reported rather than silently treated as "everything is you" — see
    /// `NO_ENCODER`.
    pub tool: Option<ExternalTool>,
}

/// Read an embedding from whatever the encoder printed.
///
/// Accepts a JSON array or plain separated numbers, because every speaker
/// encoder prints something slightly different and requiring one exact format
/// would mean the first one you try does not work for a reason that has
/// nothing to do with your voice.
pub fn parse_embedding(raw: &str) -> Result<Vec<f32>> {
    let cleaned: String = raw
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .chars()
        .map(|c| if c == ',' || c == ';' { ' ' } else { c })
        .collect();
    let mut out = Vec::new();
    for tok in cleaned.split_whitespace() {
        match tok.parse::<f32>() {
            Ok(n) if n.is_finite() => out.push(n),
            // A NaN would poison the centroid permanently and quietly: every
            // later comparison against it returns 0, which reads as "not you"
            // forever. Refusing here is the recoverable failure.
            Ok(_) => return Err(AtlasError::Platform("the encoder returned NaN".into())),
            Err(_) => {
                return Err(AtlasError::Platform(format!(
                    "the encoder printed something that isn't a number: {}",
                    tok.chars().take(40).collect::<String>()
                )))
            }
        }
    }
    if out.len() < 16 {
        return Err(AtlasError::Platform(format!(
            "the encoder returned {} numbers, which is too few to be a voice embedding",
            out.len()
        )));
    }
    Ok(out)
}

/// Run the encoder over the wav this turn was recorded into.
///
/// An installed external encoder is used when there is one. Otherwise the
/// built-in one: the clip's statistics join the machine's background (so every
/// turn teaches Atlas what ordinary voices here sound like), and once the
/// background is big enough the clip comes back standardised against it.
pub fn embed(cfg: &SpeakerConfig, vars: &Vars) -> Result<Vec<f32>> {
    if let Some(tool) = cfg.tool.as_ref() {
        if tool.available(vars) {
            return parse_embedding(&tool.run(vars, None)?);
        }
    }
    let path = vars
        .get("in_wav")
        .ok_or_else(|| AtlasError::Platform("no recording to read a voice from".into()))?;
    let bytes = std::fs::read(path)?;
    let (samples, rate) = crate::diarize::read_wav(&bytes).map_err(AtlasError::Platform)?;
    let frames = clip_frames(&samples, rate)
        .ok_or_else(|| AtlasError::Platform("under half a second of speech in that".into()))?;
    let store = crate::roots::store();
    let mut bg = background(&store);
    bg.add(&frames);
    let _ = store.save(BACKGROUND, &bg);
    bg.embed(&frames).ok_or_else(|| AtlasError::Platform(still_learning(&bg)))
}

/// Which encoder this machine uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoder {
    /// The program set as `speaker.tool`, and it is installed.
    External,
    /// Atlas's own (a supervector against the background model).
    BuiltIn,
}

pub fn which(cfg: &SpeakerConfig, vars: &Vars) -> Encoder {
    match cfg.tool.as_ref() {
        Some(t) if t.available(vars) => Encoder::External,
        _ => Encoder::BuiltIn,
    }
}

/// The machine's background of ordinary voices, as heard so far.
pub fn background(store: &crate::store::Store) -> Background {
    store.load(BACKGROUND)
}

/// Teach the background from a recording: every stretch of speech in it is a
/// clip. For a podcast, a call, the television — the voices that are not you.
/// Returns how many clips it learned from.
pub fn learn_background(samples: &[i16], rate: u32, store: &crate::store::Store) -> Result<usize> {
    let mut bg = background(store);
    let mut n = 0;
    for (s, e) in crate::vad::segments(samples, rate) {
        if let Some(f) = clip_frames(&samples[s..e], rate) {
            bg.add(&f);
            n += 1;
        }
    }
    store.save(BACKGROUND, &bg)?;
    Ok(n)
}

pub fn still_learning(bg: &Background) -> String {
    format!(
        "I'm using my own voice encoder here, and it needs to have heard {MIN_BACKGROUND} clips of \
         ordinary talk before it can tell voices apart (heard {} so far). Bring in a recording of \
         other voices — a podcast, a call — on the hub's calendar page, or just keep using Atlas.",
        bg.clips
    )
}

const BACKGROUND: &str = "voice_background";

/// Can this machine tell voices apart right now?
///
/// Yes when an external encoder is installed, or when the built-in one has
/// heard enough of a background to standardise against. A configured encoder
/// that is not installed counts as none — reporting it as present would be a
/// check that cannot fail.
pub fn available(cfg: &SpeakerConfig, vars: &Vars) -> bool {
    which(cfg, vars) == Encoder::External || background(&crate::roots::store()).ready()
}

/// Said plainly when Atlas has no way to tell your voice from anyone else's.
///
/// The wording matters: voice-lock being unavailable must never read as
/// voice-lock being *on*. With no encoder every verdict is `NotEnrolled`, and
/// `voiceid::handle` deliberately proceeds on that — never gate on a system
/// that has not been taught your voice. So the honest description is that
/// Atlas is listening to everyone, which is what it has always done.
pub const NO_ENCODER: &str =
    "I can't tell voices apart on this machine yet — no external speaker encoder is installed, and \
     my own one hasn't heard enough ordinary talk to compare against, so I listen to whoever \
     speaks. Nothing has changed for the worse: this is how Atlas has always behaved. But \
     voice-lock isn't protecting you until it has, and you've enrolled.";

// ---------------------------------------------------------------------------
// The built-in encoder.
//
// No external encoder installed used to mean no speaker check at all. This is
// the in-house one, the classical GMM-UBM recipe (Reynolds, Quatieri & Dunn
// 2000): a 16-component mixture fitted to cepstral frames of ordinary talk
// this machine has heard (the "universal background model"), and each clip
// described by how far it pulls that mixture's means (`gmm::supervector`).
// Clips are compared sound-class by sound-class, which is what makes a
// two-second clip about the voice rather than about the words. Weaker than a
// trained neural encoder; `doctor` says which one is in use.
// ---------------------------------------------------------------------------

/// Mixture components in the background model.
pub const COMPONENTS: usize = 16;
/// MAP relevance factor: how much evidence a clip needs to move a component.
pub const RELEVANCE: f32 = 16.0;
/// Length of a built-in embedding.
pub const BUILTIN_DIMS: usize = COMPONENTS * crate::mfcc::CEPS;
/// Frames the background keeps (a reservoir sample of everything heard).
const POOL: usize = 6000;

/// The voiced frames of one clip. `None` under half a second of speech.
pub fn clip_frames(samples: &[i16], rate: u32) -> Option<Vec<[f32; crate::mfcc::CEPS]>> {
    let all = crate::mfcc::frames(samples, rate);
    let v = crate::mfcc::voiced(&all, 30.0);
    (v.len() >= 50).then(|| v.iter().map(|f| f.c).collect())
}

/// What ordinary talk on this machine sounds like: a sample of frames from
/// every clip heard, and the mixture fitted to them.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Background {
    pub clips: u64,
    pool: Vec<[f32; crate::mfcc::CEPS]>,
    seen: u64,
    #[serde(default)]
    ubm: Option<crate::gmm::Gmm>,
    #[serde(default)]
    trained_at: u64,
}

impl Background {
    /// Take in one clip's frames; refit the mixture every ten clips once
    /// there are enough.
    pub fn add(&mut self, frames: &[[f32; crate::mfcc::CEPS]]) {
        self.clips += 1;
        for f in frames {
            self.seen += 1;
            if self.pool.len() < POOL {
                self.pool.push(*f);
            } else {
                // Reservoir sampling, with a deterministic generator so the
                // same clips always give the same model.
                let j = (self.seen.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407) >> 17) % self.seen;
                if (j as usize) < POOL {
                    self.pool[j as usize] = *f;
                }
            }
        }
        if self.clips >= MIN_BACKGROUND && (self.ubm.is_none() || self.clips >= self.trained_at + 10) {
            self.ubm = crate::gmm::Gmm::fit(&self.pool, COMPONENTS, 10);
            self.trained_at = self.clips;
        }
    }

    /// The model is fitted, so clips can be compared.
    pub fn ready(&self) -> bool {
        self.ubm.is_some()
    }

    pub fn embed(&self, frames: &[[f32; crate::mfcc::CEPS]]) -> Option<Vec<f32>> {
        self.ubm.as_ref().map(|g| g.supervector(frames, RELEVANCE))
    }

    /// A background from these clips alone, fitted at once (for a recording
    /// with nothing better to compare against, and for tests).
    pub fn of(clips: &[Vec<[f32; crate::mfcc::CEPS]>]) -> Background {
        let mut b = Background::default();
        for c in clips {
            b.clips += 1;
            b.pool.extend_from_slice(c);
        }
        b.ubm = crate::gmm::Gmm::fit(&b.pool, COMPONENTS, 10);
        b.trained_at = b.clips;
        b
    }
}

/// Clips the background must have heard before voice-lock trusts the
/// built-in encoder. Below it every verdict stays `NotEnrolled`, which
/// listens to everyone — the behaviour Atlas has always had.
pub const MIN_BACKGROUND: u64 = 30;

/// Embeddings for every stretch of speech in a recording, for telling its
/// voices apart: against the machine's background when it has one, against
/// the recording's own speech when not, and centred on the recording, so what
/// is compared is how the voices differ from each other rather than how this
/// microphone differs from the ones the background heard.
pub fn recording_embeddings(samples: &[i16], rate: u32, machine: &Background) -> Vec<((usize, usize), Option<Vec<f32>>)> {
    let segs = crate::vad::segments(samples, rate);
    let frames: Vec<Option<Vec<[f32; crate::mfcc::CEPS]>>> = segs.iter().map(|(s, e)| clip_frames(&samples[*s..*e], rate)).collect();
    let own;
    let bg = if machine.ready() {
        machine
    } else {
        own = Background::of(&frames.iter().flatten().cloned().collect::<Vec<_>>());
        &own
    };
    let raw: Vec<Option<Vec<f32>>> = frames.iter().map(|f| f.as_ref().and_then(|f| bg.embed(f))).collect();
    let present: Vec<&Vec<f32>> = raw.iter().flatten().collect();
    if present.is_empty() {
        return segs.into_iter().map(|s| (s, None)).collect();
    }
    let dims = present[0].len();
    let mean: Vec<f32> = (0..dims).map(|d| present.iter().map(|v| v[d]).sum::<f32>() / present.len() as f32).collect();
    segs.into_iter()
        .zip(raw)
        .map(|(s, e)| (s, e.map(|v| v.iter().zip(&mean).map(|(a, b)| a - b).collect())))
        .collect()
}

/// Clusters of centred embeddings form above this: "more alike than the
/// average pair in the recording".
pub const GROUP_CENTRED_AT: f32 = 0.0;
