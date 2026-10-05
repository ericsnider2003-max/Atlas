//! Hearing a quiet voice without being asked to shout.
//!
//! **Why this exists (Eric, 29 Sep 2026: "I feel like I have to yell to get
//! Atlas to hear me").** His laptop's `hearing` record had the webcam
//! microphone he was using at -90.3 dB in a quiet room -- a very low input
//! level in Windows, or a microphone that gates itself to digital silence
//! between words -- and the only check between a recording and the speech
//! engine (`audio::check_speech`) asked for speech above a fixed -62 dBFS
//! "however quiet the room". On a microphone whose room reads -90, a voice
//! at -68 dB stands 22 dB clear of the room and was thrown away as silence
//! (only its loudest syllables reached -62, for less than the quarter second
//! of speech the check asks for); the same words 20 dB louder got through.
//! So he was, exactly, being asked to raise his voice.
//!
//! What changes:
//!
//! * **Speech is judged against the room, not a fixed line.** A clip is
//!   speech when enough of it stands `SPEECH_OVER_ROOM_DB` above its own
//!   quiet parts; the only absolute line left (`audio::QUIETEST_SPEECH_DB`)
//!   is just above a dead device's dither, so a stream of zeros and clicks is
//!   still never transcribed.
//! * **The level is made right before the speech engine hears it.** Each
//!   microphone's speech level is remembered over its last `REMEMBERED`
//!   utterances, and what is handed to whisper is turned up to
//!   `TARGET_SPEECH_DB` -- by the typical level, not by the loudest syllable
//!   of one clip, so a cough doesn't set the gain -- with two ceilings: the
//!   room is never raised above `NOISE_CEILING_DB` (turning up a hiss until it
//!   is as loud as a voice makes whisper write words that were never said),
//!   and never more than `MAX_GAIN_DB`. Peaks are held under full scale.
//! * **The speech detector already works on the room, not on levels.**
//!   `vad` learns the room and votes on how far a window stands above it, so
//!   it gives the same answer for a voice at -60 dB over a -90 dB room as at
//!   -30 over -60: turning the stream up in front of it would change nothing
//!   except while the gain moved, when it would shift the room it had
//!   learned. The one absolute line the cutter used (`endpoint`'s -38 dB,
//!   while the detector is still learning the room) is room-relative now too
//!   (`utterance::Segmenter`).
//! * **Which microphone hears you best** is decided by how far your voice
//!   stands above the room on it (`snr_db`), measured from what you actually
//!   said, not by how loud a second of an empty room was (`hearing`).
//! * **A very low input level is said plainly** (`quiet_input`), with where
//!   to raise it -- and, on Windows, raised (`miclevel`), once, keeping the
//!   level it was at.

use serde::{Deserialize, Serialize};

/// Frames the level is read in.
pub const FRAME_MS: usize = 30;
/// Speech handed to the speech engine sits about here (RMS, dBFS).
pub const TARGET_SPEECH_DB: f32 = -20.0;
/// The room is never turned up past this: a hiss as loud as a voice is
/// transcribed as words.
pub const NOISE_CEILING_DB: f32 = -50.0;
/// The most a clip is ever turned up (100x).
pub const MAX_GAIN_DB: f32 = 40.0;
/// Peaks are held at or under this share of full scale (about -1 dBFS).
pub const PEAK_CEILING: f32 = 0.9;
/// Utterances remembered per microphone.
pub const REMEMBERED: usize = 8;
/// A typical speech level under this, over `QUIET_AFTER` utterances, is a
/// microphone set too low: said, and on Windows raised.
pub const LOW_INPUT_DB: f32 = -42.0;
/// Utterances before a microphone is judged too quiet.
pub const QUIET_AFTER: usize = 3;
/// Under this signal-to-noise a microphone isn't really hearing you.
pub const MIN_SNR_DB: f32 = 10.0;

/// What one utterance sounded like on the microphone.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Measured {
    /// The voice's level: the energy mean of the frames that are speech.
    pub speech_db: f32,
    /// The room's level in the same clip: its quietest fifth.
    pub noise_db: f32,
}

fn frame_levels(samples: &[i16], rate: u32) -> Vec<f32> {
    let frame = (rate as usize * FRAME_MS / 1000).max(1);
    samples.chunks(frame).filter(|c| c.len() == frame).map(crate::audio::level_db).collect()
}

/// The room's level in a clip (its quietest fifth of frames), and the bar a
/// frame must clear to be speech: `SPEECH_OVER_ROOM_DB` above the room, and
/// never under a dead device's dither (`QUIETEST_SPEECH_DB`).
fn room_and_bar(levels: &[f32]) -> Option<(f32, f32)> {
    if levels.is_empty() {
        return None;
    }
    let mut sorted = levels.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let room = sorted[sorted.len() / 5];
    Some((room, (room + crate::audio::SPEECH_OVER_ROOM_DB).max(crate::audio::QUIETEST_SPEECH_DB)))
}

