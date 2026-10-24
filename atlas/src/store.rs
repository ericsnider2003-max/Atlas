//! Durable local state. Atomic writes: temp file then rename, so a crash
//! mid-save leaves the previous good copy rather than a truncated one.

use crate::error::Result;
use serde::{de::DeserializeOwned, Serialize};
use std::path::{Path, PathBuf};

#[path = "store/state_lock.rs"]
mod state_lock;
pub use state_lock::AccessGuard;
pub use state_lock::StateGuard;
pub fn state_access(root: &Path, exclusive: bool) -> std::io::Result<AccessGuard> {
    state_lock::access(root, exclusive)
}
/// Fail closed before owner/permission reads if an earlier restore stopped.
pub fn begin_state_command(root: &Path, restoring: bool) -> Result<AccessGuard> {
    let access = state_access(root, restoring)?;
    if access.root.join(".restore-journal.json").try_exists()? {
        if restoring {
            crate::safety::recover_restore(&access.root)?;
            return Ok(access);
        }
        let root = access.root.clone();
        drop(access);
        let exclusive = state_access(&root, true)?;
        crate::safety::recover_restore(&root)?;
        drop(exclusive);
        return Ok(state_access(&root, false)?);
    }
    Ok(access)
}

pub fn state_transaction(root: &Path) -> std::io::Result<StateGuard> {
    let guard = state_lock::try_acquire(root)?;
    if guard.root.join(".restore-journal.json").try_exists()? {
        crate::safety::recover_restore(&guard.root)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
    }
    Ok(guard)
}
/// For the control loop, which must not wait on the state lock for long: try
/// for a quarter of a second, because whatever holds it (a worker writing one
/// file) lets go in milliseconds. A refusal after that is real.
pub fn state_transaction_briefly(root: &Path) -> std::io::Result<StateGuard> {
    for _ in 0..25 {
        match state_transaction(root) {
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(10))
            }
            other => return other,
        }
    }
    state_transaction(root)
}
pub fn wait_for_state_transaction(root: &Path) -> std::io::Result<StateGuard> {
    state_lock::acquire(root)
}
pub(crate) fn try_state_guard(root: &Path) -> std::io::Result<StateGuard> {
    state_lock::try_acquire(root)
}
pub fn state_root_for(path: &Path) -> std::io::Result<Option<PathBuf>> {
    state_lock::root_for(path)
}
pub fn bind_owned_output(root: &Path, folder: &Path) -> std::io::Result<()> {
    state_lock::bind_output(root, folder)
}

pub fn write_whole_in_state(root: &Path, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let _guard = state_transaction(root)?;
    write_whole(path, bytes)
}

pub fn remove_state_file(root: &Path, path: &Path) -> std::io::Result<()> {
    let _guard = state_transaction(root)?;
    std::fs::remove_file(path)
}

pub fn write_owned_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write_whole(path, bytes)
}

/// Worker-only retry: the control loop uses the nonblocking writer above.
/// A snapshot may briefly own the state lock; stop is checked before every
/// attempt and no destination is changed while an attempt is refused.
pub fn write_owned_file_until(
    path: &Path,
    bytes: &[u8],
    stop: &dyn Fn() -> bool,
) -> std::io::Result<()> {
    let started = std::time::Instant::now();
    loop {
        if stop() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "stopped before saving Atlas output",
            ));
        }
        match write_owned_file(path, bytes) {
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    && started.elapsed() < std::time::Duration::from_secs(10) =>
            {
                std::thread::sleep(std::time::Duration::from_millis(20))
            }
            result => return result,
        }
    }
}

pub fn remove_owned_file(path: &Path) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or(Path::new("."));
    let _output = if parent.join(".atlas-output-write.lock").try_exists()? {
        Some(owned_output_lease(parent, true)?)
    } else {
        None
    };
    let _state = state_lock::for_path(path)?;
    std::fs::remove_file(path)
}

/// A live recording holds a shared file lease; a backup takes it exclusively
/// and fails promptly while a WAV's length/header are still being written.
pub fn owned_output_lease(folder: &Path, snapshot: bool) -> std::io::Result<std::fs::File> {
    std::fs::create_dir_all(folder)?;
    let path = folder.join(".atlas-output-write.lock");
    if std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "the output lock is a symbolic link",
        ));
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    let locked = if snapshot {
        file.try_lock()
    } else {
        file.try_lock_shared()
    };
    locked.map_err(|e| std::io::Error::new(std::io::ErrorKind::WouldBlock, format!("Atlas output is still being recorded or copied ({e}); try backup again after recording finishes")))?;
    Ok(file)
}

/// Bumped whenever a stored structure changes shape incompatibly.
///
/// Without this, upgrading Atlas silently discards everything it learned —
/// the file fails to parse, `load` returns the default, and months of
/// approval history and scheduled posts vanish with no error. Now a mismatch
/// preserves the old file and says so.
pub const SCHEMA: u32 = 1;

#[derive(serde::Serialize, serde::Deserialize)]
struct Envelope<T> {
    schema: u32,
    data: T,
}

/// What a file held that this version of Atlas doesn't know about (5 Oct
/// 2026, Q12): fields a newer Atlas wrote. Reading drops them, since serde
/// keeps only the fields a type has, and the next save wrote the file without
/// them -- so going back to an older version (a rollback) and forward again
/// lost whatever the newer one had added, silently.
///
/// Kept as the parts of the file the type couldn't account for, found by
/// reading the file into the type, writing it back out, and comparing; and
/// put back when the record is saved. Only keys the type has never heard
/// of: one it knows and left out (an option you cleared) is never brought
/// back. Lists are matched element by element through their `id` when they
/// have one, else by position when the list is the same length; an element
/// that's gone takes its unknown fields with it.
#[derive(Debug, Clone, PartialEq)]
pub enum Unknown {
    /// An object's keys this version lacks, and what's unknown inside the
    /// keys it has.
    Keys(Vec<(String, serde_json::Value)>, Vec<(String, Unknown)>),
    /// What's unknown inside a list's elements: (its `id`, its position,
    /// the list's length then, what's unknown in it).
    Items(Vec<(Option<serde_json::Value>, usize, usize, Unknown)>),
}

/// The parts of `raw` (what the file holds) that `known` (the same value
/// after a round trip through this version's type) doesn't.
pub fn unknown_part(raw: &serde_json::Value, known: &serde_json::Value) -> Option<Unknown> {
    use serde_json::Value;
    match (raw, known) {
        (Value::Object(r), Value::Object(k)) => {
            let mut new_keys = Vec::new();
            let mut inner = Vec::new();
            for (key, v) in r {
                match k.get(key) {
                    None => new_keys.push((key.clone(), v.clone())),
                    Some(kv) => {
                        if let Some(u) = unknown_part(v, kv) {
                            inner.push((key.clone(), u));
                        }
                    }
                }
            }
            (!new_keys.is_empty() || !inner.is_empty()).then_some(Unknown::Keys(new_keys, inner))
        }
        (Value::Array(r), Value::Array(k)) => {
            let mut items = Vec::new();
            for (i, rv) in r.iter().enumerate() {
                let id = rv.get("id").cloned();
                let counterpart = match &id {
                    Some(id) => k.iter().find(|kv| kv.get("id") == Some(id)),
                    None if r.len() == k.len() => k.get(i),
                    None => None,
                };
                if let Some(u) = counterpart.and_then(|kv| unknown_part(rv, kv)) {
                    items.push((id, i, r.len(), u));
                }
            }
            (!items.is_empty()).then_some(Unknown::Items(items))
        }
        _ => None,
    }
}

