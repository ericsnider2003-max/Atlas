//! The languages Atlas can *prove*, not just generate.
//!
//! Being in `craft::Lang` is a claim: this language has a proof ladder here —
//! a real toolchain that says whether the code is right — not merely that a
//! model can emit some of it. These defend that every language in the enum
//! carries that ladder, that the scaffold a draft is laid down in matches the
//! commands the ladder will run against it, and that a request names the right
//! one.

use atlas::build_it::lang_from_words;
use atlas::craft::{ladder, lang_of_dir, Lang, Tells};

/// Every language with a ladder. `ladder` and `plain` are exhaustive matches,
/// so a new `Lang` variant can't be added without the compiler pointing here
/// too — this list can't silently fall behind the enum.
const ALL: [Lang; 5] =
    [Lang::Rust, Lang::Python, Lang::Go, Lang::JavaScript, Lang::TypeScript];

#[test]
fn every_language_has_a_soundness_gate() {
    // The load-bearing rung: "does it compile / parse / type-check". A ladder
    // without it would run tests on code that doesn't build — noise the whole
    // design exists to avoid. So no language may be added without one.
    for lang in ALL {
        let rungs = ladder(lang);
        assert!(
            rungs.iter().any(|g| g.tells == Tells::Sound),
            "{} has no soundness gate",
            lang.plain()
        );
        assert!(
            rungs.iter().any(|g| g.tells == Tells::Behaviour),
            "{} has no behaviour gate",
            lang.plain()
        );
    }
}

#[test]
fn the_scaffold_matches_what_the_ladder_will_run() {
    // A gate that names a file (`node --check main.js`, `tsc --noEmit` needing
    // its tsconfig, `go build` needing a go.mod) must have that file laid down
    // by the draft scaffold, or the very first check fails on a missing file
    // rather than on the code.
    for lang in ALL {
        let files = lang.draft_files("// code\n");
        let names: Vec<String> = files.iter().map(|(p, _)| p.clone()).collect();
        // The unit of code is always present under a name of the language's
        // own extension.
        assert!(
            names.iter().any(|n| n.ends_with(&format!(".{}", lang.ext()))),
            "{} scaffold has no .{} file: {names:?}",
            lang.plain(),
            lang.ext()
        );
    }

    // The specific agreements worth pinning.
    let ts: Vec<String> = Lang::TypeScript.draft_files("x").into_iter().map(|(p, _)| p).collect();
    assert!(ts.iter().any(|p| p == "tsconfig.json"), "tsc --noEmit needs a tsconfig: {ts:?}");

    let go: Vec<String> = Lang::Go.draft_files("x").into_iter().map(|(p, _)| p).collect();
    assert!(go.iter().any(|p| p == "go.mod"), "go build ./... needs a go.mod: {go:?}");

    let js: Vec<String> = Lang::JavaScript.draft_files("x").into_iter().map(|(p, _)| p).collect();
    assert!(js.iter().any(|p| p == "main.js"), "node --check main.js needs main.js: {js:?}");
}

#[test]
fn paths_map_to_the_right_language() {
    assert_eq!(Lang::of_path("src/main.go"), Some(Lang::Go));
    assert_eq!(Lang::of_path("app.ts"), Some(Lang::TypeScript));
    assert_eq!(Lang::of_path("component.tsx"), Some(Lang::TypeScript));
    assert_eq!(Lang::of_path("index.js"), Some(Lang::JavaScript));
    assert_eq!(Lang::of_path("index.mjs"), Some(Lang::JavaScript));
    assert_eq!(Lang::of_path("lib.rs"), Some(Lang::Rust));
    assert_eq!(Lang::of_path("thing.py"), Some(Lang::Python));
    assert_eq!(Lang::of_path("notes.txt"), None);
}

#[test]
fn a_typescript_project_is_not_mistaken_for_javascript() {
    // A TS repo has both a package.json and a tsconfig.json. The tsconfig has
    // to win, or every TS project would read as JS and get the weaker ladder.
    let dir = std::env::temp_dir().join(format!("atlas-lang-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("package.json"), "{}").unwrap();
    std::fs::write(dir.join("tsconfig.json"), "{}").unwrap();
    assert_eq!(lang_of_dir(&dir), Some(Lang::TypeScript));

    // Just a package.json is JavaScript.
    std::fs::remove_file(dir.join("tsconfig.json")).unwrap();
    assert_eq!(lang_of_dir(&dir), Some(Lang::JavaScript));

    // A go.mod is Go.
    std::fs::write(dir.join("go.mod"), "module x").unwrap();
    // (package.json still there, but go.mod is checked first.)
    assert_eq!(lang_of_dir(&dir), Some(Lang::Go));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_request_names_the_language_it_asks_for() {
    assert_eq!(lang_from_words("write a Go service that pings a host", Lang::Rust), Lang::Go);
    assert_eq!(lang_from_words("a TypeScript function for the dashboard", Lang::Rust), Lang::TypeScript);
    assert_eq!(lang_from_words("some javascript for the web page", Lang::Rust), Lang::JavaScript);
    assert_eq!(lang_from_words("a node script", Lang::Rust), Lang::JavaScript);
    // The old cases still hold.
    assert_eq!(lang_from_words("a python script to sort files", Lang::Rust), Lang::Python);
    assert_eq!(lang_from_words("a rust function", Lang::Python), Lang::Rust);
    // Nothing named falls back to the default.
    assert_eq!(lang_from_words("just make it work", Lang::Rust), Lang::Rust);
}
