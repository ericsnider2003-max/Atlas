//! Keeping what matters, discarding what doesn't, under a hard ceiling.
//!
//! The governing principle: **store pointers, not payloads.**
//!
//! Atlas never copies your documents into its own store. The index holds a
//! path, a size, and a timestamp — about 200 bytes per file, so a 200,000 file
//! index is roughly 40MB. When you ask what's in a document, Atlas re-reads
//! the original. Recall costs nothing to keep because the data is already on
//! your disk; duplicating it would be the expensive mistake.
//!
//! What actually grows is the stuff Atlas *generates*: screenshots, wav
//! scratch files, logs, and its own history. Those are what this module bounds.

use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    /// Wav files from the current turn. Dead the moment the turn ends.
    Scratch,
    /// Screenshots and webcam frames. Big, and stale within the hour.
    Captures,
    /// Rotating logs. Already bounded, checked here for completeness.
    Logs,
    /// Research notes. Small text, high value, kept longest.
    Notes,
    /// Atlas's own learned state. Compacted, never deleted wholesale.
    State,
    /// Something whose age could not be read.
    ///
    /// Its own class rather than a default, so the eviction rules cannot reach
    /// it. A file Atlas cannot date is a file it has no basis for deleting,
    /// and dating it to 1970 made it the first candidate rather than the last.
    Unknown,
    /// Somebody else's business — `data/trash` and `data/backups`.
    ///
    /// **These two subtrees have their own lifecycles and this module must
    /// not touch them.** `data/trash` is emptied by `safety::Trash::expire`
    /// after `keep_days`; `data/backups` is thinned by
    /// `safety::prune_backups`. Retention surveys `install_root()/data`,
    /// which contains both, and `classify` matched on extensions — so a
    /// trashed `photo.png` at `data/trash/12-photo.png` came out as
    /// `Captures` with a 24-hour limit and a trashed `.wav` as `Scratch`
    /// with a ten-minute one.
    ///
    /// The cost was not a tidier disk. `apply` deletes with `remove_file`,
    /// permanently and not through the trash, **while the ledger still lists
    /// the file** — so saying "undo" afterwards fails on a file the ledger
    /// says is there. `reclaim.rs`'s own promise ("Nothing is deleted. Things
    /// are moved to Atlas's trash, which keeps them 30 days… Every reclaim is
    /// reversible for a month") was untrue for every `.png`, `.jpg` and
    /// `.wav` in it, within a day.
    NotOurs,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RetentionConfig {
    /// Delete a recording the moment it has been transcribed.
    ///
    /// The transcript is the thing you wanted; the audio is a byproduct. A
    /// folder of recordings of yourself is a liability with no upside, and
    /// keeping it "just in case" is how it ends up backed up somewhere.
    #[serde(default = "yes")]
    pub delete_audio_after_transcribing: bool,
    /// Everything Atlas generates, together, may not exceed this.
    pub total_budget_mb: u64,
    pub scratch_minutes: u64,
    pub captures_hours: u64,
    pub logs_mb: u64,
    pub notes_days: u64,
    /// Approval records kept individually before collapsing to counts.
    pub approvals_detailed: usize,
    /// Conversation turns kept before folding into workflow memory.
    pub session_turns: usize,
}

fn yes() -> bool {
    true
}

