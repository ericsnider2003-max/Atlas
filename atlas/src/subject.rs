//! Working out what "this" is.
//!
//! You say "explain this". The clipboard is empty. A system that answers
//! "there's nothing on the clipboard" is technically correct and useless —
//! you were obviously looking at something.
//!
//! So "this" is resolved against everything Atlas can see, cheapest first:
//! what you copied, what you have selected, the window in front of you, the
//! file you just opened, the thing you were last talking about. Each source
//! carries a confidence, and when two are equally plausible Atlas asks which
//! rather than picking.

use serde::{Deserialize, Serialize};

/// Something "this" could be pointing at.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Subject {
    Clipboard(String),
    /// Text highlighted in the focused app.
    Selection(String),
    /// The window you're looking at.
    Window { app: String, title: String },
    /// A file, usually the one most recently opened or created.
    File(String),
    /// What you were last discussing.
    Topic(String),
    /// A screenshot or webcam frame Atlas took.
    Capture(String),
}

impl Subject {
    /// How Atlas refers to it out loud.
    pub fn name(&self) -> String {
        match self {
            Subject::Clipboard(_) => "what you copied".into(),
            Subject::Selection(_) => "what you've got selected".into(),
            Subject::Window { app, title } => {
                if title.trim().is_empty() {
                    app.clone()
                } else {
                    format!("{title} in {app}")
                }
            }
            Subject::File(p) => std::path::Path::new(p)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| p.clone()),
            Subject::Topic(t) => t.clone(),
            Subject::Capture(_) => "what's on your screen".into(),
        }
    }

    /// The text to hand the model, if there is any.
    pub fn text(&self) -> Option<&str> {
        match self {
            Subject::Clipboard(t) | Subject::Selection(t) => Some(t),
            _ => None,
        }
    }
}

/// Everything Atlas can currently see. Any of it may be missing.
#[derive(Debug, Clone, Default)]
pub struct Candidates {
    pub clipboard: Option<String>,
    pub selection: Option<String>,
    pub focused_app: Option<String>,
    pub focused_title: Option<String>,
    /// Seconds since the focused window changed. A window you just switched
    /// to is more likely to be what you mean.
    pub dwell_secs: u64,
    /// Most recently modified file in a watched folder, and how long ago.
    pub recent_file: Option<(String, u64)>,
    pub last_topic: Option<String>,
    pub last_capture: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Resolution {
    /// Confident enough to act.
    Found { subject: Subject, why: String },
    /// Two or more plausible things. Ask rather than guess.
    Ambiguous { options: Vec<Subject>, question: String },
    /// Nothing to point at.
    Nothing(String),
}

/// A hint in the wording about what kind of thing is meant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wants {
    Text,
    File,
    Screen,
    Any,
}

pub fn wants(said: &str) -> Wants {
    let t = said.to_lowercase();
    if ["read", "summarise", "summarize", "explain", "translate", "reply to", "rewrite"]
        .iter()
        .any(|v| t.contains(v))
        && ["file", "document", "pdf", "doc"].iter().any(|v| t.contains(v))
    {
        return Wants::File;
    }
    if ["on my screen", "on screen", "what i'm looking at", "what im looking at", "this window"]
        .iter()
        .any(|v| t.contains(v))
    {
        return Wants::Screen;
    }
    if ["file", "document", "pdf", "spreadsheet"].iter().any(|v| t.contains(v)) {
        return Wants::File;
    }
    if ["explain", "summarise", "summarize", "translate", "reply", "rewrite", "fix"]
        .iter()
        .any(|v| t.contains(v))
    {
        return Wants::Text;
    }
    Wants::Any
}

/// Resolve "this".
///
/// Ordering is deliberate. Selecting something is the most explicit act, so it
/// beats a clipboard that might hold a password you copied an hour ago.
pub fn resolve(said: &str, c: &Candidates) -> Resolution {
    let want = wants(said);
    let mut scored: Vec<(f32, Subject, &str)> = Vec::new();

    if let Some(sel) = c.selection.as_ref().filter(|s| !s.trim().is_empty()) {
        // You highlighted it. Nothing is more explicit than that.
        scored.push((0.95, Subject::Selection(sel.clone()), "you've got it selected"));
    }
    if let Some(clip) = c.clipboard.as_ref().filter(|s| !s.trim().is_empty()) {
        scored.push((0.75, Subject::Clipboard(clip.clone()), "you copied it"));
    }
    if let Some((path, age)) = &c.recent_file {
        // A file saved in the last couple of minutes is very likely the one.
        let score = if *age < 120 { 0.8 } else if *age < 1800 { 0.5 } else { 0.25 };
        scored.push((score, Subject::File(path.clone()), "you just saved it"));
    }
    if let Some(app) = &c.focused_app {
        // Freshly switched to means you're probably referring to it. Sitting
        // in the same window all afternoon is weaker evidence.
        let score = if c.dwell_secs < 60 { 0.7 } else { 0.45 };
        scored.push((
            score,
            Subject::Window {
                app: app.clone(),
                title: c.focused_title.clone().unwrap_or_default(),
            },
            "it's what's in front of you",
        ));
    }
    if let Some(t) = &c.last_topic {
        scored.push((0.35, Subject::Topic(t.clone()), "it's what we were on"));
    }
    if let Some(cap) = &c.last_capture {
        scored.push((0.3, Subject::Capture(cap.clone()), "it's what I last looked at"));
    }

    // The wording pushes one kind up and the others down.
    for (score, subject, _) in scored.iter_mut() {
        let matches_want = matches!((want, &*subject), (Wants::Text, Subject::Clipboard(_) | Subject::Selection(_)) | (Wants::File, Subject::File(_)) | (Wants::Screen, Subject::Window { .. } | Subject::Capture(_)) | (Wants::Any, _));
        if matches_want {
            *score += 0.15;
        } else if want != Wants::Any {
            *score -= 0.3;
        }
    }

    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.retain(|(s, _, _)| *s > 0.2);

    match scored.split_first() {
        None => Resolution::Nothing(
            "I can't tell what you mean — nothing copied, nothing selected, nothing open.".into(),
        ),
        Some(((best, subject, why), rest)) => {
            // Two things nearly as likely as each other is not a guess worth
            // making silently.
            if let Some((second, other, _)) = rest.first() {
                if best - second < 0.12 {
                    return Resolution::Ambiguous {
                        question: format!("{} or {}?", subject.name(), other.name()),
                        options: vec![subject.clone(), other.clone()],
                    };
                }
            }
            Resolution::Found { subject: subject.clone(), why: why.to_string() }
        }
    }
}

/// What Atlas says as it acts, so a wrong guess is obvious immediately.
pub fn confirm(r: &Resolution) -> String {
    match r {
        Resolution::Found { subject, why } => format!("Taking {} — {why}.", subject.name()),
        Resolution::Ambiguous { question, .. } => question.clone(),
        Resolution::Nothing(why) => why.clone(),
    }
}
