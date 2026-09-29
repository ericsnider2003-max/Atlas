//! Fixing what you type, as you type it.
//!
//! Grammarly is a cloud service: every keystroke you make goes to a server.
//! For a system whose whole premise is that nothing leaves your machine,
//! that's the wrong shape — so this is a local proofreader instead.
//!
//! It cannot do what a large model does, and it doesn't try. What it does is
//! catch the errors people *actually* make at speed, which turn out to be a
//! small and very repetitive set: dropped apostrophes, doubled words, the
//! wrong one of a homophone pair, a lower-case start after a full stop.
//! Between them those account for most of what you'd want caught.
//!
//! The important design rule: **fix silently only what has one possible
//! correction.** "dont" is unambiguously "don't". "its" might be right. Fixing
//! the first automatically and merely flagging the second is the difference
//! between helpful and infuriating.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// One obvious fix. Applied without asking.
    Certain,
    /// Probably wrong, but the other reading exists. Flagged.
    Likely,
    /// Worth a look. Never touched automatically.
    Maybe,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fix {
    /// Where in the text.
    pub at: usize,
    pub len: usize,
    pub was: String,
    pub becomes: String,
    pub kind: Kind,
    /// Why, in a few words.
    pub because: String,
}

/// Contractions people drop the apostrophe from. Every one of these has
/// exactly one correction, which is why they can be fixed silently.
const APOSTROPHE: &[(&str, &str)] = &[
    ("dont", "don't"), ("doesnt", "doesn't"), ("didnt", "didn't"),
    ("cant", "can't"), ("couldnt", "couldn't"), ("wont", "won't"),
    ("wouldnt", "wouldn't"), ("shouldnt", "shouldn't"), ("isnt", "isn't"),
    ("arent", "aren't"), ("wasnt", "wasn't"), ("werent", "weren't"),
    ("hasnt", "hasn't"), ("havent", "haven't"), ("hadnt", "hadn't"),
    ("im", "I'm"), ("ive", "I've"), ("ill", "I'll"), ("id", "I'd"),
    ("youre", "you're"), ("youve", "you've"), ("youll", "you'll"),
    ("theyre", "they're"), ("theyve", "they've"), ("thats", "that's"),
    ("whats", "what's"), ("lets", "let's"), ("hes", "he's"), ("shes", "she's"),
    ("weve", "we've"), ("wed", "we'd"), ("theres", "there's"),
];

/// Misspellings with one plausible correction.
const TYPOS: &[(&str, &str)] = &[
    ("teh", "the"), ("adn", "and"), ("nad", "and"), ("thre", "there"),
    ("recieve", "receive"), ("seperate", "separate"), ("definately", "definitely"),
    ("occured", "occurred"), ("neccessary", "necessary"), ("accomodate", "accommodate"),
    ("untill", "until"), ("wich", "which"), ("becuase", "because"), ("beacuse", "because"),
    ("thier", "their"), ("freind", "friend"), ("wierd", "weird"),
    ("diagnos", "diagnose"), ("asses", "assess"), ("wwant", "want"),
    ("alot", "a lot"), ("infact", "in fact"), ("aswell", "as well"),
    ("everytime", "every time"), ("atleast", "at least"),
];

/// Pairs where which one is right depends on the sentence.
const HOMOPHONES: &[(&str, &str, &str)] = &[
    ("its", "it's", "\"it's\" is \"it is\"; \"its\" is possessive"),
    ("your", "you're", "\"you're\" is \"you are\""),
    ("their", "they're", "\"they're\" is \"they are\""),
    ("there", "their", "\"their\" is possessive"),
    ("then", "than", "\"than\" compares; \"then\" is time"),
    ("affect", "effect", "\"affect\" is the verb; \"effect\" is the noun"),
    ("loose", "lose", "\"lose\" is the verb"),
    ("to", "too", "\"too\" means also or excessively"),
];

/// Words that follow "it's" but never "its".
const AFTER_ITS_CONTRACTION: &[&str] = &[
    "a", "an", "the", "not", "just", "been", "going", "still", "already",
    "probably", "definitely", "always", "never", "worth", "time", "fine",
    "good", "bad", "hard", "easy", "clear", "obvious", "important", "my",
    "your", "our", "too", "very", "really", "quite", "only", "about",
    "possible", "impossible", "true", "false", "done", "ready", "over",
];

