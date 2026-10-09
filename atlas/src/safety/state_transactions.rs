//! Disposable fixtures for backup/restore transactions; never an installed root.
#![cfg(test)]
use super::*;

#[cfg(windows)]
#[test]
fn native_rename_terminates_every_utf16_length_class_and_preserves_exact_identity() {
    use std::os::windows::ffi::OsStrExt;
    let area = Area::new(); let root = std::fs::canonicalize(&area.0).unwrap();
    for unicode in ["plain", "crab-🦀"] {
        for remainder in 0..4 {
            let source = root.join(format!("source-{unicode}-{remainder}"));
            let mut name = format!("destination-{unicode}-{remainder}");
            while root.join(&name).as_os_str().encode_wide().count() % 4 != remainder { name.push('x'); }
            let destination = root.join(name);
            std::fs::write(&source, b"exact native original").unwrap();
            let held = MoveCohort::hold(&source, &|| false).unwrap();
            assert!(held.rename_file(&destination).unwrap());
            assert!(destination.is_file(), "reported success must have exact named destination for UTF16 class {remainder}");
            assert!(!source.exists()); assert_eq!(std::fs::read(&destination).unwrap(), b"exact native original");
            drop(held);
        }
    }
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 8, "no stray suffixed names or temporary objects");
}

#[cfg(windows)]
#[test]
fn native_directory_rename_terminates_each_utf16_length_class_without_stray_names() {
    use std::os::windows::ffi::OsStrExt;
    let area = Area::new(); let root = std::fs::canonicalize(&area.0).unwrap();
    for remainder in 0..4 {
        let source = root.join(format!("source-{remainder}")); std::fs::create_dir(&source).unwrap();
        let mut name = format!("folder-🦀-{remainder}");
        while root.join(&name).as_os_str().encode_wide().count() % 4 != remainder { name.push('x'); }
        let destination = root.join(name); let held = MoveCohort::hold(&source, &|| false).unwrap();
        assert!(rename_held_no_replace(&held.objects[0].1, &destination).unwrap());
        assert!(destination.is_dir() && !source.exists()); drop(held);
    }
    assert_eq!(std::fs::read_dir(root).unwrap().count(), 4);
}

#[test]
fn a_verifier_sees_only_private_copy_and_cannot_publish_changed_bytes() {
    let area = Area::new(); let source = area.0.join("source"); let destination = area.0.join("destination");
    std::fs::write(&source, b"original").unwrap();
    let verified = std::cell::Cell::new(false);
    let verify = |copy: &Path| -> Result<()> {
        assert_ne!(copy, source.as_path(), "verifier must never receive the owner original");
        assert_eq!(std::fs::read(copy).unwrap(), b"original");
        std::fs::write(copy, b"newbytes")?; verified.set(true); Ok(())
    };
    assert!(move_controlled_checked(&source, &destination, &|| false, true, Some(&verify), true, None).is_err());
    assert!(verified.get()); assert_eq!(std::fs::read(source).unwrap(), b"original"); assert!(!destination.exists());
    assert_eq!(std::fs::read_dir(&area.0).unwrap().count(), 1, "only private temporary copy is cleaned up");
}

#[test]
fn a_forced_directory_copy_verifier_has_no_access_to_the_original_path() {
    let area = Area::new(); let source = area.0.join("source"); let destination = area.0.join("destination");
    std::fs::create_dir_all(source.join("empty")).unwrap(); std::fs::write(source.join("file"), b"original").unwrap();
    let verify = |copy: &Path| -> Result<()> {
        assert_ne!(copy, source.as_path()); assert!(copy.join("empty").is_dir());
        std::fs::write(copy.join("file"), b"newbytes")?; Ok(())
    };
    assert!(move_controlled_checked(&source, &destination, &|| false, true, Some(&verify), true, None).is_err());
    assert_eq!(std::fs::read(source.join("file")).unwrap(), b"original"); assert!(source.join("empty").is_dir()); assert!(!destination.exists());
}

#[cfg(windows)]
#[test]
fn held_move_blocks_source_replacement_and_same_length_writer() {
    let area = Area::new(); let source = area.0.join("source"); let destination = area.0.join("destination");
    std::fs::write(&source, b"original").unwrap();
    let verify = |copy: &Path| -> Result<()> {
        assert!(std::fs::rename(&source, area.0.join("replaced")).is_err(), "held original cannot be replaced");
        assert!(std::fs::write(&source, b"newbytes").is_err(), "same-length write must be denied");
        assert_eq!(std::fs::read(copy).unwrap(), b"original"); Ok(())
    };
    move_controlled_checked(&source, &destination, &|| false, true, Some(&verify), true, None).unwrap();
    assert!(!source.exists()); assert_eq!(std::fs::read(destination).unwrap(), b"original");
}

#[cfg(windows)]
#[test]
fn an_existing_native_writer_refuses_before_copy_or_deletion() {
    let area = Area::new(); let source = area.0.join("source"); let destination = area.0.join("destination");
    std::fs::write(&source, b"original").unwrap();
    let writer = std::fs::OpenOptions::new().write(true).open(&source).unwrap();
    assert!(move_without_overwrite_controlled(&source, &destination, &|| false).is_err());
    assert!(!destination.exists()); drop(writer); assert_eq!(std::fs::read(source).unwrap(), b"original");
}