/// How loud the voice and the room were in `samples`. `None` when there is
/// not enough speech in it to say (`audio::MIN_SPEECH_MS`).
pub fn measure_voice(samples: &[i16], rate: u32) -> Option<Measured> {
    let levels = frame_levels(samples, rate);
    if levels.len() < 4 {
        return None;
    }
    let (room, bar) = room_and_bar(&levels)?;
    let speech: Vec<f32> = levels.iter().copied().filter(|l| *l >= bar).collect();
    if (speech.len() * FRAME_MS) < crate::audio::MIN_SPEECH_MS as usize {
        return None;
    }
    let energy = speech.iter().map(|l| 10f64.powf(f64::from(*l) / 10.0)).sum::<f64>() / speech.len() as f64;
    Some(Measured { speech_db: (10.0 * energy.max(1e-12).log10()) as f32, noise_db: room })
}

/// The gain, in dB, that brings speech at `speech_db` to `TARGET_SPEECH_DB`
/// without lifting a room at `noise_db` past `NOISE_CEILING_DB` or going over
/// `MAX_GAIN_DB`. Never below zero: loud speech is left as it is.
pub fn gain_db(speech_db: f32, noise_db: f32) -> f32 {
    (TARGET_SPEECH_DB - speech_db).min(NOISE_CEILING_DB - noise_db).min(MAX_GAIN_DB).max(0.0)
}

/// `samples` turned up by `gain_db`, less if that would take the loudest
/// peak past `PEAK_CEILING` of full scale: a clip is never clipped.
pub fn levelled(samples: &[i16], gain_db: f32) -> Vec<i16> {
    if gain_db <= 0.0 || samples.is_empty() {
        return samples.to_vec();
    }
    let mut g = 10f32.powf(gain_db / 20.0);
    let peak = f32::from(samples.iter().map(|s| s.unsigned_abs()).max().unwrap_or(1)).max(1.0);
    let ceiling = PEAK_CEILING * f32::from(i16::MAX);
    if peak * g > ceiling {
        g = ceiling / peak;
    }
    samples.iter().map(|s| (f32::from(*s) * g).clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16).collect()
}

/// What Atlas has heard of your voice on one microphone.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MicLevel {
    pub name: String,
    /// The speech level of the last few utterances, newest last.
    pub speech: Vec<f32>,
    /// The room under them, newest last.
    pub noise: Vec<f32>,
}

fn median(v: &[f32]) -> Option<f32> {
    if v.is_empty() {
        return None;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some(s[s.len() / 2])
}

/// Every microphone's remembered levels. Kept in the store as `RECORD`,
/// written only by whatever transcribes (the microphone's thread).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Leveller {
    pub mics: Vec<MicLevel>,
}

impl Leveller {
    pub const RECORD: &'static str = "loudness";

    fn mic(&self, name: &str) -> Option<&MicLevel> {
        self.mics.iter().find(|m| m.name == name)
    }

    /// An utterance heard through `mic`.
    pub fn heard(&mut self, mic: &str, m: Measured) {
        if mic.trim().is_empty() {
            return;
        }
        let i = match self.mics.iter().position(|x| x.name == mic) {
            Some(i) => i,
            None => {
                self.mics.push(MicLevel { name: mic.to_string(), ..MicLevel::default() });
                self.mics.len() - 1
            }
        };
        let e = &mut self.mics[i];
        e.speech.push(m.speech_db);
        e.noise.push(m.noise_db);
        while e.speech.len() > REMEMBERED {
            e.speech.remove(0);
        }
        while e.noise.len() > REMEMBERED {
            e.noise.remove(0);
        }
    }

    /// How many utterances are remembered for `mic`.
    pub fn heard_count(&self, mic: &str) -> usize {
        self.mic(mic).map_or(0, |m| m.speech.len())
    }

    /// Your typical speech level on `mic` (the median of what's remembered),
    /// once there are at least two utterances to go on.
    pub fn speech_db(&self, mic: &str) -> Option<f32> {
        self.mic(mic).filter(|m| m.speech.len() >= 2).and_then(|m| median(&m.speech))
    }

    /// The typical room under it.
    fn noise_db(&self, mic: &str) -> Option<f32> {
        self.mic(mic).filter(|m| m.noise.len() >= 2).and_then(|m| median(&m.noise))
    }

    /// How far your voice stands above the room on `mic`.
    pub fn snr_db(&self, mic: &str) -> Option<f32> {
        Some(self.speech_db(mic)? - self.noise_db(mic)?)
    }