/// Put what was unknown back into `target` (what this version is saving).
pub fn put_back_unknown(u: &Unknown, target: &mut serde_json::Value) {
    use serde_json::Value;
    match (u, target) {
        (Unknown::Keys(new_keys, inner), Value::Object(t)) => {
            for (k, v) in new_keys {
                t.entry(k.clone()).or_insert_with(|| v.clone());
            }
            for (k, sub) in inner {
                if let Some(tv) = t.get_mut(k) {
                    put_back_unknown(sub, tv);
                }
            }
        }
        (Unknown::Items(items), Value::Array(t)) => {
            let len_now = t.len();
            for (id, i, len_then, sub) in items {
                let el = match id {
                    Some(id) => t.iter_mut().find(|e| e.get("id") == Some(id)),
                    None if *len_then == len_now => t.get_mut(*i),
                    None => None,
                };
                if let Some(el) = el {
                    put_back_unknown(sub, el);
                }
            }
        }
        _ => {}
    }
}

/// What each record's file held that this version doesn't know, by path,
/// from when this process last read it. Empty for nearly every record.
static UNKNOWN: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<PathBuf, Unknown>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// Is this `…/data/state/profiles`?
fn ends_with_data_state_profiles(p: &Path) -> bool {
    let mut it = p.components().rev().filter_map(|c| c.as_os_str().to_str());
    it.next() == Some("profiles") && it.next() == Some("state") && it.next() == Some("data")
}

/// What this process last wrote (or found already there) at each state
/// file: the content's hash, and the file's size and modified time just
/// after. See `Store::save`.
type Written = std::collections::HashMap<PathBuf, (u64, u64, Option<std::time::SystemTime>)>;
static WRITTEN: std::sync::LazyLock<std::sync::Mutex<Written>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(Written::new()));

/// Records kept by `Store::load_kept`: size and modified time when read, and
/// the value.
type Kept = std::collections::HashMap<
    PathBuf,
    (
        u64,
        Option<std::time::SystemTime>,
        std::sync::Arc<dyn std::any::Any + Send + Sync>,
    ),
>;
static KEPT: std::sync::LazyLock<std::sync::Mutex<Kept>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(Kept::new()));

fn content_hash(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

fn remember_written(path: &Path, hash: u64) {
    let Ok(m) = std::fs::metadata(path) else {
        return;
    };
    if let Ok(mut w) = WRITTEN.lock().or_else(crate::crash::unpoison) {
        w.insert(path.to_path_buf(), (hash, m.len(), m.modified().ok()));
    }
    remember_seen(path, Some((m.len(), m.modified().ok())));
}

/// Note what the file at `path` (current format, `text`) holds that `T`
/// doesn't know, for `save` to put back (Q12). Costs one more round trip
/// through JSON per read, and records nothing in the usual case.
fn remember_unknown<T: DeserializeOwned + Serialize>(path: &Path, text: &str) {
    let raw = serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|v| v.get("data").cloned());
    let known = serde_json::from_str::<Envelope<T>>(text)
        .ok()
        .and_then(|e| serde_json::to_value(&e.data).ok());
    let unknown = match (raw, known) {
        (Some(raw), Some(known)) => unknown_part(&raw, &known),
        _ => None,
    };
    if let Ok(mut u) = UNKNOWN.lock().or_else(crate::crash::unpoison) {
        match unknown {
            Some(x) => {
                u.insert(path.to_path_buf(), x);
            }
            None => {
                u.remove(path);
            }
        }
    }
}

/// Write `bytes` to the temp file `tmp` and flush them with `sync`. If any
/// step fails the temp file is removed, not left beside the real one as a
/// `<name>.<pid>.json.tmp` (2026-10-21).
fn write_temp_and_sync(
    tmp: &Path,
    bytes: &[u8],
    sync: &dyn Fn(&std::fs::File) -> std::io::Result<()>,
) -> std::io::Result<()> {
    use std::io::Write;
    let written = std::fs::File::create(tmp).and_then(|mut f| {
        f.write_all(bytes)?;
        sync(&f)
    });
    // Nothing to take away when the create itself failed.
    if written.is_err() && tmp.is_file() {
        crate::heard!(std::fs::remove_file(tmp));
    }
    written
}

/// A file's size and modified time, or `None` when it isn't there.
type Stamp = Option<(u64, Option<std::time::SystemTime>)>;

fn stamp_of(path: &Path) -> Stamp {
    std::fs::metadata(path)
        .ok()
        .map(|m| (m.len(), m.modified().ok()))
}

/// Each state file as this process last read or wrote it (5 Oct 2026, Q13).
///
/// The daemon keeps its records in memory and writes them all back after
/// every turn; a command run beside it (`atlas calendar add ...`, the hub's
/// own CLI, a second Atlas on the same folder) writes the same files. Last
/// writer won, silently: the daemon's next save put its stale copy back over
/// the command's change. Knowing how the file looked when this process last
/// had it is what lets a save see that someone else has written it since
/// (`changed_elsewhere`), and the daemon reload it first.
static SEEN: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<PathBuf, Stamp>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

fn remember_seen(path: &Path, stamp: Stamp) {
    if let Ok(mut s) = SEEN.lock().or_else(crate::crash::unpoison) {
        s.insert(path.to_path_buf(), stamp);
    }
}

/// Saves that failed, whoever made them and whether or not they looked at the
/// result (5 Oct 2026).
///
/// `persist` checks the sixteen records it saves itself, but 287 other call
/// sites save with `let _ = store.save(..)` -- the vault, connected accounts,
/// calendar links, the outbox of feedback -- and a full disk or a locked file
/// made every one of them fail without a word. Recording the failure here,
/// inside the one function they all go through, means no call site can make
/// it silent: `Daemon::persist` drains what belongs to its own store into
/// `persist_failures`, and the person is told once, as for its own records.
/// Bounded, so a disk that stays full can't grow it without limit.
static FAILED_SAVES: std::sync::Mutex<Vec<(PathBuf, String, String, bool)>> =
    std::sync::Mutex::new(Vec::new());
const MOST_FAILED_SAVES_KEPT: usize = 64;

fn record_failed_save(root: &Path, name: &str, error: &str, transient: bool) {
    if let Ok(mut v) = FAILED_SAVES.lock().or_else(crate::crash::unpoison) {
        v.retain(|(r, n, _, _)| !(r == root && n == name));
        if v.len() >= MOST_FAILED_SAVES_KEPT {
            v.remove(0);
        }
        v.push((
            root.to_path_buf(),
            name.to_string(),
            error.to_string(),
            transient,
        ));
    }
}

/// The saves into `root` that failed since the last time this was asked, as
/// (record name, error). Taken, so each is reported once.
pub fn take_failed_saves(root: &Path) -> Vec<(String, String)> {
    take_failed_saves_detailed(root)
        .into_iter()
        .map(|(name, error, _)| (name, error))
        .collect()
}

pub fn take_failed_saves_detailed(root: &Path) -> Vec<(String, String, bool)> {
    let Ok(mut v) = FAILED_SAVES.lock().or_else(crate::crash::unpoison) else {
        return Vec::new();
    };
    let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut *v)
        .into_iter()
        .partition(|(r, _, _, _)| r == root);
    *v = rest;
    mine.into_iter()
        .map(|(_, n, e, transient)| (n, e, transient))
        .collect()
}

/// A record name as a `&'static str`, for `Daemon::persist_failures`. The
/// names are a fixed, small set (one per kind of record), so each is leaked
/// once and reused.
pub fn intern_record_name(name: &str) -> &'static str {
    static NAMES: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());
    let Ok(mut v) = NAMES.lock().or_else(crate::crash::unpoison) else {
        return "a record";
    };
    if let Some(n) = v.iter().find(|n| **n == name) {
        return n;
    }
    if v.len() >= 512 {
        return "a record";
    }
    let n: &'static str = Box::leak(name.to_string().into_boxed_str());
    v.push(n);
    n
}