impl Default for RetentionConfig {
    fn default() -> Self {
        RetentionConfig {
            delete_audio_after_transcribing: true,
            total_budget_mb: 500,
            scratch_minutes: 10,
            captures_hours: 24,
            logs_mb: 4,
            notes_days: 365,
            approvals_detailed: 200,
            session_turns: 40,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub path: PathBuf,
    pub class: Class,
    pub bytes: u64,
    pub modified: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Plan {
    Delete { path: PathBuf, why: String },
    Keep,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Usage {
    pub scratch: u64,
    pub captures: u64,
    pub logs: u64,
    pub notes: u64,
    pub state: u64,
    /// Bytes Atlas could not date, and therefore will not evict.
    pub unknown: u64,
    /// `data/trash` and `data/backups` — real disk, and not this module's to
    /// reduce. See [`Class::NotOurs`].
    pub not_ours: u64,
}

impl Usage {
    pub fn total(&self) -> u64 {
        self.scratch + self.captures + self.logs + self.notes + self.state + self.unknown + self.not_ours
    }
    pub fn total_mb(&self) -> u64 {
        self.total() / (1024 * 1024)
    }
    pub fn add(&mut self, class: Class, bytes: u64) {
        match class {
            Class::Scratch => self.scratch += bytes,
            Class::Captures => self.captures += bytes,
            Class::Logs => self.logs += bytes,
            Class::Notes => self.notes += bytes,
            Class::State => self.state += bytes,
            // Counted toward the total but not attributed, because attributing
            // it would imply Atlas knows what it is.
            Class::Unknown => self.unknown += bytes,
            // Counted, because it is real disk the person is paying for and
            // a total that left it out would understate the folder. Held
            // apart from `state`, because `irreducible` reads `state` as
            // "cannot be reduced" -- and the trash and the backups can be,
            // by the two functions that own them.
            Class::NotOurs => self.not_ours += bytes,
        }
    }
}

/// What kind of file this is, judged on its path **inside** the data folder.
///
/// ## Why `within` and not the whole path
///
/// This used to lowercase the absolute path and ask whether it contained
/// `"/tmp"`. That is a substring of the whole path, so an install root with a
/// `tmp` component in it — `ATLAS_HOME=/home/eric/tmp/atlas`, or a zip
/// unpacked to `C:\Users\Eric\tmp\atlas` — made **every file under
/// `data/`** `Class::Scratch` with a ten-minute limit: the state, the notes,
/// the backups, the trash, the crash note. Hourly total erasure of everything
/// Atlas knows, and of every backup it could have been restored from, on a
/// machine whose only sin was where it was unzipped.
///
/// Judging the path relative to the root fixes that and the trash overlap in
/// one move, because relative to `data/` the trash is visibly `trash/…`.
///
/// `classify` remains for a caller that genuinely has only a name; it is the
/// same rules over the whole path and carries the same hazard, so `plan`'s
/// caller uses `classify_within`.
pub fn classify_within(root: &Path, path: &Path) -> Class {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let p = rel.to_string_lossy().to_lowercase().replace('\\', "/");
    // First, and before anything looks at an extension. See `Class::NotOurs`.
    if p == "trash" || p.starts_with("trash/") || p == "backups" || p.starts_with("backups/") {
        return Class::NotOurs;
    }
    classify_relative(&p)
}

pub fn classify(path: &Path) -> Class {
    classify_relative(&path.to_string_lossy().to_lowercase().replace('\\', "/"))
}

fn classify_relative(p: &str) -> Class {
    if p.contains("/notes") || p.starts_with("notes") {
        Class::Notes
    } else if p.contains("/logs") || p.starts_with("logs") || p.ends_with(".log") || p.contains(".log.") {
        Class::Logs
    } else if p.ends_with(".png") || p.ends_with(".jpg") || p.ends_with(".jpeg") {
        Class::Captures
    } else if p.ends_with(".wav") || p.contains("/tmp") || p.starts_with("tmp") {
        Class::Scratch
    } else {
        Class::State
    }
}

/// Walk Atlas's own data directory. Never touches anything outside it.
pub fn survey(root: &Path) -> Vec<Item> {
    let mut out = Vec::new();
    // The root is carried down, because classification is about where a file
    // sits *inside* the data folder and not about the absolute path. See
    // `classify_within`: judging the absolute path made an install under a
    // `tmp` directory delete its own state every hour.
    walk(root, root, &mut out, 0);
    out
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<Item>, depth: u32) {
    if depth > 6 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            walk(root, &p, out, depth + 1);
            continue;
        }
        let Ok(md) = e.metadata() else { continue };
        let stamp = md
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        // `unwrap_or(0)` here was worse than the same line in `index`. There an
        // unreadable date buries a file at the bottom of a search; here it
        // dates the file to 1970, which is precisely what marks it as old
        // enough to delete. A file Atlas could not read the age of became the
        // first thing it proposed removing.
        let Some(modified) = stamp else {
            out.push(Item { class: Class::Unknown, path: p, bytes: md.len(), modified: 0 });
            continue;
        };
        let class = classify_within(root, &p);
        out.push(Item { class, path: p, bytes: md.len(), modified });
    }
}

pub fn usage(items: &[Item]) -> Usage {
    let mut u = Usage::default();
    for i in items {
        u.add(i.class, i.bytes);
    }
    u
}

/// Decide what to delete.
///
/// Two passes. First, anything past its own class's age limit goes — that
/// alone usually keeps things well under budget. Second, if the total is
/// *still* over, evict oldest-first by class priority: scratch, then captures,
/// then logs. Notes and state are never evicted for space; if those alone
/// exceed the budget you have a configuration problem, not a cleanup problem,
/// and Atlas says so rather than silently deleting what it learned.
pub fn plan(items: &[Item], cfg: &RetentionConfig, now: u64) -> Vec<Plan> {
    let mut plans = Vec::new();
    let mut survivors: Vec<&Item> = Vec::new();

    for i in items {
        let age = now.saturating_sub(i.modified);
        let limit = match i.class {
            Class::Scratch => Some(cfg.scratch_minutes * 60),
            Class::Captures => Some(cfg.captures_hours * 3600),
            Class::Notes => Some(cfg.notes_days * 86400),
            // `None` means no limit, so nothing here is ever proposed for
            // deletion. A file whose age is unreadable is one Atlas has no
            // basis for removing.
            Class::Logs | Class::State | Class::Unknown => None,
            // Never. Two policies deleting out of one folder is how the
            // trash's thirty-day promise became untrue for anything with a
            // `.png` on the end of it.
            Class::NotOurs => None,
        };
        match limit {
            Some(l) if age > l => plans.push(Plan::Delete {
                path: i.path.clone(),
                why: format!("{:?} older than its limit", i.class),
            }),
            _ => survivors.push(i),
        }
    }

    let budget_bytes = cfg.total_budget_mb * 1024 * 1024;
    let mut total: u64 = survivors.iter().map(|i| i.bytes).sum();
    if total <= budget_bytes {
        return plans;
    }

    // Oldest first, cheapest class first. Notes and state are not candidates.
    let mut evictable: Vec<&&Item> = survivors
        .iter()
        // `NotOurs` is deliberately absent: the budget pass must not reach
        // into the trash or the backups either. If the disk is full, the
        // answer is `Trash::expire` and `prune_backups`, which know what
        // those files are.
        .filter(|i| matches!(i.class, Class::Scratch | Class::Captures | Class::Logs))
        .collect();
    evictable.sort_by_key(|i| (i.class, i.modified));

    for i in evictable {
        if total <= budget_bytes {
            break;
        }
        total -= i.bytes;
        plans.push(Plan::Delete {
            path: i.path.clone(),
            why: format!("over the {}MB budget", cfg.total_budget_mb),
        });
    }
    plans
}

/// Is this path really inside `root`?
///
/// Canonicalised on both sides, so `..` segments and symlinks are resolved
/// before the comparison rather than after. A string prefix check would accept
/// `data/../../Users` and follow a symlink out of the tree without noticing.
///
/// A path that cannot be canonicalised — because it has already gone, or
/// cannot be read — is not inside anything, and there is nothing to delete.
fn inside(root: &Path, path: &Path) -> bool {
    let (Ok(root), Ok(path)) = (root.canonicalize(), path.canonicalize()) else {
        return false;
    };
    path.starts_with(&root)
}

/// Carry out a plan. Returns bytes reclaimed.
///
/// `root` is the only place deletion is permitted, and every path is checked
/// against it here rather than trusted from the plan.
///
/// This is the one irreversible operation in the module, and it used to take
/// whatever path a `Plan::Delete` carried. That was safe only because the one
/// caller happened to build its plans from a survey of `data`. `Plan` is
/// public, serialisable, and reaches disk, so "the only caller is careful" is
/// a property of today rather than of the code — and the point of no return is
/// exactly where a check is worth having, not one frame earlier.
///
/// Directories are refused outright: `remove_file` would fail on one anyway,
/// but saying so is better than a silent no-op that reads as success.
pub fn apply(plans: &[Plan], root: &Path) -> u64 {
    let mut freed = 0;
    for p in plans {
        let Plan::Delete { path, .. } = p else { continue };
        if !inside(root, path) {
            continue;
        }
        let Ok(md) = std::fs::metadata(path) else { continue };
        if !md.is_file() {
            continue;
        }
        if std::fs::remove_file(path).is_ok() {
            freed += md.len();
        }
    }
    freed
}

/// Everything `apply` would refuse to touch, and why.
///
/// Separate from `apply` so a refusal can be surfaced instead of silently
/// skipped. A plan that names a path outside the tree is a bug somewhere
/// upstream, and swallowing it means never finding out.
pub fn out_of_bounds(plans: &[Plan], root: &Path) -> Vec<PathBuf> {
    plans
        .iter()
        .filter_map(|p| match p {
            Plan::Delete { path, .. } if !inside(root, path) => Some(path.clone()),
            _ => None,
        })
        .collect()
}

/// True when notes and learned state alone blow the budget — a situation
/// Atlas reports rather than resolves by deleting what it knows.
pub fn irreducible(u: &Usage, cfg: &RetentionConfig) -> bool {
    (u.notes + u.state) > cfg.total_budget_mb * 1024 * 1024
}

/// Remove a recording once its transcript exists.
///
/// Called immediately after transcription rather than on a timer, so the
/// window in which a recording of your voice exists on disk is measured in
/// seconds rather than hours. The transcript is the thing you wanted; the
/// audio is a byproduct, and keeping it "just in case" is how it ends up in a
/// backup somewhere.
pub fn discard_audio(path: &std::path::Path, cfg: &RetentionConfig) -> bool {
    if !cfg.delete_audio_after_transcribing {
        return false;
    }
    std::fs::remove_file(path).is_ok()
}

/// A recording that removes itself however the transcription ends.
///
/// ## What was wrong
///
/// `discard_audio` had **no caller anywhere in `src/`**. Its doc makes a
/// privacy promise — *"the window in which a recording of your voice exists
/// on disk is measured in seconds rather than hours"* — and
/// `delete_audio_after_transcribing` is a setting a person can turn on and
/// off. Neither did anything.
///
/// The two places that convert audio did their own cleanup instead, with a
/// hard-coded `let _ = std::fs::remove_file(&wav)` on the last line of the
/// happy path. So the setting was ignored, the result was discarded, and —
/// the part that matters — **every early return leaked the file**:
///
/// * `listen_to` returns early on "I couldn't make out the words" and on
///   "There was nothing said in that".
/// * `transcribe_timed` returns early when ffmpeg fails.
///
/// Those are the *common* endings for a bad recording, not the rare ones. And
/// since `back_up` now walks subfolders, anything left in a scratch directory
/// that happens to sit under the state folder would start going into every
/// backup as well.
///
/// ## Why a guard rather than more call sites
///
/// A cleanup that has to be remembered on every return path is a cleanup that
/// gets forgotten on the one added next — which is exactly how this happened.
/// Dropping does it, so there is no path out of the function that skips it.
pub struct Recording {
    path: std::path::PathBuf,
    delete: bool,
    /// Files the transcriber writes beside the audio, which are just as much
    /// a copy of what was said: whisper's `.srt` and `.txt`.
    alongside: Vec<std::path::PathBuf>,
}

impl Recording {
    pub fn new(path: &std::path::Path, cfg: &RetentionConfig) -> Recording {
        Recording {
            path: path.to_path_buf(),
            delete: cfg.delete_audio_after_transcribing,
            alongside: Vec::new(),
        }
    }

    /// Also clean up something the transcriber writes next to the audio.
    pub fn and_also(&mut self, p: &std::path::Path) -> &mut Recording {
        self.alongside.push(p.to_path_buf());
        self
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        if !self.delete {
            return;
        }
        // `discard_audio` re-reads the flag, which is deliberate: this struct
        // and that function must not be able to disagree about what the
        // setting means.
        let cfg = RetentionConfig { delete_audio_after_transcribing: true, ..Default::default() };
        discard_audio(&self.path, &cfg);
        for p in &self.alongside {
            discard_audio(p, &cfg);
        }
    }
}
