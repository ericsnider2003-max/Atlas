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

#[path = "safety/restore_transaction.rs"]
mod restore_transaction;
pub fn recover_restore(state: &Path) -> Result<()> {
    let guard = crate::store::wait_for_state_transaction(state)?;
    restore_transaction::recover(&guard.root)
}

#[path = "safety/state_transactions.rs"]
mod state_transactions;

#[derive(Debug, Clone)]
pub struct ArchivedRecovery {
    pub archive: String,
    pub kind: String,
    pub record: Discarded,
}

/// Restored recovery bytes stay separate from today's trash identifiers.
/// Callers present this list for an explicit owner decision, never auto-undo.
pub fn archived_recovery(state: &Path) -> Result<Vec<ArchivedRecovery>> {
    let guard = crate::store::state_transaction(state)?;
    // Restore journals bind archive paths to the canonical locked root.
    // Use that same spelling here (Windows canonical paths include \\?\).
    let state = guard.root.as_path();
    let mut result = Vec::new();
    for entry in std::fs::read_dir(state)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with(".restored-recovery-.restoring-") { continue; }
        let metadata = std::fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() { return Err(AtlasError::Platform("a recovery archive has an unsafe type".into())); }
        for kind in ["trash", "configured-trash"] {
            let folder = entry.path().join(kind);
            if !folder.exists() { continue; }
            if std::fs::symlink_metadata(&folder)?.file_type().is_symlink() { return Err(AtlasError::Platform("a recovery archive has an unsafe path".into())); }
            let trash = Trash::new(TrashConfig { dir: folder.display().to_string(), keep_days: u64::MAX });
            let records = match trash.read_ledger() {
                LedgerState::Fresh => continue,
                LedgerState::Read(records) => records,
                _ => return Err(AtlasError::Platform("an archived recovery ledger cannot be read; its files were retained".into())),
            };
            let mut ids = std::collections::BTreeSet::new();
            for record in records {
                let held = Path::new(&record.held);
                if !ids.insert(record.id) || held.parent() != Some(folder.as_path()) || held.file_name().is_none() {
                    return Err(AtlasError::Platform("an archived recovery record points outside its archive; nothing was moved".into()));
                }
                match std::fs::symlink_metadata(held) {
                    Ok(metadata) if metadata.file_type().is_symlink() => return Err(AtlasError::Platform("an archived recovery file is a symbolic link; nothing was moved".into())),
                    Ok(_) => {},
                    // A completed return whose final ledger write failed can
                    // be retried to finish its receipt without moving again.
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound && record.returning && Path::new(&record.original).exists() => {},
                    Err(error) => return Err(error.into()),
                }
                result.push(ArchivedRecovery { archive: name.clone(), kind: kind.into(), record });
            }
        }
    }
    result.sort_by(|a, b| (&a.archive, &a.kind, a.record.id).cmp(&(&b.archive, &b.kind, b.record.id)));
    Ok(result)
}

pub fn return_archived_file(state: &Path, archive: &str, kind: &str, id: u64) -> Result<String> {
    let _guard = crate::store::state_transaction(state)?;
    // Resolve exclusively through the validated inventory. Never accept a
    // caller-supplied folder path or reuse a live trash identifier.
    let selected = archived_recovery(state)?.into_iter().find(|entry| entry.archive == archive && entry.kind == kind && entry.record.id == id)
        .ok_or_else(|| AtlasError::Platform("that archived recovery file is no longer available".into()))?;
    let folder = Path::new(&selected.record.held).parent().unwrap();
    let trash = Trash::new(TrashConfig { dir: folder.display().to_string(), keep_days: u64::MAX });
    trash.undo(id).map(|record| format!("Returned {} from the saved recovery archive.", record.original))
}

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
const BACKUP_MANIFEST: &str = ".atlas-backup-manifest.json";
static NEXT_SAFETY_OPERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[derive(Serialize, Deserialize)]
struct BackupEntry { relative: PathBuf, bytes: u64, sha256: String }
#[derive(Serialize, Deserialize)]
struct BackupManifest { version: u32, entries: Vec<BackupEntry>, excluded: Vec<String>, #[serde(default)] roots: Vec<(String, String)>, #[serde(default)] directories: Vec<PathBuf> }
const OWNED_OUTPUTS: &str = ".atlas-owned-outputs";
const OWNED_FOLDERS: [&str; 5] = ["notes", "calls", "reading", "trash", "recovery"];

/// Bind configured roots durably before generic writers can use them. A folder
/// belonging to another Atlas root or linked outside its declared root fails.
pub fn check_configured_scope(store: &crate::store::Store, notes: &Path, trash: &Path) -> Result<()> {
    crate::store::bind_owned_output(store.root(), notes)?;
    crate::store::bind_owned_output(store.root(), trash)?;
    let state = std::fs::canonicalize(store.root())?;
    for actual in [notes, trash] {
        let actual = std::fs::canonicalize(actual)?;
        if state.starts_with(&actual) { return Err(AtlasError::Platform("a configured output root contains the state root; backup/restore was stopped before copying".into())); }
    }
    Ok(())
}

fn same_folder(first: &Path, second: &Path) -> bool { first == second || std::fs::canonicalize(first).ok().zip(std::fs::canonicalize(second).ok()).is_some_and(|(first, second)| first == second) }

fn hold_output_children(folder: &Path, leases: &mut Vec<std::fs::File>) -> Result<()> {
    for entry in std::fs::read_dir(folder)? {
        let path = entry?.path(); let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() { return Err(AtlasError::Platform(format!("backup won't follow the symbolic link at {}", path.display()))); }
        if metadata.is_dir() {
            if path.join(".atlas-output-write.lock").try_exists()? { leases.push(crate::store::owned_output_lease(&path, true)?); }
            hold_output_children(&path, leases)?;
        }
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<String> {
    use std::io::Read;
    use sha2::{Digest, Sha256};
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.file_type().is_symlink() { return Err(AtlasError::Platform("a snapshot file is not a regular file".into())); }
    let mut file = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop { let n = file.read(&mut buffer)?; if n == 0 { break; } hash.update(&buffer[..n]); }
    Ok(format!("{:x}", hash.finalize()))
}

fn copy_file_durably(source: &Path, destination: &Path) -> std::io::Result<u64> {
    let meta = std::fs::symlink_metadata(source)?;
    if !meta.is_file() || meta.file_type().is_symlink() { return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "snapshot source is not a regular file")); }
    let mut input = std::fs::File::open(source)?;
    let mut output = std::fs::OpenOptions::new().write(true).create_new(true).open(destination)?;
    let copied = std::io::copy(&mut input, &mut output)?;
    if copied != meta.len() { return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "snapshot source changed while it was copied")); }
    // Windows FlushFileBuffers needs the write handle, not File::open's
    // read-only handle. Keeping this handle also supports read-only sources.
    output.sync_all()?;
    Ok(copied)
}

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
    let store = crate::store::Store::new(state.to_path_buf());
    back_up_with_inputs(state, cfg, t, &store.notes_dir(), &store.trash_dir())
}

