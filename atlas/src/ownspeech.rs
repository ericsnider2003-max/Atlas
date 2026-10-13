//! Atlas hearing itself.
//!
//! On laptop speakers the microphone picks up Atlas's own voice, and the
//! speech-to-text engine turns it into a transcript that looks exactly like
//! the user talking. The level guard in `micthread` only decides whether
//! something is loud enough to be a barge-in; it cannot tell *whose* words
//! they are. This compares text with text: a short ring remembers what Atlas
//! recently said (the exact words handed to the voice, read-aloud mail and
//! notifications included), and an open-mic transcript that is mostly
//! contained in it, heard soon after, is Atlas's own echo and is dropped.
//!
//! Only open-mic arrivals are ever judged here (`daemon::turn_from`); typed,
//! hub-typed and wake-word turns never are.

use std::collections::VecDeque;

/// Utterances remembered.
pub const KEEP: usize = 6;
/// An utterance can still be echoing this long after it was queued to speak.
pub const WINDOW_SECS: u64 = 60;
/// ...and this long after Atlas finished speaking (room tail, slow ASR).
pub const TAIL_SECS: u64 = 15;
/// Share of the heard words that must be found, in order, in what was said.
pub const OVERLAP: f32 = 0.7;
/// Fewer heard words than this must equal a whole recent utterance.
pub const MIN_WORDS: usize = 3;
/// A short answer to a question Atlas just asked is the user, not the echo.
const SHORT_ANSWER_WORDS: usize = 5;
/// Bounds on the work done per transcript.
const MAX_HEARD: usize = 200;
const MAX_SPOKEN: usize = 6_000;

struct Item {
    at: u64,
    words: Vec<String>,
    question: bool,
}

/// What Atlas recently said aloud.
#[derive(Default)]
pub struct Ring {
    items: VecDeque<Item>,
    ended_at: u64,
}

/// Lowercase words with punctuation taken out. Apostrophes join ("don't").
fn words(text: &str) -> Vec<String> {
    let plain: String = text
        .chars()
        .filter(|c| *c != '\'' && *c != '\u{2019}')
        .flat_map(|c| c.to_lowercase())
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    plain.split_whitespace().map(str::to_owned).collect()
}

fn is_content(w: &str) -> bool {
    w.chars().count() >= 4
}

/// Same word, or one slip of the recogniser in a word long enough to bear it.
fn same(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.len() < 5 || b.len() < 5 || a.len().abs_diff(b.len()) > 1 || a[0] != b[0] {
        return false;
    }
    // At most one substitution, insertion or deletion.
    let (mut i, mut j, mut edits) = (0, 0, 0);
    while i < a.len() && j < b.len() {
        if a[i] == b[j] {
            i += 1;
            j += 1;
            continue;
        }
        edits += 1;
        if edits > 1 {
            return false;
        }
        match a.len().cmp(&b.len()) {
            std::cmp::Ordering::Equal => {
                i += 1;
                j += 1;
            }
            std::cmp::Ordering::Greater => i += 1,
            std::cmp::Ordering::Less => j += 1,
        }
    }
    edits + (a.len() - i) + (b.len() - j) <= 1
}

/// Length of the longest common subsequence, words compared with `same`.
fn lcs(heard: &[String], said: &[String]) -> usize {
    let mut row = vec![0usize; said.len() + 1];
    for h in heard {
        let mut diag = 0;
        for (j, s) in said.iter().enumerate() {
            let up = row[j + 1];
            row[j + 1] = if same(h, s) {
                diag + 1
            } else {
                row[j + 1].max(row[j])
            };
            diag = up;
        }
    }
    row[said.len()]
}

/// Is `heard` (already split into words) mostly inside `said` (what Atlas
/// said, in order)?
fn contained(heard: &[String], said: &[String]) -> bool {
    let n = heard.len();
    if n < MIN_WORDS || said.is_empty() {
        return false;
    }
    // Two or more real words, so "is it the" cannot match any long reply.
    let content: Vec<&String> = heard.iter().filter(|w| is_content(w)).collect();
    if content.len() < 2 {
        return false;
    }
    let found = content
        .iter()
        .filter(|w| said.iter().any(|s| same(w, s)))
        .count();
    if (found as f32) < OVERLAP * content.len() as f32 {
        return false;
    }
    lcs(heard, said) as f32 >= OVERLAP * n as f32
}

impl Ring {
    /// Atlas is about to say `text` (the words as sent to the voice).
    pub fn note(&mut self, text: &str, t: u64) {
        let mut w = words(text);
        if w.is_empty() {
            return;
        }
        w.truncate(MAX_SPOKEN);
        self.items.push_back(Item {
            at: t,
            words: w,
            question: text.trim_end().ends_with('?'),
        });
        while self.items.len() > KEEP {
            self.items.pop_front();
        }
    }

    /// Atlas finished speaking at `t`: the tail starts now.
    pub fn finished(&mut self, t: u64) {
        self.ended_at = t;
    }

