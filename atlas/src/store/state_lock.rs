//! One OS-held lock for an entire state root, shared by every process.
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

pub const MARKER: &str = ".atlas-state-root";
pub const LOCK: &str = ".state-operations.lock";
const ACCESS_LOCK: &str = ".state-access.lock";
const VERSION: &[u8] = b"Atlas state root v1\n";
pub const OUTPUT_OWNER: &str = ".atlas-output-owner";

thread_local! {
    // Reentrancy shares the actual OS lock, rather than replacing it with a
    // process-local mutex. Other threads open their own handle and wait.
    static HELD: RefCell<HashMap<PathBuf, Weak<File>>> = RefCell::new(HashMap::new());
    static ACCESS: RefCell<HashMap<PathBuf, (bool, Weak<File>)>> = RefCell::new(HashMap::new());
}

/// Must stay on its acquiring thread; the last nested guard releases the lock.
pub struct StateGuard { _file: Rc<File>, pub root: PathBuf }

/// Lifetime read lease for a whole command, or exclusive restore lease.
pub struct AccessGuard { _file: Rc<File>, pub root: PathBuf }

pub(super) fn access(explicit_root: &Path, exclusive: bool) -> io::Result<AccessGuard> {
    let root = root_for(explicit_root)?.unwrap_or_else(|| explicit_root.to_path_buf());
    std::fs::create_dir_all(&root)?;
    let root = std::fs::canonicalize(root)?;
    if let Some((held_exclusive, file)) = ACCESS.with(|held| held.borrow().get(&root).and_then(|(mode, file)| file.upgrade().map(|file| (*mode, file)))) {
        if exclusive && !held_exclusive { return Err(io::Error::new(io::ErrorKind::WouldBlock, "this command already holds a state read lease; restore requires a separate stopped command")); }
        return Ok(AccessGuard { _file: file, root });
    }
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(root.join(ACCESS_LOCK))?;
    let locked = if exclusive { file.try_lock() } else { file.try_lock_shared() };
    locked.map_err(|e| io::Error::new(io::ErrorKind::WouldBlock, format!("Atlas state is in use or being restored ({e}); this command has not read or changed state")))?;
    let file = Rc::new(file);
    ACCESS.with(|held| { let mut held = held.borrow_mut(); held.retain(|_, (_, f)| f.strong_count() > 0); held.insert(root.clone(), (exclusive, Rc::downgrade(&file))); });
    Ok(AccessGuard { _file: file, root })
}

/// An explicit store root owns custom layouts. Real profile stores and raw
/// writers all use the same outer data/state root, including at first startup.
pub(super) fn root_for(path: &Path) -> io::Result<Option<PathBuf>> {
    let mut root = None;
    for ancestor in path.ancestors() {
        match std::fs::read_to_string(ancestor.join(OUTPUT_OWNER)) {
            Ok(owner) => {
                let owned = owner.strip_prefix("Atlas output root v1\n").map(PathBuf::from).filter(|path| path.is_absolute()).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "the Atlas output ownership marker is invalid"))?;
                if std::fs::read(owned.join(MARKER))? != VERSION { return Err(io::Error::new(io::ErrorKind::InvalidData, "the Atlas output's state root marker is invalid")); }
                return Ok(Some(owned));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {},
            Err(error) => return Err(error),
        }
        if ancestor.parent().and_then(Path::file_name).is_some_and(|n| n == "data")
            && ancestor.file_name().is_some_and(|n| ["notes", "calls", "reading", "trash", "recovery"].iter().any(|name| n == *name)) {
            root = ancestor.parent().map(|data| data.join("state"));
        }
        let structural = ancestor.file_name().is_some_and(|n| n == "state")
            && ancestor.parent().and_then(Path::file_name).is_some_and(|n| n == "data");
        match std::fs::read(ancestor.join(MARKER)) {
            Ok(bytes) if bytes == VERSION => root = Some(ancestor.to_path_buf()),
            Ok(_) => return Err(io::Error::new(io::ErrorKind::InvalidData, "the Atlas state-root marker is invalid; no state was changed")),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                if structural { root = Some(ancestor.to_path_buf()); }
            }
            Err(e) => return Err(e),
        }
    }
    Ok(root)
}

