//! Interruptible speech.
//!
//! Atlas speaks in chunks and listens between them. The industry default is to
//! stop the moment *any* voice is detected, which gives a false-interrupt rate
//! you notice — someone coughs, a colleague talks, and the assistant cuts
//! itself off. Backchannel noises ("mm", "yeah") are the worst case: pure
//! energy detection reads them as an interruption when they mean "keep going".
//!
//! So this is deliberately conservative: **only an explicit stop or pause
//! interrupts.** Everything else is heard, kept, and handled after Atlas
//! finishes the sentence it is on.
//!
//! The bug this design has to avoid: recording as *said* something you never
//! heard. Whatever was cut off is tracked separately so "carry on" resumes
//! exactly there -- at the start of the sentence that was cut, which you
//! heard only part of -- and the transcript reflects what actually reached
//! you. The speaking itself is `speakthread`'s.

use crate::attention::{hear, Heard};

#[derive(Debug, Clone, PartialEq)]
pub struct Delivery {
    /// Chunks that actually reached you.
    pub spoken: Vec<String>,
    /// Chunks cut off. Never recorded as said.
    pub unspoken: Vec<String>,
    /// What you said to interrupt, if you did.
    pub interrupted_by: Option<String>,
}

impl Delivery {
    pub fn was_interrupted(&self) -> bool {
        self.interrupted_by.is_some()
    }
    /// What is left to say if you ask it to carry on.
    pub fn remaining_text(&self) -> String {
        self.unspoken.join(" ")
    }
}

/// Only an explicit stop or pause takes the floor.
///
/// Returning false for everything else is the whole point: a cough, a
/// colleague, or "mm-hmm" must not cut Atlas off mid-sentence.
pub fn is_interruption(said: &str) -> bool {
    // `Panic` included. "Stop everything" is the phrase `firstrun` teaches
    // for when Atlas has got something wrong, and the commonest moment to
    // need it is while Atlas is talking -- which is exactly when a phrase
    // that is not an interruption cannot get through. It was missing here,
    // so the emergency stop could not barge in on the thing you wanted
    // stopped.
    matches!(hear(said), Some(Heard::Pause) | Some(Heard::Cancel) | Some(Heard::Panic))
}

/// Break a reply into speakable chunks.
///
/// Sentence-sized, because that is the granularity at which stopping sounds
/// deliberate rather than glitchy. Very long sentences are split at commas so
/// a rambling one is still interruptible.
pub fn split(text: &str) -> Vec<String> {
    const LONG: usize = 160;
    let mut out = Vec::new();
    let mut current = String::new();

    for c in text.chars() {
        current.push(c);
        let boundary = matches!(c, '.' | '!' | '?');
        // A sentence's end, or a comma once a sentence has run long.
        if (boundary && current.trim().len() > 1) || (current.len() >= LONG && c == ',') {
            out.push(current.trim().to_string());
            current.clear();
        }
    }
    if !current.trim().is_empty() {
        out.push(current.trim().to_string());
    }
    if out.is_empty() && !text.trim().is_empty() {
        out.push(text.trim().to_string());
    }
    out
}

/// Balance natural phrases before one-ahead synthesis. A short opening must
/// not be followed by a much longer synthesis job while it plays.
pub fn playback_chunks(text: &str) -> Vec<String> {
    fn parts(text: &str, clauses: bool) -> Vec<String> {
        let mut result = Vec::new(); let mut start = 0;
        for (index, character) in text.char_indices() {
            let end = index + character.len_utf8();
            let next = text[end..].chars().next();
            if (matches!(character, '.' | '!' | '?') || clauses && matches!(character, ',' | ';' | ':'))
                && next.is_none_or(char::is_whitespace) && !text[start..end].trim().is_empty() {
                result.push(text[start..end].trim().to_string()); start = end;
            }
        }
        if !text[start..].trim().is_empty() { result.push(text[start..].trim().to_string()); }
        result
    }
    fn divide(phrase: &str, out: &mut Vec<String>) {
        if phrase.len() > 60 {
            let split = [" while ", " and ", " so ", " because "]
                .iter().flat_map(|word| phrase.match_indices(word).map(|(index, _)| index))
                .filter(|index| *index >= 24 && phrase.len() - *index >= 24)
                .min_by_key(|index| index.abs_diff(phrase.len() / 2));
            if let Some(index) = split { divide(phrase[..index].trim(), out); divide(phrase[index..].trim(), out); return; }
        }
        out.push(phrase.to_string());
    }
    let sentences = parts(text, false);
    let shortest = sentences.iter().map(String::len).min().unwrap_or(0);
    let longest = sentences.iter().map(String::len).max().unwrap_or(0);
    if longest <= 110 && (shortest == 0 || longest <= shortest.saturating_mul(2)) { return sentences; }
    let mut phrases = Vec::new();
    for phrase in parts(text, true) { divide(&phrase, &mut phrases); }
    // A streamed sentence has no prior audio to rebalance retrospectively.
    // Keep its natural clauses small instead of queueing one long synthesis.
    if sentences.len() <= 1 { return phrases; }
    let mut chunks = Vec::new(); let mut chunk = String::new();
    for phrase in phrases {
        if !chunk.is_empty() && chunk.len() + 1 + phrase.len() > 110 {
            chunks.push(std::mem::take(&mut chunk));
        }
        if !chunk.is_empty() { chunk.push(' '); }
        chunk.push_str(&phrase);
    }
    if !chunk.is_empty() { chunks.push(chunk); }
    chunks
}

