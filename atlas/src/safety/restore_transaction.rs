//! Write-ahead restore journal. An interrupted restore is rolled back before
//! another command may read state. Originals stay in the normal trash ledger.
use super::*;
const JOURNAL: &str = ".restore-journal.json";
const DIRECTORY_OWNER: &str = ".atlas-restore-directory-owner";

#[derive(Serialize, Deserialize)]
struct Entry { relative: PathBuf, original: Option<String>, replacement: String, #[serde(default)] output: Option<String>, #[serde(default)] directory_owner: Option<String> }
#[derive(Serialize, Deserialize)]
struct Journal { version: u32, staging: String, trash_dir: String, keep_days: u64, committed: bool, entries: Vec<Entry>, #[serde(default)] notes_root: Option<PathBuf> }

fn save(root: &Path, journal: &Journal) -> Result<()> {
    crate::store::write_json(&root.join(JOURNAL), journal)?;
    Ok(())
}

fn valid_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty() && path.components().all(|c| matches!(c, std::path::Component::Normal(_)))
}

fn checked_target(root: &Path, relative: &Path) -> Result<PathBuf> {
    if !valid_relative(relative) { return Err(AtlasError::Platform("restore path is invalid".into())); }
    let mut target = root.to_path_buf();
    for part in relative.components() {
        target.push(part.as_os_str());
        match std::fs::symlink_metadata(&target) {
            Ok(meta) if meta.file_type().is_symlink() => return Err(AtlasError::Platform(format!("restore won't follow the symbolic link at {}", target.display()))),
            Ok(_) => {},
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
            Err(e) => return Err(e.into()),
        }
    }
    Ok(target)
}

fn entry_target(root: &Path, entry: &Entry, notes_root: Option<&Path>) -> Result<PathBuf> {
    match entry.output.as_deref() {
        None => checked_target(root, &entry.relative),
        Some(folder @ ("notes" | "calls" | "reading" | "configured-notes")) => {
            let prefix = Path::new(OWNED_OUTPUTS).join(folder);
            let relative = entry.relative.strip_prefix(prefix).map_err(|_| AtlasError::Platform("restore output path is invalid".into()))?;
            let destination = if folder == "configured-notes" {
                let destination = notes_root.ok_or_else(|| AtlasError::Platform("restore is missing its configured notes binding".into()))?;
                if crate::store::state_root_for(destination)?.and_then(|path| std::fs::canonicalize(path).ok()).as_deref() != Some(root) { return Err(AtlasError::Platform("configured restore notes no longer belong to this state root; recovery was stopped".into())); }
                destination.to_path_buf()
            } else { crate::store::Store::new(root.to_path_buf()).data_dir().join(folder) };
            if relative.as_os_str().is_empty() {
                if std::fs::symlink_metadata(&destination).is_ok_and(|metadata| metadata.file_type().is_symlink()) { return Err(AtlasError::Platform("restore output root is a symbolic link".into())); }
                Ok(destination)
            } else { checked_target(&destination, relative) }
        }
        Some(_) => Err(AtlasError::Platform("restore output root is invalid".into())),
    }
}

fn directory_witness(root: &Path, entry: &Entry, staging: &Path, notes_root: Option<&Path>) -> Result<PathBuf> {
    let recorded = Path::new(&entry.replacement);
    if let Some(folder) = &entry.output {
        let base = entry_target(root, &Entry { relative: Path::new(OWNED_OUTPUTS).join(folder), original: None, replacement: String::new(), output: Some(folder.clone()), directory_owner: None }, notes_root)?;
        let parent = base.parent().ok_or_else(|| AtlasError::Platform("restore output root has no parent".into()))?;
        let prefix = format!("{}-directory-", staging.file_name().unwrap().to_string_lossy());
        if !matches!(recorded.parent(), Some(candidate) if candidate == parent || candidate == base) || !recorded.file_name().is_some_and(|name| name.to_string_lossy().starts_with(&prefix)) { return Err(AtlasError::Platform("directory witness is outside its current output root; recovery was stopped".into())); }
        checked_target(recorded.parent().unwrap(), Path::new(recorded.file_name().unwrap()))
    } else {
        if !valid_relative(recorded) || recorded.components().next().is_none_or(|component| component.as_os_str() != ".directory-witnesses") { return Err(AtlasError::Platform("directory recovery witness path is invalid".into())); }
        checked_target(staging, recorded)
    }
}

fn cleanup_private_witness(path: &Path, token: &str) -> Result<()> {
    if !path.try_exists()? { return Ok(()); }
    if std::fs::symlink_metadata(path)?.file_type().is_symlink() { return Err(AtlasError::Platform("private directory witness is a symbolic link; it was retained".into())); }
    let children = std::fs::read_dir(path)?.collect::<std::io::Result<Vec<_>>>()?;
    if children.is_empty() { std::fs::remove_dir(path)?; return Ok(()); }
    if children.len() != 1 || children[0].file_name() != DIRECTORY_OWNER || std::fs::symlink_metadata(children[0].path())?.file_type().is_symlink() || std::fs::read(children[0].path())? != token.as_bytes() { return Err(AtlasError::Platform("private directory witness contains changed files; they were retained".into())); }
    std::fs::remove_file(children[0].path())?; std::fs::remove_dir(path)?;
    Ok(())
}

fn publish_directory(source: &Path, destination: &Path) -> Result<()> {
    #[cfg(windows)] { std::fs::rename(source, destination)?; Ok(()) }
    #[cfg(target_os = "linux")] {
        use std::os::unix::ffi::OsStrExt;
        let source = std::ffi::CString::new(source.as_os_str().as_bytes()).map_err(|_| AtlasError::Platform("directory path contains a null byte".into()))?;
        let destination = std::ffi::CString::new(destination.as_os_str().as_bytes()).map_err(|_| AtlasError::Platform("directory path contains a null byte".into()))?;
        let result = unsafe { libc::renameat2(libc::AT_FDCWD, source.as_ptr(), libc::AT_FDCWD, destination.as_ptr(), libc::RENAME_NOREPLACE) };
        if result != 0 { return Err(std::io::Error::last_os_error().into()); }
        Ok(())
    }
    #[cfg(not(any(windows, target_os = "linux")))] { let _ = (source, destination); Err(AtlasError::Platform("safe directory publication is unavailable on this platform; restore was not started".into())) }
}

