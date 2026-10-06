//! Backup and undo.
//!
//! Two gaps from the audit, and they are the same gap seen from either side:
//! everything Atlas has learned lives in one folder with no copy, and nothing
//! it does can be taken back.
//!
//! Neither is exotic. A backup is a dated copy with old ones pruned. Undo is
//! a trash folder plus a record of what came from where. Both are cheap, and
//! both are the difference between an assistant you can trust with your files
//! and one you can't.

use crate::error::{AtlasError, Result};
use crate::store::now;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ---------- backup ----------

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct BackupConfig {
    pub enabled: bool,
    pub dir: String,
    /// Seconds between backups.
    pub every_secs: u64,
    /// How many dated copies to keep.
    pub keep: usize,
}

impl Default for BackupConfig {
    fn default() -> Self {
        // Empty, not `"data/backups"`. A default that *looks* like a usable
        // path is what let three call sites use it unresolved and write to
        // whatever folder the process happened to be standing in. Empty is
        // unusable on purpose: it means "the install's own backups folder",
        // and only `resolved` can say where that is.
        BackupConfig { enabled: true, dir: String::new(), every_secs: 86_400, keep: 7 }
    }
}

impl BackupConfig {
    /// `dir` ships as a bare relative path in the shipped config, which
    /// means every install — and every test that shares a working
    /// directory — shares one real backup folder rather than each having
    /// its own. The same "path that looks per-install and is not" bug
    /// already found and fixed three times in this tree (`data/index.md`,
    /// the model-call trace log, `atlas trace`'s read path). Resolved
    /// against the store's own root, the one directory that genuinely
    /// belongs to this install. Left alone if already absolute, so a
    /// deliberately shared or external backup location still works.
    pub fn resolved(mut self, install_root: &Path) -> BackupConfig {
        if self.dir.trim().is_empty() {
            // Unset: the install's own `data/backups`, the sibling of
            // `data/state` that `upgrade::YOURS` names.
            //
            // Through `Store::backups_dir` rather than by joining
            // `"data"/"backups"` here. `store.rs` says why in the comment
            // above its five path accessors: "no module except this one and
            // `roots.rs` has to spell a `data/…` path out as a literal --
            // that is not tidiness: the literal is the bug." This was the
            // literal, and it was the third derivation of `data/backups` in
            // the tree (the other two -- `Store::backups_dir` and
            // `roots::backups_dir` -- were the ones with no caller, which is
            // how `tests/dead_methods.rs` found this). Store::new of a root
            // that does not end in `data/state` returns that root unchanged
            // from `install_root()`, so this is the same path, derived once.
            self.dir = crate::store::Store::new(install_root)
                .backups_dir()
                .to_string_lossy()
                .into_owned();
            return self;
        }
        let d = PathBuf::from(&self.dir);
        if d.is_relative() {
            self.dir = install_root.join(d).to_string_lossy().into_owned();
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Backup {
    pub path: PathBuf,
    pub at: u64,
    /// How many files it holds. `None` means the folder could not be read —
    /// which is not the same as it holding nothing.
    pub files: Option<usize>,
    pub bytes: u64,
}

impl Backup {
    /// How it reads when listed.
    pub fn line(&self) -> String {
        match self.files {
            Some(n) => format!("{} — {n} files", self.path.display()),
            None => format!("{} — I couldn't read what's in it", self.path.display()),
        }
    }

    /// Is this one safe to restore from?
    pub fn trustworthy(&self) -> bool {
        matches!(self.files, Some(n) if n > 0)
    }
}

/// The prefix `back_up` gives a file it has not finished writing.
///
/// Public because `list_backups` has to know it: a leftover `.thread` from a
/// backup that died mid-copy is not a backed-up file, and counting it as one
/// is what let a run of failed backups evict the real ones. See the note on
/// `count_real_files`.
const PART_PREFIX: &str = ".part-";

/// Copy the state folder to a dated directory.
///
/// Files are copied to a temporary name and renamed into place, so a backup
/// interrupted halfway leaves no half-written file that looks complete.
///
/// ## Subfolders, which this used to walk straight past
///
/// It was `if !p.is_file() { continue }` over one `read_dir`. The state
/// folder is mostly flat JSON, so that looked right — and two real
/// directories live in it:
///
/// * **`handoffs/`** — the actual files other people have sent you.
///   `household::ReceivedFile::stored_at` points into it from the inbox. A
///   backup carried the inbox entries naming those files and none of the
///   files, so a restore produced an inbox full of items whose attachments
///   were gone.
/// * **`landed-over/`** — the previous version of every file Atlas landed on
///   its own. `selfwork::land`'s doc calls it the thing that makes a
///   self-landed fix "something you can put back without asking it". It was
///   never in a backup.
///
/// Nothing reported this, because `files: Some(n)` counted the flat files,
/// `n > 0`, and `Backup::trustworthy()` therefore said yes.
///
/// ## Cleaning up after itself
///
/// A failure part-way used to leave the partial `state-<t>` directory on
/// disk, and `list_backups` counted everything in it — including the `.name`
/// temp file of the copy that failed. So a failed backup reported
/// `files: Some(1)`, which is `trustworthy()`, which meant
/// `due_for_backup` saw a recent backup and stopped trying for `every_secs`,
/// and `prune_backups` kept it and deleted a real one underneath it. That is
/// exactly the failure `prune_backups`' own doc describes and believes it
/// fixed, arriving through `back_up`'s own temp files.
///
/// Two answers, because either alone is not enough: the partial directory is
/// removed when this returns an error, and the temp name is one
/// `count_real_files` knows to ignore for the case where nothing gets to run
/// any cleanup — a power cut, a kill.
pub fn back_up(state: &Path, cfg: &BackupConfig, t: u64) -> Result<Backup> {
    if !state.is_dir() {
        return Err(AtlasError::Platform(format!("nothing at {}", state.display())));
    }
    let dest = PathBuf::from(&cfg.dir).join(format!("state-{t}"));
    std::fs::create_dir_all(&dest)?;

    // Never walk into the place the backup is being written.
    //
    // In a normal install the backups are a SIBLING of the state folder —
    // `data/backups` beside `data/state`, which `BackupConfig::resolved` and
    // `upgrade::YOURS` both spell out. But the source is `store.root()` and
    // the destination is derived from `install_root()`, and `Store::new` of a
    // root that does not end in `data/state` returns that root unchanged as
    // the install root. So on any layout where the store root is not
    // `.../data/state` — a custom `ATLAS_HOME`, a profile directory, the
    // temporary root a test uses — the destination lands *inside* the source.
    //
    // The old one-level loop never noticed, because it skipped every
    // subdirectory and so never saw `data/` at all. Recursing turns that
    // layout into a backup copying itself into itself until the path runs
    // out, which is how `tests/off_the_tick.rs` found it.
    //
    // The whole backups directory, not just this run's `state-<t>`. Skipping
    // only the current destination leaves the PREVIOUS backups inside the
    // walk, so the second backup swallows the first, the third swallows both,
    // and each one is bigger than the last until the disk goes.
    //
    // Canonicalised so that `..`, a symlink or a different spelling of the
    // same directory cannot get past the comparison; and falling back to the
    // path as given, because a directory that does not exist yet cannot be
    // canonicalised and the check still has to mean something.
    let backups_root = PathBuf::from(&cfg.dir);
    let skip = std::fs::canonicalize(&backups_root).unwrap_or(backups_root);

    let mut files = 0usize;
    let mut bytes = 0u64;
    match copy_state_into_skipping(state, &dest, &skip, &mut files, &mut bytes) {
        Ok(()) => Ok(Backup { path: dest, at: t, files: Some(files), bytes }),
        Err(e) => {
            // A backup that failed is not a backup. Left on disk it has a
            // recent timestamp, which is worse than no backup at all.
            crate::heard!(std::fs::remove_dir_all(&dest));
            Err(e)
        }
    }
}

/// One level of the state folder, and everything under it.
///
/// `skip` is a directory never descended into — see the note in `back_up`
/// about the destination being able to sit inside the source.
fn copy_state_into_skipping(
    from: &Path,
    to: &Path,
    skip: &Path,
    files: &mut usize,
    bytes: &mut u64,
) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        let p = e.path();
        let Some(name) = p.file_name().map(|n| n.to_owned()) else { continue };
        // A file that is no longer there is skipped, not an error.
        //
        // The old one-level loop was `if !p.is_file() { continue }`, which
        // treated a vanished path as "not a file" and moved on. Propagating
        // instead — which the first version of this did — turns the ordinary
        // case of the daemon writing a record between the listing and the
        // read into a **failed backup**, and a failed backup is now removed
        // outright. `tests/off_the_tick.rs` caught exactly that: the
        // scheduled backup runs on the crew while the tick keeps persisting,
        // so the race is the normal state of affairs rather than a rare one.
        let meta = match std::fs::symlink_metadata(&p) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };
        // Not followed, for the same reason `copy_tree` does not follow them:
        // a link out of the state folder is a way out of the state folder,
        // and a backup is not the place to discover that.
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            let here = std::fs::canonicalize(&p).unwrap_or_else(|_| p.clone());
            if here == *skip || here.starts_with(skip) {
                continue;
            }
            copy_state_into_skipping(&p, &to.join(&name), skip, files, bytes)?;
            continue;
        }
        let tmp = to.join(format!("{PART_PREFIX}{}", name.to_string_lossy()));
        match std::fs::copy(&p, &tmp) {
            Ok(_) => {}
            // Same reasoning: gone between the listing and the copy.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                crate::heard!(std::fs::remove_file(&tmp));
                continue;
            }
            Err(e) => return Err(e.into()),
        }
        crate::store::rename_patiently(&tmp, &to.join(&name))?;
        *files += 1;
        *bytes += meta.len();
    }
    Ok(())
}