pub fn back_up_with_inputs(state: &Path, cfg: &BackupConfig, t: u64, notes: &Path, trash: &Path) -> Result<Backup> {
    if !state.is_dir() {
        return Err(AtlasError::Platform(format!("nothing at {}", state.display())));
    }
    if state.join(OWNED_OUTPUTS).try_exists()? { return Err(AtlasError::Platform("the reserved backup output folder already exists in live state; no backup was written".into())); }
    std::fs::create_dir_all(&cfg.dir)?;
    let _publishing = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(PathBuf::from(&cfg.dir).join(".backup-publishing.lock"))?;
    _publishing.try_lock().map_err(|e| AtlasError::Platform(format!("another backup is being published ({e}); no backup was changed")))?;
    let dest = PathBuf::from(&cfg.dir).join(format!("state-{t}"));
    if std::fs::symlink_metadata(&dest).is_ok() {
        return Err(AtlasError::Platform("a backup already exists at that timestamp; it was left intact".into()));
    }
    let staging = PathBuf::from(&cfg.dir).join(format!(".part-backup-{t}-{}-{}", std::process::id(), NEXT_SAFETY_OPERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    std::fs::create_dir(&staging)?;

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
    let copied = (|| -> Result<()> {
        let _snapshot = crate::store::wait_for_state_transaction(state)?;
        recover_restore(state)?;
        // Recordings retain this lease until their header and bytes are closed.
        // Refuse promptly rather than copy a half-written WAV or stall a call.
        let store = crate::store::Store::new(state.to_path_buf());
        check_configured_scope(&store, notes, trash)?;
        let data = store.data_dir();
        let mut roots: Vec<(String, PathBuf)> = OWNED_FOLDERS.into_iter().map(|folder| (folder.to_owned(), data.join(folder))).collect();
        if !same_folder(notes, &store.notes_dir()) { roots.push(("configured-notes".into(), notes.to_path_buf())); }
        if !same_folder(trash, &store.trash_dir()) { roots.push(("configured-trash".into(), trash.to_path_buf())); }
        for (index, (_, source)) in roots.iter().enumerate() {
            let Ok(source) = std::fs::canonicalize(source) else { continue };
            for (_, other) in &roots[index + 1..] {
                let Ok(other) = std::fs::canonicalize(other) else { continue };
                if source.starts_with(&other) || other.starts_with(&source) { return Err(AtlasError::Platform("configured Atlas output roots overlap; backup/restore needs an unambiguous root for each file and was stopped".into())); }
            }
        }
        let mut output_leases = Vec::new();
        for (_, source) in &roots {
            if source.is_dir() {
                output_leases.push(crate::store::owned_output_lease(&source, true)?);
                hold_output_children(&source, &mut output_leases)?;
            }
        }
        let mut trash_leases = Vec::new();
        for (kind, source) in &roots { if kind.ends_with("trash") && source.is_dir() { trash_leases.push(Trash::new(TrashConfig { dir: source.display().to_string(), keep_days: 30 }).lock()?); } }
        let excluded: Vec<PathBuf> = roots.iter().filter_map(|(_, source)| std::fs::canonicalize(source).ok()).collect();
        copy_state_into_skipping(state, &staging, &skip, &excluded, &mut files, &mut bytes)?;
        for (folder, source) in &roots {
            if source.is_dir() {
                copy_state_into_skipping(source, &staging.join(OWNED_OUTPUTS).join(folder), &skip, &[], &mut files, &mut bytes)?;
            }
        }
        let mut included = Vec::new();
        gather(&staging, Path::new(""), &mut included)?;
        let entries = included.into_iter().map(|(path, relative)| Ok(BackupEntry {
            relative, bytes: std::fs::metadata(&path)?.len(), sha256: hash_file(&path)?,
        })).collect::<Result<Vec<_>>>()?;
        let mut directories = Vec::new();
        gather_directories(&staging, Path::new(""), &mut directories)?;
        let manifest = BackupManifest { version: 2, entries, directories, excluded: vec![
            "state root/operation/access lock metadata; active restore staging; unfinished atomic writes".into(),
            "please_stop and hub_door.json are live process controls; release_download is a replaceable installer download cache".into(),
            "tray/frames-* and updater probes are derived/transient; data logs/cache and external originals held by reference are outside this backup".into(),
            "Atlas-owned data notes/calls/reading/trash/recovery are included; restore preserves live trash and the current household key, archiving their saved recovery copies".into(),
        ], roots: roots.into_iter().filter(|(_, source)| source.is_dir()).map(|(kind, source)| (kind, source.display().to_string())).collect() };
        crate::store::write_json(&staging.join(BACKUP_MANIFEST), &manifest)?;
        Ok(())
    })();
    match copied.and_then(|_| {
        if std::fs::symlink_metadata(&dest).is_ok() { return Err(AtlasError::Platform("the dated backup already exists; it was left intact".into())); }
        // All Atlas publishers hold the same backup-directory OS lock. This
        // publishes a complete tree in one rename; nothing partial is listed.
        std::fs::rename(&staging, &dest)?;
        Ok(())
    }) {
        Ok(()) => Ok(Backup { path: dest, at: t, files: Some(files), bytes }),
        Err(e) => {
            // A backup that failed is not a backup. Left on disk it has a
            // recent timestamp, which is worse than no backup at all.
            crate::heard!(std::fs::remove_dir_all(&staging));
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
    excluded: &[PathBuf],
    files: &mut usize,
    bytes: &mut u64,
) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let p = e.path();
        if std::fs::canonicalize(&p).ok().is_some_and(|path| excluded.contains(&path)) { continue; }
        let Some(name) = p.file_name().map(|n| n.to_owned()) else { continue;
        };
        let name_text = name.to_string_lossy();
        // A custom store puts data/ inside its root. Those output trees still
        // use the reserved layout, so live trash/key material is never restored
        // accidentally through the ordinary state-file plan.
        if from.file_name().is_some_and(|name| name == "data")
            && from.parent().is_some_and(|parent| parent.join(".atlas-state-root").is_file())
            && OWNED_FOLDERS.contains(&name_text.as_ref()) { continue; }
        if name_text == ".atlas-state-root"
            || name_text == BACKUP_MANIFEST
            || name_text == ".restore-journal.json"
            || name_text == "please_stop"
            || name_text == "hub_door.json"
            || name_text == "release_download"
            || (name_text.starts_with(".health-check-") && name_text.ends_with(".tmp"))
            || (from.file_name().is_some_and(|name| name == "tray") && name_text.starts_with("frames-"))
            || (!from.components().any(|c| c.as_os_str() == "tray" || c.as_os_str() == "handoffs") && name_text.ends_with(".writing"))
            || name_text == ".state-operations.lock"
            || name_text == ".state-access.lock"
            || name_text == ".atlas-output-write.lock"
            || name_text == ".atlas-output-owner"
            || name_text == "running.os.lock"
            || name_text == "running.lock"
            || name_text == "running.claim"
            || name_text == "ledger.lock"
            || name_text.starts_with(".restoring-")
        {
            continue;
        }
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
            Err(e) => return Err(e.into()),
        };
        // Not followed, for the same reason `copy_tree` does not follow them:
        // a link out of the state folder is a way out of the state folder,
        // and a backup is not the place to discover that.
        if meta.file_type().is_symlink() {
            return Err(AtlasError::Platform(format!("backup won't omit or follow the symbolic link at {}", p.display())));
        }
        if meta.is_dir() {
            let here = std::fs::canonicalize(&p).unwrap_or_else(|_| p.clone());
            if here == *skip || here.starts_with(skip) {
                continue;
            }
            copy_state_into_skipping(&p, &to.join(&name), skip, excluded, files, bytes)?;
            continue;
        }
        let tmp = to.join(format!("{PART_PREFIX}{}", name.to_string_lossy()));
        match copy_file_durably(&p, &tmp) {
            Ok(_) => {}
            // Same reasoning: gone between the listing and the copy.
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
        if name.starts_with(PART_PREFIX) || name == BACKUP_MANIFEST {
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
    let Ok(rd) = std::fs::read_dir(&cfg.dir) else { return out;
    };
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

/// Restore must not run beside a daemon holding stale in-memory state. A
/// stale/malformed heartbeat still cannot prove that its holder has stopped.
pub fn may_restore(store: &crate::store::Store) -> Result<()> {
    let singleton = crate::onlyone::OnlyOne::at(&store.data_dir());
    match std::fs::symlink_metadata(singleton.path()) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(_) => Err(AtlasError::Platform("I couldn't confirm Atlas is stopped, so restore hasn't changed any files. Close Atlas completely before restoring.".into())),
        Err(e) => Err(AtlasError::Platform(format!("I couldn't check whether Atlas has stopped ({e}), so restore hasn't changed any files."))),
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

pub fn restore_with_notes(backup: &Path, state: &Path, trash: &Trash, mine: &crate::household::Household, notes: &Path) -> Result<usize> {
    restore_transaction::restore_with_notes(backup, state, trash, mine, notes)
}

/// Every file under `dir`, as (full path, path relative to the root).
fn gather(dir: &Path, rel: &Path, out: &mut Vec<(PathBuf, PathBuf)>) -> Result<()> {
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        let p = e.path();
        let meta = std::fs::symlink_metadata(&p)?;
        if meta.file_type().is_symlink() {
            continue;
        }
        let Some(name) = p.file_name().map(|n| n.to_owned()) else {
            continue;
        };
        let here = rel.join(&name);
        if meta.is_dir() {
            gather(&p, &here, out)?;
        } else if name != BACKUP_MANIFEST && name != ".atlas-state-root" && name != ".restore-journal.json"
            && !name.to_string_lossy().starts_with(PART_PREFIX)
            && (rel.components().any(|c| c.as_os_str() == "tray" || c.as_os_str() == "handoffs") || !name.to_string_lossy().ends_with(".writing"))
            && name != ".state-operations.lock" && name != ".state-access.lock"
            && name != ".atlas-output-write.lock" && name != "running.os.lock" && name != "running.lock" && name != "running.claim"
            && name != ".atlas-output-owner" && name != "ledger.lock"
        {
            // A temp file from a backup that died mid-copy is not something
            // to restore.
            out.push((p, here));
        }
    }
    Ok(())
}

fn gather_directories(dir: &Path, rel: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?; let metadata = std::fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_symlink() { return Err(AtlasError::Platform("a snapshot contains a symbolic link; no restore was started".into())); }
        if metadata.is_dir() {
            let relative = rel.join(entry.file_name());
            out.push(relative.clone()); gather_directories(&entry.path(), &relative, out)?;
        }
    }
    out.sort();
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
    /// Intent was durably saved before moving. Older ledgers describe held items.
    #[serde(default)]
    pub pending: bool,
    /// Undo was durably started; reconcile a crash after the move on retry.
    #[serde(default)]
    pub returning: bool,
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
        TrashConfig {
            dir: String::new(),
            keep_days: 30,
        }
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
    let Ok(meta) = std::fs::symlink_metadata(p) else { return;
    };
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
// Recovery transfers use the same held-source primitive as sorting and undo.
// No pathname deletion or recursive source deletion is permitted.
fn move_across(from: &Path, to: &Path) -> Result<()> {
    move_without_overwrite_controlled(from, to, &|| false)
}

/// Shared no-clobber transfer for sorting and its recovery path.


#[cfg(windows)]
struct MoveCohort { objects: Vec<(PathBuf, std::fs::File, bool)> }

/// False means the native rename refused before publication (copy may follow).
/// A reported success with a wrong path/identity is a distinct retained error.
#[cfg(windows)]
pub(crate) fn rename_held_no_replace(file: &std::fs::File, to: &Path) -> Result<bool> {
    use std::os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle};
    use windows::Win32::{Foundation::{BOOLEAN, HANDLE}, Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, FileRenameInfo, FILE_RENAME_INFO, GetFileInformationByHandle, GetFinalPathNameByHandleW, GETFINALPATHNAMEBYHANDLE_FLAGS, SetFileInformationByHandle}};
    let Some(parent) = to.parent() else { return Ok(false) };
    let parent = std::fs::canonicalize(parent)?;
    let Some(name) = to.file_name() else { return Ok(false) };
    let target = parent.join(name);
    let name: Vec<u16> = target.as_os_str().encode_wide().collect();
    if name.len() >= 32_768 { return Err(AtlasError::Platform("native destination name is too long; held original retained".into())); }
    let mut expected = BY_HANDLE_FILE_INFORMATION::default();
    unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut expected) }.map_err(|e| AtlasError::Platform(format!("held source identity cannot be verified: {e}")))?;
    // Win32 path conversion reads the NUL-terminated name even though the
    // logical FileNameLength excludes that terminator. Rounded allocation
    // padding is not a terminator when the name ends exactly on its boundary.
    let bytes = std::mem::offset_of!(FILE_RENAME_INFO, FileName) + (name.len() + 1) * 2;
    let mut storage = vec![0u64; bytes.div_ceil(8)];
    let renamed = unsafe {
        let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
        (*info).Anonymous.ReplaceIfExists = BOOLEAN(0); (*info).RootDirectory = HANDLE(std::ptr::null_mut());
        (*info).FileNameLength = (name.len() * 2) as u32;
        let destination = std::ptr::addr_of_mut!((*info).FileName).cast::<u16>();
        std::ptr::copy_nonoverlapping(name.as_ptr(), destination, name.len()); destination.add(name.len()).write(0);
        SetFileInformationByHandle(HANDLE(file.as_raw_handle()), FileRenameInfo, info.cast(), bytes as u32)
    };
    if renamed.is_err() { return Ok(false); }
    let mut buffer = vec![0u16; 32_768];
    let length = unsafe { GetFinalPathNameByHandleW(HANDLE(file.as_raw_handle()), &mut buffer, GETFINALPATHNAMEBYHANDLE_FLAGS(0)) } as usize;
    if length == 0 || length >= buffer.len() { return Err(AtlasError::Platform("native rename reported success but the held object's final path cannot be verified; no cleanup was attempted".into())); }
    let actual = String::from_utf16(&buffer[..length]).map_err(|_| AtlasError::Platform("native rename final path cannot be decoded; no cleanup was attempted".into()))?;
    if !actual.eq_ignore_ascii_case(&target.to_string_lossy()) { return Err(AtlasError::Platform(format!("native rename reached unexpected held-object path {actual}; inspect this path and the saved recovery locations. No cleanup was attempted"))); }
    let is_directory = file.metadata()?.is_dir();
    let published = std::fs::OpenOptions::new().read(true).custom_flags(0x0020_0000 | if is_directory { 0x0200_0000 } else { 0 }).open(&target).map_err(|e| AtlasError::Platform(format!("native rename reached {actual} but its destination cannot be inspected ({e}); no cleanup was attempted")))?;
    let mut observed = BY_HANDLE_FILE_INFORMATION::default();
    unsafe { GetFileInformationByHandle(HANDLE(published.as_raw_handle()), &mut observed) }.map_err(|e| AtlasError::Platform(format!("native destination at {actual} lost verifiable identity ({e}); no cleanup was attempted")))?;
    if (expected.dwVolumeSerialNumber, expected.nFileIndexHigh, expected.nFileIndexLow) != (observed.dwVolumeSerialNumber, observed.nFileIndexHigh, observed.nFileIndexLow) {
        return Err(AtlasError::Platform(format!("native destination identity changed at {actual}; both objects retained without cleanup")));
    }
    Ok(true)
}
#[cfg(windows)]
impl MoveCohort {
    fn rename_file(&self, to: &Path) -> Result<bool> {
        if self.objects.len() != 1 || self.objects[0].2 { return Ok(false); }
        rename_held_no_replace(&self.objects[0].1, to)
    }
    fn hold(root: &Path, stopped: &impl Fn() -> bool) -> Result<Self> {
        use std::os::windows::fs::OpenOptionsExt;
        fn visit(path: &Path, objects: &mut Vec<(PathBuf, std::fs::File, bool)>, stop: &impl Fn() -> bool) -> Result<()> {
            if stop() { return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "file movement stopped before reservation").into()); }
            if objects.len() >= 100_000 { return Err(AtlasError::Platform("too many source objects to reserve safely".into())); }
            let before = std::fs::symlink_metadata(path)?;
            if before.file_type().is_symlink() || (!before.is_file() && !before.is_dir()) { return Err(AtlasError::Platform("source is not an ordinary file or directory".into())); }
            // DELETE is needed only by moves. No WRITE/DELETE sharing: a
            // competing writer, replacement or root rename must fail closed.
            let held = std::fs::OpenOptions::new().read(true).access_mode(0x8001_0000).share_mode(1).custom_flags(0x0020_0000 | if before.is_dir() { 0x0200_0000 } else { 0 }).open(path)?;
            let metadata = held.metadata()?;
            if metadata.file_type().is_symlink() || metadata.is_dir() != before.is_dir() || metadata.is_file() != before.is_file() { return Err(AtlasError::Platform("source changed type while being reserved".into())); }
            objects.push((path.to_path_buf(), held, metadata.is_dir()));
            if metadata.is_dir() { for entry in std::fs::read_dir(path)? { visit(&entry?.path(), objects, stop)?; } }
            Ok(())
        }
        let root = std::fs::canonicalize(root)?;
        let mut objects = Vec::new(); visit(&root, &mut objects, stopped)?; Ok(Self { objects })
    }
    fn validate(&self, stopped: &impl Fn() -> bool) -> Result<()> {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::{Foundation::HANDLE, Storage::FileSystem::{GetFinalPathNameByHandleW, GETFINALPATHNAMEBYHANDLE_FLAGS}};
        let known: std::collections::HashSet<_> = self.objects.iter().map(|(path, _, _)| path.as_path()).collect();
        for (path, held, directory) in &self.objects {
            if stopped() { return Err(AtlasError::Platform("movement stopped; inspect the recorded source and destination before retrying".into())); }
            let mut text = vec![0u16; 32_768];
            let length = unsafe { GetFinalPathNameByHandleW(HANDLE(held.as_raw_handle()), &mut text, GETFINALPATHNAMEBYHANDLE_FLAGS(0)) } as usize;
            if length == 0 || length >= text.len() { return Err(AtlasError::Platform("a held source location cannot be verified; both locations retained".into())); }
            let actual = String::from_utf16(&text[..length]).map_err(|_| AtlasError::Platform("a held source location cannot be decoded".into()))?;
            if !actual.eq_ignore_ascii_case(&path.to_string_lossy()) { return Err(AtlasError::Platform("a held source was relocated; both locations retained".into())); }
            if *directory {
                for entry in std::fs::read_dir(path)? {
                    let child = entry?.path();
                    if !known.contains(child.as_path()) { return Err(AtlasError::Platform("new source entries appeared; both locations retained for recovery".into())); }
                }
            }
        }
        Ok(())
    }
    fn verify_copy(&self, payload: &Path, stopped: &impl Fn() -> bool) -> Result<()> {
        use std::io::{Read, Seek, SeekFrom};
        use sha2::{Digest, Sha256};
        fn digest(mut file: std::fs::File, stop: &impl Fn() -> bool) -> Result<[u8; 32]> {
            file.seek(SeekFrom::Start(0))?;
            let mut hash = Sha256::new(); let mut buffer = [0u8; 64 * 1024];
            loop {
                if stop() { return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "movement stopped during content verification; original retained").into()); }
                let count = file.read(&mut buffer)?; if count == 0 { break; } hash.update(&buffer[..count]);
            }
            Ok(hash.finalize().into())
        }
        let root = &self.objects[0].0;
        for (path, held, directory) in &self.objects {
            let relative = path.strip_prefix(root).map_err(|_| AtlasError::Platform("source escaped its reserved root".into()))?;
            let copied = if relative.as_os_str().is_empty() { payload.to_path_buf() } else { payload.join(relative) };
            if *directory { if !std::fs::symlink_metadata(copied)?.is_dir() { return Err(AtlasError::Platform("copied source directory is missing".into())); } }
            else if digest(held.try_clone()?, stopped)? != digest(std::fs::File::open(copied)?, stopped)? { return Err(AtlasError::Platform("copied bytes do not match held original; source retained".into())); }
        }
        Ok(())
    }
    fn dispose(self, stopped: &impl Fn() -> bool) -> Result<()> {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::{Foundation::{BOOLEAN, HANDLE}, Storage::FileSystem::{FileDispositionInfo, FILE_DISPOSITION_INFO, SetFileInformationByHandle}};
        self.validate(stopped)?;
        for (_, held, _) in self.objects.into_iter().rev() {
            if stopped() { return Err(AtlasError::Platform("movement stopped during source cleanup; inspect both saved locations before retrying".into())); }
            let disposition = FILE_DISPOSITION_INFO { DeleteFile: BOOLEAN(1) };
            unsafe { SetFileInformationByHandle(HANDLE(held.as_raw_handle()), FileDispositionInfo, (&disposition as *const FILE_DISPOSITION_INFO).cast(), std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32) }
                .map_err(|e| AtlasError::Platform(format!("source cleanup incomplete; both locations remain in recovery: {e}")))?;
            drop(held);
        }
        Ok(())
    }
}