#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Store {
            root: root.as_ref().to_path_buf(),
        }
    }

    /// Where this store's own state lives — notes, the tray, the vault,
    /// peer tokens, the journal. On a real install this is `data/state`.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Hold across a group of saves so a backup sees either whole group.
    pub fn transaction(&self) -> Result<StateGuard> {
        Ok(state_transaction(&self.root)?)
    }

    /// The same, for a worker thread with something to keep: waits (up to ten
    /// seconds) for whoever holds the lock instead of failing at once. The
    /// control loop uses `transaction`, which never waits.
    pub fn transaction_waiting(&self) -> Result<StateGuard> {
        Ok(wait_for_state_transaction(&self.root)?)
    }

    /// The install's own root — the directory a fresh download unpacks
    /// into, and the one `atlas update`'s `upgrade::YOURS` list is checked
    /// against. `data/backups`, `data/notes` and `data/logs` are its
    /// siblings, not `root()`'s: `upgrade::YOURS` lists `data/state` and
    /// `data/backups` as two separate entries under the same install root,
    /// and `atlas update` really does check for `data/backups` there — a
    /// config path resolved against `root()` instead (as `BackupConfig` and
    /// `ResearchConfig` briefly were) would put backups somewhere
    /// `atlas update` never looks, which is a worse bug than the one that
    /// resolving against nothing was.
    ///
    /// On the real install, `root()` is `data/state`, so this is two
    /// levels up. In a test, `root()` is usually some arbitrary isolated
    /// temp directory with no `data/state` structure inside it at all —
    /// there, going up two levels would climb out of the test's own
    /// sandbox into whatever happens to be above it. Detected by name
    /// rather than assumed: only a root whose last two components are
    /// literally `data/state` gets the two-levels-up treatment; anything
    /// else is trusted to already be its own root, which is exactly what
    /// keeps every isolated test its own island.
    /// A second person's state lives deeper — `data/state/profiles/<id>` —
    /// and this is the landmine that shape would otherwise step on. Detecting
    /// only `data/state` would leave a profile's `install_root()` pointing at
    /// the profile folder, so `data/backups`, `data/logs` and `data/notes`
    /// would all resolve *inside* it: every person would get their own
    /// backups folder that `atlas update` never looks at, which is the
    /// "path that looks per-install and is not" bug in a new place. Both
    /// shapes are recognised by name, for the same reason the first one is.
    pub fn install_root(&self) -> PathBuf {
        // `data/state/profiles/<id>` — four levels up.
        let under_profiles = self
            .root
            .parent()
            .map(|p| ends_with_data_state_profiles(p))
            .unwrap_or(false);
        if under_profiles {
            return self
                .root
                .ancestors()
                .nth(4)
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| self.root.clone());
        }

        let is_data_state = self.root.file_name().map(|n| n == "state").unwrap_or(false)
            && self
                .root
                .parent()
                .and_then(|p| p.file_name())
                .map(|n| n == "data")
                .unwrap_or(false);
        if is_data_state {
            self.root
                .parent()
                .and_then(|p| p.parent())
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| self.root.clone())
        } else {
            self.root.clone()
        }
    }

    /// `data/` — the parent of this store's own folder on a real install.
    ///
    /// These five accessors exist so that no module except this one and
    /// `roots.rs` has to spell a `data/…` path out as a literal. That is not
    /// tidiness: the literal is the bug. Every one of the four "a path that
    /// looks per-install and is not" defects in this tree was a call site
    /// writing its own `"data/something"` and getting the base wrong, and
    /// `tests/one_install_root.rs` now fails the build if a sixth appears.
    pub fn data_dir(&self) -> PathBuf {
        self.install_root().join("data")
    }

    /// `data/logs` — what Atlas did, and when.
    pub fn logs_dir(&self) -> PathBuf {
        self.data_dir().join("logs")
    }

    /// `data/notes` — what you have written.
    pub fn notes_dir(&self) -> PathBuf {
        self.data_dir().join("notes")
    }

    /// `data/backups` — snapshots `atlas backups` can restore from. A
    /// sibling of `data/state`, not a child: `upgrade::YOURS` checks for it
    /// there, literally.
    pub fn backups_dir(&self) -> PathBuf {
        self.data_dir().join("backups")
    }

    /// `data/trash` — the 30-day holding pen for anything Atlas removed.
    pub fn trash_dir(&self) -> PathBuf {
        self.data_dir().join("trash")
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.join(format!("{name}.json"))
    }

    /// Missing or corrupt file yields the default. State that fails to parse
    /// must not stop Atlas from starting — but it must not be destroyed
    /// either, so anything unreadable is preserved alongside.
    ///
    /// ## Missing and unreadable are different, and conflating them lost data
    ///
    /// This was `let Ok(text) = read_to_string(..) else { return default }`.
    /// Every read error became "empty", not just "not there" — and the
    /// realistic ones are not rare on the platform Atlas runs on:
    /// `ERROR_SHARING_VIOLATION` from antivirus or OneDrive holding a state
    /// file open, `EACCES`, `EMFILE` under load. `safety.rs` cites exactly
    /// that scenario as realistic in its own comments.
    ///
    /// The `preserve` path was not taken, because the file is still sitting
    /// there perfectly intact. So the subsystem ran as empty, and **the next
    /// `save` wrote the default over a good file** — the notes, the tray, the
    /// backlog, gone because a virus scanner held a handle for a second. The
    /// doc above said "anything unreadable is preserved alongside", which was
    /// true only of a parse failure.
    ///
    /// Now: a file that exists and cannot be read is preserved before the
    /// default is returned, so the next save cannot land on it.
    pub fn load<T: DeserializeOwned + Default + Serialize>(&self, name: &str) -> T {
        self.load_bounded(name, MAX_LOAD_BYTES)
    }

    /// `load` with a per-record read budget (20 Oct 2026).
    ///
    /// A store must never write a file its own load will refuse. The default
    /// budget is 16 MiB, and the file index grew past it: 17 MB of perfectly
    /// valid JSON was set aside as unreadable on every start, Atlas rescanned
    /// from empty and wrote another 17 MB file. A record that is legitimately
    /// large names its own budget here and must pass the same number to
    /// `save_bounded`, so the two sides cannot disagree. A file refused only
    /// for its size is set aside as `oversized`, not `unreadable`.
    pub fn load_bounded<T: DeserializeOwned + Default + Serialize>(
        &self,
        name: &str,
        max_bytes: u64,
    ) -> T {
        let path = self.path(name);
        // Taken before reading: a write between the two leaves this older
        // than the file, so the change is seen as someone else's, never missed.
        let stamp = stamp_of(&path);
        let text = match read_with_patience(&path, max_bytes) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                remember_seen(&path, None);
                return T::default();
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::InvalidData
                    && e.to_string() == OVERSIZED_MESSAGE =>
            {
                self.preserve(name, OVERSIZED);
                return T::default();
            }
            Err(_) => {
                // It is there and it could not be read, even after waiting a
                // moment. Move it out of the way of the next save, which is
                // the one thing that would make a transient failure
                // permanent.
                self.preserve(name, "unreadable");
                return T::default();
            }
        };

        // Current format.
        if let Ok(env) = serde_json::from_str::<Envelope<T>>(&text) {
            if env.schema == SCHEMA {
                remember_seen(&path, stamp);
                // Not for a record with its own large budget (the index): this
                // is two more full parses and a JSON tree of the whole text,
                // so near the cap the peak was gigabytes (2026-10-21). Such a
                // record is rebuilt from the disk, so nothing unknown to keep.
                if max_bytes > MAX_LOAD_BYTES {
                    if let Ok(mut u) = UNKNOWN.lock().or_else(crate::crash::unpoison) {
                        u.remove(&path);
                    }
                } else {
                    remember_unknown::<T>(&path, &text);
                }
                return env.data;
            }
            // A future or older shape. Keep it; do not silently overwrite.
            self.preserve(name, "schema");
            return T::default();
        }
        // Pre-envelope files, so an existing install keeps working.
        if let Ok(v) = serde_json::from_str::<T>(&text) {
            remember_seen(&path, stamp);
            return v;
        }
        // Corrupt. Keep the evidence rather than clobbering it on next save.
        self.preserve(name, "corrupt");
        T::default()
    }

    /// Read state needed for an irreversible action without defaulting or moving evidence.
    pub fn load_checked<T: DeserializeOwned + Serialize>(&self, name: &str) -> Result<Option<T>> {
        self.load_checked_bounded(name, 16 * 1024 * 1024)
    }

    /// Checked snapshots may use a smaller caller-specific allocation budget.
    pub fn load_checked_bounded<T: DeserializeOwned + Serialize>(
        &self,
        name: &str,
        maximum: u64,
    ) -> Result<Option<T>> {
        use std::io::Read;
        let maximum = maximum.min(16 * 1024 * 1024);
        let path = self.path(name);
        let mut file = match std::fs::File::open(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                remember_seen(&path, None);
                return Ok(None);
            }
            Err(e) => return Err(e.into()),
        };
        let metadata = file.metadata()?;
        let invalid = |why: &str| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{name}: {why}; state was left untouched"),
            )
        };
        if !metadata.is_file() || metadata.len() > maximum {
            return Err(invalid("not a regular state file within the read budget").into());
        }
        let before = Some((metadata.len(), metadata.modified().ok()));
        let mut text = String::new();
        (&mut file).take(maximum + 1).read_to_string(&mut text)?;
        if text.len() as u64 > maximum || stamp_of(&path) != before {
            return Err(invalid("state changed or exceeded its read budget").into());
        }
        let raw: serde_json::Value =
            serde_json::from_str(&text).map_err(|_| invalid("malformed state"))?;
        let (data, envelope) = if let Some(schema) = raw.get("schema") {
            if schema.as_u64() != Some(u64::from(SCHEMA)) {
                return Err(invalid("unsupported state format").into());
            }
            (
                raw.get("data")
                    .cloned()
                    .ok_or_else(|| invalid("missing state data"))?,
                text,
            )
        } else {
            let envelope = serde_json::json!({"schema": SCHEMA, "data": raw.clone()}).to_string();
            (raw, envelope)
        };
        let value: T = serde_json::from_value(data)
            .map_err(|_| invalid("state does not match this record"))?;
        remember_seen(&path, before);
        remember_unknown::<T>(&path, &envelope);
        Ok(Some(value))
    }

    /// Move an unreadable file aside so the next save cannot destroy it.
    ///
    /// The rename's failure is recorded rather than discarded. It used to be
    /// `let _ = rename(..)`, and a failed rename leaves the file exactly where
    /// the next save will overwrite it — which is the one thing this function
    /// exists to prevent, failing silently.
    fn preserve(&self, name: &str, why: &str) {
        let Ok(_guard) = state_transaction(&self.root) else {
            record_failed_save(
                &self.root,
                name,
                "state was busy; the unreadable file was left intact",
                true,
            );
            return;
        };
        note_set_aside(&self.root, name, why);
        let from = self.path(name);
        let to = self.root.join(format!("{name}.{why}.{}.json.bak", now()));
        if rename_patiently(&from, &to).is_err() {
            // Nothing here can fix it, and something has to know. `preserved`
            // is what `doctor` reads; this is what makes the count include
            // the ones that could not be moved, rather than only the ones
            // that could.
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.root.join("preserve-failed.log"))
            {
                use std::io::Write;
                crate::kept!(writeln!(
                    f,
                    "{}: could not move {} aside ({why}). The next save will \
                     overwrite it.",
                    now(),
                    from.display()
                ));
            }
        }
    }

    /// `load`, kept in memory until the file changes (27 Sep 2026).
    ///
    /// For records read on every page of the hub -- how it looks, who is on
    /// your roster -- which used to be read and parsed from disk for every
    /// request, several times a page. Kept per path with the file's size and
    /// modified time; any write, from this store, another one or another
    /// process, moves those and the next call reads the file again. A file
    /// that isn't there is never kept, so it is looked for every time.
    pub fn load_kept<T>(&self, name: &str) -> T
    where
        T: DeserializeOwned + Serialize + Default + Clone + Send + Sync + 'static,
    {
        let path = self.path(name);
        let stamp = std::fs::metadata(&path)
            .ok()
            .map(|m| (m.len(), m.modified().ok()));
        if let (Some(stamp), Ok(kept)) = (stamp, KEPT.lock()) {
            if let Some((len, when, value)) = kept.get(&path) {
                if (*len, *when) == stamp {
                    if let Some(v) = value.downcast_ref::<T>() {
                        return v.clone();
                    }
                }
            }
        }
        let value: T = self.load(name);
        if let (Some((len, when)), Ok(mut kept)) = (stamp, KEPT.lock()) {
            kept.insert(path, (len, when, std::sync::Arc::new(value.clone())));
        }
        value
    }

    /// Has someone else -- another process, a command, a hand edit -- written
    /// this record since this process last read or wrote it? `false` for a
    /// record this process hasn't touched yet: it has nothing to be stale
    /// about.
    pub fn changed_elsewhere(&self, name: &str) -> bool {
        let path = self.path(name);
        let seen = SEEN
            .lock()
            .or_else(crate::crash::unpoison)
            .ok()
            .and_then(|s| s.get(&path).copied());
        match seen {
            Some(seen) => stamp_of(&path) != seen,
            None => false,
        }
    }

    /// `save`, for the control loop: a worker (a backup, a send) may hold the
    /// state for a few milliseconds, and the loop must not wait on it for
    /// long nor lose what it was handed. Tries for a quarter of a second.
    pub fn keep_briefly<T: Serialize>(&self, name: &str, value: &T) -> Result<()> {
        for _ in 0..25 {
            match self.save(name, value) {
                Err(crate::error::AtlasError::Io(io))
                    if io.kind() == std::io::ErrorKind::WouldBlock =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(10))
                }
                other => return other,
            }
        }
        self.save(name, value)
    }

    pub fn save<T: Serialize>(&self, name: &str, value: &T) -> Result<()> {
        self.save_bounded(name, value, MAX_LOAD_BYTES)
    }

    /// `save` that refuses to write a record larger than `max_bytes` (20 Oct
    /// 2026). Pair it with `load_bounded` and the same number.
    fn save_bounded<T: Serialize>(&self, name: &str, value: &T, max_bytes: u64) -> Result<()> {
        self.save_in(name, value, max_bytes, Layout::Pretty)
    }

    /// `save_bounded`, written as compact JSON rather than indented. For the
    /// records that are large and only ever read by Atlas, where the
    /// indentation is a fifth or more of the file for no reader.
    pub fn save_compact_bounded<T: Serialize>(
        &self,
        name: &str,
        value: &T,
        max_bytes: u64,
    ) -> Result<()> {
        self.save_in(name, value, max_bytes, Layout::Compact)
    }

    fn save_in<T: Serialize>(
        &self,
        name: &str,
        value: &T,
        max_bytes: u64,
        layout: Layout,
    ) -> Result<()> {
        let r = self.save_unrecorded(name, value, max_bytes, layout);
        if let Err(e) = &r {
            let transient = matches!(e, crate::error::AtlasError::Io(io) if io.kind() == std::io::ErrorKind::WouldBlock);
            record_failed_save(
                &self.root,
                name,
                &failed_save_text(&self.root, name, e),
                transient,
            );
        } else if let Ok(mut failures) = FAILED_SAVES.lock().or_else(crate::crash::unpoison) {
            failures.retain(|(root, record, _, _)| !(root == &self.root && record == name));
        }
        r
    }

    fn save_unrecorded<T: Serialize>(
        &self,
        name: &str,
        value: &T,
        max_bytes: u64,
        layout: Layout,
    ) -> Result<()> {
        let _guard = self.transaction()?;
        let final_path = self.path(name);
        // Never `unwrap_or_default` here: a value that won't serialize wrote
        // an empty file over the good one, atomically, and returned Ok -- the
        // next load set the empty file aside and the data was gone (Q3).
        let bad = |e: serde_json::Error| {
            crate::error::AtlasError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{name} couldn't be written as JSON: {e}"),
            ))
        };
        let unknown = UNKNOWN
            .lock()
            .or_else(crate::crash::unpoison)
            .ok()
            .and_then(|u| u.get(&final_path).cloned());
        let body = match unknown {
            // What a newer Atlas wrote and this one doesn't know, put back (Q12).
            Some(u) => {
                let mut data = serde_json::to_value(value).map_err(bad)?;
                put_back_unknown(&u, &mut data);
                let whole = serde_json::json!({ "schema": SCHEMA, "data": data });
                match layout {
                    Layout::Pretty => serde_json::to_string_pretty(&whole),
                    Layout::Compact => serde_json::to_string(&whole),
                }
                .map_err(bad)?
            }
            None => {
                let whole = Envelope {
                    schema: SCHEMA,
                    data: value,
                };
                match layout {
                    Layout::Pretty => serde_json::to_string_pretty(&whole),
                    Layout::Compact => serde_json::to_string(&whole),
                }
                .map_err(bad)?
            }
        };

        // Never write what this store's own load would refuse (20 Oct 2026).
        //
        // `load` set aside any file over its budget as unreadable, but `save`
        // had no budget at all, so a record that grew past it was written
        // happily and then discarded at the next start -- the file index did
        // exactly that, 17 MB against a 16 MiB limit, every restart. The
        // oversized body is not written; the previous good file stays where
        // it is, and the error reaches `record_failed_save` so it is shown.
        if body.len() as u64 > max_bytes {
            return Err(crate::error::AtlasError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "{name} is {} bytes, past the {max_bytes}-byte budget for this record; \
                     it was not written and the previous file was kept",
                    body.len()
                ),
            )));
        }

        // Unchanged content is not written.
        //
        // `daemon::persist` saves thirteen subsystems after every turn, and on
        // a normal turn almost none of them changed -- you asked what was
        // outstanding and the modes, the flows, the watcher and the rest are
        // exactly as they were. That was thirteen writes and thirteen renames,
        // about a millisecond of disk per turn, to put back bytes that were
        // already there.
        //
        // Compared against the file rather than against a remembered hash.
        // `Store` is `Clone` and holds nothing but a path, so two stores can
        // point at one directory and a remembered hash in one of them would be
        // wrong about what the other wrote. The file cannot be wrong about it.
        //
        // This can only skip a write that would have produced byte-identical
        // content, so nothing it does is visible to anything except the disk:
        // a changed value still writes, a missing file still writes, and the
        // temp-then-rename atomicity is untouched on the path that writes.
        //
        // And without reading the file, when it can be known cheaply (27 Sep
        // 2026): `persist` compares about thirty records after every turn,
        // and each comparison read the whole file. `WRITTEN` remembers, for
        // the whole process, the content hash this process last wrote or
        // matched at each path together with the file's size and modified
        // time at that moment. The same hash with the file's size and time
        // unchanged means the bytes on disk are the ones being saved -- a
        // stat, not a read. Anything else (another store, another process, a
        // hand edit, a coarse clock) moves the size or time, and falls back
        // to the comparison against the file above, which cannot be wrong.
        let hash = content_hash(body.as_bytes());
        let seen = std::fs::metadata(&final_path)
            .ok()
            .map(|m| (m.len(), m.modified().ok()));
        if let (Some(now), Ok(written)) = (seen, WRITTEN.lock()) {
            if written.get(&final_path) == Some(&(hash, now.0, now.1)) {
                return Ok(());
            }
        }
        if let Ok(existing) = std::fs::read(&final_path) {
            if existing == body.as_bytes() {
                remember_written(&final_path, hash);
                return Ok(());
            }
        }

        std::fs::create_dir_all(&self.root)?;

        // Someone else wrote this since we read it, and what we're about to
        // write differs from theirs: theirs is kept beside it, named, and you
        // are told, rather than overwritten without a trace (Q13). The daemon
        // reloads such records before it works (`Daemon::take_outside_changes`),
        // so this is the narrow race left over, not the common case.
        if self.changed_elsewhere(name) && final_path.is_file() {
            let theirs = self.root.join(format!("{name}.theirs.{}.json.bak", now()));
            if std::fs::copy(&final_path, &theirs).is_ok() {
                note_set_aside(&self.root, name, THEIRS);
            }
        }

        // The temp file is named per PROCESS, not per record.
        //
        // It was `final_path.with_extension("json.tmp")` -- derived only from
        // the record name, so every process writing `notes` used the same
        // `notes.json.tmp`. And a second process is not hypothetical here:
        // `main.rs` says so deliberately, in the comment above its single-
        // instance lock -- "a quick one-off command while --daemon is already
        // running in the background is exactly the kind of thing Atlas should
        // still answer, and locking that out too would make the CLI useless
        // whenever the daemon is up."
        //
        // So the daemon persists ~16 records on its tick while you run
        // `atlas tasks add ...`, both call `fs::write` on one path, the two
        // writes interleave, and whichever renames second publishes a file
        // that is half of each. `load` cannot parse it, `preserve(name,
        // "corrupt")` moves it aside, and the record comes back as
        // `T::default()` -- so your notes, or the approvals, or the queue,
        // silently read as empty, with a `.corrupt.<ts>.json.bak` nobody
        // looks in. That is the same "you notice weeks later that something
        // didn't stick" failure the instance lock exists to prevent, arriving
        // through the door the lock deliberately leaves open.
        //
        // The pid makes the temp path unique, so the two writers no longer
        // touch the same bytes. `rename` is atomic, so last-writer-wins is
        // the outcome -- which is the semantics this was always assumed to
        // have.
        let tmp = final_path.with_extension(format!("{}.json.tmp", std::process::id()));

        // Flushed to the device before the rename, so the claim at the top of
        // this file -- "a crash mid-save leaves the previous good copy rather
        // than a truncated one" -- also holds for a power cut and not only
        // for a process dying. Without it the rename can be durable while the
        // data it points at is not, which on NTFS and ext4 is how you get a
        // zero-length file where a good one used to be. `save` already skips
        // writes whose content is unchanged, so this costs nothing on the
        // common tick where nothing moved.
        write_temp_and_sync(&tmp, body.as_bytes(), &|f| f.sync_all())?;

        // On failure, take our own temp file with us. A hard kill still
        // leaves one behind, but it is named with the pid that made it rather
        // than sitting on the name the next writer wants.
        if let Err(e) = rename_patiently(&tmp, &final_path) {
            crate::heard!(std::fs::remove_file(&tmp));
            return Err(e.into());
        }
        remember_written(&final_path, hash);
        Ok(())
    }

    /// Files set aside because they could not be read.
    pub fn preserved(&self) -> Vec<PathBuf> {
        std::fs::read_dir(&self.root)
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.to_string_lossy().ends_with(".json.bak"))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn exists(&self, name: &str) -> bool {
        self.path(name).is_file()
    }

    /// File a record under another name, byte for byte (`vault::bring_in_set_aside`
    /// retiring the old vault). Refuses to replace one already there.
    pub fn file_as(&self, name: &str, new_name: &str) -> Result<()> {
        let _guard = self.transaction()?;
        let to = self.path(new_name);
        if to.exists() {
            return Err(crate::error::AtlasError::Io(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("{new_name} is already there"),
            )));
        }
        std::fs::rename(self.path(name), to)?;
        Ok(())
    }
}