/// Files in a backup that are actually backed-up files.
///
/// Recursive, so `handoffs/` and `landed-over/` count; and skipping anything
/// still carrying `PART_PREFIX`, so a copy that died half way cannot make an
/// empty backup look like a real one.
fn count_real_files(dir: &Path) -> Option<usize> {
    let rd = std::fs::read_dir(dir).ok()?;
    let mut n = 0;
    for e in rd {
        let e = e.ok()?;
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with(PART_PREFIX) {
            continue;
        }
        let p = e.path();
        if p.is_dir() {
            n += count_real_files(&p)?;
        } else {
            n += 1;
        }
    }
    Some(n)
}

/// What is on disk, or why it can't be listed.
///
/// An unreadable backup folder used to return an empty list, which reads as
/// "you have no backups" — the answer most likely to make someone stop
/// worrying at exactly the wrong moment.
pub fn backups(cfg: &BackupConfig) -> std::result::Result<Vec<Backup>, String> {
    match std::fs::read_dir(&cfg.dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("couldn't read {}: {e}", cfg.dir)),
        Ok(_) => Ok(list_backups(cfg)),
    }
}

pub fn list_backups(cfg: &BackupConfig) -> Vec<Backup> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(&cfg.dir) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        let Some(at) = p
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_prefix("state-"))
            .and_then(|n| n.parse::<u64>().ok())
        else {
            continue;
        };
        // `unwrap_or(0)` made an unreadable backup look like an empty one,
        // which is the difference between "this restore will do nothing" and
        // "I can't tell you what this restore would do".
        //
        // And `read_dir(&p).count()` counted one level, every entry: a
        // subfolder counted as one file however much was in it, and a
        // leftover temp file from a backup that died mid-copy counted as a
        // backed-up file. See `count_real_files`.
        let files = count_real_files(&p);
        out.push(Backup { path: p, at, files, bytes: 0 });
    }
    out.sort_by_key(|b| b.at);
    out
}

