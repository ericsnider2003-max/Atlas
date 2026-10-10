//! `store::read_json` reads a small JSON file whole. A file that is there but
//! damaged must be set aside, not left for the next save to overwrite.

#[test]
fn a_damaged_file_is_set_aside_and_a_good_one_is_read() {
    let dir = std::env::temp_dir().join(format!("atlas-damaged-json-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("list.json");

    // Missing: the default, nothing created.
    let none: Vec<u32> = atlas::store::read_json(&f);
    assert!(none.is_empty());

    std::fs::write(&f, b"[1,2,3]").unwrap();
    let ok: Vec<u32> = atlas::store::read_json(&f);
    assert_eq!(ok, vec![1, 2, 3]);
    assert!(f.exists());

    std::fs::write(&f, b"[1,2,").unwrap();
    let bad: Vec<u32> = atlas::store::read_json(&f);
    assert!(bad.is_empty());
    assert!(!f.exists(), "the damaged file must not stay where the next save overwrites it");
    let kept: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert!(kept.iter().any(|n| n.starts_with("list.json.corrupt.") && n.ends_with(".bak")), "{kept:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
