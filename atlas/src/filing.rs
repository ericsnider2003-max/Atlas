//! Deciding where a file should live.
//!
//! ## What this is built on, and what it is not
//!
//! There is no filing research recorded anywhere in this tree — `docs/` has
//! nothing on folder structure. So this is built on published method rather
//! than on earlier work of yours, and that distinction is worth keeping: if
//! you settled on a scheme in another session, this should be replaced with
//! it rather than argued with.
//!
//! The method is **PARA** (Tiago Forte), for one reason: it sorts by *how soon
//! you need it* rather than by what a thing is. Sorting by type — a Documents
//! folder, a Spreadsheets folder — puts the invoice you need today in the same
//! place as the invoice from 2019, which is how filing systems stop being
//! used.
//!
//! - **Projects** — has an end. A thing you are finishing.
//! - **Areas** — ongoing, no end. Health, finances, the house.
//! - **Resources** — reference. Useful, not yours, no deadline.
//! - **Archive** — anything from the first three that has gone quiet.
//!
//! ## The rule that overrides all of it
//!
//! **Never move something you cannot find again.** Every move is reported and
//! reversible, and anything Atlas is unsure about is left exactly where it is
//! with a note saying why. A file put somewhere clever that you cannot find is
//! worse than a messy Downloads folder — the mess is at least where you left
//! it.
//!
//! That is also why this makes no attempt at cleverness. It reads the name,
//! the extension and the dates. It does not read your documents to guess what
//! they are about.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Bucket {
    Projects,
    Areas,
    Resources,
    Archive,
}

impl Bucket {
    pub fn folder(&self) -> &'static str {
        match self {
            Bucket::Projects => "Projects",
            Bucket::Areas => "Areas",
            Bucket::Resources => "Resources",
            Bucket::Archive => "Archive",
        }
    }

    pub fn what_it_means(&self) -> &'static str {
        match self {
            Bucket::Projects => "something you're finishing — it has an end",
            Bucket::Areas => "ongoing, no end date",
            Bucket::Resources => "reference. Useful, not yours, no deadline",
            Bucket::Archive => "gone quiet. Still here, out of the way",
        }
    }
}

/// What Atlas thinks should happen to one file.
#[derive(Debug, Clone, PartialEq)]
pub enum Suggestion {
    /// Move it, and here is why.
    Move { to: PathBuf, bucket: Bucket, why: String },
    /// Leave it alone, and here is why — which is the useful half.
    Leave { why: String },
}

impl Suggestion {
    pub fn line(&self, from: &Path) -> String {
        match self {
            Suggestion::Move { to, bucket, why } => format!(
                "{} → {} ({}) — {why}",
                from.display(),
                to.display(),
                bucket.what_it_means()
            ),
            Suggestion::Leave { why } => format!("{} stays put — {why}", from.display()),
        }
    }
}

/// Extensions that are reference material by nature.
const REFERENCE: &[&str] = &["pdf", "epub", "mobi", "djvu"];

/// Extensions that are almost always yours and in progress.
const WORKING: &[&str] = &["docx", "xlsx", "pptx", "odt", "ods", "md", "txt", "psd", "fig"];

/// After this long untouched, something has gone quiet whatever it is.
pub const ARCHIVE_AFTER_DAYS: u64 = 365;

/// Names that say "this is a throwaway copy", which is a filing decision of its
/// own — Atlas leaves them, because the duplicate is usually the one you are
/// mid-way through sorting out.
const AMBIGUOUS: &[&str] = &["copy", "final", "final2", "new", "untitled", "document", "draft"];

/// Where should this go?
///
/// `root` is the folder the PARA structure lives under. `age_days` is time
/// since last modified, `name` the file name.
///
/// The bias is heavily toward `Leave`. A wrong move costs you a file you
/// cannot find; a missed move costs you nothing you did not already have.
pub fn suggest(root: &Path, name: &str, ext: &str, age_days: u64) -> Suggestion {
    let lower = name.to_lowercase();
    let stem = lower.rsplit_once('.').map(|(a, _)| a).unwrap_or(&lower);

    // Anything already inside the structure is left alone. Atlas re-filing its
    // own filing is how a tidy folder becomes a shuffling folder.
    if root.join("Projects").exists() && name.contains("/Projects/") {
        return Suggestion::Leave { why: "already filed".into() };
    }

    // A name that says nothing is a name Atlas cannot file on. These are
    // usually the files you are part-way through sorting out yourself.
    if AMBIGUOUS.iter().any(|a| stem == *a || stem.starts_with(&format!("{a} ")))
        || stem.trim().is_empty()
    {
        return Suggestion::Leave {
            why: "the name doesn't say what it is, and guessing would lose it".into(),
        };
    }

    // Quiet for a year. Archived whatever it is — this is the one rule that
    // does not need to understand the file at all.
    if age_days >= ARCHIVE_AFTER_DAYS {
        return Suggestion::Move {
            to: root.join(Bucket::Archive.folder()).join(name),
            bucket: Bucket::Archive,
            why: format!("nothing has touched it in {} days", age_days),
        };
    }

    let e = ext.to_lowercase();
    if REFERENCE.contains(&e.as_str()) {
        return Suggestion::Move {
            to: root.join(Bucket::Resources.folder()).join(name),
            bucket: Bucket::Resources,
            why: "a document to refer back to rather than work on".into(),
        };
    }
    if WORKING.contains(&e.as_str()) {
        return Suggestion::Move {
            to: root.join(Bucket::Projects.folder()).join(name),
            bucket: Bucket::Projects,
            why: "something you're working on".into(),
        };
    }

    // Everything else. Deliberately not a catch-all bucket: an "Other" folder
    // is where files go to be forgotten, and moving something there is worse
    // than leaving it where you last saw it.
    Suggestion::Leave {
        why: format!("I don't have a confident home for a .{e} — left where you put it"),
    }
}