/// Delete all but the newest `keep` — **worthless ones first.**
///
/// It used to delete strictly oldest-first, counting every `state-<t>`
/// directory as a backup regardless of what was in it. `back_up` does
/// `create_dir_all(&dest)?` and then copies with `?` inside the loop, so a
/// failure part-way leaves an empty or partial directory behind and returns
/// `Err` without cleaning it up.
///
/// So once backups start failing — a full disk, or one state file locked by
/// antivirus on Windows — every attempt still left a junk directory with a
/// *recent* timestamp. After `keep` failures the junk filled the quota and
/// this function deleted the real backups underneath it, oldest first, to
/// stay under the limit. Eight days of failure and `restore` from any of the
/// seven survivors copies nothing.
///
/// `Backup::trustworthy()` already existed for exactly this distinction —
/// "Is this one safe to restore from?" — and nothing called it. It is called
/// here now: anything with no files in it is dropped before anything with
/// files, so a run of failures can no longer evict a good backup.
pub fn prune_backups(cfg: &BackupConfig) -> usize {
    let all = list_backups(cfg);
    if all.len() <= cfg.keep {
        return 0;
    }
    let drop = all.len() - cfg.keep;

    // Junk first, oldest-first within each group. `list_backups` is already
    // sorted by timestamp, and `sort_by_key` is stable, so partitioning on
    // trustworthiness keeps that order inside both halves.
    //
    // An UNREADABLE backup (`files: None`) is not treated as junk: "I can't
    // tell you what this restore would do" is not the same as "this restore
    // would do nothing", which is the distinction `list_backups` goes out of
    // its way to preserve two functions above. Deleting one because it could
    // not be read would throw away a backup that may be perfectly good.
    let mut order: Vec<&Backup> = all.iter().collect();
    order.sort_by_key(|b| match b.files {
        Some(0) => 0, // definitely empty: goes first
        _ => 1,       // has files, or could not be read: keep as long as possible
    });

    let mut removed = 0;
    for b in order.iter().take(drop) {
        if std::fs::remove_dir_all(&b.path).is_ok() {
            removed += 1;
        }
    }
    removed
}

