//! Recovery must survive a blocked destination and a failed durable record.
use atlas::safety::{Trash, TrashConfig};
use atlas::tune::{undo_tune_change, TuneUndo};
use std::fs;

struct Area(std::path::PathBuf);
impl Area {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "atlas-recovery-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for Area {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_partial_undo_is_not_complete_and_can_be_retried() {
    let dir = Area::new();
    let a = dir.path().join("a");
    let b = dir.path().join("b");
    let moved_a = dir.path().join("moved-a");
    let moved_b = dir.path().join("moved-b");
    fs::write(&moved_a, "first").unwrap();
    fs::write(&moved_b, "second").unwrap();
    fs::write(&b, "new occupant").unwrap();
    let undo = TuneUndo::Moves(vec![
        (a.clone(), moved_a.clone()),
        (b.clone(), moved_b.clone()),
    ]);
    let mut saved = undo.clone();
    assert!(
        atlas::tune::undo_tune_change_with_checkpoint(&undo, &mut |next| {
            saved = next.clone();
            Ok(())
        })
        .is_err(),
        "a partial undo must remain pending"
    );
    assert_eq!(fs::read_to_string(&a).unwrap(), "first");
    assert_eq!(fs::read_to_string(&b).unwrap(), "new occupant");
    fs::remove_file(&b).unwrap();
    undo_tune_change(&saved).expect("retry only attempts files still pending");
    assert_eq!(fs::read_to_string(&b).unwrap(), "second");
}

#[test]
fn a_stranded_trash_destination_is_never_overwritten() {
    let dir = Area::new();
    let held = dir.path().join("trash");
    fs::create_dir(&held).unwrap();
    fs::write(held.join("1-file.txt"), "previous stranded file").unwrap();
    let original = dir.path().join("file.txt");
    fs::write(&original, "new file").unwrap();
    let trash = Trash::new(TrashConfig {
        dir: held.display().to_string(),
        keep_days: 30,
    });
    let taken = trash.take(&original, "test").unwrap();
    assert_eq!(
        fs::read_to_string(held.join("1-file.txt")).unwrap(),
        "previous stranded file"
    );
    assert_ne!(taken.held, held.join("1-file.txt").display().to_string());
    trash.undo(taken.id).unwrap();
    assert_eq!(fs::read_to_string(original).unwrap(), "new file");
}

#[test]
fn failure_to_write_the_trash_record_leaves_the_original_in_place() {
    let dir = Area::new();
    let held = dir.path().join("trash");
    fs::create_dir(&held).unwrap();
    // The existing writer cannot replace this directory with its temp file.
    fs::create_dir(held.join("ledger.json.writing")).unwrap();
    let original = dir.path().join("file.txt");
    fs::write(&original, "keep me").unwrap();
    let trash = Trash::new(TrashConfig {
        dir: held.display().to_string(),
        keep_days: 30,
    });
    assert!(trash.take(&original, "test").is_err());
    assert_eq!(fs::read_to_string(original).unwrap(), "keep me");
}

fn pending_entry(
    original: &std::path::Path,
    held: &std::path::Path,
    returning: bool,
) -> serde_json::Value {
    serde_json::json!({"id":1,"original":original,"held":held,"why":"test", "at":1,"pending":true,"returning":returning})
}

#[test]
fn an_interrupted_trash_move_recovers_before_and_after_the_move() {
    for moved in [false, true] {
        let dir = Area::new();
        let held_dir = dir.path().join("trash");
        fs::create_dir(&held_dir).unwrap();
        let original = dir.path().join("file.txt");
        let held = held_dir.join("1-file.txt");
        fs::write(if moved { &held } else { &original }, "keep me").unwrap();
        fs::write(
            held_dir.join("ledger.json"),
            serde_json::to_vec(&vec![pending_entry(&original, &held, false)]).unwrap(),
        )
        .unwrap();
        let trash = Trash::new(TrashConfig {
            dir: held_dir.display().to_string(),
            keep_days: 30,
        });
        assert_eq!(
            trash.expire(100 * 86_400),
            0,
            "an unresolved intent must never expire"
        );
        trash.undo(1).unwrap();
        assert_eq!(fs::read_to_string(&original).unwrap(), "keep me");
        assert!(trash.ledger().is_empty());
    }
}

#[test]
fn an_interrupted_undo_can_finish_its_record_without_moving_the_file_again() {
    let dir = Area::new();
    let held_dir = dir.path().join("trash");
    fs::create_dir(&held_dir).unwrap();
    let original = dir.path().join("file.txt");
    let held = held_dir.join("1-file.txt");
    fs::write(&original, "returned file").unwrap();
    fs::write(
        held_dir.join("ledger.json"),
        serde_json::to_vec(&vec![pending_entry(&original, &held, true)]).unwrap(),
    )
    .unwrap();
    let trash = Trash::new(TrashConfig {
        dir: held_dir.display().to_string(),
        keep_days: 30,
    });
    trash.undo(1).unwrap();
    assert_eq!(fs::read_to_string(original).unwrap(), "returned file");
    assert!(trash.ledger().is_empty());
}

