//! Saying the same thing again.
//!
//! Eric's evening on the laptop (29 Sep 2026): a 4B model, given its own
//! earlier replies in the conversation, answered almost every sentence with
//! the same one -- "But hey, if you want me to stop, I'll stop. If you want
//! me to try, I'll try. Either way, I'm here. What's your next move? A joke?
//! A memory? ..." -- with a different first few words each time. The guard
//! that was there compared only the first sentence, word for word, so a
//! reply that opened differently went through whole.
//!
//! What this module decides, for `brain`, `thread` and the summary:
//!
//! - **Is this sentence one Atlas already said** in the last few replies
//!   (near enough: most of its words, in order), or a stock closer that is
//!   never worth saying ("What's your next move?", "Either way, I'm here")?
//!   Those sentences are not said (`SentenceFilter`).
//! - **Is a whole reply a near copy** of a recent one (`near_copy`): shared
//!   word trigrams, or a long stretch in common.
//! - **What of a past reply goes back to the model** (`for_history`): its
//!   first sentence or two, without the closers -- a model shown its own
//!   loop copies it.
//!
//! Pure text in, text out: no model, no state, so every rule is tested on
//! the evening's own replies.

/// Closers a voice assistant never needs, from the evening and from what
/// small models reach for. Matched on a sentence's words, whole.
pub const STOCK_CLOSERS: &[&str] = &[
    "whats your next move",
    "what's your next move",
    "a joke",
    "a memory",
    "either way im here",
    "either way im tuned in",
    "either way ill be here",
    "ive got your back",
    "whats on your mind",
    "anything else",
    "anything else i can help with",
    "let me know if you need anything else",
    "let me know if theres anything else",
    "is there anything else",
    "how can i help",
    "how can i help you today",
    "what would you like to do next",
    "what do you want to do next",
    "or keep going",
    "or keep chatting",
    "keep going",
    "want me to look it up",
    // 30 Sep 2026, a real Qwen3.5-2B through the new prompt: every chat reply
    // closed on a question about his evening.
    "what do you want me to do",
    "what do you want me to do next",
    "what else do you want to check out tonight",
    "what else do you want to do",
    "what are your plans for tonight",
    "what are we doing for tonight",
    "what are we doing tonight",
    "did you get enough coffee",
    "what do you want to know",
];

/// Phrases that only ever come from a reply's boilerplate, looked for
/// anywhere in a text (a summary line, say), not only as a whole sentence.
pub const BOILERPLATE: &[&str] = &[
    "whats your next move",
    "a joke a memory",
    "either way im here",
    "either way im tuned in",
    "im tuned in",
    "testing if i can still hear you",
    "if you want me to stop ill stop",
    "if you want me to try ill try",
    "ive got your back",
];

/// Does `text` carry a reply's boilerplate anywhere in it?
pub fn carries_boilerplate(text: &str) -> bool {
    let w = format!(" {} ", words(text).join(" "));
    BOILERPLATE.iter().any(|b| w.contains(&format!(" {b} ")))
}

/// Words of `s`: lower case, letters and digits, apostrophes dropped
/// ("I'm" and "Im" are one word), in order.
pub fn words(s: &str) -> Vec<String> {
    s.to_lowercase()
        .replace(['\u{2019}', '\''], "")
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

fn joined(s: &str) -> String {
    words(s).join(" ")
}

/// Is this sentence only a stock closer?
pub fn is_stock_closer(sentence: &str) -> bool {
    let w = joined(sentence);
    if w.is_empty() {
        return false;
    }
    STOCK_CLOSERS.iter().any(|c| joined(c) == w)
        // "Or maybe you're testing if I can still hear you when you're not
        // talking?" -- the evening's other closer, whatever it opened with.
        || w.contains("testing if i can still hear you")
        || (w.starts_with("either way") && w.split(' ').count() <= 6)
}

/// The sentences of `text`, split after `.`, `!` or `?` (or `…`) followed
/// by a space or the end. Kept whole, with their punctuation.
pub fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        cur.push(c);
        if matches!(c, '.' | '!' | '?' | '…') && chars.get(i + 1).is_none_or(|n| n.is_whitespace()) {
            let s = cur.trim().to_string();
            if !s.is_empty() {
                out.push(s);
            }
            cur.clear();
        }
    }
    let s = cur.trim().to_string();
    if !s.is_empty() {
        out.push(s);
    }
    out
}

fn trigrams(w: &[String]) -> std::collections::HashSet<String> {
    if w.len() < 3 {
        return w.iter().cloned().collect();
    }
    w.windows(3).map(|t| t.join(" ")).collect()
}

