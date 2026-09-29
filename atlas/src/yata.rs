//! A shared document two devices can both edit while apart, that comes back
//! together without a conflict to settle.
//!
//! **Sources:** Nicolaescu, Jahns, Derntl & Klamma (2016), *Near Real-Time
//! Peer-to-Peer Shared Editing on Extensible Data Types* (the YATA paper),
//! and the integration loop as `yjs/yjs` (MIT, `src/structs/Item.js`,
//! `integrate`) implements it: every character carries a unique id and the
//! ids of its left and right neighbours when it was typed; a concurrent
//! insert at the same place is ordered by those origins and then by device,
//! so every replica arrives at the same text whatever order the edits came
//! in. Deletes leave a tombstone. Clean-room, characters only.
//!
//! **Why Atlas wants it.** Sync (`sync`) settles a clash on a *field* by
//! asking you — right for "the meeting is at 3" versus "at 4", wrong for a
//! page of notes both machines added to while apart, where there is nothing
//! to choose between: both edits should simply be there. The ops travel as
//! ordinary `What::Captured` events (they can never clash), so the sync
//! format does not change.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Hash)]
pub struct Id {
    pub site: String,
    pub clock: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Op {
    Insert { id: Id, left: Option<Id>, right: Option<Id>, ch: char },
    Delete { id: Id },
}

#[derive(Debug, Clone)]
struct Item {
    id: Id,
    left: Option<Id>,
    right: Option<Id>,
    ch: char,
    deleted: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Doc {
    pub site: String,
    clock: u64,
    items: Vec<Item>,
    /// Ops whose neighbours haven't arrived yet; tried again after each op.
    waiting: Vec<Op>,
}

impl Doc {
    pub fn new(site: &str) -> Doc {
        Doc { site: site.to_string(), ..Default::default() }
    }

    pub fn text(&self) -> String {
        self.items.iter().filter(|i| !i.deleted).map(|i| i.ch).collect()
    }

    fn index_of(&self, id: &Id) -> Option<usize> {
        self.items.iter().position(|i| &i.id == id)
    }

    fn visible_index(&self, n: usize) -> Option<usize> {
        self.items.iter().enumerate().filter(|(_, i)| !i.deleted).nth(n).map(|(k, _)| k)
    }

    /// Apply an op from anywhere — this device or another. Idempotent, and
    /// order-tolerant: an op that arrives before its neighbours waits.
    pub fn apply(&mut self, op: Op) {
        if !self.try_apply(&op) {
            if !self.waiting.contains(&op) {
                self.waiting.push(op);
            }
            return;
        }
        // Anything waiting on what just landed.
        loop {
            let before = self.waiting.len();
            let pending = std::mem::take(&mut self.waiting);
            for o in pending {
                if !self.try_apply(&o) {
                    self.waiting.push(o);
                }
            }
            if self.waiting.len() == before {
                break;
            }
        }
    }

    fn try_apply(&mut self, op: &Op) -> bool {
        match op {
            Op::Delete { id } => match self.index_of(id) {
                Some(k) => {
                    self.items[k].deleted = true;
                    true
                }
                None => false,
            },
            Op::Insert { id, left, right, ch } => {
                if self.index_of(id).is_some() {
                    return true; // seen it
                }
                let l = match left {
                    Some(x) => match self.index_of(x) {
                        Some(k) => k as isize,
                        None => return false,
                    },
                    None => -1,
                };
                let r = match right {
                    Some(x) => match self.index_of(x) {
                        Some(k) => k,
                        None => return false,
                    },
                    None => self.items.len(),
                };
                if id.site == self.site {
                    self.clock = self.clock.max(id.clock);
                }
                // YATA: walk the items between the origins; an item that
                // shares our left origin goes first if its device sorts
                // lower; an item whose left origin lies inside the scanned
                // run belongs to that earlier insert and is stepped over.
                let mut dest = (l + 1) as usize;
                let mut scanned: Vec<Id> = Vec::new();
                let mut conflicting: Vec<Id> = Vec::new();
                let mut o = (l + 1) as usize;
                while o < r {
                    let it = &self.items[o];
                    scanned.push(it.id.clone());
                    conflicting.push(it.id.clone());
                    if it.left == *left {
                        if it.id.site < id.site {
                            dest = o + 1;
                            conflicting.clear();
                        } else if it.right == *right {
                            break;
                        }
                    } else if it.left.as_ref().is_some_and(|x| scanned.contains(x)) {
                        if !it.left.as_ref().is_some_and(|x| conflicting.contains(x)) {
                            dest = o + 1;
                            conflicting.clear();
                        }
                    } else {
                        break;
                    }
                    o += 1;
                }
                self.items.insert(dest, Item { id: id.clone(), left: left.clone(), right: right.clone(), ch: *ch, deleted: false });
                true
            }
        }
    }