#[test]
fn a_failed_sorting_record_prevents_the_filesystem_move() {
    let dir = Area::new();
    let original = dir.path().join("file.txt");
    fs::write(&original, "keep me").unwrap();
    let into = dir.path().join("Documents");
    let state = dir.path().join("state");
    fs::create_dir(&state).unwrap();
    fs::create_dir(state.join("tune_undo.json")).unwrap();
    let store = atlas::store::Store::new(state);
    let plan = atlas::organize::SortPlan {
        folders: vec![dir.path().to_path_buf()],
        moves: vec![atlas::organize::SortMove {
            from: original.clone(),
            into: into.clone(),
            reason: atlas::organize::Reason::OldInstaller,
        }],
        ..Default::default()
    };
    let sys = atlas::system::SystemConfig {
        enabled: true,
        file_roots: vec![dir.path().display().to_string()],
        ..Default::default()
    };
    // Older than the "being saved now" guard, without sleeping in the test.
    fs::File::options()
        .write(true)
        .open(&original)
        .unwrap()
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
        .unwrap();
    let done = atlas::organize::carry_out_moves_recorded(
        &plan,
        &sys,
        atlas::store::now(),
        &mut |from, to| {
            store
                .save(
                    atlas::tune::TUNE_UNDO_RECORD,
                    &vec![(
                        1,
                        TuneUndo::Moves(vec![(from.to_path_buf(), to.to_path_buf())]),
                    )],
                )
                .map_err(|e| e.to_string())
        },
    );
    assert_eq!(fs::read_to_string(original).unwrap(), "keep me");
    assert!(!into.join("file.txt").exists());
    assert!(done.moved.is_empty());
    assert_eq!(done.not.len(), 1);
}

#[test]
fn a_saved_move_intent_recovers_after_a_crash_at_either_side_of_the_move() {
    for moved in [false, true] {
        let dir = Area::new();
        let original = dir.path().join("file.txt");
        let held = dir.path().join("moved.txt");
        fs::write(&original, "keep me").unwrap();
        let intent = atlas::tune::RecordedMove::new(&original, &held).unwrap();
        if moved {
            fs::rename(&original, &held).unwrap();
        }
        let store = atlas::store::Store::new(dir.path().join("state"));
        store
            .save(
                atlas::tune::TUNE_UNDO_RECORD,
                &vec![(
                    1,
                    TuneUndo::RecordedMoves {
                        moves: vec![intent],
                        made: Vec::new(),
                    },
                )],
            )
            .unwrap();
        let record: Vec<(u64, TuneUndo)> =
            atlas::store::Store::new(store.root()).load(atlas::tune::TUNE_UNDO_RECORD);
        undo_tune_change(&record[0].1).unwrap();
        assert_eq!(fs::read_to_string(original).unwrap(), "keep me");
        assert!(!held.exists());
    }
}

#[test]
fn a_destination_created_after_planning_is_never_overwritten() {
    let dir = Area::new();
    let original = dir.path().join("file.txt");
    fs::write(&original, "source").unwrap();
    let dest = dir.path().join("dest");
    let (moved, errors) =
        atlas::tune::move_files_into_recorded(&[original.clone()], &dest, &mut |_, to| {
            fs::write(to, "new occupant").unwrap();
            Ok(())
        });
    assert!(moved.is_empty());
    assert_eq!(errors.len(), 1);
    assert_eq!(fs::read_to_string(&original).unwrap(), "source");
    assert_eq!(
        fs::read_to_string(dest.join("file.txt")).unwrap(),
        "new occupant"
    );
}

#[test]
fn an_unrelated_original_is_not_mistaken_for_a_completed_move() {
    let dir = Area::new();
    let original = dir.path().join("file.txt");
    let held = dir.path().join("moved.txt");
    fs::write(&original, "the actual file").unwrap();
    let intent = atlas::tune::RecordedMove::new(&original, &held).unwrap();
    fs::write(&original, "an unrelated replacement").unwrap();
    let undo = TuneUndo::RecordedMoves {
        moves: vec![intent],
        made: Vec::new(),
    };
    let error = undo_tune_change(&undo).unwrap_err();
    assert!(error.contains("can't confirm"), "{error}");
    assert_eq!(
        fs::read_to_string(original).unwrap(),
        "an unrelated replacement"
    );
}

#[test]
fn an_edited_returned_file_is_recognized_after_a_failed_completion_save() {
    let dir = Area::new();
    let original = dir.path().join("file.txt");
    let held = dir.path().join("moved.txt");
    fs::write(&original, "initial file").unwrap();
    let intent = atlas::tune::RecordedMove::new(&original, &held).unwrap();
    fs::rename(&original, &held).unwrap();
    fs::write(&held, "edited file").unwrap();
    let undo = TuneUndo::RecordedMoves {
        moves: vec![intent],
        made: Vec::new(),
    };
    let mut saved = undo.clone();
    let mut checkpoints = 0;
    let result = atlas::tune::undo_tune_change_with_checkpoint(&undo, &mut |next| {
        checkpoints += 1;
        if checkpoints == 3 {
            return Err("injected failed completion save".into());
        }
        saved = next.clone();
        Ok(())
    });
    assert!(result.unwrap_err().contains("retry is safe"));
    assert_eq!(fs::read_to_string(&original).unwrap(), "edited file");
    undo_tune_change(&saved).unwrap();
    assert_eq!(fs::read_to_string(original).unwrap(), "edited file");
}