#[cfg(windows)]
#[test]
fn a_late_directory_child_retains_all_originals_and_pending_receipt_after_restart() {
    let area = Area::new(); let source = area.0.join("source"); let destination = area.0.join("held");
    std::fs::create_dir(&source).unwrap(); std::fs::write(source.join("old"), b"original").unwrap();
    let trash = Trash::new(TrashConfig { dir: area.0.join("trash").display().to_string(), keep_days: 0 });
    let receipt = Discarded { id: 1, original: source.display().to_string(), held: destination.display().to_string(), why: "synthetic pending move".into(), at: 1, pending: true, returning: false };
    trash.write_ledger(&[receipt.clone()]).unwrap();
    let verify = |_copy: &Path| -> Result<()> {
        assert!(std::fs::rename(&source, area.0.join("replacement")).is_err());
        assert!(std::fs::write(source.join("old"), b"newbytes").is_err());
        Ok(())
    };
    let stopped = || {
        if destination.exists() && !source.join("new-owner").exists() { std::fs::write(source.join("new-owner"), b"newer owner bytes").unwrap(); }
        false
    };
    assert!(move_controlled_checked(&source, &destination, &stopped, true, Some(&verify), true, None).is_err());
    assert_eq!(std::fs::read(source.join("old")).unwrap(), b"original");
    assert_eq!(std::fs::read(source.join("new-owner")).unwrap(), b"newer owner bytes");
    let reopened = Trash::new(trash.cfg.clone()); assert_eq!(reopened.ledger()[0].id, receipt.id);
    assert!(reopened.undo(receipt.id).is_err(), "both locations must be inspected, never blindly overwritten");
    assert_eq!(reopened.expire(u64::MAX), 0); assert!(source.join("new-owner").exists());
}

#[cfg(windows)]
#[test]
fn partial_directory_cleanup_preserves_both_locations_and_durable_pending_identity() {
    let area = Area::new(); let source = area.0.join("source"); let destination = area.0.join("held");
    std::fs::create_dir(&source).unwrap(); std::fs::write(source.join("a"), b"first").unwrap(); std::fs::write(source.join("b"), b"second").unwrap();
    let trash = Trash::new(TrashConfig { dir: area.0.join("trash").display().to_string(), keep_days: 0 });
    let receipt = Discarded { id: 1, original: source.display().to_string(), held: destination.display().to_string(), why: "synthetic pending move".into(), at: 1, pending: true, returning: false };
    trash.write_ledger(&[receipt]).unwrap();
    let stopped = || destination.exists() && std::fs::read_dir(&source).unwrap().count() < 2;
    assert!(move_without_overwrite_controlled(&source, &destination, &stopped).is_err());
    assert_eq!(std::fs::read(destination.join("a")).unwrap(), b"first"); assert_eq!(std::fs::read(destination.join("b")).unwrap(), b"second");
    assert_eq!(std::fs::read_dir(&source).unwrap().count(), 1, "exactly one held original was disposed before stop");
    let reopened = Trash::new(trash.cfg.clone()); assert!(reopened.ledger()[0].pending); assert!(reopened.undo(1).is_err());
    assert_eq!(reopened.expire(u64::MAX), 0); assert!(destination.join("a").exists() && destination.join("b").exists());
}

#[test]
fn trash_expiry_defers_real_lock_contention_but_reports_invalid_lock_storage() {
    let area = Area::new(); let trash = Trash::new(TrashConfig { dir: area.0.join("trash").display().to_string(), keep_days: 0 });
    let source = area.0.join("source"); std::fs::write(&source, b"retained").unwrap();
    #[cfg(windows)] { trash.take(&source, "synthetic").unwrap(); }
    let guard = trash.lock().unwrap(); assert_eq!(trash.expire(u64::MAX), 0); drop(guard);
    let _ = crate::unheard::take();
    std::fs::remove_file(Path::new(&trash.cfg.dir).join("ledger.lock")).unwrap();
    std::fs::create_dir(Path::new(&trash.cfg.dir).join("ledger.lock")).unwrap();
    assert_eq!(trash.expire(u64::MAX), 0);
    let failures = crate::unheard::take();
    assert!(failures.iter().any(|entry| entry.error.contains("Trash cleanup couldn't lock") && entry.error.contains(&trash.cfg.dir) && entry.error.contains("No files expired") && entry.error.contains("ledger.lock")), "genuine storage failure must identify the failed operation, exact recovery folder, preserved files and repair step: {failures:?}");
    #[cfg(windows)] { assert_eq!(std::fs::read(&trash.ledger()[0].held).unwrap(), b"retained"); }
    std::fs::remove_dir(Path::new(&trash.cfg.dir).join("ledger.lock")).unwrap();
    #[cfg(windows)] { assert_eq!(trash.expire(u64::MAX), 1, "repair allows actual expiry"); assert!(trash.ledger().is_empty()); }
}

#[cfg(windows)]
#[test]
fn trash_expiry_failed_record_save_reports_failure_without_deleting_held_bytes() {
    let area = Area::new(); let trash = Trash::new(TrashConfig { dir: area.0.join("trash").display().to_string(), keep_days: 0 });
    let source = area.0.join("source"); std::fs::write(&source, b"retained").unwrap();
    let receipt = trash.take(&source, "synthetic").unwrap();
    std::fs::create_dir(Path::new(&trash.cfg.dir).join("ledger.json.writing")).unwrap();
    let _ = crate::unheard::take(); assert_eq!(trash.expire(u64::MAX), 0);
    assert_eq!(std::fs::read(&receipt.held).unwrap(), b"retained"); assert_eq!(trash.ledger()[0].id, receipt.id);
    assert!(!crate::unheard::take().is_empty(), "failed durable save must be heard before any deletion");
}

