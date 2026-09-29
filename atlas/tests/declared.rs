//! Every module file is declared.
//!
//! `src/links.rs` existed with 211 lines and 19 tests written against it, and
//! `lib.rs` never named it. The consequence was worse than an unreachable
//! module: `tests/links.rs` could not resolve `atlas::links`, so **the whole
//! test suite failed to compile** — and `tests/wiring.rs` could not report the
//! problem, because a guard cannot run in a build that does not finish.
//!
//! That is the gap this closes. The wiring guard answers "can the program
//! reach this?", which is a question you only get to ask about code that
//! compiles. This one runs first and answers the earlier question: does
//! `lib.rs` know the file exists at all?

use std::fs;

/// Files under `src/` that are not modules.
const NOT_MODULES: &[&str] = &["lib.rs", "main.rs"];

#[test]
fn every_source_file_is_declared_in_lib() {
    let lib = fs::read_to_string("src/lib.rs").expect("src/lib.rs");
    let mut undeclared = Vec::new();

    for entry in fs::read_dir("src").expect("src/") {
        let path = entry.expect("readable entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if NOT_MODULES.contains(&name.as_str()) {
            continue;
        }
        let stem = name.trim_end_matches(".rs");
        if !lib.contains(&format!("pub mod {stem};")) && !lib.contains(&format!("mod {stem};")) {
            undeclared.push(stem.to_string());
        }
        // A module split into `src/<stem>.rs` + `src/<stem>/*.rs` (the plan for
        // daemon.rs and main.rs, 27 Sep 2026): each child file must be declared
        // by its parent, or it sits on disk compiled by nothing -- the same
        // failure one level down. No such folder exists yet, so today this
        // adds nothing to the result.
        let parent = path.clone();
        let children = std::path::Path::new("src").join(stem);
        if children.is_dir() {
            let parent_text = fs::read_to_string(&parent).expect("readable parent module");
            let mut kids: Vec<_> = fs::read_dir(&children).expect("readable child folder").flatten().map(|e| e.path()).collect();
            kids.sort();
            for kid in kids {
                if kid.extension().and_then(|e| e.to_str()) != Some("rs") {
                    continue;
                }
                let child = kid.file_stem().unwrap().to_string_lossy().to_string();
                if !parent_text.lines().any(|l| {
                    let t = l.trim_start();
                    !t.starts_with("//") && t.contains(&format!("mod {child};"))
                }) {
                    undeclared.push(format!("{stem}/{child}"));
                }
            }
        }
    }

    undeclared.sort();
    assert!(
        undeclared.is_empty(),
        "these files exist in src/ and lib.rs has never heard of them:\n  {}\n\n\
         Anything written against one of these won't compile, which takes the \
         whole suite down and stops every other guard running.",
        undeclared.join("\n  ")
    );
}

#[test]
fn every_declared_module_has_a_file() {
    // The other direction. A `pub mod` naming nothing is a build error rather
    // than a silent gap, so this is mostly here so the pair reads as one idea
    // and neither half gets deleted on its own.
    let lib = fs::read_to_string("src/lib.rs").expect("src/lib.rs");
    let mut missing = Vec::new();
    for line in lib.lines() {
        let t = line.trim();
        let Some(rest) = t.strip_prefix("pub mod ").or_else(|| t.strip_prefix("mod ")) else {
            continue;
        };
        let Some(name) = rest.strip_suffix(';') else { continue };
        let name = name.trim();
        let as_file = std::path::Path::new("src").join(format!("{name}.rs"));
        let as_dir = std::path::Path::new("src").join(name).join("mod.rs");
        if !as_file.exists() && !as_dir.exists() {
            missing.push(name.to_string());
        }
    }
    assert!(missing.is_empty(), "lib.rs declares modules with no file: {missing:?}");
}

/// True when `lib.rs` actually names this module, which is the only thing
/// that makes `atlas::<module>` resolvable from a test.
fn declared_in(lib: &str, module: &str) -> bool {
    lib.contains(&format!("pub mod {module};")) || lib.contains(&format!("mod {module};"))
}

#[test]
fn every_test_file_names_a_module_that_is_declared() {
    // Catches the same break from the other end: a test importing something
    // that isn't there takes the suite down before any guard can speak.
    //
    // This asked the wrong question until 9 Sep 2026. It checked that
    // `src/<module>.rs` **existed on disk**, but a test resolves
    // `atlas::<module>` through `lib.rs`, not through the filesystem. A file
    // that exists and is undeclared satisfied the old check and still failed
    // to compile — which is the exact failure this file's docstring was
    // written about. It shipped that way: `src/mending.rs` and `src/links.rs`
    // were left behind by two renames, `tests/mending.rs` and `tests/links.rs`
    // came with them, and this guard reported nothing while the suite could
    // not build. An absence of a finding was not an absence of a problem.
    let lib = fs::read_to_string("src/lib.rs").expect("src/lib.rs");
    let Ok(dir) = fs::read_dir("tests") else { return };
    let mut broken = Vec::new();
    for entry in dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else { continue };
        for line in text.lines() {
            let t = line.trim();
            let Some(rest) = t.strip_prefix("use atlas::") else { continue };
            let module: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if module.is_empty() {
                continue;
            }
            if !declared_in(&lib, &module) {
                let as_file = std::path::Path::new("src").join(format!("{module}.rs"));
                let why = if as_file.exists() {
                    "the file exists but lib.rs never declares it, so the path does not resolve"
                } else {
                    "there is no such module"
                };
                broken.push(format!(
                    "{} imports atlas::{module} — {why}",
                    path.file_name().unwrap().to_string_lossy()
                ));
            }
        }
    }
    broken.sort();
    broken.dedup();
    assert!(broken.is_empty(), "{}", broken.join("\n  "));
}

#[test]
fn no_two_source_files_are_byte_identical() {
    // Renames in this project have twice been done as copy-then-edit rather
    // than move: `mending.rs` -> `revise.rs` and `links.rs` -> `integrations.rs`
    // both left the original sitting in `src/`, undeclared, with its test file
    // still importing it. The suite could not compile and the two guards that
    // exist to say so were both looking at the wrong thing.
    //
    // A byte-identical pair is the fingerprint of that mistake and nothing
    // else — no legitimate reason exists to keep the same module under two
    // names. This catches it at the moment it happens rather than after a
    // handoff.
    let mut seen: Vec<(String, String)> = Vec::new();
    let mut dupes = Vec::new();
    // Every file under src/, subfolders included (27 Sep 2026: was the top
    // level only, so a copy left behind inside src/daemon/ after the split
    // would have gone unseen; no two files anywhere under src/ match today).
    for (module, body) in crate::common::source_file_set() {
        let name = format!("{module}.rs");
        if let Some((other, _)) = seen.iter().find(|(_, b)| *b == body) {
            let mut pair = [other.clone(), name.clone()];
            pair.sort();
            dupes.push(format!("{} and {} are the same file", pair[0], pair[1]));
        }
        seen.push((name, body));
    }
    dupes.sort();
    assert!(
        dupes.is_empty(),
        "a rename was done as a copy and the original was never deleted:\n  {}",
        dupes.join("\n  ")
    );
}