fn clean_owned_directory(target: &Path, token: &str, committed: bool, recovery: Option<&Path>) -> Result<()> {
    let metadata = match std::fs::symlink_metadata(target) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() { return Err(AtlasError::Platform("a restore-owned directory changed type; recovery was retained".into())); }
    let marker = target.join(DIRECTORY_OWNER);
    if std::fs::symlink_metadata(&marker).is_ok_and(|metadata| metadata.file_type().is_symlink()) { return Err(AtlasError::Platform("directory ownership evidence is a symbolic link; recovery was retained".into())); }
    let bytes = match std::fs::read(&marker) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && committed => return Ok(()),
        Err(error) => return Err(AtlasError::Platform(format!("a restore-owned directory lost its ownership evidence ({error}); recovery was retained"))),
    };
    if bytes != token.as_bytes() { return Err(AtlasError::Platform("a restore-owned directory has foreign ownership evidence; recovery was retained".into())); }
    if !committed && std::fs::read_dir(target)?.any(|entry| entry.map(|entry| entry.file_name() != DIRECTORY_OWNER).unwrap_or(true)) {
        return Err(AtlasError::Platform("a restore-owned directory contains later files; they and the recovery journal were retained".into()));
    }
    if committed { std::fs::remove_file(marker)?; }
    else {
        // Keep ownership evidence through an atomic no-clobber move back to
        // private staging. A crash cannot leave an unidentifiable empty target.
        publish_directory(target, recovery.unwrap())?;
    }
    Ok(())
}