/// Words that follow "its" but never "it's".
const AFTER_ITS_POSSESSIVE: &[&str] = &[
    "own", "self", "way", "place", "name", "size", "job", "purpose",
    "value", "use", "shape", "colour", "color", "edge", "end", "start",
    "price", "cost", "author", "title", "contents", "output", "result",
];

/// A verb or adverb after "your" means "you're". You don't own a "going".
const AFTER_YOURE: &[&str] = &[
    "going", "doing", "being", "getting", "making", "taking", "welcome",
    "right", "wrong", "sure", "probably", "definitely", "already", "still",
    "not", "never", "always", "about", "just", "the", "a", "an", "in",
    "on", "at", "so", "too", "very", "really", "trying", "looking", "working",
];

/// A noun after "you're" means "your".
const AFTER_YOUR: &[&str] = &[
    "email", "inbox", "file", "files", "laptop", "machine", "screen", "voice",
    "notes", "project", "code", "settings", "account", "money", "time",
    "name", "team", "work", "day", "week", "call", "meeting", "phone",
];

/// After "they're" comes a verb or adjective, never a possession.
const AFTER_THEYRE: &[&str] = &[
    "going", "doing", "not", "all", "just", "still", "already", "being",
    "the", "a", "an", "trying", "working", "coming", "here", "there", "on",
];

/// "Too" means also or excessively, so it precedes these.
const AFTER_TOO: &[&str] = &[
    "much", "many", "long", "short", "late", "early", "big", "small",
    "slow", "fast", "hard", "easy", "expensive", "cheap", "far", "close",
    "old", "young", "loud", "quiet", "busy", "tired",
];

/// Subject-verb pairs that are always wrong. High frequency, and the fix is
/// never ambiguous.
const AGREEMENT: &[(&str, &str, &str)] = &[
    ("he", "dont", "doesn't"), ("she", "dont", "doesn't"), ("it", "dont", "doesn't"),
    ("he", "don't", "doesn't"), ("she", "don't", "doesn't"), ("it", "don't", "doesn't"),
    ("he", "was'nt", "wasn't"), ("they", "was", "were"), ("we", "was", "were"),
    ("you", "was", "were"),
    // NOT ("there", "is", "are"): "there is a problem" is correct, and
    // "there are problems" is too -- which one depends on whether what
    // follows is singular or plural, which a word-pair check can never
    // know. That entry used to convert every "there is" to "there are"
    // unconditionally, silently breaking correct sentences roughly as
    // often as it fixed wrong ones. Caught by actually running the
    // checker against real text, not by reading the table.
];

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ProseConfig {
    pub enabled: bool,
    /// Fix the unambiguous ones without asking.
    pub fix_certain: bool,
    /// Flag the ones that might be right.
    pub flag_likely: bool,
    /// Apps never corrected in. Code and terminals are the obvious ones —
    /// "dont" in a string literal is meant to be there.
    pub never_in: Vec<String>,
    /// Words of yours that aren't mistakes.
    pub my_words: Vec<String>,
}

impl Default for ProseConfig {
    fn default() -> Self {
        ProseConfig {
            enabled: false,
            fix_certain: true,
            flag_likely: true,
            never_in: vec![
                "code".into(), "terminal".into(), "cmd".into(), "powershell".into(),
                "vs code".into(), "visual studio".into(), "sublime".into(), "vim".into(),
                "password".into(), "1password".into(), "bitwarden".into(),
            ],
            my_words: Vec::new(),
        }
    }
}

/// May Atlas correct in this window?
pub fn may_correct_in(app: &str, cfg: &ProseConfig) -> bool {
    let a = app.to_lowercase();
    !cfg.never_in.iter().any(|n| a.contains(&n.to_lowercase()))
}

fn word_at(text: &str, start: usize, len: usize) -> &str {
    &text[start..start + len]
}

