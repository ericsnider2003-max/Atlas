//! Sound & voice: when Atlas speaks, how loud, and when it may pop up.
//!
//! The design's Sound & voice page (`design/hub/locked-2026-09-21/Sound.dc.html`)
//! and the interrupt rule locked with Eric on 20–21 Sep ("pop-ups appear only
//! when Eric asks, or when it's urgent … tunable: Only when I ask / When it's
//! urgent (default) / Anything ready"). Each setting here is read where it
//! acts: `Daemon::say` asks `may_speak_now`, `Daemon::reach_you` asks
//! `may_pop_up`, and `Voice::speak` scales the synthesised audio by `volume`
//! before it plays — in-house, on the WAV itself, so no player needs a volume
//! flag.

use serde::{Deserialize, Serialize};

/// When a reply is read out loud.
pub const SPEAK_REPLIES: [&str; 3] = ["always", "hands_free", "never"];
/// When Atlas may put something in front of you unasked.
pub const POPUPS: [&str; 3] = ["ask", "urgent", "anything"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundConfig {
    /// "always" (typed turns too), "hands_free" (only when you spoke), or
    /// "never" (shown, not said).
    pub speak_replies: String,
    /// 0–100. Applied to the synthesised audio before it plays.
    pub volume: u8,
    /// Nothing is said out loud; everything still works and is shown.
    pub muted: bool,
    /// Quiet hours on or off, and the window, on your clock ("22:00").
    pub quiet_hours: bool,
    pub quiet_from: String,
    pub quiet_to: String,
    /// "ask", "urgent" (the default, the locked rule) or "anything".
    pub popups: String,
}

impl Default for SoundConfig {
    fn default() -> Self {
        SoundConfig {
            speak_replies: "hands_free".into(),
            volume: 100,
            muted: false,
            quiet_hours: false,
            quiet_from: "22:00".into(),
            quiet_to: "07:00".into(),
            popups: "urgent".into(),
        }
    }
}

/// Minutes past midnight of "HH:MM". `None` for anything else.
pub fn minutes(hhmm: &str) -> Option<u32> {
    let (h, m) = hhmm.trim().split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

impl SoundConfig {
    /// Inside quiet hours at this minute of your day? A window that crosses
    /// midnight (22:00–07:00) wraps; an unreadable time turns it off rather
    /// than silencing Atlas for a day nobody chose.
    pub fn in_quiet_hours(&self, minute_of_day: u32) -> bool {
        if !self.quiet_hours {
            return false;
        }
        let (Some(from), Some(to)) = (minutes(&self.quiet_from), minutes(&self.quiet_to)) else {
            return false;
        };
        if from == to {
            return false;
        }
        if from < to {
            (from..to).contains(&minute_of_day)
        } else {
            minute_of_day >= from || minute_of_day < to
        }
    }

    /// May a line be spoken now? `spoken_turn`: the thing being answered was
    /// said out loud (so "hands-free only" allows it).
    pub fn may_speak_now(&self, minute_of_day: u32, spoken_turn: bool) -> bool {
        if self.muted || self.in_quiet_hours(minute_of_day) {
            return false;
        }
        match self.speak_replies.as_str() {
            "never" => false,
            "always" => true,
            _ => spoken_turn,
        }
    }

    /// May something unasked be put in front of you? Otherwise it waits in the
    /// hub and your brief.
    pub fn may_pop_up(&self, urgent: bool) -> bool {
        match self.popups.as_str() {
            "ask" => false,
            "anything" => true,
            _ => urgent,
        }
    }
}

/// Scale 16-bit PCM WAV audio to `volume` percent, in place. Anything that
/// isn't a 16-bit PCM WAV is left exactly as it was: better at full volume
/// than broken.
pub fn scale_wav(bytes: &mut [u8], volume: u8) -> bool {
    if volume >= 100 || bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return false;
    }
    let mut at = 12;
    let mut bits = 0u16;
    let mut pcm = false;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let len = u32::from_le_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]]) as usize;
        let start = at + 8;
        let end = (start + len).min(bytes.len());
        if id == b"fmt " && len >= 16 && end >= start + 16 {
            pcm = u16::from_le_bytes([bytes[start], bytes[start + 1]]) == 1;
            bits = u16::from_le_bytes([bytes[start + 14], bytes[start + 15]]);
        } else if id == b"data" {
            if !pcm || bits != 16 {
                return false;
            }
            let k = volume as i32;
            for s in bytes[start..end].as_chunks_mut::<2>().0 {
                let v = i16::from_le_bytes([s[0], s[1]]) as i32 * k / 100;
                s.copy_from_slice(&(v as i16).to_le_bytes());
            }
            return true;
        }
        at = start + len + (len & 1);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_hours_wrap_midnight_and_an_unreadable_time_silences_nothing() {
        let mut s = SoundConfig { quiet_hours: true, ..Default::default() };
        assert!(s.in_quiet_hours(23 * 60), "11pm");
        assert!(s.in_quiet_hours(3 * 60), "3am");
        assert!(!s.in_quiet_hours(12 * 60), "noon");
        assert!(!s.in_quiet_hours(7 * 60), "7:00 is the end, not inside");
        s.quiet_from = "13:00".into();
        s.quiet_to = "14:00".into();
        assert!(s.in_quiet_hours(13 * 60 + 30) && !s.in_quiet_hours(14 * 60));
        s.quiet_from = "later".into();
        assert!(!s.in_quiet_hours(13 * 60 + 30));
        s.quiet_hours = false;
        s.quiet_from = "00:00".into();
        s.quiet_to = "23:59".into();
        assert!(!s.in_quiet_hours(600), "off is off");
    }

    #[test]
    fn speaking_follows_mute_quiet_and_the_reply_rule() {
        let s = SoundConfig::default();
        assert!(s.may_speak_now(600, true), "hands-free: a spoken turn is answered aloud");
        assert!(!s.may_speak_now(600, false), "a typed one isn't");
        let always = SoundConfig { speak_replies: "always".into(), ..Default::default() };
        assert!(always.may_speak_now(600, false));
        let never = SoundConfig { speak_replies: "never".into(), ..Default::default() };
        assert!(!never.may_speak_now(600, true));
        let muted = SoundConfig { muted: true, speak_replies: "always".into(), ..Default::default() };
        assert!(!muted.may_speak_now(600, true));
        let quiet = SoundConfig { quiet_hours: true, speak_replies: "always".into(), ..Default::default() };
        assert!(!quiet.may_speak_now(23 * 60, true) && quiet.may_speak_now(12 * 60, true));
    }

    #[test]
    fn pop_ups_follow_the_locked_rule() {
        let s = SoundConfig::default();
        assert!(s.may_pop_up(true) && !s.may_pop_up(false), "urgent only, by default");
        let ask = SoundConfig { popups: "ask".into(), ..Default::default() };
        assert!(!ask.may_pop_up(true));
        let any = SoundConfig { popups: "anything".into(), ..Default::default() };
        assert!(any.may_pop_up(false));
    }

    fn wav(samples: &[i16]) -> Vec<u8> {
        let mut v = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes()); // PCM
        v.extend_from_slice(&1u16.to_le_bytes()); // mono
        v.extend_from_slice(&22050u32.to_le_bytes());
        v.extend_from_slice(&44100u32.to_le_bytes());
        v.extend_from_slice(&2u16.to_le_bytes());
        v.extend_from_slice(&16u16.to_le_bytes());
        v.extend_from_slice(b"data");
        v.extend_from_slice(&((samples.len() * 2) as u32).to_le_bytes());
        for s in samples {
            v.extend_from_slice(&s.to_le_bytes());
        }
        v
    }

    #[test]
    fn volume_scales_the_samples_and_leaves_anything_else_alone() {
        let mut w = wav(&[1000, -2000, 32767]);
        assert!(scale_wav(&mut w, 50));
        let data = &w[w.len() - 6..];
        assert_eq!(i16::from_le_bytes([data[0], data[1]]), 500);
        assert_eq!(i16::from_le_bytes([data[2], data[3]]), -1000);
        let mut full = wav(&[1000]);
        assert!(!scale_wav(&mut full, 100), "full volume touches nothing");
        let mut junk = b"not a wav at all".to_vec();
        assert!(!scale_wav(&mut junk, 10));
        assert_eq!(junk, b"not a wav at all");
    }

    #[test]
    fn times_are_read_strictly() {
        assert_eq!(minutes("07:30"), Some(450));
        assert_eq!(minutes("24:00"), None);
        assert_eq!(minutes("7"), None);
    }
}
