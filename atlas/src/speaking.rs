//! What Atlas is saying right now, and how loud each moment of it is.
//!
//! The mark was drawn to move with the voice, but nothing produced a level,
//! so it never moved when Atlas spoke (doc 19, still open in doc 21). Eric,
//! 24 Sep 2026, item 4: the mark moves when Atlas speaks. Since 27 Sep the
//! mark is the Folded A, and the level drives its dot (`mark::pose`).
//!
//! The level comes from the speech itself, not a microphone and not a guess.
//! Piper writes the whole reply to a WAV file before it's played, so the
//! loudness of every 30 ms of it is known before the first sound. That
//! envelope is written beside Atlas's data together with the moment playback
//! starts, and anything drawing the mark — the Atlas window, the desktop
//! overlay — reads it and looks up "how loud is the voice now". Nothing is
//! streamed and no audio device is opened twice; the drawing side only reads
//! a small file.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Each level covers this much of the speech.
pub const FRAME_MS: u32 = 30;

/// A reply being spoken: its words, when playback started, and the loudness
/// of each frame of it (0–255).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Speaking {
    pub text: String,
    pub started_ms: u64,
    pub frame_ms: u32,
    pub levels: Vec<u8>,
}

impl Speaking {
    /// How loud the voice is at `now_ms`, 0..1. `None` once it has finished,
    /// so a file left behind by a crash can never keep the line moving.
    pub fn level_at(&self, now_ms: u64) -> Option<f32> {
        let since = now_ms.checked_sub(self.started_ms)?;
        let i = (since / self.frame_ms.max(1) as u64) as usize;
        self.levels.get(i).map(|l| *l as f32 / 255.0)
    }

    /// Is this still being said at `now_ms`? From the levels when there are
    /// some; when the speech's format gave none, from how long the words
    /// take to say (about 70 ms a character, capped at two minutes), so the
    /// captions still show and a stale file still expires.
    pub fn still_going(&self, now_ms: u64) -> bool {
        if !self.levels.is_empty() {
            return self.level_at(now_ms).is_some();
        }
        let Some(since) = now_ms.checked_sub(self.started_ms) else { return false };
        let guess = (self.text.chars().count() as u64 * 70 + 1_000).min(120_000);
        since < guess
    }
}

/// The longest caption shown on the desktop. A long reply is still spoken
/// whole; the screen shows its start, not a wall of text over your work.
pub const CAPTION_CHARS: usize = 280;

/// `text` cut to `CAPTION_CHARS` at a word, with an ellipsis if it was cut.
pub fn caption(text: &str) -> String {
    let t = text.trim();
    if t.chars().count() <= CAPTION_CHARS {
        return t.to_string();
    }
    let cut: String = t.chars().take(CAPTION_CHARS).collect();
    let at = cut.rfind(char::is_whitespace).filter(|i| *i > CAPTION_CHARS / 2).unwrap_or(cut.len());
    format!("{}…", cut[..at].trim_end())
}

/// Sound takes a moment to come out of the speakers once the player starts;
/// the line waits that long, so it moves with the voice rather than ahead
/// of it.
pub const PLAYBACK_LAG_MS: u64 = 250;

/// Where the file lives: beside Atlas's other data, where the window and the
/// overlay (separate processes) can both find it.
fn path(data_dir: &Path) -> PathBuf {
    data_dir.join("speaking.json")
}

/// Playback is starting: write what is being said and how loud it is.
pub fn begin(data_dir: &Path, text: &str, wav: &[u8], now_ms: u64) -> std::io::Result<()> {
    heard_now();
    let s = Speaking {
        text: text.to_string(),
        started_ms: now_ms,
        frame_ms: FRAME_MS,
        levels: levels_of_wav(wav, FRAME_MS).unwrap_or_default(),
    };
    std::fs::create_dir_all(data_dir)?;
    // Written whole and renamed into place, so a reader never sees half.
    let tmp = data_dir.join("speaking.json.part");
    std::fs::write(&tmp, serde_json::to_vec(&s).map_err(std::io::Error::other)?)?;
    crate::store::rename_patiently(&tmp, &path(data_dir))
}

/// The first sound of the turn being timed (Phase 0.2): set by the first
/// sentence whose playback begins after `listen_for_first_sound`, taken by
/// the turn's timing line. What Eric feels is the silence from the end of
/// his speech to this moment, not how long the model or the voice took.
static FIRST_SOUND: std::sync::Mutex<(bool, Option<std::time::Instant>)> = std::sync::Mutex::new((false, None));

/// Start watching for the turn's first sound.
pub fn listen_for_first_sound() {
    if let Ok(mut g) = FIRST_SOUND.lock().or_else(crate::crash::unpoison) {
        *g = (true, None);
    }
}

/// Playback is starting: the first time since `listen_for_first_sound`,
/// that moment is kept. Both voices call this through `begin`; a test or a
/// voice with nothing to draw can call it directly.
pub fn heard_now() {
    if let Ok(mut g) = FIRST_SOUND.lock().or_else(crate::crash::unpoison) {
        if g.0 && g.1.is_none() {
            g.1 = Some(std::time::Instant::now());
        }
    }
}

