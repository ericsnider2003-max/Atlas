//! The helpers every source-reading test goes through (`tests/common`), held
//! to what they promise. Added 27 Sep 2026 ahead of splitting daemon.rs and
//! main.rs into folders: about 150 tests read those files as text, and the
//! split is only safe if what they read is still the whole module.

use crate::common::{
    fold_split_modules, included_assets, is_fn_definition, read_source_path, source_file_set,
    source_files_of, source_of,
};

#[test]
fn a_module_without_a_folder_reads_as_exactly_its_file() {
    // Today none of the five big files has a folder, so reading them through
    // the helper must change nothing -- the promise the whole conversion of
    // the tests rested on.
    for m in ["daemon", "main", "hublive", "server"] {
        let direct = std::fs::read_to_string(format!("src/{m}.rs")).unwrap();
        if std::path::Path::new(&format!("src/{m}")).is_dir() {
            continue; // split since: covered by the next test instead
        }
        assert_eq!(source_of(m), direct, "{m}");
        assert_eq!(read_source_path(&format!("src/{m}.rs")).unwrap(), direct, "{m}");
    }
}

#[test]
fn a_folder_module_reads_its_root_first_then_every_child() {
    // `platform` has no platform.rs: mod.rs is its root.
    let files = source_files_of("platform");
    assert_eq!(files[0], std::path::Path::new("src/platform/mod.rs"));
    assert!(files.iter().any(|f| f.ends_with("win.rs")) && files.iter().any(|f| f.ends_with("mock.rs")));
    let text = source_of("platform");
    assert!(text.starts_with(&std::fs::read_to_string("src/platform/mod.rs").unwrap()));
    assert!(text.contains(&std::fs::read_to_string("src/platform/mock.rs").unwrap()));
    // The rest are sorted.
    let rest: Vec<_> = files[1..].to_vec();
    let mut sorted = rest.clone();
    sorted.sort();
    assert_eq!(rest, sorted);
}

#[test]
fn text_built_in_from_assets_is_still_the_modules_text() {
    // The hub's stylesheet moved to assets/hub/hub.css on 27 Sep 2026. What
    // the tests read as "hub.rs" still carries it, in the order included.
    let assets = included_assets(std::path::Path::new("src/hub.rs"));
    assert!(assets.iter().any(|a| a == std::path::Path::new("assets/hub/hub.css")), "{assets:?}");
    let hub = source_of("hub");
    let css = std::fs::read_to_string("assets/hub/hub.css").unwrap();
    assert!(hub.contains(&css));
    assert!(hub.contains("pointerdown"), "the drag script is part of the hub's text");
    // config/ is data, not code, and is not followed.
    let guess = source_of("guessable");
    assert_eq!(guess, std::fs::read_to_string("src/guessable.rs").unwrap());
}

#[test]
fn a_missing_module_fails_loudly() {
    assert!(read_source_path("src/no_such_module_here.rs").is_none());
    let r = std::panic::catch_unwind(|| source_of("no_such_module_here"));
    assert!(r.is_err(), "reading a module that is gone must fail, not scan nothing");
}

#[test]
fn the_whole_tree_is_every_rs_file_under_src() {
    let set = source_file_set();
    let names: Vec<&str> = set.iter().map(|(m, _)| m.as_str()).collect();
    for want in ["daemon", "main", "lib", "platform/mod", "market/bars"] {
        assert!(names.contains(&want), "{want} missing");
    }
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names.len(), sorted.len());
}

#[test]
fn a_split_modules_children_fold_into_it_and_nothing_else_does() {
    let entries = vec![
        ("daemon".to_string(), "a".to_string()),
        ("daemon/late".to_string(), "b".to_string()),
        ("daemon/tick/x".to_string(), "c".to_string()),
        ("platform/mod".to_string(), "p".to_string()),
        ("platform/win".to_string(), "w".to_string()),
        ("zeta".to_string(), "z".to_string()),
    ];
    let folded = fold_split_modules(entries);
    assert_eq!(
        folded,
        vec![
            ("daemon".to_string(), "a\nb\nc".to_string()),
            ("platform/mod".to_string(), "p".to_string()),
            ("platform/win".to_string(), "w".to_string()),
            ("zeta".to_string(), "z".to_string()),
        ]
    );
    // And against the tree as it is today: nothing folds.
    let tree = source_file_set();
    assert_eq!(fold_split_modules(tree.clone()).len(), tree.len());
}

#[test]
fn every_spelling_of_a_function_definition_is_one() {
    for yes in [
        "fn a(",
        "pub fn a(",
        "pub(crate) fn a(",
        "pub(super) fn a(",
        "pub(in crate::daemon) fn a(",
        "pub const fn a(",
        "async fn a(",
        "pub(crate) async unsafe fn a(",
        "pub extern \"C\" fn a(",
    ] {
        assert!(is_fn_definition(yes), "{yes}");
    }
    for no in ["a(", "public_fn(", "self.fn_like(", "const A: u8 = f(1);", "let f = |x| x;", "// fn a("] {
        assert!(!is_fn_definition(no), "{no}");
    }
}