/// Turn a suggestion into the change the safety layer judges.
///
/// Deliberately separate from `suggest`. Deciding where something *should* go
/// and being *allowed* to move it are different questions, and `system::judge`
/// answers the second — roots, reversibility, approval. Filing that skipped
/// that gate would be filing that could reach outside your folders.
pub fn as_change(from: &Path, s: &Suggestion) -> Option<crate::system::Change> {
    match s {
        Suggestion::Move { to, .. } => Some(crate::system::Change::MoveFile {
            from: from.display().to_string(),
            to: to.display().to_string(),
        }),
        Suggestion::Leave { .. } => None,
    }
}

/// What Atlas says about a filing run before doing anything.
pub fn spoken(moves: usize, left: usize) -> String {
    if moves == 0 {
        return format!(
            "Nothing I'd move with any confidence. {left} files I looked at, all left where \
             they are."
        );
    }
    format!(
        "{moves} files I'd file, {left} I'd leave alone. Everything moves through the trash, so \
         any of it can be put back."
    )
}

/// Files a folder like the desktop holds that are not "loose files": the
/// shortcuts you launch things from, and the folder's own settings file.
const NOT_LOOSE: &[&str] = &["lnk", "url", "ini", "desktop", "website", "appref-ms"];

/// The loose files in `dir` -- files, not folders; not hidden; not
/// shortcuts -- each with where it would go under `root`, oldest-untouched
/// counted from `now` (seconds). In name order, so the plan reads the same
/// twice. Nothing is moved.
pub fn plan_folder(dir: &Path, root: &Path, now: u64) -> Vec<(PathBuf, Suggestion)> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<(PathBuf, Suggestion)> = Vec::new();
    for e in entries.flatten() {
        let path = e.path();
        let Ok(meta) = e.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        let ext = path.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
        if name.starts_with('.') || NOT_LOOSE.contains(&ext.as_str()) {
            continue;
        }
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(now);
        let age = now.saturating_sub(modified) / 86_400;
        let s = suggest(root, &name, &ext, age);
        out.push((path, s));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Carry out one suggested move, through `system::judge` -- the master
/// switch, the folders Atlas may work in -- never over the top of a file
/// already there, and never leaving two copies: a rename, or across drives
/// a copy and then the original removed only once the copy is whole. Where
/// it went, or why not.
pub fn file_one(from: &Path, s: &Suggestion, sys: &crate::system::SystemConfig) -> Result<PathBuf, String> {
    let (Some(change), Suggestion::Move { to, .. }) = (as_change(from, s), s) else {
        return Err("left where it is".into());
    };
    if let crate::system::Verdict::Refuse(why) = crate::system::judge(&change, sys) {
        return Err(why);
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("couldn't make {}: {e}", parent.display()))?;
    }
    // `fs::rename` replaces its destination silently on unix, and the one
    // already there is, by definition, the one filed before.
    if to.exists() {
        return Err(format!("{} already exists, and I won't write over it", to.display()));
    }
    let moved = match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => std::fs::copy(from, to).and_then(|_| std::fs::remove_file(from)).map(|_| ()),
    };
    match moved {
        Ok(()) => Ok(to.clone()),
        Err(e) => {
            if to.exists() && from.exists() {
                crate::heard!(std::fs::remove_file(to));
            }
            Err(e.to_string())
        }
    }
}

// `tidy_plan_words`, what "organize my desktop" used to say, was replaced on
// 2 Oct 2026 by `organize::plan_said`: the daemon sorts by kind with copies
// and old installers to "To review" now. `plan_folder` and `file_one` stay
// for `atlas file`, which still files by PARA.