#[test]
fn cross_volume_copy_stop_removes_only_private_partial_bytes() {
    let area = Area::new();
    let source = area.0.join("original.bin");
    let destination = area.0.join("moved.bin");
    let bytes = vec![7u8; 256 * 1024];
    std::fs::write(&source, &bytes).unwrap();
    let checks = std::cell::Cell::new(0);
    let stopped = || { checks.set(checks.get() + 1); checks.get() >= 5 };
    let error = move_controlled(&source, &destination, &stopped, true).unwrap_err();
    assert!(error.to_string().contains("stopped"));
    assert_eq!(std::fs::read(&source).unwrap(), bytes);
    assert!(!destination.exists());
    assert_eq!(std::fs::read_dir(&area.0).unwrap().count(), 1);
    move_controlled(&source, &destination, &|| false, true).unwrap();
    assert!(!source.exists());
    assert_eq!(std::fs::read(&destination).unwrap(), bytes);
}

#[test]
fn a_destination_created_during_copy_is_never_replaced() {
    let area = Area::new();
    let source = area.0.join("original.bin");
    let destination = area.0.join("moved.bin");
    std::fs::write(&source, vec![8u8; 128 * 1024]).unwrap();
    let checks = std::cell::Cell::new(0);
    let stopped = || {
        checks.set(checks.get() + 1);
        if checks.get() == 4 { std::fs::write(&destination, b"other owner bytes").unwrap(); }
        false
    };
    assert!(move_controlled(&source, &destination, &stopped, true).is_err());
    assert_eq!(std::fs::read(&destination).unwrap(), b"other owner bytes");
    assert_eq!(std::fs::read(&source).unwrap(), vec![8u8; 128 * 1024]);
    assert_eq!(std::fs::read_dir(&area.0).unwrap().count(), 2);
}

#[cfg(all(unix, any(target_os = "linux", target_os = "android", target_os = "macos", target_os = "ios")))]
#[test]
fn unix_atomic_move_preserves_same_volume_files_directories_and_collision_bytes() {
    use std::io::{Seek, SeekFrom, Write};
    let area = Area::new(); let source = area.0.join("source"); let destination = area.0.join("destination");
    std::fs::write(&source, b"original").unwrap();
    let mut writer = std::fs::OpenOptions::new().write(true).open(&source).unwrap();
    move_without_overwrite_controlled(&source, &destination, &|| false).unwrap();
    writer.seek(SeekFrom::Start(0)).unwrap(); writer.write_all(b"newbytes").unwrap(); writer.sync_all().unwrap();
    assert!(!source.exists()); assert_eq!(std::fs::read(&destination).unwrap(), b"newbytes", "native rename preserves concurrent writer's actual object");
    std::fs::write(&source, b"retained").unwrap();
    assert!(move_without_overwrite_controlled(&source, &destination, &|| false).is_err());
    assert_eq!(std::fs::read(&source).unwrap(), b"retained"); assert_eq!(std::fs::read(&destination).unwrap(), b"newbytes");
    let folder = area.0.join("folder"); let moved = area.0.join("moved-folder");
    std::fs::create_dir_all(folder.join("empty")).unwrap(); std::fs::write(folder.join("file"), b"whole").unwrap();
    move_without_overwrite_controlled(&folder, &moved, &|| false).unwrap();
    assert!(!folder.exists()); assert!(moved.join("empty").is_dir()); assert_eq!(std::fs::read(moved.join("file")).unwrap(), b"whole");
}

#[cfg(all(unix, any(target_os = "linux", target_os = "android", target_os = "macos", target_os = "ios")))]
#[test]
fn unix_atomic_move_observes_stop_and_late_destination_collision() {
    let area = Area::new(); let source = area.0.join("source"); let destination = area.0.join("destination");
    std::fs::write(&source, b"retained").unwrap();
    assert!(move_without_overwrite_controlled(&source, &destination, &|| true).is_err()); assert!(!destination.exists());
    let mut before = || -> Result<()> { std::fs::write(&destination, b"other owner")?; Ok(()) };
    assert!(move_without_overwrite_recorded(&source, &destination, &|| false, &mut before).is_err());
    assert_eq!(std::fs::read(&source).unwrap(), b"retained"); assert_eq!(std::fs::read(destination).unwrap(), b"other owner");
}