pub(super) fn recover(root: &Path) -> Result<()> {
    let text = match std::fs::read(root.join(JOURNAL)) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    let journal: Journal = serde_json::from_slice(&text).map_err(|e| AtlasError::Platform(format!("restore recovery record could not be read ({e}); state commands are stopped")))?;
    if !matches!(journal.version, 1 | 2) || !journal.staging.starts_with(".restoring-") || !valid_relative(Path::new(&journal.staging)) || Path::new(&journal.staging).components().count() != 1 || journal.entries.iter().any(|e| !valid_relative(&e.relative)) {
        return Err(AtlasError::Platform("restore recovery record is invalid; no recovery files were moved".into()));
    }
    let staging = root.join(&journal.staging);
    if !journal.committed {
        let trash = Trash::new(TrashConfig { dir: journal.trash_dir.clone(), keep_days: journal.keep_days });
        let why = format!("replaced by restore {}", journal.staging);
        for entry in journal.entries.iter().rev() {
            let target = entry_target(root, entry, journal.notes_root.as_deref())?;
            if let Some(token) = &entry.directory_owner {
                clean_owned_directory(&target, token, false, Some(&directory_witness(root, entry, &staging, journal.notes_root.as_deref())?))?;
                continue;
            }
            let current = match std::fs::symlink_metadata(&target) {
                Ok(_) => Some(hash_file(&target)?),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return Err(e.into()),
            };
            let ledger = trash.read_ledger();
            let records = match ledger {
                LedgerState::Fresh => Vec::new(),
                LedgerState::Read(records) => records,
                LedgerState::Unreadable(why) | LedgerState::Corrupt(why) => return Err(AtlasError::Platform(format!("restore recovery needs the trash ledger: {why}"))),
            };
            let old = records.iter().find(|item| Path::new(&item.original) == target && item.why == why);
            match (&entry.original, old) {
                (Some(original), Some(old)) => {
                    if Path::new(&old.held).is_file() && hash_file(Path::new(&old.held))? != *original { return Err(AtlasError::Platform(format!("the saved original for {} changed; both files and the recovery record were retained", entry.relative.display()))); }
                    if current.as_ref() == Some(&entry.replacement) && Path::new(&old.held).is_file() {
                        std::fs::remove_file(&target)?;
                    } else if current.is_some() && !(current.as_ref() == Some(original) && !Path::new(&old.held).exists()) {
                        return Err(AtlasError::Platform(format!("restore recovery is waiting: {} changed after the restore; its current file and the saved original were kept", entry.relative.display())));
                    }
                    trash.undo(old.id)?;
                }
                (Some(original), None) if current.as_ref() == Some(original) => {},
                (None, None) if current.is_none() => {},
                (None, None) if current.as_ref() == Some(&entry.replacement) => { std::fs::remove_file(&target)?; },
                _ => return Err(AtlasError::Platform(format!("restore recovery cannot safely identify {} and has stopped; recovery records were retained", entry.relative.display()))),
            }
        }
    }
    if journal.committed {
        for entry in &journal.entries {
            if let Some(token) = &entry.directory_owner { clean_owned_directory(&entry_target(root, entry, journal.notes_root.as_deref())?, token, true, None)?; }
        }
    }
    for entry in &journal.entries {
        if let Some(token) = &entry.directory_owner {
            let witness = directory_witness(root, entry, &staging, journal.notes_root.as_deref())?;
            cleanup_private_witness(&witness, token)?;
        }
    }
    if staging.is_dir() { std::fs::remove_dir_all(staging)?; }
    std::fs::remove_file(root.join(JOURNAL))?;
    Ok(())
}



#[cfg(test)]
pub(super) fn restore_with_hook(backup: &Path, state: &Path, trash: &Trash, mine: &crate::household::Household, before: &mut dyn FnMut(usize) -> Result<()>) -> Result<usize> {
    let notes = crate::store::Store::new(state.to_path_buf()).notes_dir();
    restore_with_notes_and_hook(backup, state, trash, mine, &notes, before)
}

pub(super) fn restore_with_notes(backup: &Path, state: &Path, trash: &Trash, mine: &crate::household::Household, notes: &Path) -> Result<usize> {
    restore_with_notes_and_hook(backup, state, trash, mine, notes, &mut |_| Ok(()))
}

