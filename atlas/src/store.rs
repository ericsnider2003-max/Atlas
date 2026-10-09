//! Durable local state. Atomic writes: temp file then rename, so a crash
//! mid-save leaves the previous good copy rather than a truncated one.

use crate::error::Result;
use serde::{de::DeserializeOwned, Serialize};
use std::path::{Path, PathBuf};

#[path = "store/state_lock.rs"]
mod state_lock;
pub use state_lock::StateGuard;
pub use state_lock::AccessGuard;
pub fn state_access(root: &Path, exclusive: bool) -> std::io::Result<AccessGuard> { state_lock::access(root, exclusive) }
/// Fail closed before owner/permission reads if an earlier restore stopped.
pub fn begin_state_command(root: &Path, restoring: bool) -> Result<AccessGuard> {
    let access = state_access(root, restoring)?;
    if access.root.join(".restore-journal.json").try_exists()? {
        if restoring { crate::safety::recover_restore(&access.root)?; return Ok(access); }
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
        crate::safety::recover_restore(&guard.root).map_err(|e| std::io::Error::other(e.to_string()))?;
    }
    Ok(guard)
}
pub fn wait_for_state_transaction(root: &Path) -> std::io::Result<StateGuard> { state_lock::acquire(root) }
pub(crate) fn try_state_guard(root: &Path) -> std::io::Result<StateGuard> { state_lock::try_acquire(root) }
pub fn state_root_for(path: &Path) -> std::io::Result<Option<PathBuf>> { state_lock::root_for(path) }
pub fn bind_owned_output(root: &Path, folder: &Path) -> std::io::Result<()> { state_lock::bind_output(root, folder) }

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
pub fn write_owned_file_until(path: &Path, bytes: &[u8], stop: &dyn Fn() -> bool) -> std::io::Result<()> {
    let started = std::time::Instant::now();
    loop {
        if stop() { return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "stopped before saving Atlas output")); }
        match write_owned_file(path, bytes) {
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock && started.elapsed() < std::time::Duration::from_secs(10) => std::thread::sleep(std::time::Duration::from_millis(20)),
            result => return result,
        }
    }
}

pub fn remove_owned_file(path: &Path) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or(Path::new("."));
    let _output = if parent.join(".atlas-output-write.lock").try_exists()? { Some(owned_output_lease(parent, true)?) } else { None };
    let _state = state_lock::for_path(path)?;
    std::fs::remove_file(path)
}

/// A live recording holds a shared file lease; a backup takes it exclusively
/// and fails promptly while a WAV's length/header are still being written.
pub fn owned_output_lease(folder: &Path, snapshot: bool) -> std::io::Result<std::fs::File> {
    std::fs::create_dir_all(folder)?;
    let path = folder.join(".atlas-output-write.lock");
    if std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink()) { return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "the output lock is a symbolic link")); }
    let file = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path)?;
    let locked = if snapshot { file.try_lock() } else { file.try_lock_shared() };
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
    (u64, Option<std::time::SystemTime>, std::sync::Arc<dyn std::any::Any + Send + Sync>),
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
    let Ok(m) = std::fs::metadata(path) else { return };
    if let Ok(mut w) = WRITTEN.lock().or_else(crate::crash::unpoison) {
        w.insert(path.to_path_buf(), (hash, m.len(), m.modified().ok()));
    }
    remember_seen(path, Some((m.len(), m.modified().ok())));
}