#[test]
fn archived_ledger_without_held_bytes_refuses_before_changing_state() {
    let area = Area::new();
    let state = area.0.join("data/state");
    let store = crate::store::Store::new(state.clone());
    store.save("test", &"saved").unwrap();
    let trash = Trash::new(TrashConfig { dir: store.trash_dir().display().to_string(), keep_days: 30 });
    let source = area.0.join("original.txt");
    std::fs::write(&source, b"owner bytes").unwrap();
    let discarded = trash.take(&source, "fixture").unwrap();
    let cfg = BackupConfig { dir: area.0.join("backups").display().to_string(), ..BackupConfig::default() };
    let backup = back_up(&state, &cfg, 50).unwrap();
    // Legacy snapshots have no manifest, so ledger validation itself must
    // reject the missing bytes instead of inventing a recovery location.
    std::fs::remove_file(backup.path.join(BACKUP_MANIFEST)).unwrap();
    std::fs::remove_file(backup.path.join(OWNED_OUTPUTS).join("trash").join(Path::new(&discarded.held).file_name().unwrap())).unwrap();
    store.save("test", &"current").unwrap();
    let error = restore_with_notes(&backup.path, &state, &trash, &crate::household::Household::default(), &state.parent().unwrap().join("notes")).unwrap_err();
    assert!(error.to_string().contains("missing or unreadable"));
    assert_eq!(store.load::<String>("test"), "current");
    assert_eq!(std::fs::read(discarded.held).unwrap(), b"owner bytes");
}

#[test]
fn empty_held_directories_restore_and_return_without_changing_live_trash() {
    let area = Area::new(); let state = area.0.join("data/state");
    let store = crate::store::Store::new(state.clone()); store.save("test", &"saved").unwrap();
    let trash = Trash::new(TrashConfig { dir: store.trash_dir().display().to_string(), keep_days: 30 });
    let folder = area.0.join("owner-empty-folder"); std::fs::create_dir(&folder).unwrap();
    std::fs::create_dir(folder.join("nested-empty")).unwrap();
    let discarded = trash.take(&folder, "empty directory fixture").unwrap();
    let cfg = BackupConfig { dir: area.0.join("backups").display().to_string(), ..BackupConfig::default() };
    let backup = back_up(&state, &cfg, 51).unwrap();
    store.save("test", &"current").unwrap();
    restore_with_notes(&backup.path, &state, &trash, &crate::household::Household::default(), &state.parent().unwrap().join("notes")).unwrap();
    assert_eq!(store.load::<String>("test"), "saved");
    assert!(Path::new(&discarded.held).is_dir());
    assert!(backup.path.join(OWNED_OUTPUTS).join("trash").join(Path::new(&discarded.held).file_name().unwrap()).is_dir());
    assert!(!state.join(".restore-journal.json").exists());
    let restored = archived_recovery(&state).unwrap(); assert_eq!(restored.len(), 1);
    return_archived_file(&state, &restored[0].archive, &restored[0].kind, restored[0].record.id).unwrap();
    assert!(folder.join("nested-empty").is_dir());
    assert_eq!(std::fs::read_dir(folder.join("nested-empty")).unwrap().count(), 0);
    assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1, "ownership marker escaped into the owner folder");
    assert!(Path::new(&discarded.held).is_dir());
    assert!(trash.ledger().iter().any(|record| record.id == discarded.id));
}

fn directory_snapshot(area: &Area) -> (PathBuf, Backup, Trash) {
    let state = area.0.join("data/state"); let store = crate::store::Store::new(&state);
    store.save("item", &"saved").unwrap(); std::fs::create_dir(state.join("owner-empty")).unwrap();
    let cfg = BackupConfig { dir: area.0.join("backups").display().to_string(), ..BackupConfig::default() };
    let backup = back_up(&state, &cfg, 52).unwrap();
    std::fs::remove_dir(state.join("owner-empty")).unwrap(); store.save("item", &"current").unwrap();
    let trash = Trash::new(TrashConfig { dir: store.trash_dir().display().to_string(), keep_days: 30 });
    (state, backup, trash)
}

#[test]
fn failed_restore_rolls_back_only_its_new_empty_directory() {
    let area = Area::new(); let (state, backup, trash) = directory_snapshot(&area);
    let result = super::restore_transaction::restore_with_hook(&backup.path, &state, &trash, &Default::default(), &mut |operation| {
        if operation == 1 { assert!(state.join("owner-empty").is_dir()); Err(AtlasError::Platform("fixture failure after directory publication".into())) } else { Ok(()) }
    });
    assert!(result.unwrap_err().to_string().contains("every original file was put back"));
    assert!(!state.join("owner-empty").exists());
    assert_eq!(crate::store::Store::new(&state).load::<String>("item"), "current");
    assert!(!state.join(".restore-journal.json").exists());
}

#[test]
fn restart_rolls_back_a_published_empty_directory_with_ownership_intact() {
    let area = Area::new(); let (state, backup, trash) = directory_snapshot(&area);
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| super::restore_transaction::restore_with_hook(&backup.path, &state, &trash, &Default::default(), &mut |operation| { if operation == 1 { panic!("fixture process interruption after directory publication"); } Ok(()) }))).is_err());
    assert!(state.join("owner-empty").is_dir());
    recover_restore(&state).unwrap();
    assert!(!state.join("owner-empty").exists());
    assert_eq!(crate::store::Store::new(&state).load::<String>("item"), "current");
    assert!(!state.join(".restore-journal.json").exists());
}