/// Persist the binding so a raw output writer in another process acquires
/// the same native state lock even for an explicitly configured external root.
pub(super) fn bind_output(explicit_root: &Path, folder: &Path) -> io::Result<()> {
    use std::io::Write;
    let guard = try_acquire(explicit_root)?;
    std::fs::create_dir_all(folder)?;
    if std::fs::symlink_metadata(folder)?.file_type().is_symlink() { return Err(io::Error::new(io::ErrorKind::InvalidInput, "the configured Atlas output root is a symbolic link")); }
    let folder = std::fs::canonicalize(folder)?;
    if root_for(&folder)?.is_some_and(|root| std::fs::canonicalize(root).ok().as_ref() == Some(&guard.root)) { return Ok(()); }
    let marker = folder.join(OUTPUT_OWNER);
    let descriptor = format!("Atlas output root v1\n{}", guard.root.display());
    match std::fs::read_to_string(&marker) {
        Ok(existing) if existing == descriptor => return Ok(()),
        Ok(_) => return Err(io::Error::new(io::ErrorKind::InvalidInput, "the configured output folder belongs to a different Atlas state root")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {},
        Err(error) => return Err(error),
    }
    let temporary = folder.join(format!("{OUTPUT_OWNER}.{}.writing", std::process::id()));
    let written = (|| -> io::Result<()> {
        let mut file = OpenOptions::new().write(true).create_new(true).open(&temporary)?;
        file.write_all(descriptor.as_bytes())?; file.sync_all()?; drop(file);
        std::fs::rename(&temporary, &marker)
    })();
    if written.is_err() { crate::heard!(std::fs::remove_file(&temporary)); }
    written
}

pub(super) fn acquire(explicit_root: &Path) -> io::Result<StateGuard> {
    acquire_with_wait(explicit_root, std::time::Duration::from_secs(10))
}

pub(super) fn try_acquire(explicit_root: &Path) -> io::Result<StateGuard> {
    acquire_with_wait(explicit_root, std::time::Duration::ZERO)
}

fn acquire_with_wait(explicit_root: &Path, wait: std::time::Duration) -> io::Result<StateGuard> {
    let root = root_for(explicit_root)?.unwrap_or_else(|| explicit_root.to_path_buf());
    std::fs::create_dir_all(&root)?;
    let root = std::fs::canonicalize(root)?;
    if let Some(file) = HELD.with(|held| held.borrow().get(&root).and_then(Weak::upgrade)) {
        return Ok(StateGuard { _file: file, root });
    }
    let path = root.join(LOCK);
    if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "the Atlas state lock is a symbolic link"));
    }
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path)?;
    // A bounded wait keeps a failed or slow state operation from freezing a
    // second Atlas indefinitely. Killing the holder releases this OS lock.
    let started = std::time::Instant::now();
    loop {
        match file.try_lock() {
            Ok(()) => break,
            Err(std::fs::TryLockError::WouldBlock) if started.elapsed() < wait => std::thread::sleep(std::time::Duration::from_millis(20)),
            Err(std::fs::TryLockError::WouldBlock) => return Err(io::Error::new(io::ErrorKind::WouldBlock, "Atlas state is busy in another operation; no state was changed")),
            Err(std::fs::TryLockError::Error(e)) => return Err(e),
        }
    }
    match std::fs::read(root.join(MARKER)) {
        Ok(bytes) if bytes == VERSION => {},
        Ok(_) => return Err(io::Error::new(io::ErrorKind::InvalidData, "the Atlas state-root marker is invalid")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            use std::io::Write;
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let temporary = root.join(format!("{MARKER}.{}-{}.writing", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
            let written = (|| -> io::Result<()> {
                let mut marker = OpenOptions::new().write(true).create_new(true).open(&temporary)?;
                marker.write_all(VERSION)?;
                marker.sync_all()?;
                drop(marker);
                std::fs::rename(&temporary, root.join(MARKER))
            })();
            if written.is_err() { crate::heard!(std::fs::remove_file(&temporary)); }
            written?;
        },
        Err(e) => return Err(e),
    }
    let file = Rc::new(file);
    HELD.with(|held| { let mut held = held.borrow_mut(); held.retain(|_, f| f.strong_count() > 0); held.insert(root.clone(), Rc::downgrade(&file)); });
    Ok(StateGuard { _file: file, root })
}

pub(super) fn for_path(path: &Path) -> io::Result<Option<StateGuard>> {
    root_for(path.parent().unwrap_or(path))?.map(|root| try_acquire(&root)).transpose()
}
