//! An edit you made by hand to a shipped config file survives an update.
//!
//! End to end, through the real startup step and the real `Config::load`:
//! install release 1's files, edit one by hand, start release 2, and check the
//! value Atlas actually runs with. A unit test of the merge alone would pass
//! while nothing called it -- the Settings page shipped exactly that bug once
//! (see `settings_actually_stick.rs`).

use atlas::config::Config;
use atlas::yourchanges::{self, keep_hand_edits_with};
use std::fs;
use std::path::PathBuf;

fn shipped() -> Vec<(&'static str, &'static str)> {
    yourchanges::shipped_yaml()
}

fn install(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("atlas-handedits-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    atlas::firstlaunch::write_default_config(&dir).unwrap();
    // First start records what was shipped.
    let first = keep_hand_edits_with(&dir, &shipped(), &[], "1");
    assert!(first.notices.is_empty(), "a fresh install had something to say: {:?}", first.notices);
    dir
}

/// Release 2: the same files, except `record_seconds` has a new default.
fn release_2() -> Vec<(&'static str, &'static str)> {
    let tools2: &'static str = Box::leak(
        shipped()
            .iter()
            .find(|(n, _)| *n == "tools.yaml")
            .unwrap()
            .1
            .replacen("record_seconds: 8", "record_seconds: 9", 1)
            .into_boxed_str(),
    );
    shipped().into_iter().map(|(n, t)| if n == "tools.yaml" { (n, tools2) } else { (n, t) }).collect()
}

#[test]
fn a_hand_edit_is_what_atlas_runs_with_after_an_update() {
    let dir = install("runs");
    let tools = dir.join("tools.yaml");
    let text = fs::read_to_string(&tools).unwrap();
    assert!(text.contains("work_dir: \"data/tmp\""), "the test's anchor moved; pick another key");
    fs::write(&tools, text.replacen("work_dir: \"data/tmp\"", "work_dir: \"data/mine\"", 1)).unwrap();

    let k = keep_hand_edits_with(&dir, &release_2(), &[], "2");
    assert!(k.notices.iter().any(|n| n.contains("work_dir")), "{:?}", k.notices);
    // The shipped file on disk is exactly release 2.
    assert_eq!(fs::read_to_string(&tools).unwrap(), release_2().iter().find(|(n, _)| *n == "tools.yaml").unwrap().1);

    let cfg = Config::load(&dir).expect("loads");
    assert!(cfg.edits_that_went_nowhere.is_empty(), "{:?}", cfg.edits_that_went_nowhere);
    let t = cfg.tools.unwrap();
    assert_eq!(t.work_dir, "data/mine", "your edit was lost");
    assert_eq!(t.record_seconds, 9, "the release's new default did not arrive");
}

#[test]
fn a_settings_page_choice_beats_a_hand_edit_to_the_same_key() {
    let dir = install("order");
    let tools = dir.join("tools.yaml");
    let text = fs::read_to_string(&tools).unwrap();
    fs::write(&tools, text.replacen("record_seconds: 8", "record_seconds: 4", 1)).unwrap();
    keep_hand_edits_with(&dir, &shipped(), &[], "1");
    let mut p = atlas::preferences::Preferences::default();
    p.set("record_seconds", "6");
    p.save(&dir).unwrap();
    assert_eq!(Config::load(&dir).unwrap().tools.unwrap().record_seconds, 6);
    fs::remove_file(atlas::preferences::Preferences::file(&dir)).unwrap();
    assert_eq!(Config::load(&dir).unwrap().tools.unwrap().record_seconds, 4);
}

#[test]
fn an_edit_that_breaks_a_file_does_not_stop_atlas_starting() {
    let dir = install("broken");
    fs::create_dir_all(dir.join("local")).unwrap();
    fs::write(
        yourchanges::overlay_path(&dir, "tools.yaml"),
        "changes:\n  - path: [record_seconds]\n    yours: \"not a number\"\n    was: 8\n",
    )
    .unwrap();
    let cfg = Config::load(&dir).expect("a bad edit must not stop startup");
    assert_eq!(cfg.tools.unwrap().record_seconds, 8, "fell back to the shipped value");
    assert!(cfg.edits_that_went_nowhere.iter().any(|e| e.contains("cannot be read")), "{:?}", cfg.edits_that_went_nowhere);
}

#[test]
fn an_edit_to_a_section_the_release_removed_is_reported() {
    let dir = install("gone");
    fs::create_dir_all(dir.join("local")).unwrap();
    fs::write(
        yourchanges::overlay_path(&dir, "tools.yaml"),
        "changes:\n  - path: [a_section_that_was_removed, x]\n    yours: 1\n",
    )
    .unwrap();
    let cfg = Config::load(&dir).unwrap();
    assert!(cfg.edits_that_went_nowhere.iter().any(|e| e.contains("a_section_that_was_removed.x")));
}

#[test]
fn an_update_reports_the_edits_it_is_keeping() {
    let dir = install("count");
    assert_eq!(yourchanges::kept_count(&dir), 0);
    let tools = dir.join("tools.yaml");
    let text = fs::read_to_string(&tools).unwrap();
    fs::write(&tools, text.replacen("record_seconds: 8", "record_seconds: 4", 1)).unwrap();
    keep_hand_edits_with(&dir, &release_2(), &[], "2");
    assert_eq!(yourchanges::kept_count(&dir), 1);
}