#[test]
fn later_owner_files_stop_directory_rollback_without_deleting_them() {
    let area = Area::new(); let (state, backup, trash) = directory_snapshot(&area);
    let foreign = state.join("owner-empty/owner-added.txt");
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| super::restore_transaction::restore_with_hook(&backup.path, &state, &trash, &Default::default(), &mut |operation| { if operation == 1 { std::fs::write(&foreign, b"later owner bytes").unwrap(); panic!("fixture interruption with later owner content"); } Ok(()) }))).is_err());
    assert!(recover_restore(&state).unwrap_err().to_string().contains("later files"));
    assert_eq!(std::fs::read(&foreign).unwrap(), b"later owner bytes");
    assert!(state.join(".restore-journal.json").is_file());
    std::fs::remove_file(foreign).unwrap(); recover_restore(&state).unwrap();
    assert!(!state.join("owner-empty").exists());
}

#[test]
fn a_foreign_directory_created_before_publication_is_never_clobbered() {
    let area = Area::new(); let (state, backup, trash) = directory_snapshot(&area);
    let foreign = state.join("owner-empty/foreign.txt");
    let result = super::restore_transaction::restore_with_hook(&backup.path, &state, &trash, &Default::default(), &mut |operation| { if operation == 0 { std::fs::create_dir(state.join("owner-empty")).unwrap(); std::fs::write(&foreign, b"foreign directory bytes").unwrap(); } Ok(()) });
    assert!(result.is_err()); assert_eq!(std::fs::read(&foreign).unwrap(), b"foreign directory bytes");
    assert_eq!(crate::store::Store::new(&state).load::<String>("item"), "current");
    // Only the fixture's owner removes its new foreign directory; recovery
    // then sees publication never happened and cleans private staging.
    std::fs::remove_file(foreign).unwrap(); std::fs::remove_dir(state.join("owner-empty")).unwrap(); recover_restore(&state).unwrap();
}

#[test]
fn generated_outputs_restore_but_live_recovery_identity_is_preserved() {
    let area = Area::new();
    let state = area.0.join("data/state");
    let store = crate::store::Store::new(state.clone());
    store.save("test", &"saved").unwrap();
    for folder in OWNED_FOLDERS { std::fs::create_dir_all(store.data_dir().join(folder)).unwrap(); }
    for folder in ["notes", "calls", "reading", "recovery"] { std::fs::write(store.data_dir().join(folder).join("owner.txt"), format!("saved {folder}")).unwrap(); }
    let trash = Trash::new(TrashConfig { dir: store.trash_dir().display().to_string(), keep_days: 30 });
    let original = area.0.join("original.txt"); std::fs::write(&original, b"held owner bytes").unwrap();
    trash.take(&original, "fixture discarded original").unwrap();
    let cfg = BackupConfig { dir: area.0.join("backups").display().to_string(), ..BackupConfig::default() };
    let backup = back_up(&state, &cfg, 30).unwrap();
    for folder in ["notes", "calls", "reading", "recovery"] { std::fs::write(store.data_dir().join(folder).join("owner.txt"), format!("current {folder}")).unwrap(); }
    let live_ledger = std::fs::read(store.trash_dir().join("ledger.json")).unwrap();
    restore_with_notes(&backup.path, &state, &trash, &crate::household::Household::default(), &state.parent().unwrap().join("notes")).unwrap();
    for folder in ["notes", "calls", "reading"] { assert_eq!(std::fs::read_to_string(store.data_dir().join(folder).join("owner.txt")).unwrap(), format!("saved {folder}")); }
    assert_eq!(std::fs::read_to_string(store.data_dir().join("recovery/owner.txt")).unwrap(), "current recovery");
    // The restore's displaced generated files add records, but the original
    // live held record and its bytes remain accessible under their old ID.
    let old: Vec<Discarded> = serde_json::from_slice(&live_ledger).unwrap();
    assert!(trash.ledger().iter().any(|record| record.id == old[0].id && record.held == old[0].held));
    let archive = std::fs::read_dir(&state).unwrap().map(|entry| entry.unwrap().path()).find(|path| path.file_name().unwrap().to_string_lossy().starts_with(".restored-recovery-")).unwrap();
    assert_eq!(std::fs::read_to_string(archive.join("recovery/owner.txt")).unwrap(), "saved recovery");
    let archived: Vec<Discarded> = serde_json::from_slice(&std::fs::read(archive.join("trash/ledger.json")).unwrap()).unwrap();
    assert_eq!(std::fs::read(&archived[0].held).unwrap(), b"held owner bytes");
    let archived_list = archived_recovery(&state).unwrap();
    assert_eq!(archived_list.len(), 1);
    let selected = &archived_list[0];
    std::fs::write(&original, b"new original owner bytes").unwrap();
    assert!(return_archived_file(&state, &selected.archive, &selected.kind, selected.record.id).is_err());
    assert_eq!(std::fs::read(&original).unwrap(), b"new original owner bytes");
    assert_eq!(std::fs::read(&selected.record.held).unwrap(), b"held owner bytes");
    std::fs::remove_file(&original).unwrap();
    return_archived_file(&state, &selected.archive, &selected.kind, selected.record.id).unwrap();
    assert_eq!(std::fs::read(&original).unwrap(), b"held owner bytes");
    assert!(archived_recovery(&state).unwrap().is_empty());
    assert!(trash.ledger().iter().any(|record| record.id == old[0].id));
}

