//! What the voice path decided and how long each stage took, kept after the
//! session so real latency can be read later (`atlas doctor`).
//!
//! `timing` keeps the last turns in memory and `daemon` writes one log line per
//! turn; neither survives a restart in a form the doctor can read, and neither
//! said which speech-to-text engine heard the turn or why. This keeps a small
//! ledger in the store: per turn the ASR decision (engine, reason, how long,
//! whether the text was thrown away as a quiet-room hallucination) and the
//! stage times: end of speech -> text ready (`asr_ms`), text -> first words
//! from the brain (`first_words_ms`), end of speech -> first audio
//! (`first_audio_ms`), speaking (synthesis) and playback.
//! Numbers and short reasons only, never the words.

use serde::{Deserialize, Serialize};
use std::sync::Mutex;

pub const RECORD: &str = "voice_latency";
/// Turns kept.
pub const KEEP: usize = 60;

/// How the ears decided for one clip.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AsrDecision {
    /// `parakeet`, `whisper`, or `none` (silence / canceled).
    pub engine: String,
    /// Why this engine, e.g. "parakeet installed" or
    /// "parakeet failed (recognizer exceeded 8s) -> whisper".
    pub why: String,
    /// Milliseconds spent in recognition (all engines tried).
    pub asr_ms: u32,
    pub audio_ms: u32,
    /// The text was a known quiet-room phrase on a quiet/short clip and was
    /// treated as no speech.
    pub dropped_hallucination: bool,
}

impl AsrDecision {
    pub fn line(&self) -> String {
        format!(
            "asr engine={} asr_ms={} audio_ms={} dropped_hallucination={} why=\"{}\"",
            self.engine, self.asr_ms, self.audio_ms, self.dropped_hallucination, self.why
        )
    }
}

static LAST: Mutex<Option<AsrDecision>> = Mutex::new(None);

/// Remember the latest decision (the ears may run on another thread than the
/// loop that logs the turn).
pub fn note_asr(d: AsrDecision) {
    if let Ok(mut g) = LAST.lock().or_else(crate::crash::unpoison) {
        *g = Some(d);
    }
}

/// Take the latest decision, once.
pub fn take_asr() -> Option<AsrDecision> {
    LAST.lock()
        .or_else(crate::crash::unpoison)
        .ok()
        .and_then(|mut g| g.take())
}

/// One finished turn.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TurnRecord {
    pub at: u64,
    pub asr: Option<AsrDecision>,
    pub listening_ms: Option<u32>,
    /// End of speech -> text ready.
    pub hearing_ms: Option<u32>,
    /// Model asked -> first sentence handed to the voice.
    pub first_words_ms: Option<u32>,
    /// End of speech -> first sound back.
    pub first_audio_ms: Option<u32>,
    pub speaking_ms: Option<u32>,
    pub playing_ms: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Ledger {
    pub turns: Vec<TurnRecord>,
}

impl Ledger {
    pub fn add(&mut self, t: TurnRecord) {
        self.turns.push(t);
        if self.turns.len() > KEEP {
            let cut = self.turns.len() - KEEP;
            self.turns.drain(..cut);
        }
    }
}

/// Record a turn into the store (best effort; timing must never fail a turn).
pub fn record(store: &crate::store::Store, t: TurnRecord) {
    let mut ledger: Ledger = store.load(RECORD);
    ledger.add(t);
    let _ = store.save(RECORD, &ledger);
}

fn median_max(v: &mut [u32]) -> Option<(u32, u32)> {
    v.sort_unstable();
    let worst = *v.last()?;
    Some((v[v.len() / 2], worst))
}

/// One readable line: median/worst per stage and the ASR engine mix.
pub fn summary(ledger: &Ledger) -> String {
    if ledger.turns.is_empty() {
        return "no voice turns recorded yet".into();
    }
    let part = |name: &str, f: &dyn Fn(&TurnRecord) -> Option<u32>| -> Option<String> {
        let mut v: Vec<u32> = ledger.turns.iter().filter_map(f).collect();
        median_max(&mut v).map(|(m, w)| format!("{name} {m}/{w}ms"))
    };
    let mut parts: Vec<String> = [
        part("hearing", &|t| t.hearing_ms),
        part("first words", &|t| t.first_words_ms),
        part("end-of-speech->first audio", &|t| t.first_audio_ms),
        part("synth", &|t| t.speaking_ms),
        part("playback", &|t| t.playing_ms),
    ]
    .into_iter()
    .flatten()
    .collect();
    let count = |e: &str| {
        ledger
            .turns
            .iter()
            .filter(|t| t.asr.as_ref().is_some_and(|a| a.engine == e))
            .count()
    };
    let fell_back = ledger
        .turns
        .iter()
        .filter(|t| {
            t.asr
                .as_ref()
                .is_some_and(|a| a.engine == "whisper" && a.why.contains("failed"))
        })
        .count();
    let dropped = ledger
        .turns
        .iter()
        .filter(|t| t.asr.as_ref().is_some_and(|a| a.dropped_hallucination))
        .count();
    parts.push(format!(
        "engines parakeet={} whisper={} (fell back after a failure {fell_back}x, hallucinations dropped {dropped}x)",
        count("parakeet"),
        count("whisper")
    ));
    format!(
        "last {} turns, median/worst: {}",
        ledger.turns.len(),
        parts.join("; ")
    )
}

