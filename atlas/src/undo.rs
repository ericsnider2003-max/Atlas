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
    #[serde(default)]
    append_identities: std::collections::BTreeMap<u64, String>,
    #[serde(skip)]
    pending: Vec<HistoryDelta>,
}

#[derive(Debug, Clone)]
enum HistoryDelta { Append(Did, String), Undone(Did, Option<String>) }

impl History {
    pub(crate) fn identity(&self, id: u64) -> Option<&str> { self.append_identities.get(&id).map(String::as_str) }
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
        static NEXT_APPEND: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let identity = format!("{}-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos(), NEXT_APPEND.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
        self.append_identities.insert(id, identity.clone());
        self.pending.push(HistoryDelta::Append(self.done.last().unwrap().clone(), identity));
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
                self.pending.push(HistoryDelta::Undone(d.clone(), self.append_identities.get(&id).cloned()));
                true
            }
            None => false,
        }
    }

    /// Merge only retained local changes into a fresh durable history. A
    /// numeric collision is held for review rather than remapping recovery IDs.
    pub fn save_merged(&mut self, store: &crate::store::Store) -> crate::error::Result<()> {
        if self.pending.is_empty() { return Ok(()); }
        let _guard = store.transaction()?;
        let mut current = store.load_checked::<History>("undo_history")?.unwrap_or_default();
        for delta in &self.pending {
            match delta {
                HistoryDelta::Append(row, identity) => {
                    if let Some(existing) = current.done.iter().find(|item| item.id == row.id) {
                        let known = current.append_identities.get(&row.id);
                        if known != Some(identity) && !(known.is_none() && existing == row) {
                            return Err(crate::error::AtlasError::Platform("history has a conflicting recovery identifier; the pending record was retained for review".into()));
                        }
                    } else {
                        current.done.push(row.clone());
                    }
                    current.append_identities.insert(row.id, identity.clone());
                    current.next_id = current.next_id.max(row.id);
                }
                HistoryDelta::Undone(row, identity) => {
                    let known = current.append_identities.get(&row.id);
                    let existing = current.done.iter_mut().find(|item| item.id == row.id).ok_or_else(|| crate::error::AtlasError::Platform("the recovery row is missing; its pending undo acknowledgment was retained".into()))?;
                    let same = identity.as_ref().is_some_and(|key| known == Some(key)) || (identity.is_none() && existing.what == row.what && existing.area == row.area && existing.at == row.at && existing.undo == row.undo);
                    if !same { return Err(crate::error::AtlasError::Platform("the recovery row changed identity; its pending undo acknowledgment was retained".into())); }
                    existing.undone = true;
                }
            }
        }
        store.save("undo_history", &current)?;
        *self = current;
        Ok(())
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

#[cfg(test)]
mod durable_history_deltas {
    use super::*;
    struct Area(std::path::PathBuf);
    impl Area {
        fn new() -> Self { static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0); Self(std::env::temp_dir().join(format!("atlas-history-delta-{}-{}", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)))) }
        fn store(&self) -> crate::store::Store { crate::store::Store::new(self.0.join("data/state")) }
    }
    impl Drop for Area { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }

    #[test]
    fn undo_acknowledgment_preserves_a_fresh_worker_completion() {
        let area = Area::new(); let store = area.store();
        let mut original = History::default();
        let id = original.note("moving requested", "files", Undo::Atlas("put back".into()), true, 10);
        original.save_merged(&store).unwrap();
        let mut stale = original.clone();
        let mut worker: History = store.load_checked("undo_history").unwrap().unwrap();
        worker.done[0].what = "moved five files".into();
        store.save("undo_history", &worker).unwrap();
        stale.mark_undone(id);
        stale.save_merged(&store).unwrap();
        let saved: History = store.load_checked("undo_history").unwrap().unwrap();
        assert_eq!(saved.done[0].what, "moved five files");
        assert!(saved.done[0].undone);
    }

    #[test]
    fn conflicting_numeric_recovery_ids_are_retained_without_remapping() {
        let area = Area::new(); let store = area.store();
        let mut first = History::default(); let mut other = History::default();
        first.note("first owner's action", "files", Undo::Atlas("first".into()), true, 10);
        other.note("another action", "files", Undo::Atlas("other".into()), true, 11);
        first.save_merged(&store).unwrap();
        assert!(other.save_merged(&store).unwrap_err().to_string().contains("conflicting"));
        assert_eq!(other.pending.len(), 1);
        let saved: History = store.load_checked("undo_history").unwrap().unwrap();
        assert_eq!(saved.done.len(), 1);
        assert_eq!(saved.done[0].what, "first owner's action");
        assert_eq!(other.done[0].id, saved.done[0].id);
    }

    #[test]
    fn backup_contention_retains_append_until_a_successful_retry() {
        let area = Area::new(); let store = area.store();
        store.save("test", &1).unwrap();
        let root = store.root().to_path_buf();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || { let _guard = crate::store::state_transaction(&root).unwrap(); ready_tx.send(()).unwrap(); release_rx.recv().unwrap(); });
        ready_rx.recv().unwrap();
        let mut history = History::default();
        history.note("owner requested action", "settings", Undo::Cannot("completed".into()), true, 10);
        assert!(history.save_merged(&store).is_err());
        assert_eq!(history.pending.len(), 1);
        release_tx.send(()).unwrap(); worker.join().unwrap();
        history.save_merged(&store).unwrap();
        history.save_merged(&store).unwrap();
        assert!(history.pending.is_empty());
        let saved: History = store.load_checked("undo_history").unwrap().unwrap();
        assert_eq!(saved.done.len(), 1);
    }
}