/// How alike two texts are, 0 to 1: the share of word trigrams they have in
/// common (Jaccard). Short texts compare by their words.
fn likeness(a: &str, b: &str) -> f32 {
    let (wa, wb) = (words(a), words(b));
    if wa.is_empty() || wb.is_empty() {
        return 0.0;
    }
    let (ta, tb) = (trigrams(&wa), trigrams(&wb));
    let common = ta.intersection(&tb).count() as f32;
    let all = ta.union(&tb).count() as f32;
    if all == 0.0 { 0.0 } else { common / all }
}

/// The longest run of words the two texts share, in order.
pub fn longest_shared_run(a: &str, b: &str) -> usize {
    let (wa, wb) = (words(a), words(b));
    let mut best = 0;
    let mut prev = vec![0usize; wb.len() + 1];
    for x in &wa {
        let mut cur = vec![0usize; wb.len() + 1];
        for (j, y) in wb.iter().enumerate() {
            if x == y {
                cur[j + 1] = prev[j] + 1;
                best = best.max(cur[j + 1]);
            }
        }
        prev = cur;
    }
    best
}

/// A whole reply that is a near copy of `earlier`: at least `LIKE` of its
/// trigrams shared, or a run of `RUN` words in common.
pub fn near_copy(reply: &str, earlier: &str) -> bool {
    const LIKE: f32 = 0.45;
    const RUN: usize = 12;
    likeness(reply, earlier) >= LIKE || longest_shared_run(reply, earlier) >= RUN
}

/// Is this sentence (of five words or more) one of `earlier`'s sentences
/// said again -- nearly word for word -- or a closer?
pub fn said_before(sentence: &str, earlier: &[&str]) -> bool {
    if is_stock_closer(sentence) {
        return true;
    }
    let w = words(sentence);
    if w.len() < 5 {
        return false;
    }
    earlier.iter().flat_map(|e| sentences(e)).any(|s| {
        let ws = words(&s);
        ws.len() >= 4 && (ws == w || likeness(sentence, &s) >= 0.6 || longest_shared_run(sentence, &s) >= 8)
    })
}

/// A reply as the model is shown it later: its first `most` sentences, the
/// stock closers left out. Nothing, when that leaves nothing.
pub fn for_history(reply: &str, most: usize) -> String {
    sentences(reply)
        .into_iter()
        .filter(|s| !is_stock_closer(s) && !carries_boilerplate(s))
        .take(most)
        .collect::<Vec<_>>()
        .join(" ")
}

/// A reply with the closers and any sentence already said in it once taken
/// out: what is left to say.
pub fn without_closers(reply: &str) -> String {
    let mut kept: Vec<String> = Vec::new();
    for s in sentences(reply) {
        if is_stock_closer(&s) {
            continue;
        }
        let refs: Vec<&str> = kept.iter().map(|k| k.as_str()).collect();
        if said_before(&s, &refs) {
            continue;
        }
        kept.push(s);
    }
    kept.join(" ")
}

/// Sentence by sentence, as a reply streams: which may be said.
///
/// A sentence one of the recent replies already said, or a stock closer, is
/// dropped; so is one this reply already said. `first_repeated` is set when
/// the very first sentence was one of those -- the reply is starting a loop,
/// and is better asked again than trimmed.
#[derive(Debug, Default)]
pub struct SentenceFilter {
    earlier: Vec<String>,
    kept: Vec<String>,
    pub dropped: usize,
    pub first_repeated: bool,
    seen: usize,
}

impl SentenceFilter {
    pub fn new(earlier: &[&str]) -> SentenceFilter {
        SentenceFilter { earlier: earlier.iter().map(|s| s.to_string()).collect(), ..Default::default() }
    }

    /// May this sentence be said?
    pub fn pass(&mut self, sentence: &str) -> bool {
        let first = self.seen == 0;
        self.seen += 1;
        let mut against: Vec<&str> = self.earlier.iter().map(|s| s.as_str()).collect();
        against.extend(self.kept.iter().map(|s| s.as_str()));
        let repeat = said_before(sentence, &against);
        if repeat {
            self.dropped += 1;
            if first {
                self.first_repeated = true;
            }
            return false;
        }
        self.kept.push(sentence.to_string());
        true
    }

    /// Every sentence so far was dropped, or most of them were: a loop.
    pub fn looping(&self) -> bool {
        self.seen > 0 && (self.kept.is_empty() || self.dropped * 2 > self.seen)
    }

    /// What was let through.
    pub fn kept(&self) -> String {
        self.kept.join(" ")
    }
}
