//! Tuning the speech detector to your room, from two recordings.
//!
//! Round 4 measured the detector's three thresholds on five synthesized rooms
//! and shipped the best. Your room is none of them. This takes two recordings
//! made on your own microphone — the room with nobody talking (fan, air
//! conditioning, whatever is usually on), and you talking somewhere quiet —
//! and builds the test the thresholds are scored on from them:
//!
//! 1. Where you are speaking is known from the quiet recording alone: frames
//!    within 35 dB of its loudest.
//! 2. The room recording is laid under it, at the level your microphone
//!    actually picked it up (both came through the same gain), with room-only
//!    stretches either side.
//! 3. The same grid as `tests/vad_measured.rs` is scored on that mix, and the
//!    best setting is written into your settings — where the three sliders on
//!    the settings page show it and you can move it back.

use crate::vad::VadParams;

/// What the calibration found, and what it would change.
#[derive(Debug, Clone, PartialEq)]
pub struct Calibration {
    pub best: VadParams,
    /// Balanced accuracy on the mix: (speech heard + silence left alone) / 2.
    pub best_score: f64,
    /// The same, for the thresholds in use before.
    pub before_score: f64,
    pub speech_heard: f64,
    pub silence_left: f64,
    /// Seconds of each recording used.
    pub room_secs: f64,
    pub voice_secs: f64,
}

impl Calibration {
    pub fn say(&self) -> String {
        format!(
            "Scored on your room: {:.1}% right with the thresholds you had, {:.1}% with energy {} / \
             flatness {} / loud {} dB — {:.0}% of your speech heard, {:.0}% of the room left alone. \
             (From {:.0} s of room and {:.0} s of you.)",
            100.0 * self.before_score,
            100.0 * self.best_score,
            self.best.energy_db,
            self.best.flatness_db,
            self.best.loud_db,
            100.0 * self.speech_heard,
            100.0 * self.silence_left,
            self.room_secs,
            self.voice_secs
        )
    }
}

/// The grid searched: the same one the shipped defaults were chosen on.
pub const ENERGY: [f64; 9] = [0.5, 1.0, 1.5, 2.0, 3.0, 4.0, 6.0, 8.0, 10.0];
pub const FLATNESS: [f64; 7] = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 8.0];
pub const LOUD: [f64; 7] = [4.0, 6.0, 8.0, 10.0, 12.0, 16.0, 20.0];

/// Build the test mix and search the grid.
pub fn calibrate(room: &[i16], voice: &[i16], rate: u32, current: VadParams) -> Result<Calibration, String> {
    let frame = (rate as usize / 100).max(1);
    if room.len() < rate as usize * 5 {
        return Err("the room recording needs at least five seconds of the room with nobody talking".into());
    }
    let (mix, truth) = build_mix(room, voice, rate)?;
    let score = |p: VadParams| -> (f64, f64, f64) { score(&mix, &truth, rate, frame, p) };
    let (before, _, _) = score(current);
    let mut best = (f64::MIN, current, 0.0, 0.0);
    for e in ENERGY {
        for fl in FLATNESS {
            for loud in LOUD {
                let p = VadParams { energy_db: e, flatness_db: fl, loud_db: loud };
                let (s, h, q) = score(p);
                if s > best.0 + 1e-9 {
                    best = (s, p, h, q);
                }
            }
        }
    }
    Ok(Calibration {
        best: best.1,
        best_score: best.0,
        before_score: before,
        speech_heard: best.2,
        silence_left: best.3,
        room_secs: room.len() as f64 / rate as f64,
        voice_secs: voice.len() as f64 / rate as f64,
    })
}