/// A worker transfer publishes only a complete copy. Stop never removes the
/// source or a destination owned by somebody else.
fn move_without_overwrite_controlled(from: &Path, to: &Path, stopped: &impl Fn() -> bool) -> Result<()> {
    move_controlled(from, to, stopped, false)
}

pub(crate) fn move_without_overwrite_recorded(from: &Path, to: &Path, stopped: &impl Fn() -> bool, before: &mut dyn FnMut() -> Result<()>) -> Result<()> {
    move_controlled_checked(from, to, stopped, false, None, true, Some(before))
}

pub(crate) fn copy_without_overwrite_verified(from: &Path, to: &Path, stopped: &impl Fn() -> bool, verify: &dyn Fn(&Path) -> Result<()>) -> Result<()> {
    move_controlled_checked(from, to, stopped, true, Some(verify), false, None)
}

fn move_controlled(from: &Path, to: &Path, stopped: &impl Fn() -> bool, force_copy: bool) -> Result<()> {
    move_controlled_checked(from, to, stopped, force_copy, None, true, None)
}

#[cfg(unix)]
fn atomic_move_no_replace(from: &Path, to: &Path) -> Result<()> {
    use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};
    let expected = std::fs::symlink_metadata(from)?;
    let source = std::ffi::CString::new(from.as_os_str().as_bytes()).map_err(|_| AtlasError::Platform("source path contains a null byte".into()))?;
    let destination = std::ffi::CString::new(to.as_os_str().as_bytes()).map_err(|_| AtlasError::Platform("destination path contains a null byte".into()))?;
    #[cfg(target_os = "linux")]
    let result = unsafe { libc::renameat2(libc::AT_FDCWD, source.as_ptr(), libc::AT_FDCWD, destination.as_ptr(), libc::RENAME_NOREPLACE) } as i64;
    #[cfg(target_os = "android")]
    let result = unsafe { libc::syscall(libc::SYS_renameat2, libc::AT_FDCWD, source.as_ptr(), libc::AT_FDCWD, destination.as_ptr(), 1u32) } as i64;
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    let result = unsafe { libc::renamex_np(source.as_ptr(), destination.as_ptr(), libc::RENAME_EXCL) } as i64;
    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos", target_os = "ios")))]
    return Err(AtlasError::Platform("atomic no-clobber movement is unavailable on this platform; original retained".into()));
    #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos", target_os = "ios"))]
    {
        if result != 0 { return Err(AtlasError::Platform(format!("atomic no-clobber move refused; original retained (cross-volume disposal requires a supported native cohort): {}", std::io::Error::last_os_error()))); }
        let moved = std::fs::symlink_metadata(to)?;
        if moved.dev() != expected.dev() || moved.ino() != expected.ino() { return Err(AtlasError::Platform("source identity changed during atomic movement; inspect both saved locations, no newer bytes were deleted".into())); }
        Ok(())
    }
}