/// Every word in the text with where it starts.
fn words(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices() {
        let is_word = c.is_alphanumeric() || c == '\'';
        match (is_word, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push((s, i - s));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((s, text.len() - s));
    }
    out
}

fn keep_case(original: &str, replacement: &str) -> String {
    if original.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
        let mut c = replacement.chars();
        match c.next() {
            Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            None => replacement.into(),
        }
    } else {
        replacement.into()
    }
}

/// Read the text and find what's wrong.
pub fn check(text: &str, cfg: &ProseConfig) -> Vec<Fix> {
    let mut fixes = Vec::new();
    let ws = words(text);
    let mine: Vec<String> = cfg.my_words.iter().map(|w| w.to_lowercase()).collect();

    for (idx, (start, len)) in ws.iter().enumerate() {
        let w = word_at(text, *start, *len);
        let lower = w.to_lowercase();

        // Your own words are not mistakes.
        if mine.contains(&lower) {
            continue;
        }

        // Doubled word: "the the". Always wrong, always one fix.
        if let Some((ps, pl)) = idx.checked_sub(1).and_then(|i| ws.get(i)) {
            let prev = word_at(text, *ps, *pl).to_lowercase();
            if prev == lower && lower.len() > 1 && lower != "had" && lower != "that" {
                fixes.push(Fix {
                    at: *start,
                    len: *len,
                    was: w.to_string(),
                    becomes: String::new(),
                    kind: Kind::Certain,
                    because: format!("\"{prev}\" twice"),
                });
                continue;
            }
        }

        // Subject-verb first: "he dont" is "he doesn't", and the apostrophe
        // rule would otherwise turn it into "he don't" and stop.
        if let Some((ps, pl)) = idx.checked_sub(1).and_then(|i| ws.get(i)) {
            let prev = word_at(text, *ps, *pl).to_lowercase();
            if let Some(fix) = agreement(&prev, &lower, w, *start, *len) {
                fixes.push(fix);
                continue;
            }
        }

        // A dropped apostrophe.
        if let Some((_, right)) = APOSTROPHE.iter().find(|(bad, _)| *bad == lower) {
            // "id", "im", "ill", "wed" and "its" are also real words, so they
            // only count at the start of a sentence or when the sentence
            // can't work otherwise.
            let risky = ["id", "ill", "wed", "shed", "hes"].contains(&lower.as_str());
            if risky && !starts_sentence(text, *start) {
                continue;
            }
            fixes.push(Fix {
                at: *start,
                len: *len,
                was: w.to_string(),
                becomes: keep_case(w, right),
                kind: Kind::Certain,
                because: "missing apostrophe".into(),
            });
            continue;
        }

        // A typo with one plausible correction.
        if let Some((_, right)) = TYPOS.iter().find(|(bad, _)| *bad == lower) {
            fixes.push(Fix {
                at: *start,
                len: *len,
                was: w.to_string(),
                becomes: keep_case(w, right),
                kind: Kind::Certain,
                because: "spelling".into(),
            });
            continue;
        }

        // "i" alone is always "I".
        if w == "i" {
            fixes.push(Fix {
                at: *start,
                len: *len,
                was: w.to_string(),
                becomes: "I".into(),
                kind: Kind::Certain,
                because: "\"I\" is capitalised".into(),
            });
            continue;
        }

        // Lower case after a full stop.
        //
        // No `continue` here: a word at the start of a sentence can be wrong
        // in more than one way, and stopping after the capital means "its" at
        // the start of a sentence is never checked at all.
        // `len` is BYTES, and the first character is not always one byte.
        //
        // This was `len: 1`. Every `Fix` is applied by
        // `out.replace_range(f.at..f.at + f.len, ..)`, and a byte range that
        // ends in the middle of a character is not a slice Rust will take --
        // it panics with "end of range should be a character boundary".
        //
        // So "école starts today" or "über alles" or "ñandú runs" -- any
        // sentence whose first word begins with a non-ASCII letter -- crashed
        // Atlas. Not the turn: the process. `crash::caught` wraps `tick`
        // (daemon.rs), and this is on the conversation path, which is not
        // wrapped, so the panic unwound out of the run loop.
        //
        // `len_utf8()` is the byte width of the character actually being
        // replaced, which is what the range wanted all along.
        if let Some(first) = w.chars().next().filter(|_| starts_sentence(text, *start)) {
            if first.is_lowercase() {
                fixes.push(Fix {
                    at: *start,
                    len: first.len_utf8(),
                    was: first.to_string(),
                    becomes: first.to_uppercase().to_string(),
                    kind: Kind::Certain,
                    because: "start of a sentence".into(),
                });
            }
        }

        // Homophones, resolved from the word after where that settles it.
        let next = ws.get(idx + 1).map(|(s, l)| word_at(text, *s, *l).to_lowercase());
        if let Some(fix) = homophone(w, &lower, *start, *len, next.as_deref()) {
            fixes.push(fix);
        }
    }

    fixes.retain(|f| match f.kind {
        Kind::Certain => cfg.fix_certain || cfg.flag_likely,
        Kind::Likely | Kind::Maybe => cfg.flag_likely,
    });
    drop_overlaps(fixes)
}