/// Note what the file at `path` (current format, `text`) holds that `T`
/// doesn't know, for `save` to put back (Q12). Costs one more round trip
/// through JSON per read, and records nothing in the usual case.
fn remember_unknown<T: DeserializeOwned + Serialize>(path: &Path, text: &str) {
    let raw = serde_json::from_str::<serde_json::Value>(text).ok().and_then(|v| v.get("data").cloned());
    let known = serde_json::from_str::<Envelope<T>>(text).ok().and_then(|e| serde_json::to_value(&e.data).ok());
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

/// A file's size and modified time, or `None` when it isn't there.
type Stamp = Option<(u64, Option<std::time::SystemTime>)>;

fn stamp_of(path: &Path) -> Stamp {
    std::fs::metadata(path).ok().map(|m| (m.len(), m.modified().ok()))
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
static FAILED_SAVES: std::sync::Mutex<Vec<(PathBuf, String, String, bool)>> = std::sync::Mutex::new(Vec::new());
const MOST_FAILED_SAVES_KEPT: usize = 64;

fn record_failed_save(root: &Path, name: &str, error: &str, transient: bool) {
    if let Ok(mut v) = FAILED_SAVES.lock().or_else(crate::crash::unpoison) {
        v.retain(|(r, n, _, _)| !(r == root && n == name));
        if v.len() >= MOST_FAILED_SAVES_KEPT {
            v.remove(0);
        }
        v.push((root.to_path_buf(), name.to_string(), error.to_string(), transient));
    }
}

/// The saves into `root` that failed since the last time this was asked, as
/// (record name, error). Taken, so each is reported once.
pub fn take_failed_saves(root: &Path) -> Vec<(String, String)> {
    take_failed_saves_detailed(root).into_iter().map(|(name, error, _)| (name, error)).collect()
}

pub fn take_failed_saves_detailed(root: &Path) -> Vec<(String, String, bool)> {
    let Ok(mut v) = FAILED_SAVES.lock().or_else(crate::crash::unpoison) else { return Vec::new() };
    let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut *v).into_iter().partition(|(r, _, _, _)| r == root);
    *v = rest;
    mine.into_iter().map(|(_, n, e, transient)| (n, e, transient)).collect()
}

/// A record name as a `&'static str`, for `Daemon::persist_failures`. The
/// names are a fixed, small set (one per kind of record), so each is leaked
/// once and reused.
pub fn intern_record_name(name: &str) -> &'static str {
    static NAMES: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());
    let Ok(mut v) = NAMES.lock().or_else(crate::crash::unpoison) else { return "a record" };
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
        Store { root: root.as_ref().to_path_buf() }
    }

    /// Where this store's own state lives — notes, the tray, the vault,
    /// peer tokens, the journal. On a real install this is `data/state`.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Hold across a group of saves so a backup sees either whole group.
    pub fn transaction(&self) -> Result<StateGuard> { Ok(state_transaction(&self.root)?) }

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
        let path = self.path(name);
        // Taken before reading: a write between the two leaves this older
        // than the file, so the change is seen as someone else's, never missed.
        let stamp = stamp_of(&path);
        let text = match read_with_patience(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                remember_seen(&path, None);
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
                remember_unknown::<T>(&path, &text);
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
    pub fn load_checked_bounded<T: DeserializeOwned + Serialize>(&self, name: &str, maximum: u64) -> Result<Option<T>> {
        use std::io::Read;
        let maximum = maximum.min(16 * 1024 * 1024);
        let path = self.path(name);
        let mut file = match std::fs::File::open(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => { remember_seen(&path, None); return Ok(None); }
            Err(e) => return Err(e.into()),
        };
        let metadata = file.metadata()?;
        let invalid = |why: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{name}: {why}; state was left untouched"));
        if !metadata.is_file() || metadata.len() > maximum { return Err(invalid("not a regular state file within the read budget").into()); }
        let before = Some((metadata.len(), metadata.modified().ok()));
        let mut text = String::new();
        (&mut file).take(maximum + 1).read_to_string(&mut text)?;
        if text.len() as u64 > maximum || stamp_of(&path) != before { return Err(invalid("state changed or exceeded its read budget").into()); }
        let raw: serde_json::Value = serde_json::from_str(&text).map_err(|_| invalid("malformed state"))?;
        let (data, envelope) = if let Some(schema) = raw.get("schema") {
            if schema.as_u64() != Some(u64::from(SCHEMA)) { return Err(invalid("unsupported state format").into()); }
            (raw.get("data").cloned().ok_or_else(|| invalid("missing state data"))?, text)
        } else {
            let envelope = serde_json::json!({"schema": SCHEMA, "data": raw.clone()}).to_string();
            (raw, envelope)
        };
        let value: T = serde_json::from_value(data).map_err(|_| invalid("state does not match this record"))?;
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
            record_failed_save(&self.root, name, "state was busy; the unreadable file was left intact", true);
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
        let stamp = std::fs::metadata(&path).ok().map(|m| (m.len(), m.modified().ok()));
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
        let seen = SEEN.lock().or_else(crate::crash::unpoison).ok().and_then(|s| s.get(&path).copied());
        match seen {
            Some(seen) => stamp_of(&path) != seen,
            None => false,
        }
    }

    pub fn save<T: Serialize>(&self, name: &str, value: &T) -> Result<()> {
        let r = self.save_unrecorded(name, value);
        if let Err(e) = &r {
            let transient = matches!(e, crate::error::AtlasError::Io(io) if io.kind() == std::io::ErrorKind::WouldBlock);
            record_failed_save(&self.root, name, &e.to_string(), transient);
        } else if let Ok(mut failures) = FAILED_SAVES.lock().or_else(crate::crash::unpoison) {
            failures.retain(|(root, record, _, _)| !(root == &self.root && record == name));
        }
        r
    }

    fn save_unrecorded<T: Serialize>(&self, name: &str, value: &T) -> Result<()> {
        let _guard = self.transaction()?;
        let final_path = self.path(name);
        // Never `unwrap_or_default` here: a value that won't serialize wrote
        // an empty file over the good one, atomically, and returned Ok -- the
        // next load set the empty file aside and the data was gone (Q3).
        let bad = |e: serde_json::Error| {
            crate::error::AtlasError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{name} couldn't be written as JSON: {e}")))
        };
        let unknown = UNKNOWN.lock().or_else(crate::crash::unpoison).ok().and_then(|u| u.get(&final_path).cloned());
        let body = match unknown {
            // What a newer Atlas wrote and this one doesn't know, put back (Q12).
            Some(u) => {
                let mut data = serde_json::to_value(value).map_err(bad)?;
                put_back_unknown(&u, &mut data);
                serde_json::to_string_pretty(&serde_json::json!({ "schema": SCHEMA, "data": data })).map_err(bad)?
            }
            None => serde_json::to_string_pretty(&Envelope { schema: SCHEMA, data: value }).map_err(bad)?,
        };

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
        let seen = std::fs::metadata(&final_path).ok().map(|m| (m.len(), m.modified().ok()));
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
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(body.as_bytes())?;
            f.sync_all()?;
        }

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
            return Err(crate::error::AtlasError::Io(std::io::Error::new(std::io::ErrorKind::AlreadyExists, format!("{new_name} is already there"))));
        }
        std::fs::rename(self.path(name), to)?;
        Ok(())
    }
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// How many times, and how far apart, a file that exists but can't be read
/// is tried again before it is set aside. A virus scanner or OneDrive holds a
/// file for a moment; one failed read used to be enough to start that part
/// of Atlas empty (28 Sep 2026).
const READ_TRIES: u32 = 5;
const READ_PAUSE_MS: u64 = 100;

