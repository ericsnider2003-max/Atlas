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
