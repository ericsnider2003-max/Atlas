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
        if boundary && current.trim().len() > 1 {
            out.push(current.trim().to_string());
            current.clear();
        } else if current.len() >= LONG && c == ',' {
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

// `deliver` -- speak a reply chunk by chunk, checking for an interruption
// between chunks -- was here until 28 Sep 2026. The reply now plays on its
// own thread and the same rule is applied while each chunk plays
// (`speakthread::Saying`, which returns this module's `Delivery`); keeping a
// second copy of the rule that nothing called would have been two rules to
// keep agreeing. Its tests drive `Saying` now (tests/speech_activity.rs).

/// What Atlas says when it has been cut off. Short — you interrupted because
/// you wanted the floor, not another paragraph.
pub fn acknowledge(d: &Delivery) -> String {
    match &d.interrupted_by {
        Some(said) if hear(said) == Some(Heard::Cancel) => "Stopped.".into(),
        Some(_) => "Paused.".into(),
        None => String::new(),
    }
}