/// `read_to_string`, tried again a few times when the file is there but
/// locked. A missing file is answered at once.
fn read_with_patience(path: &std::path::Path) -> std::io::Result<String> {
    let mut last = None;
    for attempt in 0..READ_TRIES {
        match std::fs::read_to_string(path) {
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
    cfg!(windows) && (e.kind() == std::io::ErrorKind::PermissionDenied || matches!(e.raw_os_error(), Some(5 | 32 | 33)))
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
    rename_patiently_with(from, to, &RENAME_PAUSES_MS, is_a_passing_lock, |f, t| std::fs::rename(f, t))
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
pub fn read_json<T: serde::de::DeserializeOwned + Default>(path: &Path) -> T {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// `value` written to `path` as pretty JSON, whole (`write_whole`).
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    write_whole(path, &serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?)
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
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
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

fn note_set_aside(root: &std::path::Path, name: &str, why: &str) {
    let mut v = SET_ASIDE.lock().unwrap_or_else(|p| p.into_inner());
    v.push(SetAside { root: root.to_path_buf(), name: name.to_string(), why: why.to_string(), at: now(), told: false });
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
    let what = names.iter().map(|n| n.replace('_', " ")).collect::<Vec<_>>().join(", ");
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
    for s in v.iter_mut().filter(|s| !s.told && s.root == store.root) {
        s.told = true;
        let list = if s.why == THEIRS { &mut theirs } else { &mut unreadable };
        if !list.contains(&s.name) {
            list.push(s.name.clone());
        }
    }
    let mut said = Vec::new();
    if !unreadable.is_empty() {
        said.push(set_aside_sentence(&unreadable));
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
    let what = names.iter().map(|n| n.replace('_', " ")).collect::<Vec<_>>().join(", ");
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
        assert_eq!(s.install_root(), PathBuf::from("/tmp/atlas-some-test-abc123"));
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