/// Two fixes covering the same text can't both be applied.
///
/// This happens at the start of a sentence, where a word can be both
/// lower-case and the wrong homophone: fixing the capital and fixing the word
/// are two edits at the same position, and applying both corrupts the text.
/// The larger fix wins — but it inherits the capital, or "its going" at the
/// start of a sentence becomes "it's going" with a lower-case i.
fn drop_overlaps(mut fixes: Vec<Fix>) -> Vec<Fix> {
    fixes.sort_by(|a, b| a.at.cmp(&b.at).then(b.len.cmp(&a.len)));
    let mut kept: Vec<Fix> = Vec::new();
    let mut dropped_capital_at: Vec<usize> = Vec::new();

    for f in fixes {
        let clash = kept.iter().position(|k| f.at < k.at + k.len && k.at < f.at + f.len);
        match clash {
            None => kept.push(f),
            Some(_) if f.because == "start of a sentence" => dropped_capital_at.push(f.at),
            Some(_) => {}
        }
    }
    for f in kept.iter_mut() {
        if dropped_capital_at.contains(&f.at) && !f.becomes.is_empty() {
            let mut c = f.becomes.chars();
            if let Some(first) = c.next() {
                f.becomes = first.to_uppercase().collect::<String>() + c.as_str();
            }
        }
    }
    kept
}

fn starts_sentence(text: &str, at: usize) -> bool {
    let before = &text[..at];
    let trimmed = before.trim_end();
    trimmed.is_empty() || trimmed.ends_with(['.', '!', '?', '\n'])
}

/// Which of a homophone pair was meant, from the word after it.
///
/// This is where the conservative version gave up too early. "Your going" is
/// not ambiguous — you cannot own a "going" — and flagging it rather than
/// fixing it is the sort of caution that makes a tool annoying without making
/// it safer.
fn homophone(w: &str, lower: &str, at: usize, len: usize, next: Option<&str>) -> Option<Fix> {
    let n = next.unwrap_or("");

    let fix = |becomes: &str, kind: Kind, because: String| {
        Some(Fix { at, len, was: w.into(), becomes: keep_case(w, becomes), kind, because })
    };

    match lower {
        "its" if AFTER_ITS_CONTRACTION.contains(&n) => {
            fix("it's", Kind::Certain, format!("\"its {n}\" can't be possessive"))
        }
        "its" => None,
        "it's" if AFTER_ITS_POSSESSIVE.contains(&n) => {
            fix("its", Kind::Certain, format!("\"it's {n}\" means \"it is {n}\""))
        }
        "it's" => None,

        "your" if AFTER_YOURE.contains(&n) => {
            fix("you're", Kind::Certain, format!("you can't own a \"{n}\""))
        }
        "your" => None,
        "you're" if AFTER_YOUR.contains(&n) => {
            fix("your", Kind::Certain, format!("\"you are {n}\" doesn't parse"))
        }
        "you're" => None,

        "their" if AFTER_THEYRE.contains(&n) => {
            fix("they're", Kind::Certain, format!("\"their {n}\" can't be possessive"))
        }
        "their" => None,
        "there" if AFTER_THEYRE.contains(&n) && n != "the" && n != "a" && n != "an" => {
            fix("they're", Kind::Likely, "this looks like \"they are\"".into())
        }
        "there" => None,

        "to" if AFTER_TOO.contains(&n) => {
            fix("too", Kind::Certain, format!("\"too {n}\" is the comparison"))
        }
        "to" => None,

        // The rest genuinely need the sentence, not the next word.
        _ => {
            let (_, other, why) = HOMOPHONES.iter().find(|(a, _, _)| *a == lower)?;
            fix(other, Kind::Maybe, why.to_string())
        }
    }
}