/// Room alone, you over the room, room alone. Truth per 10 ms frame.
fn build_mix(room: &[i16], voice: &[i16], rate: u32) -> Result<(Vec<i16>, Vec<bool>), String> {
    let frame = (rate as usize / 100).max(1);
    let frames: Vec<f64> = voice
        .chunks(frame)
        .filter(|c| c.len() == frame)
        .map(|c| 10.0 * (c.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / frame as f64).max(1.0).log10())
        .collect();
    let peak = frames.iter().cloned().fold(f64::MIN, f64::max);
    let spoken: Vec<bool> = frames.iter().map(|d| *d >= peak - 35.0).collect();
    let voiced_secs = spoken.iter().filter(|x| **x).count() as f64 / 100.0;
    if voiced_secs < 2.0 {
        return Err("the recording of you needs at least two seconds of talking".into());
    }
    let lead = 3 * rate as usize;
    let total = lead + frames.len() * frame + lead;
    let mut mix = Vec::with_capacity(total);
    let mut truth = Vec::with_capacity(total / frame);
    for i in 0..total {
        let r = room[i % room.len()] as i32;
        let v = if i >= lead && i - lead < frames.len() * frame { voice[i - lead] as i32 } else { 0 };
        mix.push((r + v).clamp(-32768, 32767) as i16);
    }
    for f in 0..total / frame {
        let i = f * frame;
        truth.push(i >= lead && (i - lead) / frame < spoken.len() && spoken[(i - lead) / frame]);
    }
    Ok((mix, truth))
}

fn score(mix: &[i16], truth: &[bool], rate: u32, frame: usize, p: VadParams) -> (f64, f64, f64) {
    let mut v = crate::vad::Vad::tuned(rate, p);
    let (mut tp, mut sp, mut tn, mut sn) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for (i, c) in mix.chunks(frame).filter(|c| c.len() == frame).enumerate() {
        let said = v.window(c).map(|s| s > 0.5).unwrap_or(false);
        // The first 300 ms are the detector learning the room.
        if i < 30 || i >= truth.len() {
            continue;
        }
        if truth[i] {
            sp += 1.0;
            tp += said as u8 as f64;
        } else {
            sn += 1.0;
            tn += (!said) as u8 as f64;
        }
    }
    let (h, q) = (tp / sp.max(1.0), tn / sn.max(1.0));
    ((h + q) / 2.0, h, q)
}

/// Write the calibrated thresholds into your settings (the preferences layer
/// the settings page writes), so they show on the page and can be moved back.
fn keep(c: &Calibration, config_dir: &std::path::Path) -> crate::error::Result<()> {
    let mut p = crate::preferences::Preferences::load(config_dir);
    p.set("endpoint.vad_energy_db", &c.best.energy_db.to_string());
    p.set("endpoint.vad_flatness_db", &c.best.flatness_db.to_string());
    p.set("endpoint.vad_loud_db", &c.best.loud_db.to_string());
    p.save(config_dir)
}

/// What a calibration from two recordings found, and whether it was kept.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub calibration: Calibration,
    /// Written into your settings (it beat what you had).
    pub kept: bool,
}

impl Outcome {
    pub fn say(&self) -> String {
        if self.kept {
            format!("{} Kept: they're in your settings now, where you can move them back.", self.calibration.say())
        } else {
            format!("{} The thresholds you have are already as good as any on the grid, so nothing changed.", self.calibration.say())
        }
    }
}

/// From the two recordings as files. Keeps the result when it does better
/// than what you had.
pub fn from_recordings(room_wav: &[u8], voice_wav: &[u8], current: VadParams, config_dir: &std::path::Path) -> Result<Outcome, String> {
    let (room, r1) = crate::diarize::read_wav(room_wav)?;
    let (voice, r2) = crate::diarize::read_wav(voice_wav)?;
    if r1 != r2 {
        return Err(format!("the two recordings are at different rates ({r1} and {r2} Hz) — record both the same way"));
    }
    let c = calibrate(&room, &voice, r1, current)?;
    if c.best == current || c.best_score <= c.before_score + 0.005 {
        return Ok(Outcome { calibration: c, kept: false });
    }
    keep(&c, config_dir).map_err(|e| e.to_string())?;
    Ok(Outcome { calibration: c, kept: true })
}
