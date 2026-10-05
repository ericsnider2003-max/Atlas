//! Is that a voice, or the room? — speech detection that learns the room.
//!
//! **Source:** Moattar & Homayounpour (2009), *A Simple but Efficient
//! Real-Time Voice Activity Detection Algorithm* (EUSIPCO): per 10 ms frame,
//! three features — energy, the dominant frequency, and spectral flatness —
//! each compared with the room; a frame is speech when at least two of the
//! three say so, the room's level is re-learned from quiet frames, and runs
//! too short to be speech or silence are absorbed. Three changes from the
//! paper, each found failing on a test: energy is dB above the learned floor
//! (6 dB) rather than `40·log10(min E)`, which goes negative for a quiet
//! room; the frequency vote asks for a dominant frequency in the voice band
//! (80–1100 Hz) rather than "185 Hz above the room's", which broadband noise
//! passes and fails at random; and the room is re-learned only from frames
//! within 3 dB of it, because re-learning from every frame judged silent let
//! one misjudged syllable lift the floor until speech stopped counting at
//! all. The FFT is an in-place radix-2. Clean-room.
//!
//! **Why Atlas wants it.** `endpoint` decided "you've stopped talking" with a
//! fixed −38 dBFS line. A laptop fan or an air conditioner sits above that
//! line, so in a noisy room the turn never ended until the 20-second hard
//! stop; a quiet speaker in a quiet room sat below it and was cut off. This
//! learns the room in the first third of a second and then asks whether
//! what's above it is shaped like a voice.
//!
//! Silero VAD (a small neural model, MIT) is better still, and was
//! considered: it needs an ONNX runtime crate and a model file fetched from
//! outside the tree — third-party weight the project rule asks to avoid
//! unless it's earned. This is the in-house baseline it would have to beat.

const FFT: usize = 256;

// The FFT is `mfcc::fft_in_place` -- one copy for both.
use crate::mfcc::fft_in_place as fft;

/// (energy dB, dominant frequency Hz, spectral flatness dB) of one frame.
fn features(frame: &[i16], rate: u32) -> (f64, f64, f64) {
    let e: f64 = frame.iter().map(|s| (*s as f64 / 32768.0).powi(2)).sum::<f64>() / frame.len().max(1) as f64;
    let e_db = 10.0 * e.max(1e-12).log10();
    let mut re = vec![0.0; FFT];
    let mut im = vec![0.0; FFT];
    for (i, s) in frame.iter().take(FFT).enumerate() {
        // Hann window, so a frame's edges don't smear into every bin.
        let w = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (frame.len().min(FFT) as f64 - 1.0).max(1.0)).cos();
        re[i] = *s as f64 / 32768.0 * w;
    }
    fft(&mut re, &mut im);
    let mags: Vec<f64> = (1..FFT / 2).map(|k| (re[k] * re[k] + im[k] * im[k]).sqrt().max(1e-12)).collect();
    let (kmax, _) = mags.iter().enumerate().fold((0, 0.0), |b, (i, m)| if *m > b.1 { (i, *m) } else { b });
    let f = (kmax + 1) as f64 * rate as f64 / FFT as f64;
    let am = mags.iter().sum::<f64>() / mags.len() as f64;
    let gm = (mags.iter().map(|m| m.ln()).sum::<f64>() / mags.len() as f64).exp();
    (e_db, f, 10.0 * (gm / am).log10())
}

/// The detector's three thresholds, in dB. The defaults are **measured**:
/// see `DEFAULT_MEASURED` below and `tests/round4.rs`, which re-measures them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VadParams {
    /// Frame energy above the learned room floor for the loudness vote (the
    /// voice-band vote asks for half of it).
    pub energy_db: f64,
    /// How much less flat than the room's spectrum a frame must be.
    pub flatness_db: f64,
    /// Above the floor by this much, a frame is speech on loudness alone.
    pub loud_db: f64,
}

impl Default for VadParams {
    fn default() -> Self {
        DEFAULT_MEASURED
    }
}

/// Chosen by grid search for the best balanced accuracy (mean of hit rate
/// on speech frames and on silence frames), averaged over five rooms — a
/// quiet room, a fan at 10 and 5 dB SNR, an air conditioner (brown noise) at
/// 5 dB and mains hum at 10 dB — around two voices of real synthesized
/// speech (espeak-ng), frame truth from the clean recording. 23 Sep 2026:
/// 0.851, against 0.791 for the values chosen by hand the day before (6, 5,
/// 20), which heard only a third to a half of the speech in the noisy rooms.
/// Synthesized voices and made noise, not your room: `endpoint.vad_*` in
/// settings is there for the day a real room disagrees.
pub const DEFAULT_MEASURED: VadParams = VadParams { energy_db: 1.0, flatness_db: 4.0, loud_db: 12.0 };

#[derive(Debug, Clone)]
pub struct Vad {
    p: VadParams,
    rate: u32,
    frame: usize,
    learned: usize,
    silent: usize,
    floor_db: f64,
    floor_sfm: f64,
    leftover: Vec<i16>,
}

/// Frames of room the detector listens to before it judges anything.
const LEARN_FRAMES: usize = 30;

/// Quieter than any microphone's own noise: digital silence (`frame_is_speech`).
const DIGITAL_SILENCE_DB: f64 = -90.0;

