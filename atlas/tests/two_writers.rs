//! A command run beside the running Atlas no longer has its change undone
//! (5 Oct 2026 audit, Q13).
//!
//! The daemon keeps its records in memory and writes them all back after
//! every turn. `atlas calendar import invite.ics` run while it was up wrote
//! the calendar, and the daemon's next save put its own older copy back:
//! the invite was gone, without a word.
//!
//! Another writer is played here by a `Store` on the same folder spelled a
//! second way (`state/../state`): to the store it's someone else's path, as
//! another process's would be.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-two-writers-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(p.join("state")).unwrap();
    p
}

/// The same folder, as another writer would name it.
fn other_writer(dir: &Path) -> Store {
    Store::new(dir.join("state").join("..").join("state"))
}

fn later() {
    // Past the file system's clock tick, so the second write's time differs.
    std::thread::sleep(std::time::Duration::from_millis(20));
}

#[test]
fn a_record_written_by_someone_else_is_known_as_changed() {
    let dir = tmp("seen");
    let mine = Store::new(dir.join("state"));
    let _: Vec<String> = mine.load("notes_q13");
    assert!(!mine.changed_elsewhere("notes_q13"), "nothing has happened yet");
    later();
    other_writer(&dir).save("notes_q13", &vec!["theirs".to_string()]).unwrap();
    assert!(mine.changed_elsewhere("notes_q13"));
    // Read again: up to date.
    let got: Vec<String> = mine.load("notes_q13");
    assert_eq!(got, vec!["theirs".to_string()]);
    assert!(!mine.changed_elsewhere("notes_q13"));
    // Its own write never counts as someone else's.
    mine.save("notes_q13", &vec!["mine".to_string()]).unwrap();
    assert!(!mine.changed_elsewhere("notes_q13"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_save_over_someone_elses_change_keeps_theirs_and_says_so() {
    let dir = tmp("theirs");
    let mine = Store::new(dir.join("state"));
    let _: Vec<String> = mine.load("list_q13");
    later();
    other_writer(&dir).save("list_q13", &vec!["their item".to_string()]).unwrap();
    // Not read again: this save would have erased theirs.
    mine.save("list_q13", &vec!["my item".to_string()]).unwrap();
    let kept: Vec<PathBuf> = std::fs::read_dir(dir.join("state"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.to_string_lossy().contains("list_q13.theirs."))
        .collect();
    assert_eq!(kept.len(), 1, "their version wasn't kept");
    assert!(std::fs::read_to_string(&kept[0]).unwrap().contains("their item"));
    let said = atlas::store::tell_set_aside(&mine).unwrap_or_default();
    assert!(said.contains("list q13") && said.contains(".theirs."), "{said}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_invite_imported_by_a_command_survives_the_running_atlas() {
    let dir = tmp("calendar");
    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let mut d = Daemon::new(&c, &p, None, Store::new(dir.join("state")), Proactive::new(ProactiveConfig::default()));
    let t = 1_791_000_000;
    d.persist();
    later();
    // `atlas calendar import`, while Atlas runs.
    let cli = other_writer(&dir);
    let mut cal = atlas::calendar::Calendar::load(&cli);
    cal.add("Dentist from the invite", atlas::calendar::When { start: t + 86_400, end: t + 90_000, all_day: false }, None, t);
    cal.save(&cli).unwrap();
    // The running Atlas goes on: a tick, then its save.
    let _ = d.tick(t);
    d.persist();
    let on_disk = atlas::calendar::Calendar::load(&cli);
    assert!(
        on_disk.occurrences_between(t, t + 7 * 86_400).iter().any(|e| e.title == "Dentist from the invite"),
        "the running Atlas wrote its older calendar over the imported invite"
    );
    assert!(d.calendar.occurrences_between(t, t + 7 * 86_400).iter().any(|e| e.title == "Dentist from the invite"), "and it knows about it");
    let _ = std::fs::remove_dir_all(&dir);
}