    /// The gain for a clip from `mic`: by your typical level when it's known,
    /// else by this clip's own; never lifting this clip's room past the
    /// ceiling.
    pub fn gain_for(&self, mic: &str, clip: Measured) -> f32 {
        let speech = self.speech_db(mic).unwrap_or(clip.speech_db);
        crate::leveller::gain_db(speech, clip.noise_db)
    }

    /// Is `mic` set so low you'd have to raise your voice? `Some(typical
    /// speech level)` when it is, over at least `QUIET_AFTER` utterances.
    pub fn quiet_input(&self, mic: &str) -> Option<f32> {
        if self.heard_count(mic) < QUIET_AFTER {
            return None;
        }
        self.speech_db(mic).filter(|db| *db < LOW_INPUT_DB)
    }
}

/// A clip ready for the speech engine, from `mic`: `None` when nobody is
/// speaking in it (whisper is never asked about a quiet room); otherwise the
/// clip turned up to the target level, with what was heard remembered in
/// `lev` so the next clip is levelled by your typical voice.
pub fn for_speech_to_text(samples: &[i16], rate: u32, mic: &str, lev: &mut Leveller) -> Option<Vec<i16>> {
    match crate::audio::check_speech(samples, rate) {
        crate::audio::SpeechCheck::Silence => None,
        other => match crate::leveller::measure_voice(samples, rate) {
            Some(m) => {
                let g = lev.gain_for(mic, m);
                lev.heard(mic, m);
                Some(crate::leveller::levelled(samples, g))
            }
            // Too short to measure, and given the benefit of the doubt.
            None => Some(match other {
                crate::audio::SpeechCheck::Quiet(g) => crate::audio::turned_up(samples, g),
                _ => samples.to_vec(),
            }),
        },
    }
}

/// The running Atlas's levels, loaded once from the store and saved after
/// each utterance. One per process: the microphone's thread is the only
/// writer.
static LEVELS: std::sync::Mutex<Option<Leveller>> = std::sync::Mutex::new(None);

/// `for_speech_to_text` with the running Atlas's remembered levels.
pub fn prepare(samples: &[i16], rate: u32, mic: &str) -> Option<Vec<i16>> {
    let store = crate::roots::store();
    let Ok(mut guard) = LEVELS.lock().or_else(crate::crash::unpoison) else {
        return for_speech_to_text(samples, rate, mic, &mut Leveller::default());
    };
    let lev = guard.get_or_insert_with(|| store.load(Leveller::RECORD));
    let before = lev.clone();
    let out = for_speech_to_text(samples, rate, mic, lev);
    if *lev != before {
        let _ = store.save(Leveller::RECORD, &*lev);
    }
    out
}

/// What's remembered, as the store has it (for the daemon, which only reads).
pub fn remembered() -> Leveller {
    crate::roots::store().load(Leveller::RECORD)
}

/// The sentence for a microphone set too low: plainly, with where to raise
/// it, and what Atlas did about it. `level` is the input level Windows has it
/// at (0..1) when that could be read; `raised_to` is where Atlas put it.
pub fn quiet_input_line(mic_short: &str, speech_db: f32, level: Option<crate::miclevel::InputLevel>, raised_to: Option<f32>) -> String {
    let pct = |x: f32| format!("{}%", (x * 100.0).round() as i32);
    match (level, raised_to) {
        (Some(l), _) if l.muted => format!(
            "{mic_short} is muted in Windows, so I only hear you when you're loud. Unmute it in Sound settings (Input), and I'll hear you at a normal voice."
        ),
        (Some(l), Some(to)) => format!(
            "{mic_short} was set to {} in Windows, which is why you've had to raise your voice. I've turned it up to {} -- if you'd rather it went back, it's the input volume in Sound settings.",
            pct(l.scalar),
            pct(to)
        ),
        (Some(l), None) => format!(
            "Your voice reaches me very quietly through {mic_short} (its level in Windows is {}). I'm turning you up myself, but raising the input volume in Sound settings (Input) will make me hear you better.",
            pct(l.scalar)
        ),
        (None, _) => format!(
            "Your voice reaches me very quietly through {mic_short} (about {speech_db:.0} dB). I'm turning you up myself, but if its input volume can be raised in Sound settings (Input), I'll hear you better."
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gain_has_two_ceilings() {
        // A quiet voice in a quiet room: all the way to the target.
        assert!((gain_db(-50.0, -80.0) - 30.0).abs() < 0.01);
        // A quiet voice in a hiss: the hiss stops it.
        assert!((gain_db(-50.0, -60.0) - 10.0).abs() < 0.01);
        // Never more than 100x.
        assert!((gain_db(-75.0, -96.0) - MAX_GAIN_DB).abs() < 0.01);
        // Never turned down.
        assert_eq!(gain_db(-10.0, -60.0), 0.0);
    }
}
