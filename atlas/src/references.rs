//! "Move it to the other screen."
//!
//! Speech is full of pronouns, and an assistant that makes you name the target
//! every single time is a command line with extra steps. Atlas resolves "it",
//! "that", "this one" against what just happened.
//!
//! The rule that keeps this safe: **resolve only when there is something to
//! resolve to.** A dangling pronoun becomes a question, never a guess. Guessing
//! wrong here means acting on the wrong window.

use serde::{Deserialize, Serialize};

/// What a pronoun could be pointing at, newest first.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Referents {
    /// Last app Atlas acted on.
    pub last_app: Option<String>,
    /// App currently in the foreground.
    pub active_app: Option<String>,
    /// Last file mentioned or produced.
    pub last_file: Option<String>,
    /// Last note or research output.
    pub last_note: Option<String>,
    /// Last screenshot or webcam frame.
    pub last_capture: Option<String>,
    /// Last topic researched or discussed.
    pub last_topic: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    App,
    File,
    Topic,
    Any,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Resolution {
    /// Nothing to resolve — the text named its target.
    Unchanged(String),
    /// Pronoun replaced with a concrete referent.
    Resolved { text: String, referent: String },
    /// A pronoun with nothing behind it. Ask rather than guess.
    Ambiguous(String),
}

impl Resolution {
    pub fn text(&self) -> &str {
        match self {
            Resolution::Unchanged(t) | Resolution::Resolved { text: t, .. } => t,
            Resolution::Ambiguous(q) => q,
        }
    }
}

const PRONOUNS: &[&str] = &["it", "that", "this", "them", "those", "these", "there"];

/// Words that hint at what kind of thing is meant, so "close it" looks for an
/// app and "open that" prefers a file.
fn kind_hint(text: &str) -> Kind {
    let t = text.to_lowercase();
    if ["close", "focus", "switch", "minimise", "minimize", "move"]
        .iter()
        .any(|v| t.starts_with(v))
    {
        return Kind::App;
    }
    if ["open", "read", "summarise", "summarize", "delete", "rename"]
        .iter()
        .any(|v| t.starts_with(v))
    {
        return Kind::File;
    }
    if ["research", "look into", "more on", "tell me about"].iter().any(|v| t.contains(v)) {
        return Kind::Topic;
    }
    Kind::Any
}

impl Referents {
    /// Best candidate for the given kind, preferring the most specific and
    /// most recent thing available.
    fn best(&self, kind: Kind) -> Option<String> {
        match kind {
            Kind::App => self.last_app.clone().or_else(|| self.active_app.clone()),
            Kind::File => self
                .last_note
                .clone()
                .or_else(|| self.last_file.clone())
                .or_else(|| self.last_capture.clone()),
            Kind::Topic => self.last_topic.clone(),
            Kind::Any => self
                .last_app
                .clone()
                .or_else(|| self.last_file.clone())
                .or_else(|| self.last_note.clone())
                .or_else(|| self.last_topic.clone())
                .or_else(|| self.active_app.clone()),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.best(Kind::Any).is_none()
    }
}

/// Is a command's argument a stand-in for something said earlier ("it",
/// "that one", "those files") rather than a subject of its own?
///
/// 29 Sep 2026: "Research ways to improve in house language models ...
/// allowing it to do better ... put it into a document" was answered "About
/// what?" -- every "it" in a long request counted as a pronoun needing a
/// referent, and with none to hand the whole request became a question. A
/// pronoun stands in for the argument when it leads it or the argument is
/// no more than a few words; inside a sentence of its own it is grammar.
pub fn argument_leans_on_earlier(arg: &str) -> bool {
    let w = words(arg);
    match w.first() {
        None => false,
        Some(first) if PRONOUNS.contains(&first.as_str()) => true,
        Some(_) => w.len() <= 3 && w.iter().any(|x| PRONOUNS.contains(&x.as_str())),
    }
}

/// Is this asking to start (or get on with) research already asked for --
/// "start that research", "do the research I asked for", "get started on
/// the research" -- rather than naming a topic?
pub fn starts_the_research(said: &str) -> bool {
    let t = format!(" {} ", words(said).join(" "));
    let about_research = [" research ", " the research ", " that research ", " researching "].iter().any(|w| t.contains(w));
    let get_going = [" start", " do the ", " do that ", " get started", " begin", " go ahead", " kick off", " get on with", " carry on with"]
        .iter()
        .any(|w| t.contains(w));
    about_research && get_going
}

/// How many words, as the pronoun rules count them.
pub fn word_count(text: &str) -> usize {
    words(text).len()
}

/// Does this text lean on something said earlier?
pub fn has_pronoun(text: &str) -> bool {
    words(text).iter().any(|w| PRONOUNS.contains(&w.as_str()))
}

/// Resolve pronouns against recent context.
pub fn resolve(text: &str, refs: &Referents) -> Resolution {
    if !has_pronoun(text) {
        return Resolution::Unchanged(text.to_string());
    }
    let kind = kind_hint(text);
    let Some(referent) = refs.best(kind).or_else(|| refs.best(Kind::Any)) else {
        return Resolution::Ambiguous(ambiguity_question(text));
    };

    let mut out = Vec::new();
    let mut replaced = false;
    for w in words_keep_case(text) {
        let bare: String = w.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
        if !replaced && PRONOUNS.contains(&bare.as_str()) {
            // "there" is a place, not a thing — "move it there" means the
            // destination, which Atlas cannot infer. Leave it alone.
            if bare == "there" {
                out.push(w);
                continue;
            }
            out.push(referent.clone());
            replaced = true;
            continue;
        }
        out.push(w);
    }

    if !replaced {
        return Resolution::Unchanged(text.to_string());
    }
    Resolution::Resolved { text: out.join(" "), referent }
}

fn ambiguity_question(text: &str) -> String {
    match kind_hint(text) {
        Kind::App => "Which app?".into(),
        Kind::File => "Which file?".into(),
        Kind::Topic => "About what?".into(),
        Kind::Any => "Which one?".into(),
    }
}

fn words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|w| w.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase())
        .collect()
}

fn words_keep_case(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_string).collect()
}