/// When the turn's first sound began, once; watching stops.
pub fn take_first_sound() -> Option<std::time::Instant> {
    FIRST_SOUND.lock().or_else(crate::crash::unpoison).ok().and_then(|mut g| {
        let at = g.1.take();
        g.0 = false;
        at
    })
}

/// Playback has finished (or failed): the line goes back to rest.
pub fn end(data_dir: &Path) {
    crate::heard!(std::fs::remove_file(path(data_dir)));
}

/// What's being said, if anything.
pub fn now_saying(data_dir: &Path) -> Option<Speaking> {
    serde_json::from_slice(&std::fs::read(path(data_dir)).ok()?).ok()
}

/// The loudness of each `frame_ms` of a 16-bit PCM WAV (what piper writes),
/// scaled so the loudest frame is 255.
///
/// Scaled to the reply's own peak rather than an absolute level: a quiet
/// voice and a loud one should both use the line's full height. The square
/// root keeps quieter syllables visible instead of letting the loudest vowel
/// flatten everything else. `None` for anything that isn't 16-bit PCM.
pub fn levels_of_wav(wav: &[u8], frame_ms: u32) -> Option<Vec<u8>> {
    if wav.len() < 12 || &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return None;
    }
    let mut at = 12;
    let (mut channels, mut rate, mut bits, mut format) = (0u16, 0u32, 0u16, 0u16);
    let mut data: Option<&[u8]> = None;
    while at + 8 <= wav.len() {
        let id = &wav[at..at + 4];
        let len = u32::from_le_bytes(wav[at + 4..at + 8].try_into().ok()?) as usize;
        let body_start = at + 8;
        let body_end = body_start.saturating_add(len).min(wav.len());
        let body = &wav[body_start..body_end];
        match id {
            b"fmt " if body.len() >= 16 => {
                format = u16::from_le_bytes([body[0], body[1]]);
                channels = u16::from_le_bytes([body[2], body[3]]);
                rate = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
                bits = u16::from_le_bytes([body[14], body[15]]);
                // WAVE_FORMAT_EXTENSIBLE: the real format is the first two
                // bytes of the sub-format id, at offset 24.
                if format == 0xFFFE && body.len() >= 26 {
                    format = u16::from_le_bytes([body[24], body[25]]);
                }
            }
            b"data" => data = Some(body),
            _ => {}
        }
        // Chunks are padded to an even length.
        at = body_start.saturating_add(len + (len & 1));
    }
    let data = data?;
    if channels == 0 || rate == 0 {
        return None;
    }
    let per_frame = (rate as usize * frame_ms as usize / 1000).max(1) * channels as usize;
    // 16-bit whole numbers (piper), or 32-bit floats (some other engines);
    // `format` 0xFFFE says which in its sub-format, kept in `sub`.
    let samples: Vec<f32> = match (format, bits) {
        (1, 16) => data.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32).collect(),
        (3, 32) => data.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]) * 32767.0).collect(),
        _ => return None,
    };
    let rms: Vec<f32> = samples
        .chunks(per_frame)
        .map(|c| (c.iter().map(|s| s.powi(2)).sum::<f32>() / c.len() as f32).sqrt())
        .collect();
    let peak = rms.iter().cloned().fold(0.0_f32, f32::max);
    if peak <= 0.0 {
        return Some(vec![0; rms.len()]);
    }
    Some(rms.iter().map(|r| ((r / peak).sqrt() * 255.0).round() as u8).collect())
}

/// Milliseconds since 1970, the clock both sides of the file read.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The drawing side: keeps an eye on the file without reading it every frame.
#[derive(Debug)]
pub struct Watch {
    data_dir: PathBuf,
    last_read: Option<std::time::Instant>,
    current: Option<Speaking>,
    /// When the file last changed, so an unchanged one isn't re-read and
    /// re-parsed twenty times a second while nothing is being said.
    changed: Option<std::time::SystemTime>,
}

impl Watch {
    pub fn new(data_dir: PathBuf) -> Watch {
        Watch { data_dir, last_read: None, current: None, changed: None }
    }

    /// What's being said now, re-read at most every 50 ms — often enough
    /// that the line starts with the first syllable, rarely enough to cost
    /// nothing.
    pub fn now(&mut self) -> Option<&Speaking> {
        if self.last_read.map(|t| t.elapsed().as_millis() >= 50).unwrap_or(true) {
            self.last_read = Some(std::time::Instant::now());
            let changed = std::fs::metadata(path(&self.data_dir)).and_then(|m| m.modified()).ok();
            if changed.is_none() {
                self.current = None;
            } else if changed != self.changed || self.current.is_none() {
                self.current = now_saying(&self.data_dir);
            }
            self.changed = changed;
        }
        self.current.as_ref()
    }

    /// How loud the voice is right now, 0..1, while Atlas is speaking.
    pub fn level(&mut self) -> Option<f32> {
        let now = now_ms();
        self.now().and_then(|s| s.level_at(now))
    }
}
