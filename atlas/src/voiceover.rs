//! Reading your script over your footage.
//!
//! The recording is the easy half — piper does that, locally and for nothing.
//! The half that makes it sound deliberate rather than pasted on is the
//! timing: where the lines land, how long the gaps are, and what happens to
//! the music underneath while you're talking.
//!
//! A voiceover that ignores the picture reads as a voiceover. One that lands
//! on the cuts reads as the video.

use serde::{Deserialize, Serialize};

/// A line, and where it goes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub text: String,
    /// When it starts, in seconds.
    pub at: f32,
    /// How long it takes once spoken.
    pub lasts: f32,
}

impl Line {
    pub fn ends(&self) -> f32 {
        self.at + self.lasts
    }
}

/// Roughly how long a line takes to say.
///
/// Speech runs about 2.6 words a second at a natural pace for this sort of
/// thing — faster than conversation, slower than an advert.
pub fn how_long(text: &str, pace: f32) -> f32 {
    let words = text.split_whitespace().count() as f32;
    let base = words / (2.6 * pace);
    // Punctuation costs time. Ignoring it is why generated timing always runs
    // short and then overlaps the next line.
    let pauses = text.matches(['.', '!', '?']).count() as f32 * 0.35
        + text.matches(',').count() as f32 * 0.15;
    base + pauses
}

/// A moment in the footage worth landing on.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Beat {
    pub at: f32,
    /// A cut, a reveal, something appearing.
    pub strong: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct VoiceoverConfig {
    pub enabled: bool,
    /// Speaking pace. 1.0 is normal.
    pub pace: f32,
    /// Gap between lines.
    pub gap_secs: f32,
    /// Nudge a line onto a nearby cut if it's within this.
    pub snap_within_secs: f32,
    /// How far the music drops while you talk, in dB.
    pub duck_db: f32,
    /// How long the music takes to come back.
    pub duck_release_secs: f32,
    /// Leave this much at the end rather than finishing on the last frame.
    pub tail_secs: f32,
}

impl Default for VoiceoverConfig {
    fn default() -> Self {
        VoiceoverConfig {
            enabled: false,
            pace: 1.0,
            gap_secs: 0.45,
            snap_within_secs: 0.5,
            // 12dB is where the music is still present but never competing.
            duck_db: 12.0,
            duck_release_secs: 0.4,
            tail_secs: 0.6,
        }
    }
}

/// Lay the script over the footage.
///
/// Lines are placed in order, then nudged onto nearby cuts — a line that
/// starts half a second after a cut sounds late in a way people notice
/// without knowing why.
pub fn lay_out(script: &[String], beats: &[Beat], total_secs: f32, cfg: &VoiceoverConfig) -> Vec<Line> {
    let mut out: Vec<Line> = Vec::new();
    let mut cursor = 0.0f32;

    for text in script {
        let lasts = how_long(text, cfg.pace);
        let mut at = cursor;

        // Snap to a cut if there's one close by.
        if let Some(beat) = beats
            .iter()
            .filter(|b| (b.at - at).abs() <= cfg.snap_within_secs)
            .min_by(|a, b| {
                (a.at - at).abs().partial_cmp(&(b.at - at).abs()).unwrap_or(std::cmp::Ordering::Equal)
            })
        {
            at = beat.at;
        }

        out.push(Line { text: text.clone(), at, lasts });
        cursor = at + lasts + cfg.gap_secs;
    }

    let _ = total_secs;
    out
}

/// Does the voiceover fit?
#[derive(Debug, Clone, PartialEq)]
pub enum Fit {
    Fits { spare_secs: f32 },
    /// Longer than the footage.
    TooLong { over_secs: f32, cut_words: usize },
    /// Much shorter — the picture will sit there in silence.
    TooShort { silence_secs: f32 },
}

pub fn fits(lines: &[Line], total_secs: f32, cfg: &VoiceoverConfig) -> Fit {
    let ends = lines.last().map(|l| l.ends()).unwrap_or(0.0) + cfg.tail_secs;
    let spare = total_secs - ends;

    if spare < 0.0 {
        // Roughly how much to lose, in words rather than seconds — you cut
        // words, not seconds.
        let words = (-spare * 2.6 * cfg.pace).ceil() as usize;
        Fit::TooLong { over_secs: -spare, cut_words: words }
    } else if spare > total_secs * 0.25 && spare > 3.0 {
        Fit::TooShort { silence_secs: spare }
    } else {
        Fit::Fits { spare_secs: spare }
    }
}

/// The music level under the voice, moment by moment.
///
/// Ducking by hand is the tedious part and the part that most obviously
/// separates a finished piece from a rough one.
pub fn music_ducking(lines: &[Line], cfg: &VoiceoverConfig) -> Vec<(f32, f32, f32)> {
    lines
        .iter()
        .map(|l| {
            // Start the duck slightly early so the music is already down when
            // the voice arrives, rather than dipping underneath it.
            (
                (l.at - 0.15).max(0.0),
                l.ends() + cfg.duck_release_secs,
                -cfg.duck_db,
            )
        })
        .collect()
}

/// The ffmpeg filter for ducking, which is one expression rather than a
/// timeline.
pub fn duck_filter(cfg: &VoiceoverConfig) -> String {
    format!(
        "[music][voice]sidechaincompress=threshold=0.05:ratio={:.0}:attack=20:release={:.0}[ducked]",
        cfg.duck_db / 3.0,
        cfg.duck_release_secs * 1000.0
    )
}

/// What Atlas says about a laid-out voiceover.
pub fn spoken(lines: &[Line], fit: &Fit, snapped: usize) -> String {
    let mut s = format!("{} lines.", lines.len());
    match fit {
        Fit::Fits { spare_secs } => {
            s.push_str(&format!(" Fits with {spare_secs:.1}s to spare."));
        }
        Fit::TooLong { over_secs, cut_words } => {
            s.push_str(&format!(
                " It's {over_secs:.1}s long for the footage — about {cut_words} words to lose."
            ));
        }
        Fit::TooShort { silence_secs } => {
            s.push_str(&format!(
                " {silence_secs:.0}s of picture with nothing over it. Either say more or cut the footage."
            ));
        }
    }
    if snapped > 0 {
        s.push_str(&format!(" {snapped} lines moved onto cuts."));
    }
    s
}

/// How many lines ended up on a beat.
pub fn snapped_count(lines: &[Line], beats: &[Beat]) -> usize {
    lines
        .iter()
        .filter(|l| beats.iter().any(|b| (b.at - l.at).abs() < 0.01))
        .count()
}

/// Reading a script well is mostly about where it breathes.
///
/// A wall of text read straight through is what makes a voiceover sound
/// generated, whatever voice does it.
pub fn break_into_lines(script: &str) -> Vec<String> {
    script
        .split_inclusive(['.', '!', '?'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .flat_map(|sentence| {
            // A sentence over about 18 words wants a breath in it, and a comma
            // is where the writer already put one.
            if sentence.split_whitespace().count() > 18 {
                sentence
                    .split_inclusive(',')
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
                    .collect::<Vec<_>>()
            } else {
                vec![sentence.to_string()]
            }
        })
        .collect()
}