#[test]
fn active_recording_rejects_backup_until_the_output_is_closed() {
    let area = Area::new(); let state = area.0.join("data/state");
    let store = crate::store::Store::new(state.clone()); store.save("test", &1).unwrap();
    let calls = store.data_dir().join("calls");
    let mut writer = crate::callrec::WavOut::create(&calls.join("call.wav")).unwrap();
    writer.write(&[7, 8, 9]).unwrap();
    let cfg = BackupConfig { dir: area.0.join("backups").display().to_string(), ..BackupConfig::default() };
    assert!(back_up(&state, &cfg, 40).unwrap_err().to_string().contains("still being recorded"));
    assert!(!PathBuf::from(&cfg.dir).join("state-40").exists());
    assert_eq!(writer.close().unwrap(), 3);
    let backup = back_up(&state, &cfg, 40).unwrap();
    let bytes = std::fs::read(backup.path.join(OWNED_OUTPUTS).join("calls/call.wav")).unwrap();
    assert_eq!(bytes.len(), 50);
    assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 6);
    assert_eq!(&bytes[44..], &[7, 0, 8, 0, 9, 0]);
    let trash = Trash::new(TrashConfig { dir: store.trash_dir().display().to_string(), keep_days: 30 });
    let mut active = crate::callrec::WavOut::create(&calls.join("active.wav")).unwrap(); active.write(&[42]).unwrap();
    let before = std::fs::read(calls.join("call.wav")).unwrap();
    assert!(restore_with_notes(&backup.path, &state, &trash, &crate::household::Household::default(), &state.parent().unwrap().join("notes")).unwrap_err().to_string().contains("still being recorded"));
    assert_eq!(std::fs::read(calls.join("call.wav")).unwrap(), before);
    active.close().unwrap();
}

struct Area(PathBuf);
impl Area {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!("atlas-state-transaction-{}-{}", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Area { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }

#[test]
fn failed_restore_rolls_back_every_file_before_returning() {
    let area = Area::new();
    let state = area.0.join("state");
    let backup = area.0.join("backup");
    std::fs::create_dir(&state).unwrap();
    std::fs::create_dir_all(backup.join("z-blocked")).unwrap();
    std::fs::write(state.join("a.txt"), b"original bytes").unwrap();
    std::fs::write(state.join("z-blocked"), b"occupying file").unwrap();
    std::fs::write(backup.join("a.txt"), b"replacement bytes").unwrap();
    std::fs::write(backup.join("z-blocked/b.txt"), b"new nested file").unwrap();
    let trash = Trash::new(TrashConfig { dir: area.0.join("trash").display().to_string(), keep_days: 30 });
    assert!(restore_with_notes(&backup, &state, &trash, &crate::household::Household::default(), &state.parent().unwrap().join("notes")).is_err());
    assert_eq!(std::fs::read(state.join("a.txt")).unwrap(), b"original bytes", "a failed restore must return the whole original state");
    assert_eq!(std::fs::read(state.join("z-blocked")).unwrap(), b"occupying file");
}

#[test]
fn a_second_backup_at_the_same_timestamp_preserves_the_first() {
    let area = Area::new();
    let state = area.0.join("state");
    std::fs::create_dir(&state).unwrap();
    std::fs::write(state.join("item.json"), b"first version").unwrap();
    let config = BackupConfig { dir: area.0.join("backups").display().to_string(), ..BackupConfig::default() };
    let first = back_up(&state, &config, 10).unwrap();
    std::fs::write(state.join("item.json"), b"second version").unwrap();
    assert!(back_up(&state, &config, 10).is_err(), "a dated backup is immutable");
    assert_eq!(std::fs::read(first.path.join("item.json")).unwrap(), b"first version");
}

#[test]
fn a_failed_swap_after_the_first_file_rolls_back_real_files() {
    let area = Area::new();
    let state = area.0.join("state"); let backup = area.0.join("backup");
    std::fs::create_dir(&state).unwrap(); std::fs::create_dir(&backup).unwrap();
    for name in ["a", "b"] { std::fs::write(state.join(name), format!("old {name}")).unwrap(); std::fs::write(backup.join(name), format!("new {name}")).unwrap(); }
    let trash = Trash::new(TrashConfig { dir: area.0.join("trash").display().to_string(), keep_days: 30 });
    let result = super::restore_transaction::restore_with_hook(&backup, &state, &trash, &crate::household::Household::default(), &mut |count| {
        if count == 1 { Err(AtlasError::Platform("injected failed second swap".into())) } else { Ok(()) }
    });
    assert!(result.unwrap_err().to_string().contains("every original file was put back"));
    for name in ["a", "b"] { assert_eq!(std::fs::read_to_string(state.join(name)).unwrap(), format!("old {name}")); }
    assert!(!state.join(".restore-journal.json").exists());
}

#[test]
fn restart_recovers_an_interrupted_restore_before_reading_state() {
    let area = Area::new(); let state = area.0.join("state"); let backup = area.0.join("backup");
    std::fs::create_dir(&state).unwrap(); std::fs::create_dir(&backup).unwrap();
    for name in ["a", "b"] { std::fs::write(state.join(name), format!("old {name}")).unwrap(); std::fs::write(backup.join(name), format!("new {name}")).unwrap(); }
    let trash = Trash::new(TrashConfig { dir: area.0.join("trash").display().to_string(), keep_days: 30 });
    let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        super::restore_transaction::restore_with_hook(&backup, &state, &trash, &crate::household::Household::default(), &mut |count| { if count == 1 { panic!("fixture process interruption") } Ok(()) })
    }));
    assert!(interrupted.is_err());
    assert_eq!(std::fs::read_to_string(state.join("a")).unwrap(), "new a");
    assert!(state.join(".restore-journal.json").exists());
    let _lease = crate::store::begin_state_command(&state, false).unwrap();
    for name in ["a", "b"] { assert_eq!(std::fs::read_to_string(state.join(name)).unwrap(), format!("old {name}")); }
    assert!(!state.join(".restore-journal.json").exists());
}