pub fn now() -> u64 {
    let wall = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    crate::hlc::watch_the_clock(wall);
    wall
}

/// Is this the operating system saying the disk has no room left?
/// (21 Oct 2026.) Windows says 112 (disk full) or 39 (handle disk full);
/// Linux and macOS say 28 (no space left). Matched on the number because the
/// error kind for a full disk is not the same on every platform.
pub(crate) fn is_disk_full(e: &std::io::Error) -> bool {
    matches!(e.raw_os_error(), Some(112) | Some(39) | Some(28))
}

/// The drive or top folder a path lives on, for saying which disk is full:
/// "C:" on Windows, "/" elsewhere. `None` for a path with no clear root.
pub(crate) fn drive_of(path: &Path) -> Option<String> {
    use std::path::Component;
    // 21 Oct 2026: a long-path form (backslash backslash ? backslash C:)
    // is read as the drive it names.
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\") {
        if rest.len() >= 2 && rest.as_bytes()[1] == b':' {
            return Some(rest[..2].to_string());
        }
    }
    match path.components().next()? {
        Component::Prefix(p) => {
            let s = p.as_os_str().to_string_lossy().to_string();
            // A long-path prefix (backslash backslash question backslash) is
            // not part of the drive's name.
            Some(s.trim_start_matches(r"\\?\").to_string())
        }
        Component::RootDir => Some("/".to_string()),
        _ => None,
    }
}

/// What to say when a file write fails: a full disk gets a plain sentence and
/// the next step, anything else keeps its own words. `what` is the thing that
/// could not be written ("the index"), `at` where it was going.
pub(crate) fn plain_write_failure(what: &str, at: &Path, e: &std::io::Error) -> String {
    if is_disk_full(e) {
        let disk = match drive_of(at) {
            Some(d) => format!("the disk ({d})"),
            None => "the disk".to_string(),
        };
        return format!(
            "couldn't save {what}: {disk} is full -- free some space and I'll save again"
        );
    }
    format!("couldn't save {what}: {e}")
}

fn failed_save_text(root: &Path, name: &str, e: &crate::error::AtlasError) -> String {
    match e {
        crate::error::AtlasError::Io(io) if is_disk_full(io) => {
            plain_write_failure(&name.replace('_', " "), root, io)
        }
        other => other.to_string(),
    }
}

/// How many times, and how far apart, a file that exists but can't be read
/// is tried again before it is set aside. A virus scanner or OneDrive holds a
/// file for a moment; one failed read used to be enough to start that part
/// of Atlas empty (28 Sep 2026).
const READ_TRIES: u32 = 5;
const READ_PAUSE_MS: u64 = 100;
const MAX_LOAD_BYTES: u64 = 16 * 1024 * 1024;

/// How a record is laid out on disk. Pretty is the default, so a person can
/// read and diff it; compact is for the large ones.
#[derive(Clone, Copy)]
enum Layout {
    Pretty,
    Compact,
}

/// `preserve` reason for a file refused only because of its size.
const OVERSIZED: &str = "oversized";

/// What `read_with_patience` says when a file is refused for size alone, so
/// `load_bounded` can tell "too big for me" from "could not be read" and set
/// it aside under the truthful name (20 Oct 2026).
const OVERSIZED_MESSAGE: &str = "state file exceeds the load budget";

/// `read_to_string`, tried again a few times when the file is there but
/// locked. A missing file is answered at once. Refuses anything over `max`
/// bytes without reading past it.
fn read_with_patience(path: &std::path::Path, max: u64) -> std::io::Result<String> {
    let mut last = None;
    for attempt in 0..READ_TRIES {
        let result = (|| {
            let file = std::fs::File::open(path)?;
            let metadata = file.metadata()?;
            if !metadata.is_file() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "state path is not a regular file",
                ));
            }
            if metadata.len() > max {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    OVERSIZED_MESSAGE,
                ));
            }
            let mut text = String::new();
            use std::io::Read;
            file.take(max + 1).read_to_string(&mut text)?;
            if text.len() as u64 > max {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    OVERSIZED_MESSAGE,
                ));
            }
            Ok(text)
        })();
        match result {
            Ok(t) => return Ok(t),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(e),
            // Bytes that aren't text won't become text by waiting.
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => return Err(e),
            Err(e) => {
                last = Some(e);
                if attempt + 1 < READ_TRIES {
                    std::thread::sleep(std::time::Duration::from_millis(READ_PAUSE_MS));
                }
            }
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("unreadable")))
}

