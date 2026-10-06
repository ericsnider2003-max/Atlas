//! Typing what you say.
//!
//! Not a command — the words go into whatever window you're looking at. For
//! anyone who thinks faster than they type, this is the highest-frequency use
//! of a voice assistant there is, and every piece of it already existed.
//!
//! The thing that makes dictation usable rather than infuriating is knowing
//! when you meant a word and when you meant an instruction. "New paragraph" is
//! almost never a phrase you wanted typed. "Full stop" usually is a full stop.
//! But "period drama" is not punctuation, so the rule has to be about position
//! and isolation, not just the word.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct DictateConfig {
    pub enabled: bool,
    /// Turn spoken punctuation into marks.
    pub spoken_punctuation: bool,
    /// Capitalise after a full stop and at the start.
    pub auto_capitalise: bool,
    /// Apps never dictated into, whatever you say. A misheard sentence in a
    /// chat window is public; in a document it is a typo.
    pub never_into: Vec<String>,
    /// Stop dictating after this long with nothing said.
    pub idle_stop_secs: u64,
}

impl Default for DictateConfig {
    fn default() -> Self {
        DictateConfig {
            enabled: false,
            spoken_punctuation: true,
            auto_capitalise: true,
            never_into: vec!["discord".into(), "slack".into(), "teams".into()],
            idle_stop_secs: 45,
        }
    }
}

/// What a spoken phrase turns into.
#[derive(Debug, Clone, PartialEq)]
pub enum Piece {
    Text(String),
    Punctuation(&'static str),
    NewLine,
    NewParagraph,
    /// Undo the last thing typed.
    Scratch,
    /// Leave dictation.
    Stop,
}

/// Words that are sometimes punctuation and sometimes just words.
///
/// "A period drama" is not a full stop. "Put a dash there" is not a dash.
/// Dropping these was the easy answer and the wrong one — the surrounding
/// words say which is meant, and reading them is the job.
pub const AMBIGUOUS: &[&str] = &["period", "comma", "dash", "hyphen", "colon", "quote"];

/// Which reading of an ambiguous word is meant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reading {
    /// The punctuation mark.
    Mark,
    /// The word itself.
    Literal,
    /// Genuinely can't tell — worth asking rather than guessing.
    Unsure,
}

/// Words that make what follows a thing rather than a mark.
///
/// You do not say "a" before a full stop. If one of these comes immediately
/// before, the word is a noun.
const DETERMINERS: &[&str] = &[
    "a", "an", "the", "this", "that", "these", "those", "my", "your", "his",
    "her", "its", "our", "their", "one", "another", "each", "every", "some",
    "no", "any", "which", "what",
];

/// Words that only ever follow a noun, never a mark.
const AFTER_A_NOUN: &[&str] = &[
    "drama", "piece", "key", "mark", "splice", "case", "of", "in", "was", "is",
    "drive", "film", "series", "cut", "line", "sign", "button", "point",
];

/// Read the words either side and decide which was meant.
///
/// The rules are about grammar rather than about the word: nothing follows a
/// determiner but a noun, and nothing precedes a mark but the end of a
/// clause.
pub fn read_ambiguous(before: &str, _word: &str, after: &str) -> Reading {
    let prev = before.split_whitespace().last().unwrap_or("").to_lowercase();
    let next = after.split_whitespace().next().unwrap_or("").to_lowercase();
    let prev = prev.trim_matches(|c: char| !c.is_alphanumeric());
    let next = next.trim_matches(|c: char| !c.is_alphanumeric());

    // "a period", "the dash" — you never say that about punctuation.
    if DETERMINERS.contains(&prev) {
        return Reading::Literal;
    }
    // "period drama", "comma splice" — the next word makes it a noun.
    if AFTER_A_NOUN.contains(&next) {
        return Reading::Literal;
    }
    // Nothing at all before it: "comma" alone is a mark you're inserting.
    if prev.is_empty() && next.is_empty() {
        return Reading::Mark;
    }
    // At the very end of what you said, after actual words.
    if next.is_empty() && !prev.is_empty() {
        return Reading::Mark;
    }
    // Mid-sentence with words either side is the normal dictation case:
    // "hello comma how are you". A mark is far more likely than someone
    // narrating the word.
    if !prev.is_empty() && !next.is_empty() {
        return Reading::Mark;
    }
    Reading::Unsure
}