/// "He don't" and friends. Always wrong, never ambiguous.
fn agreement(prev: &str, lower: &str, w: &str, at: usize, len: usize) -> Option<Fix> {
    let (_, _, right) = AGREEMENT.iter().find(|(s, v, _)| *s == prev && *v == lower)?;
    Some(Fix {
        at,
        len,
        was: w.into(),
        becomes: keep_case(w, right),
        kind: Kind::Certain,
        because: format!("\"{prev} {right}\""),
    })
}

/// Compare "better then" and similar, which the word-by-word pass can't see.
pub fn check_phrases(text: &str) -> Vec<Fix> {
    let mut out = Vec::new();
    let lower = text.to_lowercase();
    // "-er then" and "more ... then" are comparisons, so they want "than".
    for (i, _) in lower.match_indices(" then ") {
        let before: Vec<&str> = lower[..i].split_whitespace().collect();
        let prev = before.last().copied().unwrap_or("");
        let comparative = prev.ends_with("er") && prev.len() > 3
            || ["more", "less", "better", "worse", "rather", "other"].contains(&prev);
        if comparative {
            out.push(Fix {
                at: i + 1,
                len: 4,
                was: "then".into(),
                becomes: "than".into(),
                kind: Kind::Likely,
                because: format!("\"{prev}\" is comparing, so it wants \"than\""),
            });
        }
    }
    out
}

/// Apply the fixes that are safe to apply without asking.
///
/// Applied back to front so earlier positions stay valid.
pub fn apply_certain(text: &str, fixes: &[Fix]) -> (String, usize) {
    let mut out = text.to_string();
    let mut certain: Vec<&Fix> = fixes.iter().filter(|f| f.kind == Kind::Certain).collect();
    certain.sort_by_key(|f| std::cmp::Reverse(f.at));

    let mut n = 0;
    for f in certain {
        if f.at + f.len > out.len() {
            continue;
        }
        if f.becomes.is_empty() {
            // A doubled word: take the word and the space before it.
            //
            // `rfind` gives the byte index where the last non-space character
            // STARTS, so "one past it" is `i + that character's width`, not
            // `i + 1`. It was `i + 1`, which lands mid-character whenever the
            // word before the repeat ends in a non-ASCII letter -- "café café"
            // put `from` on the second byte of `é` and
            // `replace_range` panicked with "start of range should be a
            // character boundary", taking the process with it.
            //
            // Same defect as the sentence-capital fix above, in the other
            // direction: one assumed a character is one byte wide going
            // forward, this one going back.
            let from = out[..f.at]
                .char_indices()
                .rev()
                .find(|(_, c)| !c.is_whitespace())
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(f.at);
            out.replace_range(from..f.at + f.len, "");
        } else {
            out.replace_range(f.at..f.at + f.len, &f.becomes);
        }
        n += 1;
    }
    (out, n)
}

/// Corrections you keep undoing.
///
/// The third time Atlas changes something back that you changed back, the
/// problem is Atlas. This catches that rather than waiting for you to notice
/// and go looking for a setting.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Overreach {
    /// Word to how many times you've undone the correction.
    undone: Vec<(String, u32)>,
}

impl Overreach {
    pub fn you_undid(&mut self, word: &str) {
        let w = word.to_lowercase();
        match self.undone.iter_mut().find(|(x, _)| *x == w) {
            Some((_, n)) => *n += 1,
            None => self.undone.push((w, 1)),
        }
    }

    /// Should Atlas stop correcting this?
    pub fn leave_alone(&self, word: &str) -> bool {
        let w = word.to_lowercase();
        self.undone.iter().any(|(x, n)| *x == w && *n >= 3)
    }