/// Phrases whisper writes for a quiet or near-empty clip, beyond the
/// whole-transcript silence list. "Thank you" is a real thing to say, so these
/// are only treated as no speech when the clip itself was quiet or very short.
pub const QUIET_HALLUCINATIONS: &[&str] = &[
    "you",
    "thank you",
    "thanks",
    "thank you very much",
    "thanks for watching",
    "thank you for watching",
    "bye",
    "bye bye",
    "okay",
    "ok",
    "yeah",
    "so",
    "the end",
];

/// A clip counts as quiet below this RMS (about -38 dBFS) or shorter than this.
pub const QUIET_RMS: f32 = 0.0125;
pub const SHORT_CLIP_MS: u32 = 900;

/// RMS of 16-bit samples, 0.0..1.0.
pub fn rms(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples
        .iter()
        .map(|&s| {
            let x = s as f64 / 32768.0;
            x * x
        })
        .sum();
    (sum / samples.len() as f64).sqrt() as f32
}

/// Is `text` a known hallucination phrase heard on a clip too quiet or short
/// to hold it? Then it is treated as no speech.
pub fn is_quiet_hallucination(text: &str, rms: f32, audio_ms: u32) -> bool {
    let plain: String = text
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '\'' {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    QUIET_HALLUCINATIONS.contains(&plain.as_str()) && (rms < QUIET_RMS || audio_ms < SHORT_CLIP_MS)
}

/// How long the local recognizer may take before Atlas gives up on it and
/// falls back: a fixed start-up allowance plus a share of the clip length.
/// Never the old flat 30 s.
pub fn parakeet_timeout_ms(audio_ms: u32) -> u64 {
    (6_000 + audio_ms as u64 * 3 / 2).min(20_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(h: u32, fa: u32) -> TurnRecord {
        TurnRecord {
            hearing_ms: Some(h),
            first_audio_ms: Some(fa),
            ..Default::default()
        }
    }

    #[test]
    fn ledger_keeps_only_recent() {
        let mut l = Ledger::default();
        for i in 0..(KEEP as u32 + 10) {
            l.add(turn(i, i));
        }
        assert_eq!(l.turns.len(), KEEP);
        assert_eq!(l.turns[0].hearing_ms, Some(10));
    }

    #[test]
    fn summary_reports_median_worst_and_engines() {
        let mut l = Ledger::default();
        assert!(summary(&l).contains("no voice turns"));
        for (h, f) in [(300, 1500), (500, 2500), (4000, 9000)] {
            let mut t = turn(h, f);
            t.asr = Some(AsrDecision {
                engine: "parakeet".into(),
                ..Default::default()
            });
            l.add(t);
        }
        l.add(TurnRecord {
            asr: Some(AsrDecision {
                engine: "whisper".into(),
                why: "parakeet failed (x) -> whisper".into(),
                dropped_hallucination: true,
                ..Default::default()
            }),
            ..Default::default()
        });
        let s = summary(&l);
        assert!(s.contains("hearing 500/4000ms"), "{s}");
        assert!(s.contains("end-of-speech->first audio 2500/9000ms"), "{s}");
        assert!(s.contains("parakeet=3 whisper=1"), "{s}");
        assert!(s.contains("fell back after a failure 1x"), "{s}");
        assert!(s.contains("hallucinations dropped 1x"), "{s}");
    }

    #[test]
    fn decision_is_taken_once() {
        note_asr(AsrDecision {
            engine: "whisper".into(),
            why: "w".into(),
            ..Default::default()
        });
        assert_eq!(take_asr().map(|d| d.engine), Some("whisper".into()));
        assert!(take_asr().is_none());
    }

    #[test]
    fn quiet_clip_phrases_are_not_speech_but_loud_ones_are() {
        assert!(is_quiet_hallucination("Thank you.", 0.004, 3000));
        assert!(is_quiet_hallucination("you", 0.2, 400));
        assert!(!is_quiet_hallucination("Thank you", 0.1, 3000));
        assert!(!is_quiet_hallucination("what time is it", 0.001, 300));
    }

    #[test]
    fn rms_of_silence_and_tone() {
        assert_eq!(rms(&[0; 100]), 0.0);
        assert!(rms(&[16384; 100]) > 0.49);
    }

    #[test]
    fn timeout_scales_but_is_bounded() {
        assert_eq!(parakeet_timeout_ms(0), 6_000);
        assert_eq!(parakeet_timeout_ms(4_000), 12_000);
        assert_eq!(parakeet_timeout_ms(60_000), 20_000);
    }

    #[test]
    fn record_round_trips_through_store() {
        let dir = std::env::temp_dir().join(format!("atlas_voicelat_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let store = crate::store::Store::new(&dir);
        record(&store, turn(100, 200));
        record(&store, turn(300, 400));
        let l: Ledger = store.load(RECORD);
        assert_eq!(l.turns.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