/// What Atlas asks when it genuinely can't tell.
pub fn ask_which(word: &str) -> String {
    format!("Did you mean the {word} mark, or the word?")
}

/// Phrases that are instructions rather than words.
///
/// The multi-word ones are unambiguous — nobody says "full stop" or "new
/// paragraph" literally. The single ambiguous words go through
/// `read_ambiguous` first.
fn as_instruction(word: &str) -> Option<Piece> {
    Some(match word {
        "full stop" | "period" => Piece::Punctuation("."),
        "comma" => Piece::Punctuation(","),
        "question mark" => Piece::Punctuation("?"),
        "exclamation mark" | "exclamation point" => Piece::Punctuation("!"),
        "colon" => Piece::Punctuation(":"),
        "semicolon" | "semi colon" => Piece::Punctuation(";"),
        "dash" | "hyphen" => Piece::Punctuation("-"),
        "open bracket" => Piece::Punctuation("("),
        "close bracket" => Piece::Punctuation(")"),
        "quote" | "open quote" => Piece::Punctuation("\""),
        "new line" | "newline" => Piece::NewLine,
        "new paragraph" => Piece::NewParagraph,
        "scratch that" | "delete that" | "undo that" => Piece::Scratch,
        "stop dictating" | "stop dictation" | "that's it" | "thats it" => Piece::Stop,
        _ => return None,
    })
}

/// Break a spoken line into text and instructions.
pub fn parse(said: &str, cfg: &DictateConfig) -> Vec<Piece> {
    let mut out = Vec::new();
    if !cfg.spoken_punctuation {
        if !said.trim().is_empty() {
            out.push(Piece::Text(said.trim().to_string()));
        }
        return out;
    }

    // Longest phrases first, so "exclamation mark" beats "mark".
    let mut keys: Vec<&str> = vec![
        "exclamation mark", "exclamation point", "question mark", "new paragraph",
        "stop dictating", "stop dictation", "scratch that", "delete that", "undo that",
        "open bracket", "close bracket", "semi colon", "open quote", "full stop",
        "new line", "semicolon", "newline", "that's it", "thats it",
        "period", "comma", "colon", "dash", "hyphen", "quote",
    ];
    keys.sort_by_key(|k| std::cmp::Reverse(k.len()));

    // Matched against an ASCII-lowercased copy and *taken* from the original.
    //
    // This used to build the text out of the lowercased copy, so every proper
    // noun came back flattened — "dear sarah", "i'll call john on tuesday".
    // Dictation you have to go back and re-capitalise by hand is dictation
    // nobody uses twice. Invisible until something actually typed the result
    // into a window.
    //
    // `to_ascii_lowercase` rather than `to_lowercase` is what makes this
    // safe: it never changes the byte length, so an offset into the copy is
    // the same offset into the original. Every keyword is ASCII, so nothing
    // is lost by not folding the rest.
    let original = said.trim();
    let lower = original.to_ascii_lowercase();
    let mut rest = lower.as_str();
    let mut buffer = String::new();

    'outer: while !rest.is_empty() {
        // Exact, because `rest` is always a subslice of `lower`.
        let at = lower.len() - rest.len();
        for k in &keys {
            if rest.starts_with(k) {
                let after = &rest[k.len()..];
                let boundary = after.is_empty() || after.starts_with(' ') || after.starts_with(',');
                if boundary {
                    // An ambiguous word is read in context rather than
                    // assumed. "A period drama" keeps its period.
                    if AMBIGUOUS.contains(k) {
                        let before = &lower[..at];
                        if read_ambiguous(before, k, after) != Reading::Mark {
                            // It's a word. Fall through and take it as text,
                            // as it was said.
                            buffer.push_str(&original[at..at + k.len()]);
                            buffer.push(' ');
                            rest = after.trim_start();
                            continue 'outer;
                        }
                    }
                    if !buffer.trim().is_empty() {
                        out.push(Piece::Text(buffer.trim().to_string()));
                        buffer.clear();
                    }
                    if let Some(p) = as_instruction(k) {
                        out.push(p);
                    }
                    rest = after.trim_start();
                    continue 'outer;
                }
            }
        }
        // Take one word and try again.
        match rest.find(' ') {
            Some(i) => {
                buffer.push_str(&original[at..at + i]);
                buffer.push(' ');
                rest = &rest[i + 1..];
            }
            None => {
                buffer.push_str(&original[at..]);
                rest = "";
            }
        }
    }
    if !buffer.trim().is_empty() {
        out.push(Piece::Text(buffer.trim().to_string()));
    }
    out
}