/// Is this rename failure a lock that lets go (a virus scanner, OneDrive or
/// the search indexer holding the file open for a moment), rather than a
/// real refusal? Windows says ACCESS_DENIED (5), SHARING_VIOLATION (32) or
/// LOCK_VIOLATION (33) for those. Elsewhere a rename over a file isn't
/// blocked by a reader, so nothing is worth waiting for.
fn is_a_passing_lock(e: &std::io::Error) -> bool {
    cfg!(windows)
        && (e.kind() == std::io::ErrorKind::PermissionDenied
            || matches!(e.raw_os_error(), Some(5 | 32 | 33)))
}

/// The waits between tries of a rename that hit a passing lock: about 1.3 s
/// in all, the same patience Go's `robustio` and npm's `graceful-fs` give it
/// in spirit, kept short because the tick may be the one waiting.
const RENAME_PAUSES_MS: [u64; 7] = [10, 20, 40, 80, 160, 320, 640];

/// `fs::rename(from, to)` over an existing file, tried again while Windows
/// reports a passing lock (5 Oct 2026 audit, Q11). One failed rename used to
/// mean a lost save, said as an error, whenever a scanner looked at the file
/// at the wrong moment. Any other failure, or one that outlasts the waits,
/// is returned as it was.
pub fn rename_patiently(from: &Path, to: &Path) -> std::io::Result<()> {
    rename_patiently_with(from, to, &RENAME_PAUSES_MS, is_a_passing_lock, |f, t| {
        std::fs::rename(f, t)
    })
}

