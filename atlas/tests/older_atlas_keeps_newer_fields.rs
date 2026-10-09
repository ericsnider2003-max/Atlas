//! Going back to an older Atlas keeps what a newer one wrote (5 Oct 2026
//! audit, Q12).
//!
//! An older version reading a newer file keeps only the fields it knows;
//! its next save wrote the file without the rest. Roll back for a day and
//! forward again, and whatever the newer version had added was gone. The
//! store now puts back what it didn't understand.

use atlas::store::Store;
use serde::{Deserialize, Serialize};

/// What the newer Atlas wrote.
#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Clone)]
struct NewerItem {
    id: u64,
    title: String,
    // Added later, so read with a default, as every field added to a stored
    // type is: an item the older version made has none.
    #[serde(default)]
    colour: String,
}
#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Clone)]
struct Newer {
    name: String,
    #[serde(default)]
    nickname: String,
    items: Vec<NewerItem>,
}

/// What the older one knows.
#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Clone)]
struct OlderItem {
    id: u64,
    title: String,
}
#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Clone)]
struct Older {
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    note: Option<String>,
    items: Vec<OlderItem>,
}

fn store(tag: &str) -> (std::path::PathBuf, Store) {
    let d = std::env::temp_dir().join(format!("atlas-q12-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    (d.clone(), Store::new(d))
}

#[test]
fn an_older_version_saving_keeps_the_newer_versions_fields() {
    let (d, s) = store("keep");
    let newer = Newer {
        name: "Eric".into(),
        nickname: "E".into(),
        items: vec![
            NewerItem { id: 1, title: "one".into(), colour: "red".into() },
            NewerItem { id: 2, title: "two".into(), colour: "blue".into() },
        ],
    };
    s.save("profile_q12", &newer).unwrap();

    // The older Atlas: reads, changes what it knows, adds and removes items.
    let mut old: Older = s.load("profile_q12");
    old.name = "Eric S".into();
    old.items.retain(|i| i.id != 1);
    old.items.push(OlderItem { id: 3, title: "three".into() });
    old.items[0].title = "two, renamed".into();
    s.save("profile_q12", &old).unwrap();

    // Forward again.
    let back: Newer = s.load("profile_q12");
    assert_eq!(back.name, "Eric S", "the older version's own change stands");
    assert_eq!(back.nickname, "E", "a field the older version doesn't know survived its save");
    let two = back.items.iter().find(|i| i.id == 2).unwrap();
    assert_eq!((two.title.as_str(), two.colour.as_str()), ("two, renamed", "blue"), "kept inside a list, matched by id");
    assert!(back.items.iter().all(|i| i.id != 1), "a removed item stays removed");
    assert_eq!(back.items.iter().find(|i| i.id == 3).unwrap().colour, "", "a new item has nothing to inherit");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_field_this_version_knows_and_cleared_is_not_brought_back() {
    let (d, s) = store("cleared");
    s.save("note_q12", &Older { name: "x".into(), note: Some("remember".into()), items: vec![] }).unwrap();
    let mut o: Older = s.load("note_q12");
    o.note = None; // cleared: skip_serializing_if leaves the key out
    s.save("note_q12", &o).unwrap();
    let back: Older = s.load("note_q12");
    assert_eq!(back.note, None, "a known field cleared on purpose came back");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_comparison_itself() {
    use serde_json::json;
    let raw = json!({ "a": 1, "new": true, "list": [ { "id": 7, "x": 1, "y": 2 }, { "x": 3 } ] });
    let known = json!({ "a": 1, "list": [ { "id": 7, "x": 1 }, { "x": 3 } ] });
    let u = atlas::store::unknown_part(&raw, &known).expect("something unknown");
    let mut saving = json!({ "a": 5, "list": [ { "id": 9, "x": 0 }, { "id": 7, "x": 4 } ] });
    atlas::store::put_back_unknown(&u, &mut saving);
    assert_eq!(saving, json!({ "a": 5, "new": true, "list": [ { "id": 9, "x": 0 }, { "id": 7, "x": 4, "y": 2 } ] }));
    assert_eq!(atlas::store::unknown_part(&known, &known), None, "nothing unknown, nothing kept");
}

#[test]
fn no_stored_type_renames_a_field_by_alias() {
    // An alias makes the old name look unknown; put back beside the new
    // name, the next read would fail on a duplicate field. Rename with a
    // migration instead, or teach `unknown_part` about the alias first.
    let mut found = Vec::new();
    for (f, text) in crate::common::source_file_set() {
        for (n, line) in text.lines().enumerate() {
            if line.contains("serde(alias") || line.contains("alias = \"") {
                found.push(format!("src/{f}.rs:{}", n + 1));
            }
        }
    }
    assert!(found.is_empty(), "serde alias in: {found:?}");
}

#[test]
fn unnamed_list_items_are_matched_by_position_only_when_the_list_kept_its_length() {
    use serde_json::json;
    let raw = json!({ "list": [ { "x": 1, "new": "a" }, { "x": 2, "new": "b" } ] });
    let known = json!({ "list": [ { "x": 1 }, { "x": 2 } ] });
    let u = atlas::store::unknown_part(&raw, &known).expect("something unknown");
    let mut same_len = json!({ "list": [ { "x": 9 }, { "x": 8 } ] });
    atlas::store::put_back_unknown(&u, &mut same_len);
    assert_eq!(same_len, json!({ "list": [ { "x": 9, "new": "a" }, { "x": 8, "new": "b" } ] }));
    let mut grown = json!({ "list": [ { "x": 9 }, { "x": 8 }, { "x": 7 } ] });
    atlas::store::put_back_unknown(&u, &mut grown);
    assert_eq!(grown, json!({ "list": [ { "x": 9 }, { "x": 8 }, { "x": 7 } ] }), "a list that changed length is not guessed at");
}

#[test]
fn install_root_is_found_from_a_plain_a_state_and_a_profile_folder() {
    use std::path::PathBuf;
    let p = |s: &str| PathBuf::from(s);
    assert_eq!(Store::new(p("/i/data/state/profiles/ann")).install_root(), p("/i"), "a profile's root is the install, four levels up");
    assert_eq!(Store::new(p("/i/data/state")).install_root(), p("/i"));
    assert_eq!(Store::new(p("/i/data/other/profiles/ann")).install_root(), p("/i/data/other/profiles/ann"), "only the exact shape is trusted");
    assert_eq!(Store::new(p("/i/state/profiles/ann")).install_root(), p("/i/state/profiles/ann"), "data/ is part of the shape");
    assert_eq!(Store::new(p("/i/data/state/people/ann")).install_root(), p("/i/data/state/people/ann"), "profiles is part of the shape");
}

#[test]
fn install_root_needs_both_the_state_name_and_the_data_parent() {
    use std::path::PathBuf;
    let p = |s: &str| PathBuf::from(s);
    // Named `state` but not under `data`: not an install's state folder.
    assert_eq!(Store::new(p("/i/other/state")).install_root(), p("/i/other/state"), "state under something other than data");
    // Under `data` but not named `state`.
    assert_eq!(Store::new(p("/i/data/other")).install_root(), p("/i/data/other"), "a data child that is not state");
}

#[test]
fn the_data_folders_are_spelled_out_in_one_place() {
    use std::path::PathBuf;
    let p = |s: &str| PathBuf::from(s);
    for root in ["/i/data/state", "/i/data/state/profiles/ann"] {
        let s = Store::new(p(root));
        assert_eq!(s.data_dir(), p("/i/data"), "{root}");
        assert_eq!(s.logs_dir(), p("/i/data/logs"), "{root}");
        assert_eq!(s.notes_dir(), p("/i/data/notes"), "{root}");
        assert_eq!(s.backups_dir(), p("/i/data/backups"), "{root}");
        assert_eq!(s.trash_dir(), p("/i/data/trash"), "{root}");
    }
}

#[test]
fn a_file_that_cannot_be_read_is_set_aside_not_treated_as_missing() {
    let (d, s) = store("unreadable");
    // Not text, so reading it as text fails with something other than "not there".
    std::fs::write(d.join("blob_q8.json"), [0xffu8, 0xfe, 0xfd]).unwrap();
    let got: Older = s.load("blob_q8");
    assert_eq!(got, Older::default());
    assert!(!d.join("blob_q8.json").exists(), "the unreadable file was left where the next save would overwrite it");
    let kept = s.preserved();
    assert!(
        kept.iter().any(|p| p.to_string_lossy().contains("blob_q8.unreadable.")),
        "set aside under a name that says why: {kept:?}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_kept_record_is_the_saved_one_and_follows_the_file() {
    let (d, s) = store("kept");
    let one = Older { name: "one".into(), note: None, items: vec![] };
    s.save("kept_q8", &one).unwrap();
    let a: Older = s.load_kept("kept_q8");
    assert_eq!(a, one, "what was saved, not a default");
    let again: Older = s.load_kept("kept_q8");
    assert_eq!(again, one);
    // Changed on disk (a different length, so the stamp moves whatever the clock's grain).
    let two = Older { name: "two, and longer".into(), note: Some("n".into()), items: vec![OlderItem { id: 1, title: "t".into() }] };
    s.save("kept_q8", &two).unwrap();
    let b: Older = s.load_kept("kept_q8");
    assert_eq!(b, two, "a changed file is read again, not served from memory");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn overwriting_your_own_record_sets_nothing_aside() {
    let (d, s) = store("own");
    s.save("own_q8", &Older { name: "one".into(), note: None, items: vec![] }).unwrap();
    s.save("own_q8", &Older { name: "two".into(), note: None, items: vec![] }).unwrap();
    let names: Vec<String> = std::fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert!(names.iter().all(|n| !n.contains(".theirs.")), "nobody else wrote it, yet something was kept as theirs: {names:?}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_record_exists_once_saved_and_can_be_filed_under_another_name() {
    let (d, s) = store("filed");
    assert!(!s.exists("a_q8"), "nothing saved yet");
    assert!(s.preserved().is_empty(), "nothing set aside yet");
    s.save("a_q8", &Older { name: "a".into(), note: None, items: vec![] }).unwrap();
    assert!(s.exists("a_q8"));
    s.file_as("a_q8", "b_q8").unwrap();
    assert!(!s.exists("a_q8"), "moved, not copied");
    assert!(s.exists("b_q8"));
    let b: Older = s.load("b_q8");
    assert_eq!(b.name, "a", "byte for byte");
    // Refuses to replace one already there.
    s.save("c_q8", &Older { name: "c".into(), note: None, items: vec![] }).unwrap();
    assert!(s.file_as("c_q8", "b_q8").is_err());
    assert!(s.exists("c_q8"), "a refused filing leaves the original");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn now_is_the_real_clock() {
    // After Sep 2020, and never behind itself.
    let a = atlas::store::now();
    assert!(a > 1_600_000_000, "{a}");
    assert!(atlas::store::now() >= a);
}

/// How long `load` takes on a record file that can't be read. Run three
/// times and the quickest kept, so a busy machine cannot make it look slow.
fn quickest_load(tag: &str, make: &dyn Fn(&std::path::Path)) -> std::time::Duration {
    let mut best = std::time::Duration::MAX;
    for i in 0..3 {
        let (d, s) = store(&format!("{tag}{i}"));
        make(&d.join("slow_q8.json"));
        let t = std::time::Instant::now();
        let got: Older = s.load("slow_q8");
        let took = t.elapsed();
        assert_eq!(got, Older::default());
        best = best.min(took);
        let _ = std::fs::remove_dir_all(&d);
    }
    best
}

#[test]
fn a_locked_file_is_waited_for_a_few_times_but_a_missing_or_binary_one_is_not() {
    use std::time::Duration;
    // Nothing there: answered at once.
    let missing = quickest_load("missing", &|_p| {});
    assert!(missing < Duration::from_millis(300), "a missing file was waited for: {missing:?}");
    // Not text: waiting will not turn it into text.
    let binary = quickest_load("binary", &|p| std::fs::write(p, [0xffu8, 0xfe]).unwrap());
    assert!(binary < Duration::from_millis(300), "bytes that are not text were waited for: {binary:?}");
    // There but unreadable (a folder stands in for a locked file): five tries,
    // four pauses of 100 ms between them.
    let locked = quickest_load("locked", &|p| std::fs::create_dir(p).unwrap());
    assert!(locked >= Duration::from_millis(390), "gave up too soon: {locked:?}");
    assert!(locked < Duration::from_millis(490), "waited more than four pauses: {locked:?}");
}

#[test]
fn a_rename_is_tried_again_only_while_the_failure_is_a_passing_lock() {
    use atlas::store::rename_patiently_with;
    use std::cell::Cell;
    use std::path::Path;
    let refusal = || std::io::Error::other("busy");
    let (a, b) = (Path::new("a"), Path::new("b"));

    // A lock that lets go on the third try: Ok, after three tries.
    let tries = Cell::new(0);
    let r = rename_patiently_with(a, b, &[0, 0, 0], |_| true, |_, _| {
        tries.set(tries.get() + 1);
        if tries.get() < 3 { Err(refusal()) } else { Ok(()) }
    });
    assert!(r.is_ok());
    assert_eq!(tries.get(), 3, "waited and tried again while it was a passing lock");

    // A lock that never lets go: the waits run out and the error comes back.
    let tries = Cell::new(0);
    let r = rename_patiently_with(a, b, &[0, 0], |_| true, |_, _| {
        tries.set(tries.get() + 1);
        Err(refusal())
    });
    assert!(r.is_err());
    assert_eq!(tries.get(), 3, "one try, then one per wait");

    // Any other failure is returned at once, not waited out.
    let tries = Cell::new(0);
    let r = rename_patiently_with(a, b, &[0, 0, 0], |_| false, |_, _| {
        tries.set(tries.get() + 1);
        Err(refusal())
    });
    assert!(r.is_err());
    assert_eq!(tries.get(), 1, "a real refusal is not retried");
}

#[test]
fn a_small_json_file_outside_a_store_is_written_whole_and_read_back() {
    use atlas::store::{read_json, write_json, write_whole};
    let d = std::env::temp_dir().join(format!("atlas-q8-json-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let path = d.join("nested").join("devices.json");

    // Missing: the default.
    assert_eq!(read_json::<Vec<String>>(&path), Vec::<String>::new());

    let list = vec!["phone".to_string(), "ipad".to_string()];
    write_json(&path, &list).unwrap();
    assert!(path.is_file(), "write_json put nothing on disk");
    assert_eq!(read_json::<Vec<String>>(&path), list, "what was written is what is read, not the default");

    // Whole, replacing what was there, and nothing left beside it.
    write_whole(&path, b"[\"only\"]").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"[\"only\"]");
    assert_eq!(read_json::<Vec<String>>(&path), vec!["only".to_string()]);
    let beside: Vec<_> = std::fs::read_dir(path.parent().unwrap()).unwrap().flatten().collect();
    assert_eq!(beside.len(), 1, "a temp file was left behind");

    // Unreadable as JSON: the default.
    write_whole(&path, b"not json").unwrap();
    assert_eq!(read_json::<Vec<String>>(&path), Vec::<String>::new());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_rename_that_fails_for_a_real_reason_is_not_waited_out() {
    // Where a rename over a file is not blocked by a reader (anywhere but
    // Windows) nothing is a passing lock, so a missing source fails at once
    // instead of after the 1.3 s of waits.
    if cfg!(windows) {
        return;
    }
    let d = std::env::temp_dir().join(format!("atlas-q8-rename-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let t = std::time::Instant::now();
    let r = atlas::store::rename_patiently(&d.join("not_there"), &d.join("target"));
    assert!(r.is_err());
    assert!(t.elapsed() < std::time::Duration::from_millis(1000), "waited out a failure that was not a lock: {:?}", t.elapsed());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn what_was_set_aside_is_listed_for_its_own_store_and_time_only() {
    use atlas::store::{set_aside_sentence, set_aside_since};
    let (da, a) = store("aside_a");
    let (db, b) = store("aside_b");
    std::fs::write(da.join("mine_q8.json"), [0xffu8]).unwrap();
    std::fs::write(db.join("theirs_q8.json"), [0xffu8]).unwrap();
    let _: Older = a.load("mine_q8");
    let _: Older = b.load("theirs_q8");

    let mine = set_aside_since(&a, 0);
    assert_eq!(mine.len(), 1, "only this store's own: {mine:?}");
    assert_eq!((mine[0].name.as_str(), mine[0].why.as_str()), ("mine_q8", "unreadable"));
    let at = mine[0].at;
    assert!(at > 1_600_000_000, "stamped with the time it happened: {at}");
    let other = set_aside_since(&b, 0);
    assert_eq!(other.len(), 1);
    assert_eq!(other[0].name, "theirs_q8");

    // Since: from the moment it happened it is there, a second later it is not.
    assert_eq!(set_aside_since(&a, at).len(), 1, "inclusive of the moment itself");
    assert!(set_aside_since(&a, at + 1).is_empty(), "older than the window");

    // The sentence names what was put aside in plain words.
    let s = set_aside_sentence(&["notes_book".to_string(), "tasks".to_string()]);
    assert!(s.contains("notes book, tasks"), "{s}");
    assert!(s.contains("nothing was deleted"), "{s}");
    let _ = std::fs::remove_dir_all(&da);
    let _ = std::fs::remove_dir_all(&db);
}

#[test]
fn a_set_aside_file_is_told_once_and_only_to_its_own_store() {
    use atlas::store::tell_set_aside;
    let (da, a) = store("told_a");
    let (db, b) = store("told_b");
    assert_eq!(tell_set_aside(&a), None, "nothing set aside, nothing to say");

    std::fs::write(da.join("alpha_q8.json"), [0xffu8]).unwrap();
    std::fs::write(db.join("beta_q8.json"), [0xffu8]).unwrap();
    let _: Older = a.load("alpha_q8");
    let _: Older = b.load("beta_q8");

    let said = tell_set_aside(&a).expect("a file was set aside");
    assert!(said.contains("alpha q8"), "{said}");
    assert!(!said.contains("beta q8"), "another store's file was told here: {said}");
    assert_eq!(tell_set_aside(&a), None, "said once");

    let theirs = tell_set_aside(&b).expect("the other store was not told by the first telling");
    assert!(theirs.contains("beta q8") && !theirs.contains("alpha q8"), "{theirs}");
    let _ = std::fs::remove_dir_all(&da);
    let _ = std::fs::remove_dir_all(&db);
}

#[test]
fn unnamed_list_items_of_a_list_that_lost_some_are_not_paired_by_position() {
    use serde_json::json;
    let raw = json!({ "list": [ { "x": 1, "new": "a" }, { "x": 2, "new": "b" }, { "x": 3, "new": "c" } ] });
    // The older version's copy came back with fewer items, so position says nothing.
    let known = json!({ "list": [ { "x": 2 }, { "x": 3 } ] });
    assert_eq!(atlas::store::unknown_part(&raw, &known), None, "items with no id and no matching length are not guessed at");
}

#[test]
fn a_failed_save_is_recorded_once_per_record_and_kept_beside_the_others() {
    // A store whose folder can't be made: its root is a plain file.
    let d = std::env::temp_dir().join(format!("atlas-q8-failed-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let root = d.join("a_file_not_a_folder");
    std::fs::write(&root, b"x").unwrap();
    let s = Store::new(&root);
    let v = Older::default();
    assert!(s.save("first_q8", &v).is_err());
    assert!(s.save("second_q8", &v).is_err());
    assert!(s.save("first_q8", &v).is_err());
    let mut got: Vec<String> = atlas::store::take_failed_saves(&root).into_iter().map(|(n, _)| n).collect();
    got.sort();
    assert_eq!(got, vec!["first_q8".to_string(), "second_q8".to_string()], "each failing record once, the others not dropped");
    assert!(atlas::store::take_failed_saves(&root).is_empty(), "taken, so each is said once");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_record_name_is_interned_once_and_comes_back_as_itself() {
    use atlas::store::intern_record_name;
    let a = intern_record_name("interned_a_q8");
    let b = intern_record_name("interned_b_q8");
    assert_eq!((a, b), ("interned_a_q8", "interned_b_q8"), "each name comes back as itself");
    assert!(std::ptr::eq(a, intern_record_name("interned_a_q8")), "the same name is leaked once and reused");
    assert!(std::ptr::eq(b, intern_record_name("interned_b_q8")));
}