#[test]
fn state_lock_child_process_fixture() {
    let Some(root) = std::env::var_os("ATLAS_TEST_STATE_ROOT") else { return };
    let root = PathBuf::from(root);
    let mode = std::env::var("ATLAS_TEST_STATE_MODE").unwrap();
    if mode == "save-blocked" { assert!(crate::store::Store::new(root).save("item", &"child").is_err()); }
    else if mode == "save-free" { crate::store::Store::new(root).save("item", &"child").unwrap(); }
    else if mode == "read-blocked" { assert!(crate::store::begin_state_command(&root, false).is_err()); }
    else if mode == "daemon-blocked" { assert!(crate::onlyone::OnlyOne::at(&root).take(99_999).unwrap_err().contains("already running")); }
    else if mode == "daemon-free" { crate::onlyone::OnlyOne::at(&root).take(99_999).unwrap(); }
    else if mode == "output-blocked" { assert!(crate::store::write_owned_file(&root.join("probe.txt"), b"must not appear").is_err()); }
    else { panic!("unknown fixture mode") }
}

#[test]
fn configured_external_outputs_share_the_lock_and_restore_to_current_roots() {
    let area = Area::new(); let store = crate::store::Store::new(area.0.join("data/state"));
    store.save("test", &1).unwrap();
    let notes = area.0.join("configured-notes"); let trash_path = area.0.join("configured-trash");
    check_configured_scope(&store, &notes, &trash_path).unwrap();
    crate::store::write_owned_file(&notes.join("note.md"), b"saved configured note").unwrap();
    let trash = Trash::new(TrashConfig { dir: trash_path.display().to_string(), keep_days: 30 });
    let original = area.0.join("original.txt"); std::fs::write(&original, b"configured held bytes").unwrap(); trash.take(&original, "fixture").unwrap();
    { let _guard = store.transaction().unwrap(); child(&notes, "output-blocked"); }
    assert!(!notes.join("probe.txt").exists());
    let cfg = BackupConfig { dir: area.0.join("backups").display().to_string(), ..BackupConfig::default() };
    let backup = back_up_with_inputs(store.root(), &cfg, 50, &notes, &trash_path).unwrap();
    let mut manifest: BackupManifest = serde_json::from_slice(&std::fs::read(backup.path.join(BACKUP_MANIFEST)).unwrap()).unwrap();
    assert!(manifest.roots.iter().any(|(kind, path)| kind == "configured-notes" && Path::new(path) == notes));
    // Source root metadata is descriptive, never a restore destination.
    for (_, source) in &mut manifest.roots { *source = area.0.join("must-not-use-source-root").display().to_string(); }
    crate::store::write_json(&backup.path.join(BACKUP_MANIFEST), &manifest).unwrap();
    crate::store::write_owned_file(&notes.join("note.md"), b"current note").unwrap();
    restore_with_notes(&backup.path, store.root(), &trash, &crate::household::Household::default(), &notes).unwrap();
    assert_eq!(std::fs::read(notes.join("note.md")).unwrap(), b"saved configured note");
    assert!(!area.0.join("must-not-use-source-root").exists());
    assert!(trash.ledger().iter().any(|record| std::fs::read(&record.held).is_ok_and(|bytes| bytes == b"configured held bytes")));
}

#[test]
fn a_configured_root_owned_by_another_store_is_refused() {
    let area = Area::new(); let first = crate::store::Store::new(area.0.join("first")); let second = crate::store::Store::new(area.0.join("second"));
    first.save("test", &1).unwrap(); second.save("test", &2).unwrap();
    let notes = area.0.join("external"); crate::store::bind_owned_output(first.root(), &notes).unwrap();
    crate::store::write_owned_file(&notes.join("note"), b"owner bytes").unwrap();
    assert!(crate::store::bind_owned_output(second.root(), &notes).is_err());
    assert_eq!(std::fs::read(notes.join("note")).unwrap(), b"owner bytes");
}

#[test]
fn overlapping_output_roots_refuse_an_ambiguous_backup() {
    let area = Area::new(); let store = crate::store::Store::new(area.0.join("data/state")); store.save("test", &1).unwrap();
    let cfg = BackupConfig { dir: area.0.join("backups").display().to_string(), ..BackupConfig::default() };
    let notes = store.notes_dir().join("nested");
    assert!(back_up_with_inputs(store.root(), &cfg, 60, &notes, &store.trash_dir()).unwrap_err().to_string().contains("overlap"));
    assert!(!PathBuf::from(&cfg.dir).join("state-60").exists());
}

#[test]
fn native_daemon_lease_blocks_a_separate_process_even_after_sleep() {
    let area = Area::new(); let singleton = crate::onlyone::OnlyOne::at(&area.0);
    singleton.take(1000).unwrap();
    std::fs::write(singleton.path(), "1000").unwrap();
    child(&area.0, "daemon-blocked");
    singleton.release();
    child(&area.0, "daemon-free"); // its native handle is released at process exit
    singleton.take(99_999).unwrap(); // dead child PID does not block restart
    singleton.release();
}