#[cfg(test)]
mod playback_balance_tests {
    use super::*;
    #[test]
    fn uneven_native_reply_keeps_words_and_balances_natural_phrases() {
        let text = "Atlas speaks each reply a sentence at a time. The first one is made while you wait, and every one after it is made while the one before it is playing, so there is no gap between them. This is Kokoro, running on the processor alone.";
        let chunks = playback_chunks(text);
        assert_eq!(chunks.join(" "), text);
        assert!(chunks.iter().all(|chunk| chunk.len() <= 110), "{chunks:?}");
        assert!(chunks.windows(2).all(|pair| pair[1].len() * 4 <= pair[0].len() * 5), "the next synthesis must fit the preceding phrase at the measured 0.8 real-time ratio: {chunks:?}");
        assert_eq!(playback_chunks("First. Second. Third."), split("First. Second. Third."));
    }
    #[test]
    fn streamed_short_then_long_sentence_queues_only_natural_short_clauses() {
        let first = "Atlas speaks each reply a sentence at a time.";
        let next = "The first one is made while you wait, and every one after it is made while the one before it is playing, so there is no gap between them.";
        let opening = playback_chunks(first);
        let following = playback_chunks(next);
        assert_eq!(following.join(" "), next);
        assert!(following.len() > 1);
        assert!(following.iter().all(|chunk| chunk.len() <= opening[0].len()), "{following:?}");
        let technical = "Read https://example.com/item?v=3.14 and keep version 3.14.";
        assert_eq!(playback_chunks(technical), vec![technical]);
    }
}

// `deliver` -- speak a reply chunk by chunk, checking for an interruption
// between chunks -- was here until 28 Sep 2026. The reply now plays on its
// own thread and the same rule is applied while each chunk plays
// (`speakthread::Saying`, which returns this module's `Delivery`); keeping a
// second copy of the rule that nothing called would have been two rules to
// keep agreeing. Its tests drive `Saying` now (tests/speech_activity.rs).

/// Why a reply stopped when you took the turn -- the talk key pressed, or
/// your voice over it -- rather than saying "stop" or "pause". Not words
/// anyone says, so it is never mistaken for them.
pub const YOUR_TURN: &str = "(you took the turn)";

/// What Atlas says when it has been cut off. Short — you interrupted because
/// you wanted the floor, not another paragraph.
///
/// Nothing at all when you simply took the turn (29 Sep 2026). Pressing the
/// talk key over a reply, or talking over it, is you starting to speak: the
/// reply stops and Atlas listens. It used to be recorded as "hold on" --
/// which reads as a pause -- so Eric heard "Paused." after nearly every reply
/// on his laptop, spoken over what he was starting to say. "Paused." is for
/// an explicit "pause" (or "hold on", "wait" said as such); what wasn't said
/// is parked for "carry on" either way.
pub fn acknowledge(d: &Delivery) -> String {
    match &d.interrupted_by {
        Some(said) if said == YOUR_TURN => String::new(),
        Some(said) if hear(said) == Some(Heard::Cancel) => "Stopped.".into(),
        Some(said) if matches!(hear(said), Some(Heard::Pause)) => "Paused.".into(),
        // "Stop everything" and anything else said as an interruption.
        Some(_) => "Stopped.".into(),
        None => String::new(),
    }
}