impl Vad {
    pub fn new(rate: u32) -> Vad {
        Vad::tuned(rate, VadParams::default())
    }

    /// With your own thresholds (`endpoint.vad` in settings).
    pub fn tuned(rate: u32, p: VadParams) -> Vad {
        Vad {
            p,
            rate,
            frame: (rate as usize / 100).max(1),
            learned: 0,
            silent: 0,
            floor_db: f64::MAX,
            floor_sfm: f64::MAX,
            leftover: Vec::new(),
        }
    }

    fn frame_is_speech(&mut self, frame: &[i16]) -> Option<bool> {
        let (e, f, sfm) = features(frame, self.rate);
        // Digital silence -- exact zeros, or all but -- is not the room.
        // Microphones with the operating system's noise suppression on
        // (Windows' Voice Clarity and Studio Effects, a muted webcam) send
        // it between words, and learning the room from it put the floor at
        // -120 dB, so the real room, when it came back, was 60 dB "above the
        // room" and counted as speech until the recording's hard stop (29 Sep
        // 2026: found cutting real speech -- espeak clips end in exact zeros
        // -- into utterances that never ended). Not speech, and nothing
        // learned from it.
        if e < DIGITAL_SILENCE_DB {
            return (self.learned >= LEARN_FRAMES).then_some(false);
        }
        if self.learned < LEARN_FRAMES {
            self.floor_db = self.floor_db.min(e);
            self.floor_sfm = self.floor_sfm.min(sfm);
            self.learned += 1;
            self.silent = self.learned;
            return None;
        }
        // Three votes, two needed: louder than the room; a dominant
        // frequency where voices put their energy (the paper's "185 Hz above
        // the room's" fails for broadband noise, whose dominant bin wanders);
        // and a peakier spectrum than the room's — a voice is harmonic, a fan
        // is flat.
        let mut votes = 0;
        if e - self.floor_db >= self.p.energy_db {
            votes += 1;
        }
        if (80.0..=1100.0).contains(&f) && e - self.floor_db >= self.p.energy_db / 2.0 {
            votes += 1;
        }
        if self.floor_sfm - sfm >= self.p.flatness_db {
            votes += 1;
        }
        let speech = votes >= 2 || e - self.floor_db >= self.p.loud_db;
        // Re-learn the room only from frames that are clearly the room. The
        // paper updates on every frame judged silent; one misjudged syllable
        // then lifts the floor toward the voice, and the voice stops counting.
        if !speech && e < self.floor_db + 3.0 {
            self.floor_db = (self.silent as f64 * self.floor_db + e) / (self.silent as f64 + 1.0);
            self.floor_sfm = (self.silent as f64 * self.floor_sfm + sfm) / (self.silent as f64 + 1.0);
            self.silent = (self.silent + 1).min(500);
        }
        Some(speech)
    }

    /// Feed a window of audio. `None` while the room is still being learned
    /// (the caller falls back to its own rule); otherwise the share of the
    /// window's frames that were speech.
    pub fn window(&mut self, samples: &[i16]) -> Option<f32> {
        self.leftover.extend_from_slice(samples);
        let (mut speech, mut judged) = (0usize, 0usize);
        let frame = self.frame;
        while self.leftover.len() >= frame {
            let f: Vec<i16> = self.leftover.drain(..frame).collect();
            if let Some(s) = self.frame_is_speech(&f) {
                judged += 1;
                speech += s as usize;
            }
        }
        (judged > 0).then(|| speech as f32 / judged as f32)
    }
}

/// Speech or not, per 10 ms frame (the room-learning frames count as not).
fn frames(samples: &[i16], rate: u32, p: VadParams) -> Vec<bool> {
    let mut v = Vad::tuned(rate, p);
    let frame = v.frame;
    samples.chunks(frame).filter(|c| c.len() == frame).map(|c| v.frame_is_speech(c).unwrap_or(false)).collect()
}

/// Where the speech is in a recording, as sample ranges: frames judged, then
/// silences shorter than 300 ms bridged and speech shorter than 100 ms dropped.
pub fn segments(samples: &[i16], rate: u32) -> Vec<(usize, usize)> {
    let frame = (rate as usize / 100).max(1);
    let marks = frames(samples, rate, VadParams::default());
    let mut runs: Vec<(usize, usize)> = Vec::new(); // frame ranges
    let mut i = 0;
    while i < marks.len() {
        if marks[i] {
            let s = i;
            while i < marks.len() && marks[i] {
                i += 1;
            }
            runs.push((s, i));
        } else {
            i += 1;
        }
    }
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for r in runs {
        match merged.last_mut() {
            Some(m) if r.0 - m.1 < 30 => m.1 = r.1,
            _ => merged.push(r),
        }
    }
    merged.into_iter().filter(|(s, e)| e - s >= 10).map(|(s, e)| (s * frame, e * frame)).collect()
}

/// The level the endpointer should see for a window: the detector's verdict
/// moved to the right side of the endpointer's own line, so a loud fan reads
/// as quiet and a quiet voice reads as speech. While the room is still being
/// learned (`share` is None) the measured level passes through unchanged.
pub fn level_for_endpoint(db: f32, share: Option<f32>, silence_below_db: f32) -> f32 {
    match share {
        Some(s) if s >= 0.3 => db.max(silence_below_db + 1.0),
        Some(_) => db.min(silence_below_db - 1.0),
        None => db,
    }
}