/// Turn pieces into the characters to type.
///
/// Punctuation attaches to the previous word with no space before it, which is
/// the bit that makes dictated text look typed rather than assembled.
pub fn render(pieces: &[Piece], cfg: &DictateConfig) -> String {
    let mut out = String::new();
    let mut capitalise_next = cfg.auto_capitalise;

    for p in pieces {
        match p {
            Piece::Text(t) => {
                if !out.is_empty() && !out.ends_with('\n') && !out.ends_with(' ') {
                    out.push(' ');
                }
                if capitalise_next {
                    let mut c = t.chars();
                    if let Some(f) = c.next() {
                        out.push_str(&f.to_uppercase().to_string());
                        out.push_str(c.as_str());
                    }
                    capitalise_next = false;
                } else {
                    out.push_str(t);
                }
            }
            Piece::Punctuation(mark) => {
                while out.ends_with(' ') {
                    out.pop();
                }
                out.push_str(mark);
                if cfg.auto_capitalise && matches!(*mark, "." | "?" | "!") {
                    capitalise_next = true;
                }
            }
            Piece::NewLine => {
                out.push('\n');
                capitalise_next = cfg.auto_capitalise;
            }
            Piece::NewParagraph => {
                out.push_str("\n\n");
                capitalise_next = cfg.auto_capitalise;
            }
            Piece::Scratch | Piece::Stop => {}
        }
    }
    out
}

/// May Atlas type into this window?
pub fn may_type_into(app: &str, cfg: &DictateConfig) -> bool {
    let a = app.to_lowercase();
    !cfg.never_into.iter().any(|n| a.contains(&n.to_lowercase()))
}

/// Why not, when it won't.
pub fn refusal(app: &str) -> String {
    format!("I don't dictate into {app} — a misheard sentence there is public.")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Off,
    On,
}

#[derive(Debug, Clone)]
pub struct Dictation {
    pub state: State,
    /// The window it started in. Moving to another one stops it.
    pub target: String,
    /// What was typed last, so "scratch that" can take it back.
    pub last: String,
    pub last_spoke: u64,
}

impl Dictation {
    pub fn start(app: &str, t: u64) -> Dictation {
        Dictation { state: State::On, target: app.into(), last: String::new(), last_spoke: t }
    }

    /// One line of speech while dictating.
    pub fn heard(
        &mut self,
        said: &str,
        focused_app: &str,
        cfg: &DictateConfig,
        t: u64,
    ) -> Result<String, String> {
        if self.state == State::Off {
            return Err("not dictating".into());
        }
        // Typing into whatever happens to be in front of you now is how
        // dictated text ends up in the wrong window.
        if !focused_app.eq_ignore_ascii_case(&self.target) {
            self.state = State::Off;
            return Err(format!("you moved to {focused_app}, so I've stopped."));
        }
        self.last_spoke = t;

        let pieces = parse(said, cfg);
        if pieces.contains(&Piece::Stop) {
            self.state = State::Off;
            return Err("Stopped dictating.".into());
        }
        if pieces.contains(&Piece::Scratch) {
            let taken_back = std::mem::take(&mut self.last);
            return Err(format!("Took back: {taken_back}"));
        }
        let text = render(&pieces, cfg);
        self.last = text.clone();
        Ok(text)
    }

    /// Nothing said for a while — stop rather than sit there listening.
    pub fn idle_check(&mut self, cfg: &DictateConfig, t: u64) -> bool {
        if self.state == State::On && t.saturating_sub(self.last_spoke) >= cfg.idle_stop_secs {
            self.state = State::Off;
            return true;
        }
        false
    }
}