/// `rename_patiently`, with the waits, the test for a passing lock and the
/// rename itself handed in, so the retrying is tested without Windows.
pub fn rename_patiently_with(
    from: &Path,
    to: &Path,
    pauses_ms: &[u64],
    passing: impl Fn(&std::io::Error) -> bool,
    mut rename: impl FnMut(&Path, &Path) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let mut pauses = pauses_ms.iter();
    loop {
        match rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) if passing(&e) => match pauses.next() {
                Some(ms) => std::thread::sleep(std::time::Duration::from_millis(*ms)),
                None => return Err(e),
            },
            Err(e) => return Err(e),
        }
    }
}

/// A small JSON file outside a `Store`, read whole: the default when it is
/// missing or unreadable. With `write_json`, the one way the device lists
/// (apns, webpush) are kept (audit Q3: each had its own copy).
///
/// A file that is there but does not parse is moved aside as
/// `<name>.corrupt.<ts>.bak` rather than left for the next `write_json` to
/// overwrite: a damaged list is evidence, not an empty list. A file that
/// merely cannot be read right now (antivirus holding it) is left untouched.
pub fn read_json<T: serde::de::DeserializeOwned + Default>(path: &Path) -> T {
    let Ok(bytes) = std::fs::read(path) else {
        return T::default();
    };
    match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(_) => {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            crate::heard!(rename_patiently(
                path,
                &path.with_file_name(format!("{name}.corrupt.{}.bak", now()))
            ));
            T::default()
        }
    }
}

/// `value` written to `path` as pretty JSON, whole (`write_whole`).
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    write_whole(
        path,
        &serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?,
    )
}

