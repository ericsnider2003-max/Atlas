//! The clipboard as a way of telling Atlas what you mean.
//!
//! Idea #1 on the list, and the cheapest context Atlas can get. You copy
//! something — an error, a paragraph, a table, a link — and say "explain
//! this". No screenshot, no vision model, no guessing which window you meant.
//! You already selected exactly the thing.
//!
//! The reply goes back to the clipboard, so you paste it where you were.

use serde::Deserialize;

/// What was copied, guessed from its shape. Enough to pick a sensible default
/// action without asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Url,
    Code,
    Error,
    Table,
    Prose,
    /// A path to something on disk.
    Path,
    Empty,
}

impl Kind {
    /// What Atlas offers to do with it when you don't say.
    pub fn default_action(&self) -> &'static str {
        match self {
            Kind::Url => "read that page",
            Kind::Code => "explain that code",
            Kind::Error => "work out what that error means",
            Kind::Table => "summarise those numbers",
            Kind::Path => "open that",
            Kind::Prose => "summarise that",
            Kind::Empty => "nothing — the clipboard is empty",
        }
    }
}

pub fn classify(text: &str) -> Kind {
    let t = text.trim();
    if t.is_empty() {
        return Kind::Empty;
    }
    let lower = t.to_lowercase();

    if t.lines().count() == 1 && (t.starts_with("http://") || t.starts_with("https://")) {
        return Kind::Url;
    }
    if t.lines().count() == 1
        && (t.contains(":\\") || t.starts_with('/') || t.starts_with("~/"))
        && !t.contains(' ')
    {
        return Kind::Path;
    }
    // Errors first: a stack trace is also code-shaped, and the error reading
    // is the more useful one.
    const ERROR_SIGNS: &[&str] = &[
        "error", "exception", "traceback", "panicked", "failed", "cannot",
        "undefined", "null reference", "stack trace", "errno",
    ];
    if ERROR_SIGNS.iter().any(|s| lower.contains(s)) {
        return Kind::Error;
    }
    const CODE_SIGNS: &[&str] =
        &["fn ", "def ", "class ", "function", "import ", "#include", "=>", "();", "{", "};"];
    let code_hits = CODE_SIGNS.iter().filter(|s| t.contains(**s)).count();
    if code_hits >= 2 {
        return Kind::Code;
    }
    // Several lines with consistent separators reads as a table.
    let lines: Vec<&str> = t.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.len() >= 3 {
        let tabbed = lines.iter().filter(|l| l.contains('\t') || l.matches(',').count() >= 2).count();
        if tabbed * 2 >= lines.len() {
            return Kind::Table;
        }
    }
    Kind::Prose
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ClipboardConfig {
    pub enabled: bool,
    /// Longest thing Atlas will take in one go.
    pub max_chars: usize,
    /// Put the answer back on the clipboard.
    pub reply_to_clipboard: bool,
    /// Never read the clipboard unless you asked in the same breath — no
    /// background watching.
    pub only_on_request: bool,
}

impl Default for ClipboardConfig {
    fn default() -> Self {
        ClipboardConfig {
            enabled: true,
            max_chars: 20_000,
            reply_to_clipboard: true,
            // A clipboard monitor would see every password you copy, so
            // this stays on. The one exception is yours to make: clipboard
            // history (`cliphist`, round 11) is off until you turn it on,
            // keeps what you copy in memory only, and skips any copy a
            // password manager marks private or that looks like a key.
            only_on_request: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Grab {
    pub text: String,
    pub kind: Kind,
    pub truncated: bool,
}

impl Grab {
    /// A line naming what it picked up, so you know it got the right thing.
    pub fn describe(&self) -> String {
        match self.kind {
            Kind::Empty => "There's nothing on the clipboard.".into(),
            _ => {
                let words = self.text.split_whitespace().count();
                let what = match self.kind {
                    Kind::Url => "a link",
                    Kind::Code => "some code",
                    Kind::Error => "an error",
                    Kind::Table => "a table",
                    Kind::Path => "a file path",
                    _ => "some text",
                };
                if self.truncated {
                    format!("Got {what}, first {words} words of it.")
                } else {
                    format!("Got {what}, {words} words.")
                }
            }
        }
    }
}

pub fn take(text: &str, cfg: &ClipboardConfig) -> Grab {
    let kind = classify(text);
    let truncated = text.chars().count() > cfg.max_chars;
    let text = if truncated {
        text.chars().take(cfg.max_chars).collect()
    } else {
        text.to_string()
    };
    Grab { text, kind, truncated }
}

/// The instruction for answering a clipboard request.
///
/// The answer is written back onto the clipboard when `reply_to_clipboard` is
/// on, so it has to be the thing you'd paste — the explanation, the rewrite,
/// the summary — and nothing else. No "Sure, here's…", no restating the
/// question, no closing offer: those would land in your paste.
pub const ANSWER_SYSTEM: &str = "\
You are answering something the person copied and asked about. Whatever you \
return may be pasted straight back where they were working, so return only the \
answer itself — the explanation, rewrite, translation or summary they asked \
for. No preamble, no sign-off, no restating the request. If they asked you to \
rewrite or translate the text, return only the rewritten text.";

/// Build the prompt for the model. What you said comes first, so an explicit
/// instruction beats the guessed one.
pub fn prompt(said: &str, grab: &Grab) -> String {
    let instruction = if said.trim().is_empty() {
        grab.kind.default_action().to_string()
    } else {
        said.trim().to_string()
    };
    format!(
        "{instruction}\n\nThis is what I copied ({:?}):\n---\n{}\n---",
        grab.kind, grab.text
    )
}

/// Phrases that mean "use what I just copied".
pub fn refers_to_clipboard(said: &str) -> bool {
    let t = said.to_lowercase();
    const HINTS: &[&str] = &[
        "this", "that", "what i copied", "what i just copied", "the clipboard",
        "my clipboard", "copied", "pasted",
    ];
    HINTS.iter().any(|h| t.contains(h))
}