fn restore_with_notes_and_hook(backup: &Path, state: &Path, trash: &Trash, mine: &crate::household::Household, notes: &Path, before: &mut dyn FnMut(usize) -> Result<()>) -> Result<usize> {
    if !backup.is_dir() { return Err(AtlasError::Platform("no such backup".into())); }
    let _access = crate::store::state_access(state, true)?;
    let guard = crate::store::wait_for_state_transaction(state)?;
    std::fs::create_dir_all(state)?;
    let actual_state = std::fs::canonicalize(state)?;
    let namespace = actual_state.strip_prefix(&guard.root).map_err(|_| AtlasError::Platform("the restore state root does not match its lock".into()))?.to_path_buf();
    let state = guard.root.as_path();
    recover(state)?;
    let store = crate::store::Store::new(state.to_path_buf());
    check_configured_scope(&store, notes, Path::new(&trash.cfg.dir))?;
    let notes_root = Some(std::fs::canonicalize(notes)?);
    // Command lifetime leases protect normal launches. Also reject standalone
    // recording writers that use the output API without a command lease.
    let mut output_paths: Vec<PathBuf> = OWNED_FOLDERS.into_iter().map(|folder| store.data_dir().join(folder)).collect();
    output_paths.extend([notes.to_path_buf(), PathBuf::from(&trash.cfg.dir)]);
    let mut seen = std::collections::HashSet::new(); let mut output_leases = Vec::new();
    for path in output_paths {
        if path.is_dir() && seen.insert(std::fs::canonicalize(&path)?) {
            output_leases.push(crate::store::owned_output_lease(&path, true)?);
            hold_output_children(&path, &mut output_leases)?;
        }
    }
    // Validate the source without Store::load's corrupt-file preservation:
    // restoring never modifies the backup being read.
    match std::fs::read(backup.join("household.json")) {
        Ok(bytes) => {
            let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| AtlasError::Platform(format!("the backup household record is unreadable ({e}); your state was left intact")))?;
            if value.get("schema").and_then(serde_json::Value::as_u64).is_some_and(|v| v != crate::store::SCHEMA as u64) { return Err(AtlasError::Platform("the backup needs a different Atlas state version".into())); }
            let household: crate::household::Household = serde_json::from_value(value.get("data").cloned().unwrap_or(value)).map_err(|e| AtlasError::Platform(format!("the backup household record is invalid ({e})")))?;
            crate::household::accept_bundle(&mine.id, &household.id).map_err(AtlasError::Platform)?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
        Err(e) => return Err(e.into()),
    }
    let mut planned = Vec::new();
    gather(backup, Path::new(""), &mut planned)?;
    let mut planned_directories = Vec::new();
    gather_directories(backup, Path::new(""), &mut planned_directories)?;
    planned.retain(|(_, relative)| relative != Path::new("household.json"));
    planned.sort_by(|a, b| a.1.cmp(&b.1));
    let staging_name = format!(".restoring-{}-{}-{}", now(), std::process::id(), NEXT_SAFETY_OPERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    let staging = state.join(&staging_name);
    std::fs::create_dir(&staging)?;
    let mut external_witnesses = Vec::new();
    let prepared = (|| -> Result<Vec<Entry>> {
        let manifest = match std::fs::read(backup.join(BACKUP_MANIFEST)) {
            Ok(bytes) => Some(serde_json::from_slice::<BackupManifest>(&bytes).map_err(|e| AtlasError::Platform(format!("backup manifest is unreadable: {e}")))?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        if let Some(manifest) = &manifest {
            let mut all = Vec::new();
            gather(backup, Path::new(""), &mut all)?;
            if !matches!(manifest.version, 1 | 2) || all.len() != manifest.entries.len() || (manifest.version == 2 && manifest.directories != planned_directories) { return Err(AtlasError::Platform("the backup no longer holds every file or directory recorded in its manifest; state was left intact".into())); }
            for recorded in &manifest.entries {
                if !valid_relative(&recorded.relative) || !all.iter().any(|(_, relative)| relative == &recorded.relative)
                    || hash_file(&backup.join(&recorded.relative))? != recorded.sha256 {
                    return Err(AtlasError::Platform("a backup file is missing or changed from its manifest; state was left intact".into()));
                }
            }
        }
        let mut entries = Vec::new();
        for (source, relative) in &planned {
            if !valid_relative(relative) { return Err(AtlasError::Platform("backup path is invalid".into())); }
            let mut output = None;
            let mut journal_relative = namespace.join(relative);
            if let Ok(owned_relative) = relative.strip_prefix(OWNED_OUTPUTS) {
                let folder = owned_relative.components().next().and_then(|c| c.as_os_str().to_str()).ok_or_else(|| AtlasError::Platform("backup output root is invalid".into()))?;
                if !OWNED_FOLDERS.contains(&folder) && !matches!(folder, "configured-notes" | "configured-trash") { return Err(AtlasError::Platform("backup output root is unsupported".into())); }
                if matches!(folder, "trash" | "recovery" | "configured-trash") {
                    // Preserve current trash IDs and the live key card. Recovery
                    // copies are retained in an operation-specific archive.
                    journal_relative = PathBuf::from(format!(".restored-recovery-{}", &staging_name)).join(owned_relative);
                } else {
                    output = Some(folder.to_owned());
                    journal_relative = relative.clone();
                }
            }
            let staged = staging.join(&journal_relative);
            std::fs::create_dir_all(staged.parent().unwrap())?;
            copy_file_durably(source, &staged)?;
            let source_hash = hash_file(&staged)?;
            if let Some(manifest) = &manifest {
                let found = manifest.entries.iter().find(|e| &e.relative == relative);
                if !matches!(manifest.version, 1 | 2) || !found.is_some_and(|e| e.sha256 == source_hash && e.bytes == std::fs::metadata(&staged).map(|m| m.len()).unwrap_or(u64::MAX)) { return Err(AtlasError::Platform(format!("the backup's recorded bytes do not match {}; no state was changed", relative.display()))); }
            }
            if relative == &Path::new(OWNED_OUTPUTS).join("trash").join("ledger.json") || relative == &Path::new(OWNED_OUTPUTS).join("configured-trash").join("ledger.json") {
                let mut records: Vec<Discarded> = serde_json::from_slice(&std::fs::read(&staged)?).map_err(|e| AtlasError::Platform(format!("the archived trash ledger is invalid ({e})")))?;
                let archive = state.join(journal_relative.parent().unwrap());
                let backup_trash = source.parent().unwrap();
                let mut ids = std::collections::BTreeSet::new();
                let mut names = std::collections::BTreeSet::new();
                for record in &mut records {
                    let held = Path::new(&record.held).file_name().ok_or_else(|| AtlasError::Platform("the archived trash file name is invalid".into()))?;
                    if !ids.insert(record.id) || !names.insert(held.to_owned()) {
                        return Err(AtlasError::Platform("the archived trash ledger has duplicate recovery identities; no state was changed".into()));
                    }
                    let saved = backup_trash.join(held);
                    let metadata = std::fs::symlink_metadata(&saved).map_err(|error| AtlasError::Platform(format!("the archived recovery file {} is missing or unreadable ({error}); no state was changed", held.to_string_lossy())))?;
                    if metadata.file_type().is_symlink() || !(metadata.is_file() || metadata.is_dir()) {
                        return Err(AtlasError::Platform("an archived recovery file has an unsafe type; no state was changed".into()));
                    }
                    record.held = archive.join(held).display().to_string();
                }
                crate::store::write_json(&staged, &records)?;
            }
            let replacement = hash_file(&staged)?;
            let entry = Entry { relative: journal_relative, original: None, replacement, output, directory_owner: None };
            let target = entry_target(state, &entry, notes_root.as_deref())?;
            let original = match std::fs::symlink_metadata(&target) {
                Ok(_) => Some(hash_file(&target)?),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return Err(e.into()),
            };
            entries.push(Entry { original, ..entry });
        }
        // Every newly needed parent is claimed separately. Existing owner
        // folders stay outside this operation's rollback ownership.
        let mut directories = std::collections::BTreeMap::new();
        let mut needed: Vec<(PathBuf, Option<String>)> = entries.iter().filter_map(|entry| entry.relative.parent().filter(|parent| !parent.as_os_str().is_empty()).map(|parent| (parent.to_path_buf(), entry.output.clone()))).collect();
        for relative in &planned_directories {
            if let Ok(owned) = relative.strip_prefix(OWNED_OUTPUTS) {
                // The reserved grouping roots are Atlas metadata; actual
                // owner folders begin below them. Required missing output
                // roots are still claimed from each child entry's parents.
                if owned.components().count() < 2 { continue; }
                let folder = owned.components().next().unwrap().as_os_str().to_str().unwrap_or("");
                if matches!(folder, "trash" | "recovery" | "configured-trash") { needed.push((PathBuf::from(format!(".restored-recovery-{staging_name}")).join(owned), None)); }
                else if matches!(folder, "notes" | "calls" | "reading" | "configured-notes") { needed.push((relative.clone(), Some(folder.into()))); }
                else { return Err(AtlasError::Platform("backup directory root is unsupported".into())); }
            } else { needed.push((namespace.join(relative), None)); }
        }
        for (mut relative, output) in needed {
            while !relative.as_os_str().is_empty() && !(output.is_some() && relative.components().count() <= 1) {
                let mut entry = Entry { relative: relative.clone(), original: None, replacement: String::new(), output: output.clone(), directory_owner: Some(String::new()) };
                let target = entry_target(state, &entry, notes_root.as_deref())?;
                match std::fs::symlink_metadata(&target) {
                    Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {},
                    Ok(_) => return Err(AtlasError::Platform("a restore directory destination is occupied by another file; no state was changed".into())),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => { directories.entry((output.clone(), relative.clone())).or_insert_with(|| { entry.directory_owner = Some(format!("{staging_name}:{}", target.display())); entry }); },
                    Err(error) => return Err(error.into()),
                }
                relative = relative.parent().unwrap_or(Path::new("")).to_path_buf();
            }
        }
        let mut directories: Vec<_> = directories.into_values().collect();
        directories.sort_by_key(|entry| entry.relative.components().count());
        #[cfg(not(any(windows, target_os = "linux")))]
        if !directories.is_empty() { return Err(AtlasError::Platform("safe directory publication is unavailable on this platform; restore was not started".into())); }
        for (index, entry) in directories.iter_mut().enumerate() {
            let witness = if let Some(folder) = &entry.output {
                let base = entry_target(state, &Entry { relative: Path::new(OWNED_OUTPUTS).join(folder), original: None, replacement: String::new(), output: Some(folder.clone()), directory_owner: None }, notes_root.as_deref())?;
                let parent = if base.is_dir() { base.as_path() } else { base.parent().unwrap() };
                parent.join(format!("{staging_name}-directory-{index}"))
            } else { staging.join(".directory-witnesses").join(index.to_string()) };
            std::fs::create_dir_all(witness.parent().unwrap())?;
            std::fs::create_dir(&witness)?;
            if entry.output.is_some() { external_witnesses.push((witness.clone(), entry.directory_owner.clone().unwrap())); }
            crate::store::write_owned_file(&witness.join(DIRECTORY_OWNER), entry.directory_owner.as_ref().unwrap().as_bytes())?;
            entry.replacement = if entry.output.is_some() { witness.display().to_string() } else { witness.strip_prefix(&staging).unwrap().display().to_string() };
        }
        directories.extend(entries);
        Ok(directories)
    })();
    let entries = match prepared { Ok(entries) => entries, Err(e) => { for (path, token) in &external_witnesses { crate::heard!(cleanup_private_witness(path, token)); } crate::heard!(std::fs::remove_dir_all(&staging)); return Err(e); } };
    let mut journal = Journal { version: 2, staging: staging_name, trash_dir: trash.cfg.dir.clone(), keep_days: trash.cfg.keep_days, committed: false, entries, notes_root };
    if let Err(e) = save(state, &journal) { for (path, token) in &external_witnesses { crate::heard!(cleanup_private_witness(path, token)); } crate::heard!(std::fs::remove_dir_all(&staging)); return Err(e); }
    let moved = (|| -> Result<usize> {
        let mut count = 0;
        for (operation, entry) in journal.entries.iter().enumerate() {
            before(operation)?;
            let target = entry_target(state, entry, journal.notes_root.as_deref())?;
            if entry.directory_owner.is_some() { publish_directory(&directory_witness(state, entry, &staging, journal.notes_root.as_deref())?, &target)?; continue; }
            std::fs::create_dir_all(target.parent().unwrap())?;
            if entry.original.is_some() { trash.take(&target, &format!("replaced by restore {}", journal.staging))?; }
            move_across(&staging.join(&entry.relative), &target)?;
            count += 1;
        }
        journal.committed = true;
        save(state, &journal)?;
        Ok(count)
    })();
    match moved {
        Ok(count) => {
            recover(state).map_err(|e| AtlasError::Platform(format!("restored {count} files; cleanup is pending ({e}). The completed restore was retained")))?;
            Ok(count)
        },
        Err(error) => {
            journal.committed = false;
            match recover(state) {
                Ok(()) => Err(AtlasError::Platform(format!("restore failed ({error}); every original file was put back"))),
                Err(recovery) => Err(AtlasError::Platform(format!("restore failed ({error}); recovery is still pending ({recovery}). State commands are stopped until the original files can be recovered"))),
            }
        }
    }
}