pub fn due_for_backup(cfg: &BackupConfig, t: u64) -> bool {
    if !cfg.enabled {
        return false;
    }
    // The last backup that actually HOLDS something.
    //
    // It was `list_backups(cfg).last()`, which is the newest directory
    // whatever is in it — so one failed backup, leaving an empty `state-<t>`
    // with a fresh timestamp, suppressed every further attempt for
    // `every_secs`. Backups then stayed "not due" for as long as they kept
    // failing, which is the one situation where trying again matters most.
    //
    // `files: None` (unreadable) counts as a backup here, for the same reason
    // `prune_backups` will not delete one: not being able to read it is not
    // evidence that it is empty.
    match list_backups(cfg).iter().rev().find(|b| b.files != Some(0)) {
        None => true,
        Some(b) => t.saturating_sub(b.at) >= cfg.every_secs,
    }
}

/// Put a backup back. Existing files are moved to trash first, so restoring
/// the wrong one is itself undoable.
///
/// `mine` is the household restoring it. If the backup carries a different
/// household's identity, nothing is copied -- a backup folder found on a
/// shared drive, a synced cloud folder, or handed over by mistake must not
/// silently become part of your own Atlas. A backup with no household file
/// at all (older than this check, or never initialized) is let through: the
/// check can only refuse a *known* mismatch, not invent one for a backup
/// that predates it.
pub fn restore(backup: &Path, state: &Path, trash: &Trash, mine: &crate::household::Household) -> Result<usize> {
    if !backup.is_dir() {
        return Err(AtlasError::Platform("no such backup".into()));
    }
    let backup_store = crate::store::Store::new(backup);
    let theirs = crate::household::Household::load(&backup_store);
    if mine.is_set() && theirs.is_set() {
        if let Err(why) = crate::household::accept_bundle(&mine.id, &theirs.id) {
            return Err(AtlasError::Platform(why));
        }
    }
    std::fs::create_dir_all(state)?;

    // Everything the backup holds, gathered before anything is touched.
    //
    // Two things were wrong with doing this in one pass.
    //
    // **Subfolders were skipped.** `if !p.is_file() { continue }` over one
    // `read_dir` walked past `handoffs/` — the files people have actually
    // sent you — and `landed-over/`, the previous version of everything Atlas
    // landed on its own. See the note on `back_up`, which had the same hole
    // from the writing end.
    //
    // **A failure part-way left the state folder half-restored**, with the
    // replaced half already moved to the trash, and returned an error
    // carrying a count of nothing. The person was then one bad read away from
    // a state folder that is neither the old one nor the new one, and no
    // sentence anywhere saying which files were which.
    //
    // So: list first, copy everything into a staging folder, and only then
    // swap. A failure while staging touches nothing at all. A failure while
    // swapping names the files that had already been replaced — they are all
    // in the trash, and `undo` can put each one back.
    let mut planned: Vec<(PathBuf, PathBuf)> = Vec::new(); // (source, relative)
    gather(backup, &PathBuf::new(), &mut planned)?;
    planned.retain(|(_, rel)| {
        // Already read and checked above -- restoring it too would let an
        // older backup from the *same* household quietly roll your device
        // list back, which is not what "restore my files" asked for.
        rel.as_os_str() != "household.json"
    });

    let staging = state.join(format!(".restoring-{}", now()));
    crate::heard!(std::fs::remove_dir_all(&staging));
    let stage_all = || -> Result<()> {
        for (src, rel) in &planned {
            let dst = staging.join(rel);
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(src, &dst)?;
        }
        Ok(())
    };
    if let Err(e) = stage_all() {
        crate::heard!(std::fs::remove_dir_all(&staging));
        return Err(AtlasError::Platform(format!(
            "couldn't read the whole backup ({e}), so I've left your files alone"
        )));
    }

    let mut n = 0;
    let mut replaced: Vec<String> = Vec::new();
    for (_, rel) in &planned {
        let target = state.join(rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if target.exists() {
            trash.take(&target, "replaced by a restore")?;
            replaced.push(rel.display().to_string());
        }
        if let Err(e) = move_across(&staging.join(rel), &target) {
            crate::heard!(std::fs::remove_dir_all(&staging));
            return Err(AtlasError::Platform(format!(
                "I restored {n} file(s) and then couldn't put {} back: {e}. The ones I \
                 replaced are in the trash — say undo to put each back. Replaced so far: {}",
                rel.display(),
                replaced.join(", ")
            )));
        }
        n += 1;
    }
    crate::heard!(std::fs::remove_dir_all(&staging));
    Ok(n)
}

/// Every file under `dir`, as (full path, path relative to the root).
fn gather(dir: &Path, rel: &Path, out: &mut Vec<(PathBuf, PathBuf)>) -> Result<()> {
    for e in std::fs::read_dir(dir)?.flatten() {
        let p = e.path();
        let meta = std::fs::symlink_metadata(&p)?;
        if meta.file_type().is_symlink() {
            continue;
        }
        let Some(name) = p.file_name().map(|n| n.to_owned()) else { continue };
        let here = rel.join(&name);
        if meta.is_dir() {
            gather(&p, &here, out)?;
        } else if !name.to_string_lossy().starts_with(PART_PREFIX) {
            // A temp file from a backup that died mid-copy is not something
            // to restore.
            out.push((p, here));
        }
    }
    Ok(())
}

// ---------- undo ----------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Discarded {
    pub id: u64,
    /// Where it came from, so it can go back.
    pub original: String,
    /// Where it is now.
    pub held: String,
    pub why: String,
    pub at: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct TrashConfig {
    pub dir: String,
    /// Delete for real after this many days.
    pub keep_days: u64,
}

impl Default for TrashConfig {
    fn default() -> Self {
        TrashConfig { dir: String::new(), keep_days: 30 }
    }
}

impl TrashConfig {
    /// The same resolution `BackupConfig` has had, which `TrashConfig` did
    /// not — and that asymmetry was live: `atlas file`, `atlas backups` and
    /// `atlas reclaim` each built a `Trash` straight from this default and
    /// wrote a `data/trash` folder into whatever directory they were run
    /// from, while the daemon quietly used a fourth location of its own
    /// (`data/state/trash`). Two trash cans, one of them wherever you were
    /// standing, and `atlas update` preserved neither.
    pub fn resolved(mut self, install_root: &Path) -> TrashConfig {
        if self.dir.trim().is_empty() {
            // Through `Store::trash_dir`, for the reason given in
            // `BackupConfig::resolved` just above: the `data/…` literal is
            // the bug this tree has fixed four times.
            self.dir = crate::store::Store::new(install_root)
                .trash_dir()
                .to_string_lossy()
                .into_owned();
            return self;
        }
        let d = PathBuf::from(&self.dir);
        if d.is_relative() {
            self.dir = install_root.join(d).to_string_lossy().into_owned();
        }
        self
    }
}

/// Nothing Atlas removes or overwrites is gone immediately.
/// What reading the trash ledger produced.
#[derive(Debug, Clone, PartialEq)]
pub enum LedgerState {
    /// No ledger has ever been written. Genuinely empty.
    Fresh,
    /// Read, with these entries.
    Read(Vec<Discarded>),
    /// The file is there and could not be opened.
    Unreadable(String),
    /// The file is there and is not valid.
    Corrupt(String),
}

impl LedgerState {
    fn entries(self) -> Vec<Discarded> {
        match self {
            LedgerState::Read(v) => v,
            _ => Vec::new(),
        }
    }

    /// Can Atlas safely add to the trash right now?
    pub fn safe_to_write(&self) -> bool {
        matches!(self, LedgerState::Fresh | LedgerState::Read(_))
    }

    pub fn trouble(&self) -> Option<String> {
        match self {
            LedgerState::Unreadable(w) => Some(format!("the trash record can't be read: {w}")),
            LedgerState::Corrupt(w) => Some(format!("the trash record is damaged: {w}")),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Files and folders, which the trash treated as the same thing.
//
// ## What was wrong
//
// `Trash::take` moved things with `rename`, and fell back to
// `copy` + `remove_file` when `rename` failed. `Trash::undo` did the same in
// reverse, and `Trash::expire` deleted with `remove_file`.
//
// **Every single thing `reclaim` offers is a directory.** `reclaim::walk`
// begins `if meta.file_type().is_symlink() || !meta.is_dir() { continue }` —
// it offers `node_modules`, `target`, `Cache`, an installer folder. Never a
// file.
//
// So:
//
// * `take` worked only while the trash and the thing being reclaimed were on
//   the same volume. `data/trash` sits under the install root; the caches are
//   in `~/Library/Caches`, `%LOCALAPPDATA%`, `~/.cargo` and project folders.
//   On any machine with more than one drive `rename` returns `EXDEV`,
//   `std::fs::copy` then fails with "Is a directory", and the whole reclaim
//   refused with a raw OS error number.
// * `undo` had the same fallback, so a folder that *did* make it into the
//   trash could not be put back across a volume boundary.
// * `expire` did `let _ = std::fs::remove_file(&i.held)`. `remove_file` fails
//   on a directory — always, on every platform — and the result was
//   discarded. The ledger entry was dropped and the folder stayed on disk.
//
// That last one is the one that matters. `reclaim::spoken` promises
// *"Everything I'd move goes to the trash for 30 days, so it's all
// reversible."* What actually happened: a four-gigabyte `node_modules` moved
// into `data/trash`, freeing nothing; thirty days later its record was
// deleted and the four gigabytes stayed, now beyond `undo` (the ledger no
// longer knows about it) and beyond `expire` (nothing looks at trash contents
// without a ledger entry). Space reclaimed: none, ever. Space recoverable:
// none, after a month.
//
// ## The rule for a cross-volume move
//
// Copy the whole tree first, and only delete the source once the copy has
// finished. A partial copy removes the partial destination and leaves the
// source untouched. The opposite order — delete as you go — turns a disk
// filling up mid-copy into a half-deleted project folder, which is the one
// outcome worse than not reclaiming anything.
// ---------------------------------------------------------------------------

/// Bytes under a path, whether it is a file or a tree.
///
/// Its own small walk rather than `reclaim::size_of`, because that one is
/// private and this module must not start depending on the survey to report
/// on the trash.
fn size_on_disk(p: &Path, total: &mut u64) {
    let Ok(meta) = std::fs::symlink_metadata(p) else { return };
    if meta.file_type().is_symlink() {
        return;
    }
    if !meta.is_dir() {
        *total += meta.len();
        return;
    }
    let Ok(rd) = std::fs::read_dir(p) else { return };
    for e in rd.flatten() {
        size_on_disk(&e.path(), total);
    }
}

/// Remove a file or a whole directory.
fn remove_whatever(p: &Path) -> std::io::Result<()> {
    // `symlink_metadata`, not `is_dir`: a symlink pointing at a directory
    // answers `true` to `is_dir`, and `remove_dir_all` through it would
    // delete the target's contents rather than the link.
    match std::fs::symlink_metadata(p) {
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(p),
        Ok(_) => std::fs::remove_file(p),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Copy a whole directory tree.
///
/// Symlinks are skipped rather than followed. A link out of a cache folder is
/// a way out of the cache folder, and copying through one would pull in
/// whatever it points at — then `remove_dir_all` on the source would take the
/// link and leave the target, so the two halves would not match.
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let src = e.path();
        let dst = to.join(e.file_name());
        let meta = std::fs::symlink_metadata(&src)?;
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            copy_tree(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

/// Move a file or a folder, across volumes when it has to be.
///
/// `rename` first, because within a volume it is atomic and instant for a
/// tree of any size. The fallback is the careful one: copy everything, then
/// delete the source, and on a failed copy clean up the destination and leave
/// the source exactly as it was.
fn move_across(from: &Path, to: &Path) -> Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    let meta = std::fs::symlink_metadata(from)?;
    if meta.is_dir() {
        if let Err(e) = copy_tree(from, to) {
            // The half-copy goes; the original stays.
            crate::heard!(std::fs::remove_dir_all(to));
            return Err(AtlasError::Platform(format!(
                "couldn't copy {} to {}: {e}. Nothing was removed.",
                from.display(),
                to.display()
            )));
        }
        std::fs::remove_dir_all(from)?;
    } else {
        if let Err(e) = std::fs::copy(from, to) {
            crate::heard!(std::fs::remove_file(to));
            return Err(AtlasError::Platform(format!(
                "couldn't copy {} to {}: {e}. Nothing was removed.",
                from.display(),
                to.display()
            )));
        }
        std::fs::remove_file(from)?;
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct Trash {
    pub cfg: TrashConfig,
}

impl Trash {
    pub fn new(cfg: TrashConfig) -> Trash {
        Trash { cfg }
    }

    fn ledger_path(&self) -> PathBuf {
        PathBuf::from(&self.cfg.dir).join("ledger.json")
    }

    /// What the ledger says, or why it can't say.
    ///
    /// This used to be `Vec<Discarded>`, with an unreadable or corrupt file
    /// collapsing to an empty list. That is the worst place in Atlas for
    /// "nothing found" and "couldn't look" to be the same answer, because
    /// `take` derives the next id from the highest one already in it. An
    /// empty read meant ids restarted at 1, the new file was written over
    /// `1-something` already in the trash, and the ledger was then rewritten
    /// with one entry — erasing the record of everything else held there.
    ///
    /// A module whose whole purpose is that nothing is gone immediately could
    /// lose all of it, quietly, on one bad read.
    pub fn read_ledger(&self) -> LedgerState {
        let path = self.ledger_path();
        if !path.exists() {
            // Never written is genuinely empty. This is the one safe case.
            return LedgerState::Fresh;
        }
        match std::fs::read_to_string(&path) {
            Err(e) => LedgerState::Unreadable(e.to_string()),
            Ok(text) => match serde_json::from_str::<Vec<Discarded>>(&text) {
                Ok(items) => LedgerState::Read(items),
                Err(e) => LedgerState::Corrupt(e.to_string()),
            },
        }
    }

    /// The entries, treating an unreadable ledger as empty.
    ///
    /// Kept for reading and reporting only. Never use it to decide what to
    /// write — `take` uses `read_ledger` so it can refuse.
    pub fn ledger(&self) -> Vec<Discarded> {
        self.read_ledger().entries()
    }

    fn write_ledger(&self, items: &[Discarded]) -> Result<()> {
        std::fs::create_dir_all(&self.cfg.dir)?;
        // `unwrap_or_default()` here wrote an empty string over the ledger if
        // serialisation ever failed — the same total loss by a second route.
        let text = serde_json::to_string_pretty(items).map_err(|e| {
            AtlasError::Platform(format!(
                "couldn't write the trash ledger, so I left the old one alone: {e}"
            ))
        })?;
        // Write beside it and rename, so a crash mid-write cannot leave a
        // half-written ledger where a whole one used to be.
        let tmp = self.ledger_path().with_extension("json.writing");
        std::fs::write(&tmp, text)?;
        crate::store::rename_patiently(&tmp, &self.ledger_path())?;
        Ok(())
    }

    /// Move a file out of the way instead of deleting it.
    pub fn take(&self, path: &Path, why: &str) -> Result<Discarded> {
        if !path.exists() {
            return Err(AtlasError::Platform(format!("{} isn't there", path.display())));
        }
        std::fs::create_dir_all(&self.cfg.dir)?;
        // Refuse rather than guess. A new id derived from a ledger that could
        // not be read collides with what is already held.
        let mut items = match self.read_ledger() {
            LedgerState::Fresh => Vec::new(),
            LedgerState::Read(items) => items,
            LedgerState::Unreadable(why) | LedgerState::Corrupt(why) => {
                return Err(AtlasError::Platform(format!(
                    "I won't move {} to the trash: the trash record is unreadable ({why}). \
                     Writing now would overwrite something already in there and lose the \
                     record of the rest. Nothing has been touched.",
                    path.display()
                )))
            }
        };
        let id = items.iter().map(|i| i.id).max().unwrap_or(0) + 1;
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        // Id-prefixed, so two files with the same name don't collide.
        let held = PathBuf::from(&self.cfg.dir).join(format!("{id}-{name}"));

        // Folders as well as files — see the note above `remove_whatever`.
        // Everything `reclaim` offers is a folder, and the old fallback here
        // could only copy a file.
        move_across(path, &held)?;

        let d = Discarded {
            id,
            original: path.display().to_string(),
            held: held.display().to_string(),
            why: why.to_string(),
            at: now(),
        };
        items.push(d.clone());
        self.write_ledger(&items)?;
        Ok(d)
    }

    /// Put the last thing back.
    pub fn undo_last(&self) -> Result<Discarded> {
        let items = self.ledger();
        let last = items.last().cloned().ok_or_else(|| {
            AtlasError::Platform("there's nothing to undo".into())
        })?;
        self.undo(last.id)
    }

    pub fn undo(&self, id: u64) -> Result<Discarded> {
        let mut items = self.ledger();
        let pos = items
            .iter()
            .position(|i| i.id == id)
            .ok_or_else(|| AtlasError::Platform(format!("nothing with id {id}")))?;
        let item = items[pos].clone();
        let original = PathBuf::from(&item.original);

        // Refusing beats silently overwriting whatever is there now.
        if original.exists() {
            return Err(AtlasError::Platform(format!(
                "{} exists again — move it first if you want the old one back",
                item.original
            )));
        }
        if let Some(parent) = original.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // The ledger entry is removed only once the move has actually
        // happened. It used to come off the list whether or not the copy
        // worked, so a failed restore lost the record of where the thing was
        // and left it in the trash with nothing pointing at it.
        move_across(Path::new(&item.held), &original)?;
        items.remove(pos);
        self.write_ledger(&items)?;
        Ok(item)
    }

    /// Permanently remove anything past its keep window.
    pub fn expire(&self, t: u64) -> usize {
        // `read_ledger`, not `ledger()`, and the difference is everything.
        //
        // `ledger()`'s own doc says: "Kept for reading and reporting only.
        // **Never use it to decide what to write** -- `take` uses
        // `read_ledger` so it can refuse." This function used it to decide
        // what to write.
        //
        // `ledger()` collapses Unreadable and Corrupt to an empty Vec. So on
        // one bad read -- a power cut mid-write, a file briefly locked by a
        // sync client or antivirus -- this computed `keep = []` and wrote
        // `[]` over the ledger, then returned 0 and reported that nothing had
        // expired. Every file held in `data/trash/` became unrecoverable
        // through `undo`, and the next `take` restarted ids at 1 and renamed
        // a new file over the `1-something` already sitting there.
        //
        // That is precisely the catastrophe `read_ledger`'s doc describes and
        // believes it prevented: "A module whose whole purpose is that
        // nothing is gone immediately could lose all of it, quietly, on one
        // bad read." It was prevented in `take` and missed here. It runs
        // hourly from the daemon's housekeeping, so the window was every
        // hour, forever.
        //
        // `undo` and `undo_last` read the same way and are safe by luck
        // rather than by design -- both return an error before reaching a
        // write when the list comes back empty. They are left alone; this is
        // the one that writes unconditionally.
        let state = self.read_ledger();
        if !state.safe_to_write() {
            // Nothing is deleted either. Expiring files while unable to
            // record that they are gone would leave the ledger describing a
            // trash folder that no longer matches it, which is a slower
            // version of the same loss.
            return 0;
        }
        let items = state.entries();
        let cutoff = self.cfg.keep_days * 86_400;
        let (old, keep): (Vec<Discarded>, Vec<Discarded>) =
            items.into_iter().partition(|i| t.saturating_sub(i.at) >= cutoff);
        // Written FIRST, then the files removed. If the write fails, nothing
        // has been deleted and the ledger still matches the folder; the other
        // order can delete files and then fail to record it.
        if self.write_ledger(&keep).is_err() {
            return 0;
        }
        // A removal that FAILS puts the entry back.
        //
        // This was `let _ = std::fs::remove_file(&i.held)`, and the discarded
        // error was not a rare case: `remove_file` fails on a directory
        // every time, on every platform, and every single thing `reclaim`
        // offers is a directory. So the record was dropped and the folder
        // stayed — invisible to `undo`, invisible to the next `expire`, and
        // taking up exactly as much space as before it was "reclaimed".
        //
        // Two ledger writes rather than one is the price. The first keeps the
        // crash-safety the comment above describes; the second is what stops
        // a folder becoming orphaned bytes. A crash between them loses the
        // record of something still on disk, which is the old behaviour — but
        // it needs an interleaving, where the old behaviour needed only a
        // folder.
        let mut gone = 0usize;
        let mut stuck: Vec<Discarded> = Vec::new();
        for i in old {
            match remove_whatever(Path::new(&i.held)) {
                Ok(()) => gone += 1,
                Err(_) => stuck.push(i),
            }
        }
        if !stuck.is_empty() {
            let mut back = keep;
            back.extend(stuck);
            back.sort_by_key(|i| i.id);
            crate::heard!(self.write_ledger(&back));
        }
        gone
    }

    /// What is in the trash that the ledger cannot account for.
    ///
    /// The other half of the same problem. Anything whose removal failed
    /// while its record was being dropped — every reclaimed folder, until the
    /// fix above — is still sitting in `data/trash` with nothing pointing at
    /// it: `undo` cannot find it, `expire` never looks at it again, and the
    /// space it was supposed to free is gone for good.
    ///
    /// Reported rather than deleted. This is a holding pen for things
    /// somebody decided to get rid of, and a function that quietly removed
    /// whatever it did not recognise in there would be the wrong instinct in
    /// the one folder where being wrong is unrecoverable.
    pub fn unaccounted(&self) -> Vec<(PathBuf, u64)> {
        let known: std::collections::BTreeSet<PathBuf> =
            self.ledger().into_iter().map(|i| PathBuf::from(i.held)).collect();
        let mut out = Vec::new();
        let Ok(rd) = std::fs::read_dir(&self.cfg.dir) else { return out };
        for e in rd.flatten() {
            let p = e.path();
            // The ledger itself and its in-progress copy are not strays.
            if p == self.ledger_path()
                || p.extension().and_then(|x| x.to_str()) == Some("writing")
            {
                continue;
            }
            if known.contains(&p) {
                continue;
            }
            let mut bytes = 0u64;
            size_on_disk(&p, &mut bytes);
            out.push((p, bytes));
        }
        out.sort_by_key(|b| std::cmp::Reverse(b.1));
        out
    }
}

#[cfg(test)]
mod resolved_dir_tests {
    use super::*;

    #[test]
    fn a_relative_dir_is_resolved_under_the_store_root() {
        let cfg = BackupConfig { dir: "data/backups".into(), ..BackupConfig::default() };
        let resolved = cfg.resolved(Path::new("/tmp/some-install"));
        // Compared as paths: Windows joins with a backslash (2 Oct 2026).
        assert_eq!(Path::new(&resolved.dir), Path::new("/tmp/some-install").join("data/backups"));
    }

    #[test]
    fn an_already_absolute_dir_is_left_alone() {
        let cfg = BackupConfig { dir: "/mnt/external/backups".into(), ..BackupConfig::default() };
        let resolved = cfg.resolved(Path::new("/tmp/some-install"));
        assert_eq!(resolved.dir, "/mnt/external/backups");
    }

    #[test]
    fn two_installs_with_different_roots_never_share_a_backup_folder() {
        let a = BackupConfig::default().resolved(Path::new("/tmp/install-a"));
        let b = BackupConfig::default().resolved(Path::new("/tmp/install-b"));
        assert_ne!(a.dir, b.dir, "each install's backups must live under its own store root");
    }
}