    fn next_id(&mut self) -> Id {
        self.clock += 1;
        Id { site: self.site.clone(), clock: self.clock }
    }

    /// Type `s` at visible position `at`. Returns the ops to send.
    pub fn insert(&mut self, at: usize, s: &str) -> Vec<Op> {
        let mut ops = Vec::new();
        let mut after: Option<usize> = if at == 0 { None } else { self.visible_index(at - 1) };
        for ch in s.chars() {
            let left = after.map(|k| self.items[k].id.clone());
            let right_idx = after.map(|k| k + 1).unwrap_or(0);
            let right = self.items.get(right_idx).map(|i| i.id.clone());
            let id = self.next_id();
            let op = Op::Insert { id: id.clone(), left, right, ch };
            self.apply(op.clone());
            after = self.index_of(&id);
            ops.push(op);
        }
        ops
    }

    /// Remove `len` visible characters starting at `at`.
    pub fn delete(&mut self, at: usize, len: usize) -> Vec<Op> {
        let ids: Vec<Id> = (at..at + len).filter_map(|n| self.visible_index(n)).map(|k| self.items[k].id.clone()).collect();
        ids.into_iter()
            .map(|id| {
                let op = Op::Delete { id };
                self.apply(op.clone());
                op
            })
            .collect()
    }