    /// What Atlas says about it, once.
    pub fn ask(&self, word: &str) -> Option<String> {
        let w = word.to_lowercase();
        self.undone
            .iter()
            .find(|(x, n)| *x == w && *n == 3)
            .map(|_| format!("I keep changing \"{word}\" and you keep changing it back. Is it yours?"))
    }
}

/// Learn how you write, from what you send.
///
/// Not to imitate you — to stop flagging things that are just how you write.
/// Someone who never uses semicolons doesn't want them suggested.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Voice {
    pub sentences_seen: u32,
    pub avg_words: f32,
    /// Words behind `contraction_rate`, so it can be blended rather than
    /// replaced.
    ///
    /// Added 19 Sep 2026 with the bug it fixes. `#[serde(default)]` so a
    /// `Voice` written before it still loads; a zero here means the rate
    /// starts again from the next text, which is the right direction for a
    /// number that was wrong anyway.
    #[serde(default)]
    pub words_seen: u32,
    /// Contractions per hundred words. High means informal.
    pub contraction_rate: f32,
    /// You start sentences with And, But, So.
    pub starts_with_conjunctions: bool,
}

impl Voice {
    pub fn learn(&mut self, text: &str) {
        let sentences: Vec<&str> = text
            .split_inclusive(['.', '!', '?'])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        if sentences.is_empty() {
            return;
        }
        let words: Vec<&str> = text.split_whitespace().collect();
        let n = sentences.len() as f32;
        let mean = words.len() as f32 / n;

        let seen = self.sentences_seen as f32;
        self.avg_words = (self.avg_words * seen + mean * n) / (seen + n);
        self.sentences_seen += sentences.len() as u32;

        // Blended, not replaced.
        //
        // This was `self.contraction_rate = contractions / words * 100.0` --
        // an assignment, two lines below `avg_words` being correctly blended
        // into its own running mean. So after learning from fifty pieces of
        // your writing, this held the rate of **the fiftieth**, and
        // `is_your_style` gated on twenty sentences before answering from one
        // text.
        //
        // The effect is worst where it matters: one terse message with no
        // contractions in it drops the rate under three, and Atlas starts
        // "correcting" the contractions in everything you write, having spent
        // weeks learning that you use them.
        //
        // Weighted by words rather than by texts, because it is a per-word
        // rate and a two-hundred-word piece is not one sample.
        let contractions = words.iter().filter(|w| w.contains('\'')).count() as f32;
        let this_rate = contractions / words.len().max(1) as f32 * 100.0;
        let words_before = self.words_seen as f32;
        let words_now = words.len() as f32;
        self.contraction_rate = if words_before + words_now > 0.0 {
            (self.contraction_rate * words_before + this_rate * words_now)
                / (words_before + words_now)
        } else {
            this_rate
        };
        self.words_seen = self.words_seen.saturating_add(words.len() as u32);

        if sentences.iter().any(|s| {
            let f = s.split_whitespace().next().unwrap_or("").to_lowercase();
            ["and", "but", "so"].contains(&f.as_str())
        }) {
            self.starts_with_conjunctions = true;
        }
    }

    /// Is this how you write, rather than a mistake?
    pub fn is_your_style(&self, what: &str) -> bool {
        if self.sentences_seen < 20 {
            return false;
        }
        match what {
            "sentence starts with a conjunction" => self.starts_with_conjunctions,
            "informal" => self.contraction_rate > 3.0,
            "short sentences" => self.avg_words < 12.0,
            _ => false,
        }
    }
}

/// What Atlas says, if anything.
///
/// Silence is the right answer for the ones it fixed — you don't want a
/// notification every time you type "dont".
pub fn spoken(fixes: &[Fix], fixed: usize) -> String {
    let flagged: Vec<&Fix> = fixes.iter().filter(|f| f.kind != Kind::Certain).collect();
    match (fixed, flagged.len()) {
        (0, 0) => String::new(),
        (_, 0) => String::new(),
        (_, 1) => format!("\"{}\" — {}?", flagged[0].was, flagged[0].because),
        (_, n) => format!("{n} things worth a look — first is \"{}\".", flagged[0].was),
    }
}