    fn live(&self, now: u64) -> Vec<&Item> {
        let tail = self.ended_at != 0 && now.saturating_sub(self.ended_at) <= TAIL_SECS;
        self.items
            .iter()
            .filter(|i| tail || now.saturating_sub(i.at) <= WINDOW_SECS)
            .collect()
    }

    /// Is this open-mic transcript Atlas's own voice coming back?
    pub fn is_echo(&self, heard: &str, now: u64) -> bool {
        let mut h = words(heard);
        if h.is_empty() {
            return false;
        }
        let live = self.live(now);
        let Some(newest) = live.last() else {
            return false;
        };
        h.truncate(MAX_HEARD);
        if h.len() < MIN_WORDS {
            // "Done." heard right after Atlas said exactly "Done."
            return live.iter().any(|i| {
                i.words.len() == h.len() && h.iter().zip(&i.words).all(|(a, b)| same(a, b))
            });
        }
        if newest.question && h.len() < SHORT_ANSWER_WORDS {
            return false;
        }
        let said: Vec<String> = live.iter().flat_map(|i| i.words.iter().cloned()).collect();
        contained(&h, &said)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPLY: &str = "Your next meeting is the budget review at three o'clock in the \
        main conference room, and Priya is bringing the quarterly numbers.";

    fn ring(text: &str) -> Ring {
        let mut r = Ring::default();
        r.note(text, 1_000);
        r
    }

    #[test]
    fn an_echo_of_a_long_reply_is_dropped() {
        let r = ring(REPLY);
        assert!(r.is_echo(REPLY, 1_004));
        // Recogniser slips: a word wrong, one missing.
        assert!(r.is_echo(
            "your next meeting is the budget review at three oclock in the main conference \
             room and Priya is bringing the quarterly number",
            1_006
        ));
    }

    #[test]
    fn what_the_user_actually_says_is_kept() {
        let r = ring(REPLY);
        assert!(!r.is_echo("what's the weather like tomorrow in Lisbon", 1_005));
        assert!(!r.is_echo("move the budget review to Thursday please", 1_005));
        assert!(!r.is_echo("", 1_005));
    }

    #[test]
    fn punctuation_and_case_do_not_matter() {
        let r = ring(REPLY);
        assert!(r.is_echo(
            "YOUR NEXT MEETING -- is the budget review; at three o'clock!",
            1_003
        ));
    }

    #[test]
    fn a_whisper_style_partial_of_a_reply_is_dropped() {
        let r = ring(REPLY);
        assert!(r.is_echo("the budget review at three oclock", 1_005));
        assert!(r.is_echo("bringing the quarterly numbers", 1_005));
    }

    #[test]
    fn short_words_are_handled_sensibly() {
        // The user's "yes" is not an echo of a long reply...
        let r = ring(REPLY);
        assert!(!r.is_echo("yes", 1_003));
        assert!(!r.is_echo("yes please do", 1_003));
        // ...but is when Atlas itself had just said exactly that.
        let r = ring("Done.");
        assert!(r.is_echo("done", 1_002));
        assert!(!r.is_echo("yes", 1_002));
        // A short answer repeating words from a question just asked is the user.
        let r = ring("Do you want me to send it now?");
        assert!(!r.is_echo("send it now", 1_003));
        assert!(!r.is_echo("yes", 1_003));
    }

    #[test]
    fn only_a_recent_reply_can_be_echoed() {
        let mut r = ring(REPLY);
        assert!(!r.is_echo(REPLY, 1_000 + WINDOW_SECS + 1));
        // The tail after Atlas stops speaking keeps a long reply in play.
        r.finished(1_100);
        assert!(r.is_echo(REPLY, 1_100 + TAIL_SECS));
        assert!(!r.is_echo(REPLY, 1_100 + TAIL_SECS + 1));
    }

    #[test]
    fn an_echo_spanning_two_utterances_and_the_ring_bound() {
        let mut r = Ring::default();
        r.note("The invoice from Northwind is due on Friday.", 500);
        r.note("It totals four hundred and twelve dollars.", 502);
        assert!(r.is_echo(
            "invoice from northwind is due on friday it totals four hundred and twelve dollars",
            505
        ));
        for i in 0..KEEP as u64 {
            r.note(
                &format!("filler sentence number {i} about nothing"),
                503 + i,
            );
        }
        assert!(!r.is_echo("invoice from northwind is due on friday", 515));
    }

    #[test]
    fn a_read_aloud_mail_is_remembered_whole() {
        let mail = "From Dana: the shipment cleared customs this morning and will reach the \
            warehouse tomorrow afternoon, please confirm the loading dock is free.";
        let r = ring(mail);
        assert!(r.is_echo(
            "shipment cleared customs this morning and will reach the warehouse",
            1_010
        ));
        assert!(!r.is_echo("tell Dana the dock is booked until Monday", 1_010));
    }
}