fn move_controlled_checked(from: &Path, to: &Path, stopped: &impl Fn() -> bool, force_copy: bool, verify: Option<&dyn Fn(&Path) -> Result<()>>, remove_source: bool, before: Option<&mut dyn FnMut() -> Result<()>>) -> Result<()> {
    fn checkpoint(stopped: &impl Fn() -> bool) -> std::io::Result<()> {
        if stopped() { Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "file movement stopped; original retained")) } else { Ok(()) }
    }
    fn copy(from: &Path, to: &Path, stopped: &impl Fn() -> bool) -> std::io::Result<()> {
        use std::io::{Read, Write};
        checkpoint(stopped)?;
        let metadata = std::fs::symlink_metadata(from)?;
        if metadata.file_type().is_symlink() { return Err(std::io::Error::other("symbolic links cannot be safely copied")); }
        if metadata.is_dir() {
            std::fs::create_dir(to)?;
            for entry in std::fs::read_dir(from)? {
                checkpoint(stopped)?;
                let entry = entry?;
                copy(&entry.path(), &to.join(entry.file_name()), stopped)?;
            }
        } else if metadata.is_file() {
            let mut input = std::fs::File::open(from)?;
            let mut output = std::fs::OpenOptions::new().write(true).create_new(true).open(to)?;
            let mut buffer = [0u8; 64 * 1024];
            let mut bytes = 0u64;
            loop {
                checkpoint(stopped)?;
                let count = input.read(&mut buffer)?;
                if count == 0 { break; }
                output.write_all(&buffer[..count])?;
                bytes += count as u64;
            }
            if bytes != metadata.len() { return Err(std::io::Error::other("source changed during copying")); }
            output.sync_all()?;
        } else { return Err(std::io::Error::other("unsupported file type")); }
        Ok(())
    }
    checkpoint(stopped)?;
    let metadata = std::fs::symlink_metadata(from)?;
    if metadata.file_type().is_symlink() { return Err(AtlasError::Platform("I won't move a symbolic link".into())); }
    match std::fs::symlink_metadata(to) {
        Ok(_) => return Err(AtlasError::Platform(format!("{} already exists; nothing was overwritten", to.display()))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
        Err(error) => return Err(error.into()),
    }
    #[cfg(windows)]
    let cohort = if remove_source { Some(MoveCohort::hold(from, stopped)?) } else { None };
    if let Some(before) = before { before()?; }
    checkpoint(stopped)?;
    #[cfg(windows)]
    if !force_copy {
        if let Some(cohort) = &cohort { if cohort.rename_file(to)? { return Ok(()); } }
    }
    #[cfg(unix)]
    if remove_source && !force_copy { checkpoint(stopped)?; return atomic_move_no_replace(from, to); }
    #[cfg(not(any(windows, unix)))]
    if remove_source { return Err(AtlasError::Platform("verified source disposal is unavailable on this platform; original retained".into())); }
    let parent = to.parent().ok_or_else(|| AtlasError::Platform("destination has no parent".into()))?;
    let temporary = parent.join(format!(".atlas-moving-{}-{}", std::process::id(), NEXT_SAFETY_OPERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    // Reserve a private parent first; cleanup therefore never removes a
    // pre-existing file, even if another process guesses a temporary name.
    std::fs::create_dir(&temporary)?;
    let payload = temporary.join("payload");
    let result = (|| -> Result<()> {
        copy(from, &payload, stopped)?;
        #[cfg(windows)]
        if let Some(cohort) = &cohort { cohort.validate(stopped)?; }
        checkpoint(stopped)?;
        if let Some(verify) = verify { verify(&payload)?; }
        #[cfg(windows)]
        if let Some(cohort) = &cohort { cohort.validate(stopped)?; cohort.verify_copy(&payload, stopped)?; }
        checkpoint(stopped)?;
        #[cfg(unix)]
        if remove_source {
            use std::os::unix::fs::MetadataExt;
            let fingerprint = |path: &Path| -> Result<String> {
                if std::fs::symlink_metadata(path)?.is_dir() {
                    crate::tune::folder_fingerprint(path, &|| stopped()).map(|(hash, _)| hash).map_err(AtlasError::Platform)
                } else { crate::tune::file_fingerprint_controlled(path, stopped).map_err(AtlasError::Platform) }
            };
            let expected_hash = fingerprint(&payload)?;
            let source = std::fs::symlink_metadata(from)?;
            if source.dev() != metadata.dev() || source.ino() != metadata.ino() || fingerprint(from)? != expected_hash {
                return Err(AtlasError::Platform("source or private copy changed during verification; original retained".into()));
            }
            checkpoint(stopped)?;
            // Publish the actual original atomically, never copy then unlink.
            // EXDEV refuses with the entire original still at its source.
            atomic_move_no_replace(from, to)?;
            let moved = std::fs::symlink_metadata(to)?;
            if moved.dev() != metadata.dev() || moved.ino() != metadata.ino() || fingerprint(to)? != expected_hash {
                return Err(AtlasError::Platform("original changed during atomic publication; inspect both recorded locations, newer bytes were preserved".into()));
            }
            return Ok(());
        }
        if metadata.is_file() {
            std::fs::hard_link(&payload, to)?;
        } else {
            #[cfg(windows)]
            std::fs::rename(&payload, to)?;
            #[cfg(not(windows))]
            return Err(AtlasError::Platform("safe directory publication is unavailable on this platform; original retained".into()));
        }
        // A stopped/failed cleanup keeps the durable source/destination intent.
        // Dispose exact held objects only after complete verified publication.
        if remove_source {
            #[cfg(windows)]
            if let Some(cohort) = cohort { cohort.dispose(stopped)?; }
        }
        Ok(())
    })();
    crate::heard!(std::fs::remove_dir_all(&temporary));
    result
}

#[derive(Debug, Clone)]
pub struct Trash {
    pub cfg: TrashConfig,
}

struct TrashGuard { _file: std::fs::File, _state: Option<crate::store::StateGuard> }

impl Trash {
    pub fn new(cfg: TrashConfig) -> Trash {
        Trash { cfg }
    }

    fn ledger_path(&self) -> PathBuf {
        PathBuf::from(&self.cfg.dir).join("ledger.json")
    }

    /// The OS releases this lock even if Atlas exits during a file operation.
    fn lock(&self) -> Result<TrashGuard> {
        let state = crate::store::state_root_for(Path::new(&self.cfg.dir))?.map(|root| crate::store::try_state_guard(&root)).transpose()?;
        std::fs::create_dir_all(&self.cfg.dir)?;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(PathBuf::from(&self.cfg.dir).join("ledger.lock"))?;
        file.try_lock().map_err(|error| match error {
            std::fs::TryLockError::WouldBlock => std::io::Error::new(std::io::ErrorKind::WouldBlock, "the trash ledger is being changed elsewhere; retry later"),
            std::fs::TryLockError::Error(error) => std::io::Error::new(error.kind(), format!("the trash ledger lock failed ({error})")),
        })?;
        Ok(TrashGuard { _file: file, _state: state })
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
        use std::io::Write;
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        crate::store::rename_patiently(&tmp, &self.ledger_path())?;
        Ok(())
    }

    /// Move a file out of the way instead of deleting it.
    pub fn take(&self, path: &Path, why: &str) -> Result<Discarded> {
        let _lock = self.lock()?;
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
        let mut id = items
            .iter()
            .map(|i| i.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| AtlasError::Platform("the trash record has no unused ids".into()))?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        // Id-prefixed, so two files with the same name don't collide.
        let mut held = PathBuf::from(&self.cfg.dir).join(format!("{id}-{name}"));
        while std::fs::symlink_metadata(&held).is_ok() {
            id = id
                .checked_add(1)
                .ok_or_else(|| AtlasError::Platform("the trash record has no unused ids".into()))?;
            held = PathBuf::from(&self.cfg.dir).join(format!("{id}-{name}"));
        }

        let mut d = Discarded {
            id,
            original: path.display().to_string(),
            held: held.display().to_string(),
            why: why.to_string(),
            at: now(),
            pending: true,
            returning: false,
        };
        // Write ahead: a failed save leaves the original untouched; a crash
        // after moving leaves both paths in the ledger for undo to reconcile.
        items.push(d.clone());
        self.write_ledger(&items)?;
        move_across(path, &held)?;
        d.pending = false;
        if let Some(entry) = items.last_mut() {
            entry.pending = false;
        }
        if let Err(e) = self.write_ledger(&items) {
            return Err(AtlasError::Platform(format!("The file is kept at {} and its recovery intent is saved, but I couldn't finish the trash record ({e}). Undo #{id} can put it back.", held.display())));
        }
        Ok(d)
    }

    /// Put the last thing back.
    pub fn undo_last(&self) -> Result<Discarded> {
        let items = self.ledger();
        let last = items
            .last()
            .cloned()
            .ok_or_else(|| AtlasError::Platform("there's nothing to undo".into()))?;
        self.undo(last.id)
    }

    pub fn undo(&self, id: u64) -> Result<Discarded> {
        let _lock = self.lock()?;
        let state = self.read_ledger();
        if let Some(why) = state.trouble() {
            return Err(AtlasError::Platform(why));
        }
        let mut items = state.entries();
        let pos = items
            .iter()
            .position(|i| i.id == id)
            .ok_or_else(|| AtlasError::Platform(format!("nothing with id {id}")))?;
        let item = items[pos].clone();
        let original = PathBuf::from(&item.original);
        let held = PathBuf::from(&item.held);
        // Intent before take, or undo completed before its final ledger save.
        if !held.exists() && original.exists() && (item.pending || item.returning) {
            items.remove(pos);
            self.write_ledger(&items)?;
            return Ok(item);
        }

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
        items[pos].returning = true;
        self.write_ledger(&items)?;
        move_across(&held, &original)?;
        items.remove(pos);
        self.write_ledger(&items)?;
        Ok(item)
    }

    /// Permanently remove anything past its keep window.
    pub fn expire(&self, t: u64) -> usize {
        let _lock = match self.lock() {
            Ok(lock) => lock,
            Err(AtlasError::Io(error)) if error.kind() == std::io::ErrorKind::WouldBlock => return 0,
            Err(error) => {
                crate::kept!(Err::<(), _>(AtlasError::Platform(format!("Trash cleanup couldn't lock its recovery folder at {} ({error}). No files expired. Check that the trash folder and ledger.lock are writable ordinary files/folders, then retry.", self.cfg.dir))));
                return 0;
            }
        };
        // Originals referenced by a restore journal must outlive any keep window.
        if _lock._state.as_ref().is_some_and(|guard| guard.root.join(".restore-journal.json").exists()) { return 0; }
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
            if let Some(error) = state.trouble() { crate::kept!(Err::<(), _>(AtlasError::Platform(error))); }
            return 0;
        }
        let items = state.entries();
        let cutoff = self.cfg.keep_days.saturating_mul(86_400);
        let (old, keep): (Vec<Discarded>, Vec<Discarded>) =
            items.iter()
            .cloned().partition(|i| !i.pending && !i.returning && t.saturating_sub(i.at) >= cutoff);
        // Check writability while keeping every recovery path. Files that
        // cannot be removed remain recorded even if the process exits next.
        if crate::kept!(self.write_ledger(&items)).is_none() {
            return 0;
        }
        // Remove the record only after deletion, so interruption leaves a
        // stale record of a deleted file rather than an unrecorded live file.
        let mut gone = 0usize;
        let mut stuck: Vec<Discarded> = Vec::new();
        for i in old {
            match remove_whatever(Path::new(&i.held)) {
                Ok(()) => gone += 1,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => stuck.push(i),
            }
        }
        let mut back = keep;
            back.extend(stuck);
            back.sort_by_key(|i| i.id);
            crate::heard!(self.write_ledger(&back));
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
        let Ok(rd) = std::fs::read_dir(&self.cfg.dir) else { return out;
        };
        for e in rd.flatten() {
            let p = e.path();
            // The ledger itself and its in-progress copy are not strays.
            if p == self.ledger_path()
                || p.file_name().is_some_and(|n| n == "ledger.lock")
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
