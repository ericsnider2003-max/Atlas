//! What did you do, and take it back.
//!
//! Atlas now touches files, settings, mail, posts and security pages. Each of
//! those keeps its own record, which is no use at all at the moment you need
//! it — you don't know which area it was in, that's why you're asking.
//!
//! So there's one list, in order, and one way to reverse things. And you don't
//! have to remember a phrase: anything that sounds like the question works,
//! because the moment you need this is the moment you'll be least inclined to
//! recall the right wording.

use serde::{Deserialize, Serialize};

/// Something Atlas did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Did {
    pub id: u64,
    /// In your words, not the module's.
    pub what: String,
    /// Which part of your world it touched.
    pub area: String,
    pub at: u64,
    /// How to take it back, if it can be.
    pub undo: Undo,
    /// Already reversed.
    pub undone: bool,
    /// You asked for it, as opposed to Atlas deciding.
    pub you_asked: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Undo {
    /// Atlas can put it back.
    Atlas(String),
    /// You'd have to, and here's where.
    You(String),
    /// It can't be taken back, and this is why.
    Cannot(String),
}

impl Undo {
    pub fn possible(&self) -> bool {
        !matches!(self, Undo::Cannot(_))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct History {
    pub done: Vec<Did>,
    next_id: u64,
}

impl History {
    pub fn note(&mut self, what: &str, area: &str, undo: Undo, you_asked: bool, now: u64) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.done.push(Did {
            id,
            what: what.into(),
            area: area.into(),
            at: now,
            undo,
            undone: false,
            you_asked,
        });
        if self.done.len() > 2000 {
            self.done.drain(0..500);
        }
        id
    }

    /// Everything since a moment, newest first.
    pub fn since(&self, when: u64) -> Vec<&Did> {
        let mut v: Vec<&Did> = self.done.iter().filter(|d| d.at >= when && !d.undone).collect();
        v.sort_by_key(|d| std::cmp::Reverse(d.at));
        v
    }

    /// The last thing, which is what "undo that" almost always means.
    pub fn last(&self) -> Option<&Did> {
        self.done.iter().rev().find(|d| !d.undone)
    }

    /// The last thing in one area, for "undo what you did to my files".
    pub fn last_in(&self, area: &str) -> Option<&Did> {
        self.done
            .iter()
            .rev()
            .find(|d| !d.undone && d.area.eq_ignore_ascii_case(area))
    }

    pub fn mark_undone(&mut self, id: u64) -> bool {
        match self.done.iter_mut().find(|d| d.id == id) {
            Some(d) => {
                d.undone = true;
                true
            }
            None => false,
        }
    }

    /// Things Atlas did without being asked, which is what you'd want to
    /// review.
    pub fn on_its_own(&self, since: u64) -> Vec<&Did> {
        self.since(since).into_iter().filter(|d| !d.you_asked).collect()
    }
}

/// What you said, understood loosely.
///
/// The moment you need this is the moment you'll be least inclined to remember
/// the right wording, so nearly anything works.
#[derive(Debug, Clone, PartialEq)]
pub enum Asking {
    /// "What have you done?"
    WhatDidYouDo { since_mins: u64 },
    /// "What did you do on your own?" — the review of the unprompted actions,
    /// not the whole list. A different, sharper question.
    WhatOnYourOwn { since_mins: u64 },
    /// "Undo that."
    UndoLast,
    /// "Undo what you did to my files."
    UndoIn(String),
    /// Not about this.
    SomethingElse,
}

const ASKS_WHAT: &[&str] = &[
    "what did you do", "what have you done", "what you been doing", "what did you just do",
    "what changed", "what's changed", "whats changed", "what did you change",
    "what have you changed", "show me what you did", "what did i miss",
    "what was that", "did you do something", "what just happened",
    "what happened", "what've you done", "whatve you done", "run me through",
    "catch me up", "anything change",
];

const ASKS_UNDO: &[&str] = &[
    "undo", "put it back", "revert", "take it back", "change it back", "reverse",
    "unpick", "roll it back", "never mind that", "cancel that", "no go back",
    "go back", "that was wrong", "stop that", "not that",
];

/// Ways of asking, specifically, for the things Atlas did *without being told*
/// — the ones you'd want to look over. Recognised on their own, because they
/// reach `understand` as the remainder after the phrase ("what did you do"
/// leaving "on your own"), where the general list words are gone.
const ASKS_ON_OWN: &[&str] = &[
    "on your own", "without asking", "without me asking", "without being asked",
    "without my say", "didn't ask", "didnt ask", "did not ask", "unprompted",
    "off your own", "by yourself",
];

/// Areas, as you'd name them rather than as the code does.
const AREAS: &[(&str, &str)] = &[
    ("file", "files"), ("folder", "files"), ("document", "files"),
    ("setting", "settings"), ("wallpaper", "settings"), ("desktop", "settings"),
    ("email", "mail"), ("mail", "mail"), ("inbox", "mail"),
    ("post", "posting"), ("posted", "posting"),
    ("window", "windows"), ("screen", "windows"),
    ("password", "security"), ("account", "security"), ("two factor", "security"),
    ("code", "atlas"), ("yourself", "atlas"),
];

pub fn understand(said: &str) -> Asking {
    let t = said.to_lowercase();

    let named_area = AREAS.iter().find(|(word, _)| t.contains(*word));
    let wants_undo = ASKS_UNDO.iter().any(|p| t.contains(p))
        // "Put my wallpaper back" has no undo word in it, but naming a thing
        // and saying "back" is unmistakably the same request.
        || (named_area.is_some() && t.contains(" back"));
    let wants_list = ASKS_WHAT.iter().any(|p| t.contains(p));

    if wants_undo {
        if let Some((_, area)) = named_area {
            return Asking::UndoIn(area.to_string());
        }
        return Asking::UndoLast;
    }
    // "What did you do on your own?" is asked for the unprompted actions in
    // particular. It is checked before the general list because those words
    // are what narrow the answer — a plain "what did you do" is the whole
    // list, this is the subset worth reviewing.
    if ASKS_ON_OWN.iter().any(|p| t.contains(p)) {
        return Asking::WhatOnYourOwn { since_mins: since_from(&t) };
    }
    if wants_list {
        return Asking::WhatDidYouDo { since_mins: since_from(&t) };
    }
    Asking::SomethingElse
}

/// How far back the wording reaches. One reading, so "today" means the same
/// thing to both questions above.
fn since_from(t: &str) -> u64 {
    if t.contains("today") {
        24 * 60
    } else if t.contains("hour") {
        60
    } else if t.contains("week") {
        7 * 24 * 60
    } else if t.contains("while i was") || t.contains("since i") || t.contains("i was out") {
        12 * 60
    } else {
        60
    }
}

/// What Atlas says about what it's done.
///
/// Grouped, because a flat list of forty file moves is not an answer.
pub fn tell(done: &[&Did]) -> String {
    if done.is_empty() {
        return "Nothing.".into();
    }
    let mut by_area: std::collections::BTreeMap<&str, usize> = Default::default();
    for d in done {
        *by_area.entry(d.area.as_str()).or_insert(0) += 1;
    }

    let newest = done[0];
    let mut s = format!("Last thing: {}.", newest.what);

    if done.len() > 1 {
        let groups: Vec<String> = by_area
            .iter()
            .map(|(area, n)| format!("{n} to your {area}"))
            .collect();
        s.push_str(&format!(" Before that, {}.", groups.join(", ")));
    }
    let unasked = done.iter().filter(|d| !d.you_asked).count();
    if unasked > 0 {
        s.push_str(&format!(" {unasked} of those I did on my own."));
    }
    s.push_str(" Say undo and I'll take back the last one.");
    s
}

/// The answer to "undo that".
#[derive(Debug, Clone, PartialEq)]
pub enum Reversal {
    /// Atlas can do it. Confirm first, since undoing can be as wrong as doing.
    CanDo { id: u64, what: String, confirm: String },
    /// You'd have to.
    OverToYou { what: String, where_: String },
    /// It's gone.
    Cannot { what: String, why: String },
    Nothing,
}

pub fn reverse(d: Option<&Did>) -> Reversal {
    let Some(d) = d else {
        return Reversal::Nothing;
    };
    match &d.undo {
        Undo::Atlas(how) => Reversal::CanDo {
            id: d.id,
            what: d.what.clone(),
            // Undoing the wrong thing is its own mistake, and it happens
            // because the last thing Atlas did isn't always the thing you're
            // annoyed about.
            confirm: format!("Undo \"{}\"? That's {how}.", d.what),
        },
        Undo::You(where_) => Reversal::OverToYou {
            what: d.what.clone(),
            where_: where_.clone(),
        },
        Undo::Cannot(why) => Reversal::Cannot {
            what: d.what.clone(),
            why: why.clone(),
        },
    }
}

/// What Atlas says when it can't.
pub fn say(r: &Reversal) -> String {
    match r {
        Reversal::Nothing => "Nothing to undo.".into(),
        Reversal::CanDo { confirm, .. } => confirm.clone(),
        Reversal::OverToYou { what, where_ } => {
            format!("I can't take back \"{what}\" — that one's {where_}.")
        }
        Reversal::Cannot { what, why } => format!("\"{what}\" can't be undone: {why}."),
    }
}