/// Write `bytes` as the whole of `path`, or leave the old file as it was:
/// written beside it under a name only this process uses, flushed to the
/// disk, then renamed over it (`rename_patiently`). For the small state files
/// kept outside a `Store` -- a crash or a full disk mid-`fs::write` left them
/// cut short, and the next start read them as empty (audit Q3/Q11).
pub fn write_whole(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let _guard = state_lock::for_path(path)?;
    use std::io::Write;
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = path.with_file_name(format!("{name}.{}.writing", std::process::id()));
    let written = std::fs::File::create(&tmp).and_then(|mut f| {
        f.write_all(bytes)?;
        f.sync_all()
    });
    let done = written.and_then(|_| rename_patiently(&tmp, path));
    if done.is_err() {
        crate::heard!(std::fs::remove_file(&tmp));
    }
    done
}

/// A saved file set aside because it couldn't be read: which, why, when, and
/// whether you've been told.
#[derive(Debug, Clone)]
pub struct SetAside {
    /// The store it was in: one Atlas's news, not every store's in the
    /// process (the tests run many side by side).
    root: PathBuf,
    pub name: String,
    pub why: String,
    pub at: u64,
    told: bool,
}

static SET_ASIDE: std::sync::Mutex<Vec<SetAside>> = std::sync::Mutex::new(Vec::new());

/// For the tests of what the hub says about a set-aside file.
#[cfg(test)]
pub(crate) fn note_set_aside_for_tests(store: &Store, name: &str, why: &str) {
    note_set_aside(&store.root, name, why);
}

fn note_set_aside(root: &std::path::Path, name: &str, why: &str) {
    let mut v = SET_ASIDE.lock().unwrap_or_else(|p| p.into_inner());
    v.push(SetAside {
        root: root.to_path_buf(),
        name: name.to_string(),
        why: why.to_string(),
        at: now(),
        told: false,
    });
    if v.len() > 50 {
        v.remove(0);
    }
}

/// What was set aside in the last day, for the hub to show.
pub fn set_aside_since(store: &Store, since: u64) -> Vec<SetAside> {
    SET_ASIDE
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .filter(|s| s.at >= since && s.root == store.root)
        .cloned()
        .collect()
}

/// The sentence for a set-aside file, in plain words.
pub fn set_aside_sentence(names: &[String]) -> String {
    set_aside_sentence_for(names, false)
}

/// The sentence for files that grew past what is read at start. Not damage,
/// and not worded as damage (20 Oct 2026).
pub fn oversized_sentence(names: &[String]) -> String {
    set_aside_sentence_for(names, true)
}

fn set_aside_sentence_for(names: &[String], oversized: bool) -> String {
    let what = names
        .iter()
        .map(|n| n.replace('_', " "))
        .collect::<Vec<_>>()
        .join(", ");
    if oversized {
        return format!(
            "My saved {what} had grown past what I read at start, so I put the file aside and started that part fresh -- \
             nothing was deleted and nothing was damaged; the old file is kept beside the others in my state folder."
        );
    }
    format!(
        "I couldn't read my saved {what} just now, so I put the file aside and started that part fresh -- \
         nothing was deleted; the old file is kept beside the others in my state folder."
    )
}

/// Said once, at the start of your next turn, when a file was set aside
/// since you were last told (not only in diagnose, where it was the only
/// place it showed).
pub fn tell_set_aside(store: &Store) -> Option<String> {
    let mut v = SET_ASIDE.lock().unwrap_or_else(|p| p.into_inner());
    let (mut unreadable, mut theirs): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    let mut oversized: Vec<String> = Vec::new();
    for s in v.iter_mut().filter(|s| !s.told && s.root == store.root) {
        s.told = true;
        let list = if s.why == THEIRS {
            &mut theirs
        } else if s.why == OVERSIZED {
            &mut oversized
        } else {
            &mut unreadable
        };
        if !list.contains(&s.name) {
            list.push(s.name.clone());
        }
    }
    let mut said = Vec::new();
    if !unreadable.is_empty() {
        said.push(set_aside_sentence(&unreadable));
    }
    if !oversized.is_empty() {
        said.push(oversized_sentence(&oversized));
    }
    if !theirs.is_empty() {
        said.push(theirs_sentence(&theirs));
    }
    (!said.is_empty()).then(|| said.join(" "))
}

/// Why a copy was kept when another writer had changed the file (Q13).
pub const THEIRS: &str = "changed elsewhere";

/// Said when a save went over a change made elsewhere: the other version
/// is kept, and where.
fn theirs_sentence(names: &[String]) -> String {
    let what = names
        .iter()
        .map(|n| n.replace('_', " "))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "My saved {what} was changed outside me (a command, or another Atlas on this folder) at the same moment I saved mine, \
         so I kept that other version beside mine in my state folder (the .theirs. file) rather than lose it."
    )
}

#[cfg(test)]
mod install_root_tests {
    use super::*;

    #[test]
    fn a_relative_data_state_root_climbs_to_nothing_which_is_why_it_is_banned() {
        // This was the production shape, and it is the whole bug. Every
        // command said `Store::new("data/state")`, so `install_root()`
        // climbed `"data"` then `""` and returned the *empty path* — and
        // `install_root().join("data/logs")` was cwd-relative all over
        // again. The rule was stated in this file's own doc comment and
        // broken by every caller of it.
        //
        // The function is left exactly as it was, because it is correct for
        // an absolute root. What changed is that nothing in `src` may build
        // a relative one any more: `roots::store()` is the only constructor
        // production code uses, and `tests/one_install_root.rs` enforces it.
        // This test stays as the record of what the empty answer meant.
        let s = Store::new("data/state");
        assert_eq!(s.install_root(), PathBuf::from(""));
        assert!(
            !s.install_root().is_absolute(),
            "this is the failure being documented, not a layout to copy"
        );
    }

    #[test]
    fn the_real_install_layout_climbs_two_levels_to_the_shared_data_parent() {
        // The production shape now: an absolute `<install>/data/state`.
        // `data/backups` must resolve as a sibling of `data/state`, because
        // `upgrade::YOURS` checks for it there, literally, as its own
        // real-filesystem entry.
        let s = Store::new("/opt/atlas/data/state");
        assert_eq!(s.install_root(), PathBuf::from("/opt/atlas"));
        assert_eq!(s.backups_dir(), PathBuf::from("/opt/atlas/data/backups"));
        assert_eq!(s.logs_dir(), PathBuf::from("/opt/atlas/data/logs"));
        assert_eq!(s.trash_dir(), PathBuf::from("/opt/atlas/data/trash"));
    }

    #[test]
    fn an_absolute_data_state_path_also_climbs_two_levels() {
        let s = Store::new("/opt/atlas/data/state");
        assert_eq!(s.install_root(), PathBuf::from("/opt/atlas"));
    }

    #[test]
    fn an_isolated_test_root_with_no_data_state_shape_is_trusted_as_its_own_root() {
        // Every test's `Store` points at an arbitrary temp directory, not a
        // `data/state` path. Climbing two levels there would escape the
        // test's own sandbox into whatever happens to sit above it in
        // `/tmp` — the one thing that must never happen.
        let s = Store::new("/tmp/atlas-some-test-abc123");
        assert_eq!(
            s.install_root(),
            PathBuf::from("/tmp/atlas-some-test-abc123")
        );
    }

    #[test]
    fn a_folder_that_merely_ends_in_state_but_is_not_under_data_is_not_climbed() {
        // Name-based detection has to be exact about both components, or a
        // coincidentally-named folder gets the wrong treatment.
        let s = Store::new("/home/eric/state");
        assert_eq!(s.install_root(), PathBuf::from("/home/eric/state"));
    }