    /// Make the text read `new`, with the fewest character inserts and
    /// deletes (`diff`), so an edit made by rewriting the whole page merges
    /// as the few characters that actually changed.
    pub fn set_text(&mut self, new: &str) -> Vec<Op> {
        let old: Vec<char> = self.text().chars().collect();
        let neu: Vec<char> = new.chars().collect();
        let mut ops = Vec::new();
        // Walk the script with a cursor into the current visible text.
        let mut pos = 0usize;
        let script = crate::diff::edits(&old, &neu);
        let mut k = 0;
        while k < script.len() {
            match script[k] {
                crate::diff::Edit::Keep(..) => {
                    pos += 1;
                    k += 1;
                }
                crate::diff::Edit::Delete(_) => {
                    let mut n = 0;
                    while k < script.len() && matches!(script[k], crate::diff::Edit::Delete(_)) {
                        n += 1;
                        k += 1;
                    }
                    ops.extend(self.delete(pos, n));
                }
                crate::diff::Edit::Insert(_) => {
                    let mut s = String::new();
                    while k < script.len() {
                        if let crate::diff::Edit::Insert(j) = script[k] {
                            s.push(neu[j]);
                            k += 1;
                        } else {
                            break;
                        }
                    }
                    let n = s.chars().count();
                    ops.extend(self.insert(pos, &s));
                    pos += n;
                }
            }
        }
        ops
    }

    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }
}

/// The sync id an op travels under: `doc:<name>:<site>:<clock>[:x]`.
fn event_id(doc: &str, op: &Op) -> String {
    match op {
        Op::Insert { id, .. } => format!("doc:{doc}:{}:{}", id.site, id.clock),
        Op::Delete { id } => format!("doc:{doc}:{}:{}:x", id.site, id.clock),
    }
}

/// A document rebuilt from the sync log: every op for `name`, in log order.
pub fn from_log(name: &str, site: &str, events: &[crate::sync::Event]) -> Doc {
    let mut d = Doc::new(site);
    let prefix = format!("doc:{name}:");
    let mut evs: Vec<&crate::sync::Event> = events.iter().collect();
    evs.sort_by(|a, b| crate::sync::effective_stamp(a).cmp(&crate::sync::effective_stamp(b)).then(a.device.cmp(&b.device)).then(a.seq.cmp(&b.seq)));
    for e in evs {
        if let crate::sync::What::Captured { id, text } = &e.what {
            if id.starts_with(&prefix) {
                if let Ok(op) = serde_json::from_str::<Op>(text) {
                    d.apply(op);
                }
            }
        }
    }
    d
}

/// Every document name in the log.
fn names_in(events: &[crate::sync::Event]) -> Vec<String> {
    let mut v: Vec<String> = events
        .iter()
        .filter_map(|e| match &e.what {
            crate::sync::What::Captured { id, .. } => id.strip_prefix("doc:").and_then(|r| r.split(':').next()).map(String::from),
            _ => None,
        })
        .collect();
    v.sort();
    v.dedup();
    v
}

/// Record ops in the sync log so they travel with the next bundle.
fn record(log: &mut crate::sync::Log, doc: &str, ops: &[Op], at: u64) {
    for op in ops {
        if let Ok(text) = serde_json::to_string(op) {
            log.append(crate::sync::What::Captured { id: event_id(doc, op), text }, at);
        }
    }
}

// ---------------------------------------------------------------- the inbox

/// Edits made from the command line while the running Atlas holds the sync
/// log. The log has one writer — the daemon, which keeps it in memory and
/// saves it whole — so a second writer's save would be overwritten on the
/// daemon's next one and the edit lost without a word. Instead each edit is
/// a small file here, and the daemon takes them in on its next tick.
pub fn queue(dir: &std::path::Path, doc: &str, ops: &[Op]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let body = serde_json::to_string(&(doc, ops)).map_err(std::io::Error::other)?;
    // Written whole, then renamed, so the daemon never reads half a file.
    let tmp = dir.join(format!(".{stamp}-{}.tmp", std::process::id()));
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, dir.join(format!("{stamp}-{}.json", std::process::id())))
}

fn queued(dir: &std::path::Path) -> Vec<(std::path::PathBuf, String, Vec<Op>)> {
    let Ok(entries) = std::fs::read_dir(dir) else { return vec![] };
    let mut files: Vec<std::path::PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    files.sort();
    files
        .into_iter()
        .filter_map(|p| {
            let text = std::fs::read_to_string(&p).ok()?;
            let (doc, ops): (String, Vec<Op>) = serde_json::from_str(&text).ok()?;
            Some((p, doc, ops))
        })
        .collect()
}

/// Take queued edits into the log. Returns the files taken, to be removed
/// once the log with them in it has been saved — not before, so a crash in
/// between repeats an edit (harmless: ops are idempotent) rather than losing it.
pub fn take_queued(dir: &std::path::Path, log: &mut crate::sync::Log, at: u64) -> Vec<std::path::PathBuf> {
    let mut taken = Vec::new();
    for (path, doc, ops) in queued(dir) {
        record(log, &doc, &ops, at);
        taken.push(path);
    }
    taken
}

/// A document as it stands: the log, plus edits still waiting in the inbox.
pub fn current(name: &str, site: &str, events: &[crate::sync::Event], inbox: &std::path::Path) -> Doc {
    let mut d = from_log(name, site, events);
    for (_, doc, ops) in queued(inbox) {
        if doc == name {
            for op in ops {
                d.apply(op);
            }
        }
    }
    d
}

/// Names of every document, in the log or waiting in the inbox.
pub fn all_names(events: &[crate::sync::Event], inbox: &std::path::Path) -> Vec<String> {
    let mut v = names_in(events);
    v.extend(queued(inbox).into_iter().map(|(_, d, _)| d));
    v.sort();
    v.dedup();
    v
}
