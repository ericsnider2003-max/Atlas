//! What Atlas did.
//!
//! An assistant that acts while you are away needs to be able to account for
//! it. Without this, coming back to a changed workspace means guessing what
//! happened — which is the fastest way to stop trusting it with anything.
//!
//! Deliberately not a debug log. Bounded, plain language, and the things that
//! actually changed the world are marked so a summary can lead with them.

use crate::error::Result;
use crate::store::{now, Store};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// You asked for something.
    Asked,
    /// A scheduled job ran.
    Scheduled,
    /// Something went out into the world.
    Published,
    /// Atlas couldn't, and filed it.
    Blocked,
    /// Atlas offered something.
    Offered,
    /// Tidying, reaping, compaction.
    Upkeep,
}

impl Kind {
    /// Did this change something outside Atlas?
    pub fn consequential(&self) -> bool {
        matches!(self, Kind::Published | Kind::Scheduled)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub at: u64,
    pub kind: Kind,
    /// One plain line. This is read aloud, so no jargon and no paths.
    pub what: String,
    #[serde(default)]
    pub ok: bool,
    /// Its place in the whole history, counting ones since dropped.
    #[serde(default)]
    pub seq: u64,
    /// The seal of the entry before it (or of what was dropped before it).
    #[serde(default)]
    pub prev: String,
    /// This entry's seal: a SHA-256 over `prev` and everything above. Empty
    /// on entries written before the log was sealed.
    #[serde(default)]
    pub hash: String,
}

impl Event {
    /// What `hash` should be, from the entry's own fields.
    pub fn seal(&self) -> String {
        let kind = serde_json::to_string(&self.kind).unwrap_or_default();
        let line = format!("{}\n{}\n{}\n{}\n{}\n{}", self.prev, self.seq, self.at, kind, self.ok, self.what);
        crate::digest::sha256_hex(line.as_bytes())
    }
}

/// What checking the log's seals found.
#[derive(Debug, Clone, PartialEq)]
pub enum Sealed {
    /// Every sealed entry checks out, and so does every recorded head that
    /// still falls inside the log.
    Intact { sealed: usize, head: String },
    /// The first place it doesn't, said plainly.
    Broken { at: u64, why: String },
}

const MAX: usize = 400;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Journal {
    pub events: Vec<Event>,
    /// Every entry's hash in a Merkle log, and a checkpoint a day
    /// (`sealedlog`, RFC 6962). The events above are trimmed to the last
    /// 400; the seal is not, which is what lets a check tell an entry that
    /// aged out from one that was edited.
    #[serde(default)]
    seal: Seal,
    /// The seal of the last entry dropped off the front, so the first kept
    /// one still has something to chain to.
    #[serde(default)]
    pub base: String,
    #[serde(default)]
    pub next_seq: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Seal {
    leaves: Vec<String>,
    /// (when, "<size> <root>") — one a day, the last 30.
    checkpoints: Vec<(u64, String)>,
}

/// What an entry is, for sealing. Every field that says what happened; none
/// that is allowed to change later.
fn canonical(e: &Event) -> String {
    format!("{}|{:?}|{}|{}", e.at, e.kind, e.ok, e.what)
}

impl Journal {
    pub fn load(store: &Store) -> Journal {
        store.load("activity")
    }
    /// Saved, and its head written to the anchor file beside it
    /// (`anchor_path`) when it has moved: a line nothing ever rewrites, so a
    /// log rewritten wholesale — seals and all — no longer matches it.
    pub fn save(&self, store: &Store) -> Result<()> {
        store.save("activity", self)?;
        if let Some(last) = self.events.last().filter(|e| !e.hash.is_empty()) {
            let path = anchor_path(store.root());
            let already = std::fs::read_to_string(&path)
                .ok()
                .and_then(|t| t.lines().last().map(|l| l.contains(&last.hash)))
                .unwrap_or(false);
            if !already {
                use std::io::Write;
                let line = serde_json::json!({ "seq": last.seq, "hash": last.hash, "at": last.at }).to_string();
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
                    crate::kept!(writeln!(f, "{line}"));
                }
            }
        }
        Ok(())
    }

    pub fn record(&mut self, kind: Kind, what: &str, ok: bool) {
        self.record_at(kind, what, ok, now());
    }

    /// Adds the entry and seals it twice, once each way both chats built
    /// (merged 26 Sep 2026):
    ///
    /// - chained to the entry before, with a running number, so changing or
    ///   removing any entry breaks every seal after it; the heads are
    ///   recorded beside the log and copied into every backup (`check`);
    /// - a leaf in a Merkle log with a checkpoint a day (RFC 6962), which
    ///   proves today's log extends every earlier day's (`verify_seal`).
    pub fn record_at(&mut self, kind: Kind, what: &str, ok: bool, t: u64) {
        // A journal written before the Merkle seal existed is sealed as it
        // stands the first time something is added — from then on, nothing
        // in it can change unnoticed.
        if self.seal.leaves.is_empty() {
            let old: Vec<String> =
                self.events.iter().map(|e| crate::sealedlog::hex(&crate::sealedlog::leaf_hash(canonical(e).as_bytes()))).collect();
            self.seal.leaves = old;
        }
        let prev = self.events.last().map(|e| e.hash.clone()).unwrap_or_else(|| self.base.clone());
        let seq = self.next_seq.max(self.events.last().map(|e| e.seq + 1).unwrap_or(0));
        let mut e = Event { at: t, kind, what: what.to_string(), ok, seq, prev, hash: String::new() };
        e.hash = e.seal();
        self.next_seq = seq + 1;
        self.seal.leaves.push(crate::sealedlog::hex(&crate::sealedlog::leaf_hash(canonical(&e).as_bytes())));
        self.events.push(e);
        if self.events.len() > MAX {
            let drop = self.events.len() - MAX;
            if let Some(last_dropped) = self.events.get(drop - 1) {
                self.base = last_dropped.hash.clone();
            }
            self.events.drain(0..drop);
        }
        let due = self.seal.checkpoints.last().map_or(true, |(at, _)| t >= at + 86_400);
        if due {
            let leaves: Vec<crate::sealedlog::Hash> =
                self.seal.leaves.iter().filter_map(|h| crate::sealedlog::unhex(h)).collect();
            let cp = crate::sealedlog::Log::from_leaves(leaves).checkpoint();
            self.seal.checkpoints.push((t, cp.to_text()));
            if self.seal.checkpoints.len() > 30 {
                self.seal.checkpoints.remove(0);
            }
        }
    }

    /// Is this still the record Atlas wrote? Every entry must still hash to
    /// its leaf, and today's log must extend every daily checkpoint — proven
    /// by an RFC 6962 consistency proof, not by trusting the file. `Err`
    /// names the first thing that does not hold.
    pub fn verify_seal(&self) -> std::result::Result<String, String> {
        if self.seal.leaves.is_empty() {
            return Ok(if self.events.is_empty() {
                "nothing recorded yet".into()
            } else {
                "not sealed yet — sealing starts with the next thing Atlas does".into()
            });
        }
        let mut leaves = Vec::with_capacity(self.seal.leaves.len());
        for (i, h) in self.seal.leaves.iter().enumerate() {
            leaves.push(crate::sealedlog::unhex(h).ok_or_else(|| format!("seal entry {i} is not a hash"))?);
        }
        if leaves.len() < self.events.len() {
            return Err(format!(
                "{} entries but only {} seals — entries were added without going through the journal",
                self.events.len(),
                leaves.len()
            ));
        }
        let base = leaves.len() - self.events.len();
        for (i, e) in self.events.iter().enumerate() {
            if crate::sealedlog::leaf_hash(canonical(e).as_bytes()) != leaves[base + i] {
                return Err(format!("\"{}\" was changed after it was written", e.what));
            }
        }
        let log = crate::sealedlog::Log::from_leaves(leaves);
        let now = log.checkpoint();
        for (at, text) in &self.seal.checkpoints {
            let old = crate::sealedlog::Checkpoint::from_text(text).ok_or_else(|| format!("checkpoint from {at} is unreadable"))?;
            let ok = log
                .consistency_proof(old.size, log.len())
                .is_some_and(|p| crate::sealedlog::verify_consistency(&old, &now, &p));
            if !ok {
                return Err(format!(
                    "the record was rewritten after {} — it no longer extends that day's checkpoint",
                    crate::digest::iso_utc(*at)
                ));
            }
        }
        Ok(format!(
            "{} entries sealed; extends all {} daily checkpoint{}",
            log.len(),
            self.seal.checkpoints.len(),
            if self.seal.checkpoints.len() == 1 { "" } else { "s" }
        ))
    }

    /// Check every seal, the chain between them, and the heads recorded in
    /// the anchor file. Entries from before the log was sealed are skipped,
    /// and said so by the count.
    pub fn check(&self, anchors: &[(u64, String)]) -> Sealed {
        let mut prev: Option<&Event> = None;
        let mut sealed = 0;
        for e in &self.events {
            if e.hash.is_empty() {
                prev = None;
                continue;
            }
            if e.seal() != e.hash {
                return Sealed::Broken { at: e.at, why: format!("entry {} was changed after it was written", e.seq) };
            }
            match prev {
                Some(p) if e.prev != p.hash || e.seq != p.seq + 1 => {
                    return Sealed::Broken {
                        at: e.at,
                        why: format!("something between entries {} and {} was removed or reordered", p.seq, e.seq),
                    };
                }
                None if sealed == 0 && !self.base.is_empty() && e.prev != self.base => {
                    return Sealed::Broken { at: e.at, why: "the start of the log doesn't match what was dropped before it".into() };
                }
                _ => {}
            }
            sealed += 1;
            prev = Some(e);
        }
        let first = self.events.iter().find(|e| !e.hash.is_empty()).map(|e| e.seq).unwrap_or(u64::MAX);
        let last = self.events.last().map(|e| e.seq).unwrap_or(0);
        for (seq, hash) in anchors {
            if *seq < first {
                // Only the newest `MAX` entries are ever kept, so a recorded
                // head can fall off the front — but only once `MAX` more have
                // been written after it. A log that starts after a head that
                // should still be in it didn't lose it by pruning: it was
                // replaced, numbered to skip past what was recorded.
                if last.saturating_sub(*seq) < MAX as u64 && first != u64::MAX {
                    return Sealed::Broken {
                        at: 0,
                        why: format!("entry {seq} was recorded and should still be here, but the log starts at {first}"),
                    };
                }
                continue;
            }
            match self.events.iter().find(|e| e.seq == *seq) {
                Some(e) if &e.hash == hash => {}
                Some(e) => {
                    return Sealed::Broken { at: e.at, why: format!("entry {seq} isn't the one that was recorded when it was written") };
                }
                None if *seq > last => {
                    return Sealed::Broken { at: 0, why: format!("entries up to {seq} were recorded, but the log now ends at {last}") };
                }
                None => {}
            }
        }
        Sealed::Intact { sealed, head: self.events.last().map(|e| e.hash.clone()).unwrap_or_default() }
    }

    pub fn since(&self, t: u64) -> Vec<&Event> {
        self.events.iter().filter(|e| e.at >= t).collect()
    }

    /// "While you were away…"
    ///
    /// Leads with what changed the world, mentions failures explicitly, and
    /// collapses upkeep into nothing — you do not need to hear about log
    /// rotation.
    pub fn brief(&self, since: u64) -> String {
        let events = self.since(since);
        let notable: Vec<&&Event> = events.iter().filter(|e| e.kind != Kind::Upkeep).collect();
        if notable.is_empty() {
            return "Nothing happened while you were away.".into();
        }

        let published: Vec<&&&Event> =
            notable.iter().filter(|e| e.kind == Kind::Published && e.ok).collect();
        let ran = notable.iter().filter(|e| e.kind == Kind::Scheduled && e.ok).count();
        let failed: Vec<&&&Event> = notable.iter().filter(|e| !e.ok).collect();
        let blocked = notable.iter().filter(|e| e.kind == Kind::Blocked).count();

        let mut parts = Vec::new();
        // Irreversible things first — that is what you most need to know.
        for p in published.iter().take(3) {
            parts.push(p.what.clone());
        }
        if published.len() > 3 {
            parts.push(format!("and {} more posts", published.len() - 3));
        }
        if ran > 0 {
            parts.push(format!("ran {ran} scheduled job{}", plural(ran)));
        }
        if blocked > 0 {
            parts.push(format!("{blocked} thing{} went on your list", plural(blocked)));
        }
        for f in failed.iter().take(2) {
            parts.push(format!("{} failed", f.what));
        }
        if parts.is_empty() {
            return "Nothing worth reporting.".into();
        }
        format!("While you were away: {}.", parts.join(", "))
    }

    /// Everything that changed the outside world, for an audit.
    pub fn consequential(&self, since: u64) -> Vec<&Event> {
        self.since(since).into_iter().filter(|e| e.kind.consequential()).collect()
    }

    pub fn last(&self) -> Option<&Event> {
        self.events.last()
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// Where the log's heads are recorded, one line each, beside the log.
pub fn anchor_path(dir: &std::path::Path) -> std::path::PathBuf {
    dir.join("activity-anchors.jsonl")
}

/// The recorded heads: (entry number, seal).
pub fn anchors(dir: &std::path::Path) -> Vec<(u64, String)> {
    std::fs::read_to_string(anchor_path(dir))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| Some((v.get("seq")?.as_u64()?, v.get("hash")?.as_str()?.to_string())))
        .collect()
}

/// Every head recorded, here and in each backup, without repeats — so the log
/// is checked against copies that were made before anyone could have touched
/// the laptop's own. Returns the heads and how many backups had some.
///
/// Eric, 25 Sep 2026: "Log in the backup." A backup copies the state folder,
/// anchors included, and a backup's anchors are a record the live file can't
/// overwrite after the fact.
fn heads_with_backups(state: &std::path::Path, backups: &[std::path::PathBuf]) -> (Vec<(u64, String)>, usize) {
    let mut all = anchors(state);
    let mut from = 0;
    for b in backups {
        let theirs = anchors(b);
        if !theirs.is_empty() {
            from += 1;
        }
        for h in theirs {
            if !all.contains(&h) {
                all.push(h);
            }
        }
    }
    all.sort();
    (all, from)
}

/// The log checked against its own heads and every backup's: the check
/// `atlas journal check`, doctor and start-up all run. Returns the finding
/// and how many backups carried heads to check against.
pub fn check_with_backups(j: &Journal, state: &std::path::Path, backup: &crate::safety::BackupConfig) -> (Sealed, usize) {
    let dirs: Vec<std::path::PathBuf> = crate::safety::list_backups(backup).into_iter().map(|b| b.path).collect();
    let (heads, from) = heads_with_backups(state, &dirs);
    (j.check(&heads), from)
}

/// `said`, plus what it was checked against.
///
/// Whether Atlas's record of what it did is the one it wrote, said plainly.
pub fn said_with_backups(s: &Sealed, backups: usize) -> String {
    let mut out = match s {
        Sealed::Intact { sealed: 0, .. } => "The activity log has nothing sealed in it yet, so there's nothing to check.".to_string(),
        Sealed::Intact { sealed, head } => format!(
            "The activity log is intact: {sealed} sealed entr{}, each chained to the one before, last seal {}.",
            if *sealed == 1 { "y" } else { "ies" },
            head.chars().take(12).collect::<String>()
        ),
        Sealed::Broken { why, .. } => format!(
            "The activity log has been altered: {why}. What it says about that stretch can't be trusted."
        ),
    };
    if matches!(s, Sealed::Intact { sealed, .. } if *sealed > 0) {
        out.push_str(&match backups {
            0 => " No backup has a copy of its heads yet, so this checks it only against the ones kept beside it.".to_string(),
            1 => " Checked against the heads kept beside it and in 1 backup.".to_string(),
            n => format!(" Checked against the heads kept beside it and in {n} backups."),
        });
    }
    out
}