    #[test]
    fn siblings_of_data_state_are_reachable_the_way_upgrade_yours_expects() {
        let s = Store::new("/opt/atlas/data/state");
        assert_eq!(s.backups_dir(), PathBuf::from("/opt/atlas/data/backups"));
        // The same path `upgrade::check` builds from `YOURS`.
        assert_eq!(s.install_root().join("data/backups"), s.backups_dir());
    }

    #[test]
    fn an_isolated_test_root_keeps_its_siblings_inside_its_own_sandbox() {
        // The property every other test in the suite depends on: a store
        // pointed at a temp directory must not scatter logs, backups or
        // trash outside it.
        let s = Store::new("/tmp/atlas-some-test-abc123");
        for d in [s.data_dir(), s.logs_dir(), s.backups_dir(), s.trash_dir()] {
            assert!(
                d.starts_with("/tmp/atlas-some-test-abc123"),
                "{} escaped the test sandbox",
                d.display()
            );
        }
    }
}

#[cfg(test)]
mod read_json_tests {
    use super::Store;

    #[test]
    fn a_damaged_file_is_set_aside_and_a_good_one_is_read() {
        let dir = std::env::temp_dir().join(format!("atlas-damaged-json-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("list.json");
        let none: Vec<u32> = super::read_json(&f);
        assert!(none.is_empty());
        std::fs::write(&f, b"[1,2,3]").unwrap();
        let ok: Vec<u32> = super::read_json(&f);
        assert_eq!(ok, vec![1, 2, 3]);
        assert!(f.exists());
        std::fs::write(&f, b"[1,2,").unwrap();
        let bad: Vec<u32> = super::read_json(&f);
        assert!(bad.is_empty());
        assert!(
            !f.exists(),
            "a damaged file must not stay where the next save overwrites it"
        );
        let kept: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            kept.iter()
                .any(|n| n.starts_with("list.json.corrupt.") && n.ends_with(".bak")),
            "{kept:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_oversized_store_load_is_preserved_without_reading_unbounded_input() {
        let dir = std::env::temp_dir().join(format!("atlas-oversized-load-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::new(&dir);
        let path = dir.join("oversized.json");
        let bytes = vec![b'x'; (super::MAX_LOAD_BYTES + 1) as usize];
        std::fs::write(&path, bytes).unwrap();

        let value: Vec<u8> = store.load("oversized");

        assert!(value.is_empty());
        assert!(!path.exists());
        assert!(std::fs::read_dir(&dir).unwrap().flatten().any(|entry| entry
            .file_name()
            .to_string_lossy()
            .starts_with("oversized.oversized.")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_record_past_the_default_budget_is_refused_and_the_old_file_stays() {
        let dir = std::env::temp_dir().join(format!("atlas-oversized-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::new(&dir);
        let small: Vec<String> = vec!["kept".to_string()];
        store.save("big", &small).unwrap();
        let before = std::fs::read(dir.join("big.json")).unwrap();

        let huge: Vec<String> = vec!["x".repeat(1024 * 1024); 17];
        let err = store.save("big", &huge).unwrap_err().to_string();

        assert!(err.contains("big") && err.contains("budget"), "{err}");
        assert_eq!(std::fs::read(dir.join("big.json")).unwrap(), before);
        assert!(
            super::take_failed_saves(&dir).iter().any(|f| f.0 == "big"),
            "the refusal must be recorded as a failed save"
        );
        let back: Vec<String> = store.load("big");
        assert_eq!(back, small);
        assert!(store.preserved().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_bounded_save_and_load_agree_on_a_large_record() {
        let dir = std::env::temp_dir().join(format!("atlas-bounded-pair-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::new(&dir);
        let huge: Vec<String> = vec!["x".repeat(1024 * 1024); 17];
        let budget = 64 * 1024 * 1024;
        store.save_compact_bounded("big", &huge, budget).unwrap();
        let back: Vec<String> = store.load_bounded("big", budget);
        assert_eq!(back, huge);
        assert!(store.preserved().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_temp_write_leaves_no_temp_file_behind() {
        let dir = std::env::temp_dir().join(format!("atlas-temp-clean-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let tmp = dir.join("x.1.json.tmp");
        let err = super::write_temp_and_sync(&tmp, b"data", &|_| {
            Err(std::io::Error::other("disk said no"))
        })
        .unwrap_err();
        assert!(err.to_string().contains("disk said no"));
        assert!(
            !tmp.exists(),
            "the temp file must be removed on a failed sync"
        );
        // And the plain success leaves it for the rename.
        super::write_temp_and_sync(&tmp, b"data", &|f| f.sync_all()).unwrap();
        assert_eq!(std::fs::read(&tmp).unwrap(), b"data");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_record_with_its_own_large_budget_skips_the_unknown_field_round_trip() {
        #[derive(Default, serde::Serialize, serde::Deserialize)]
        struct Known {
            a: u32,
        }
        let dir = std::env::temp_dir().join(format!("atlas-unknown-skip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::new(&dir);
        let file = r#"{"schema":1,"data":{"a":1,"newer":"keep me"}}"#;
        std::fs::write(dir.join("rec.json"), file).unwrap();
        // The default budget keeps what it doesn't know across a save.
        let k: Known = store.load("rec");
        store.save("rec", &Known { a: 2 }).unwrap();
        assert!(std::fs::read_to_string(dir.join("rec.json"))
            .unwrap()
            .contains("keep me"));
        let _ = k;
        // A large budget (the index) does not pay for that: nothing is kept.
        std::fs::write(dir.join("big.json"), file).unwrap();
        let k: Known = store.load_bounded("big", 64 * 1024 * 1024);
        assert_eq!(k.a, 1);
        store
            .save_compact_bounded("big", &Known { a: 3 }, 64 * 1024 * 1024)
            .unwrap();
        assert!(!std::fs::read_to_string(dir.join("big.json"))
            .unwrap()
            .contains("keep me"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    use crate::store::{failed_save_text, is_disk_full};
    use std::path::Path;

    #[test]
    fn the_operating_systems_disk_full_numbers_are_recognised() {
        assert!(is_disk_full(&std::io::Error::from_raw_os_error(112)));
        assert!(is_disk_full(&std::io::Error::from_raw_os_error(39)));
        assert!(is_disk_full(&std::io::Error::from_raw_os_error(28)));
        assert!(!is_disk_full(&std::io::Error::from_raw_os_error(5)));
    }

    #[test]
    fn a_failed_save_on_a_full_disk_says_the_disk_is_full_and_what_to_do() {
        let e = crate::error::AtlasError::Io(std::io::Error::from_raw_os_error(112));
        let root = if cfg!(windows) {
            Path::new(r"C:\Atlas\state")
        } else {
            Path::new("/var/atlas")
        };
        let said = failed_save_text(root, "reading_list", &e);
        assert!(said.contains("couldn't save reading list"), "{said}");
        assert!(said.contains("is full"), "{said}");
        assert!(said.contains("free some space"), "{said}");
        if cfg!(windows) {
            assert!(said.contains("(C:)"), "{said}");
        }
    }

    #[test]
    fn other_failures_keep_their_own_words() {
        let e = crate::error::AtlasError::Io(std::io::Error::from_raw_os_error(5));
        assert!(!failed_save_text(Path::new("x"), "a", &e).contains("is full"));
    }
}

#[cfg(test)]
mod drive_of_tests {
    use super::drive_of;
    use std::path::Path;

    #[test]
    fn the_drive_of_a_path_is_named_plainly() {
        #[cfg(windows)]
        {
            assert_eq!(
                drive_of(Path::new(r"C:\Users\eric\x.json")).as_deref(),
                Some("C:")
            );
            assert_eq!(
                drive_of(Path::new(r"\\?\D:\data\x.json")).as_deref(),
                Some("D:")
            );
        }
        assert_eq!(drive_of(Path::new("relative/x.json")), None);
    }
}