#[test]
fn a_transient_failed_save_is_cleared_after_its_pending_value_is_saved() {
    let area = Area::new(); let store = crate::store::Store::new(area.0.join("state"));
    store.save("pending", &"old").unwrap();
    let worker = store.clone(); let (ready, wait_ready) = std::sync::mpsc::channel(); let (release, wait_release) = std::sync::mpsc::channel();
    let holder = std::thread::spawn(move || { let _guard = worker.transaction().unwrap(); ready.send(()).unwrap(); wait_release.recv().unwrap(); });
    wait_ready.recv().unwrap();
    assert!(store.save("pending", &"new").is_err());
    let failure = crate::store::take_failed_saves_detailed(store.root());
    assert_eq!(failure.len(), 1); assert!(failure[0].2);
    assert!(store.save("pending", &"new").is_err());
    release.send(()).unwrap(); holder.join().unwrap();
    store.save("pending", &"new").unwrap();
    assert!(crate::store::take_failed_saves(store.root()).is_empty());
    assert_eq!(store.load::<String>("pending"), "new");
}

#[test]
fn an_output_worker_retries_a_snapshot_and_cancel_preserves_the_original() {
    let area = Area::new(); let state = area.0.join("data/state");
    let store = crate::store::Store::new(state); store.save("test", &1).unwrap();
    let path = store.notes_dir().join("worker.txt"); crate::store::write_owned_file(&path, b"original").unwrap();
    let guard = store.transaction().unwrap();
    let next = path.clone(); let (done, wait_done) = std::sync::mpsc::channel();
    let writer = std::thread::spawn(move || { let result = crate::store::write_owned_file_until(&next, b"new", &|| false); done.send(result.is_ok()).unwrap(); });
    assert!(wait_done.recv_timeout(std::time::Duration::from_millis(80)).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"original");
    drop(guard); assert!(wait_done.recv_timeout(std::time::Duration::from_secs(2)).unwrap()); writer.join().unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"new");
    let _guard = store.transaction().unwrap(); let next = path.clone();
    let stopped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)); let worker_stop = stopped.clone();
    let writer = std::thread::spawn(move || crate::store::write_owned_file_until(&next, b"must not appear", &|| worker_stop.load(std::sync::atomic::Ordering::Acquire)));
    std::thread::sleep(std::time::Duration::from_millis(60)); stopped.store(true, std::sync::atomic::Ordering::Release);
    assert_eq!(writer.join().unwrap().unwrap_err().kind(), std::io::ErrorKind::Interrupted);
    assert_eq!(std::fs::read(&path).unwrap(), b"new");
}

fn child(root: &Path, mode: &str) {
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "safety::state_transactions::state_lock_child_process_fixture", "--test-threads=1"])
        .env("ATLAS_TEST_STATE_ROOT", root).env("ATLAS_TEST_STATE_MODE", mode).output().unwrap();
    assert!(result.status.success(), "child fixture failed: {}", String::from_utf8_lossy(&result.stderr));
}

#[test]
fn an_os_state_lock_blocks_a_writer_in_another_process_and_releases() {
    let area = Area::new(); let store = crate::store::Store::new(area.0.join("state"));
    store.save("item", &"original").unwrap();
    let first = store.transaction().unwrap();
    let nested = store.transaction().unwrap();
    drop(first);
    child(store.root(), "save-blocked");
    assert_eq!(store.load::<String>("item"), "original");
    drop(nested);
    child(store.root(), "save-free");
    assert_eq!(store.load::<String>("item"), "child");
}

#[test]
fn restore_excludes_a_separate_process_before_owner_state_is_read() {
    let area = Area::new(); let state = area.0.join("state");
    let _restore = crate::store::begin_state_command(&state, true).unwrap();
    child(&state, "read-blocked");
}

#[test]
fn a_backup_waits_until_the_whole_persist_cohort_finishes() {
    let area = Area::new(); let state = area.0.join("state");
    let store = crate::store::Store::new(&state);
    store.save("a", &0u32).unwrap(); store.save("b", &0u32).unwrap();
    let (ready, wait_ready) = std::sync::mpsc::channel(); let (finish, wait_finish) = std::sync::mpsc::channel();
    let other = store.clone();
    let writer = std::thread::spawn(move || { let _guard = other.transaction().unwrap(); other.save("a", &1u32).unwrap(); ready.send(()).unwrap(); wait_finish.recv().unwrap(); other.save("b", &1u32).unwrap(); });
    wait_ready.recv().unwrap();
    let config = BackupConfig { dir: area.0.join("backups").display().to_string(), ..BackupConfig::default() };
    let snapshot_root = state.clone(); let (done, wait_done) = std::sync::mpsc::channel();
    let backup = std::thread::spawn(move || { let result = back_up(&snapshot_root, &config, 20); done.send(()).unwrap(); result });
    assert!(wait_done.recv_timeout(std::time::Duration::from_millis(100)).is_err());
    finish.send(()).unwrap(); writer.join().unwrap();
    let snapshot = backup.join().unwrap().unwrap(); let backed = crate::store::Store::new(snapshot.path);
    assert_eq!((backed.load::<u32>("a"), backed.load::<u32>("b")), (1, 1));
}