#[test]
fn a_restore_parent_failure_reports_every_file_already_replaced() {
    let dir = Area::new();
    let backup = dir.path().join("backup");
    let state = dir.path().join("state");
    fs::create_dir_all(backup.join("z-blocked")).unwrap();
    fs::create_dir(&state).unwrap();
    fs::write(backup.join("a.txt"), "new version").unwrap();
    fs::write(backup.join("z-blocked/b.txt"), "nested version").unwrap();
    fs::write(state.join("a.txt"), "old version").unwrap();
    fs::write(state.join("z-blocked"), "occupying file").unwrap();
    let trash = Trash::new(TrashConfig {
        dir: dir.path().join("trash").display().to_string(),
        keep_days: 30,
    });
    let error = atlas::safety::restore(
        &backup,
        &state,
        &trash,
        &atlas::household::Household::default(),
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("restored 1 file(s)") && error.contains("Replaced so far: a.txt"),
        "{error}"
    );
    assert_eq!(
        fs::read_to_string(state.join("a.txt")).unwrap(),
        "new version"
    );
    assert_eq!(
        fs::read_to_string(&trash.ledger()[0].held).unwrap(),
        "old version"
    );
    assert_eq!(
        fs::read_to_string(state.join("z-blocked")).unwrap(),
        "occupying file"
    );
}

#[test]
fn ordinary_undo_keeps_partial_file_recovery_available_for_retry() {
    let dir = Area::new();
    let original_a = dir.path().join("a");
    let original_b = dir.path().join("b");
    let moved_a = dir.path().join("moved-a");
    let moved_b = dir.path().join("moved-b");
    fs::write(&moved_a, "first").unwrap();
    fs::write(&moved_b, "second").unwrap();
    fs::write(&original_b, "new occupant").unwrap();
    let config = atlas::config::Config::load(std::path::Path::new("config")).unwrap();
    let platform = atlas::platform::mock::MockPlatform::new(Vec::new());
    let store = atlas::store::Store::new(dir.path().join("state"));
    let mut daemon = atlas::daemon::Daemon::new(&config, &platform, None, store, atlas::proactive::Proactive::new(Default::default()));
    let at = 1_790_991_000;
    let id = daemon.history.note("sorted two files", "files", atlas::undo::Undo::Atlas("move them back".into()), true, at);
    daemon.store.save(atlas::tune::TUNE_UNDO_RECORD, &vec![(id, TuneUndo::Moves(vec![(original_a.clone(), moved_a), (original_b.clone(), moved_b)]))]).unwrap();
    daemon.turn("undo", at + 1);
    let partial = daemon.turn("yes", at + 2);
    assert!(partial.contains("Still pending") && !partial.starts_with("Undone"), "{partial}");
    assert!(!daemon.history.done.iter().find(|entry| entry.id == id).unwrap().undone);
    let pending: Vec<(u64, TuneUndo)> = daemon.store.load(atlas::tune::TUNE_UNDO_RECORD);
    assert!(matches!(&pending[0].1, TuneUndo::RecordedMoves { moves, .. } if moves.len() == 1));
    fs::remove_file(&original_b).unwrap();
    daemon.turn("undo", at + 3);
    let complete = daemon.turn("yes", at + 4);
    assert!(complete.starts_with("Undone"), "{complete}");
    assert_eq!(fs::read_to_string(original_a).unwrap(), "first");
    assert_eq!(fs::read_to_string(original_b).unwrap(), "second");
    let pending: Vec<(u64, TuneUndo)> = daemon.store.load(atlas::tune::TUNE_UNDO_RECORD);
    assert!(pending.is_empty());
}

#[test]
fn restore_requires_a_confirmed_stopped_instance_under_the_resolved_data_root() {
    let dir = Area::new();
    let store = atlas::store::Store::new(dir.path().join("data/state"));
    let singleton = atlas::onlyone::OnlyOne::at(&store.data_dir());
    assert!(atlas::safety::may_restore(&store).is_ok());
    fs::create_dir_all(store.data_dir()).unwrap();
    let original = store.root().join("important.json");
    fs::create_dir_all(store.root()).unwrap();
    fs::write(&original, "keep exactly these bytes").unwrap();
    for record in [atlas::store::now().to_string(), "unreadable contents".into(), "1".into()] {
        fs::write(singleton.path(), record).unwrap();
        let error = atlas::safety::may_restore(&store).unwrap_err().to_string();
        assert!(error.contains("hasn't changed any files"), "{error}");
        assert_eq!(fs::read_to_string(&original).unwrap(), "keep exactly these bytes");
    }
    fs::remove_file(singleton.path()).unwrap();
    fs::create_dir(singleton.path()).unwrap();
    assert!(atlas::safety::may_restore(&store).is_err());
    fs::remove_dir(singleton.path()).unwrap();
    assert!(atlas::safety::may_restore(&store).is_ok());
}
