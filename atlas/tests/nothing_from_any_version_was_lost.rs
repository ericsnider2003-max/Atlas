//! Nothing any of the three Atlas versions built was lost in merging them.
//!
//! Why this exists: by 26 Sep 2026 Atlas had been built in three chats at
//! once, from two different starting points:
//!   - the main chat's `master`, which already held the rounds chat's rounds 6-11;
//!   - the rounds chat's `tenth-sep`;
//!   - the Atlas Project chat's handoffs 25b-25j, which were never in git
//!     (and, from 27 Sep, its 25k, 26a and 26b, imported as `project-line`).
//! Eric's standing complaint was chats losing things. A hand merge of 28
//! conflicting files is exactly where that happens silently: a function kept
//! from one side and not the other, or a test file that's in the tree but
//! never compiled.
//!
//! `tests/fixtures/what_each_version_built.tsv` lists everything each version
//! added to this crate over its own base. It was generated from git by the
//! main chat on 26 Sep, before the merge, and it isn't edited by hand, except
//! that on 26 Sep it was cut to the rows under `atlas/`: personal Atlas is
//! published on its own, so the workspace's handoff notes and other crates
//! aren't beside it to check. The whole-workspace list is kept with the
//! workspace history. It holds:
//!   - every new file;
//!   - every new `pub` item in a source module;
//!   - every new `#[test]`;
//!   - every new voice intent, `atlas <command>` and hub page.
//! This test checks each one is still in the merged tree. It allows moves: an
//! item found in another module counts, and so does a test found in another
//! file. Anything a merge deliberately left out goes in
//! `what_each_version_built_dropped.tsv`, with the reason in words. The
//! failure lists every missing row by version, so the answer to "did we lose
//! anything?" is a list, not a feeling.
//!
//! It also checks two ways of being "there but lost":
//!   - a new test file that isn't compiled: nothing includes it with
//!     `#[path]` (as `tests/all.rs` does) and it has no `[[test]]` target
//!     in Cargo.toml (autotests are off);
//!   - a new source file that no `mod` line declares.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("the crate sits in the repo").to_path_buf()
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default().replace("\r\n", "\n")
}

fn rows(name: &str) -> Vec<Vec<String>> {
    read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name))
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| l.split('\t').map(str::to_string).collect())
        .collect()
}

/// Every `.rs` file under `dir`, with its text.
fn sources(dir: &Path) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push((p.clone(), read(&p)));
            }
        }
    }
    out
}

/// Does `text` define `kind name` (at any visibility)?
fn defines(text: &str, kind: &str, name: &str) -> bool {
    let needle = format!("{kind} {name}");
    text.match_indices(&needle).any(|(i, _)| {
        let after = text[i + needle.len()..].chars().next();
        let before = text[..i].chars().last();
        !after.is_some_and(|c| c.is_alphanumeric() || c == '_') && !before.is_some_and(|c| c.is_alphanumeric() || c == '_')
    })
}

fn has_fn(text: &str, name: &str) -> bool {
    defines(text, "fn", name)
}

#[test]
fn nothing_any_version_built_is_missing_from_the_merged_tree() {
    let root = repo();
    let crate_dir = root.join("atlas");
    let inventory = rows("what_each_version_built.tsv");
    assert!(inventory.len() > 2000, "the inventory is {} rows; it should be about 3,150", inventory.len());
    let dropped: Vec<Vec<String>> = rows("what_each_version_built_dropped.tsv");
    for d in &dropped {
        assert!(
            d.len() == 5 && d[4].trim().len() >= 20,
            "a dropped row needs version, kind, where, name and a reason of 20+ characters: {d:?}"
        );
    }
    let is_dropped = |r: &[String]| dropped.iter().any(|d| d[..4] == r[..4]);

    let src = sources(&crate_dir.join("src"));
    let tests = sources(&crate_dir.join("tests"));
    let all_src: String = src.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>().join("\n");
    let all_tests: String = tests.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>().join("\n");
    let commands = read(&crate_dir.join("config/commands.yaml"));
    let main_rs = crate::common::source_of("main").replace("\r\n", "\n");
    let hub_rs = crate::common::source_of("hub").replace("\r\n", "\n");
    let cargo = read(&crate_dir.join("Cargo.toml"));

    let mut missing: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut moved = 0usize;
    for r in &inventory {
        let (version, kind, place) = (&r[0], &r[1], &r[2]);
        let name = r.get(3).cloned().unwrap_or_default();
        if is_dropped(r) {
            continue;
        }
        let here = root.join(place);
        let text = read(&here);
        let found = match kind.as_str() {
            "file" => {
                let there = here.exists();
                // Present but never compiled is lost too.
                if there && place.starts_with("atlas/tests/") && place.ends_with(".rs") && !place.contains("/common/") && !place.contains("/fixtures/") {
                    let file = place.rsplit('/').next().unwrap();
                    // Compiled if tests/all.rs or another test file includes it
                    // (`#[path = "..."]`), or it has its own [[test]] target.
                    if !all_tests.contains(&format!("\"{file}\"")) && !cargo.contains(&format!("\"tests/{file}\"")) {
                        missing.entry(version.clone()).or_default().push(format!("{place}: in the tree but not compiled (no #[path] include or [[test]] target)"));
                        continue;
                    }
                }
                if there && place.starts_with("atlas/src/") && place.ends_with(".rs") {
                    let stem = here.file_stem().unwrap().to_string_lossy().to_string();
                    let special = ["main", "lib", "mod"].contains(&stem.as_str()) || place.starts_with("atlas/src/bin/");
                    if !special && !all_src.contains(&format!("mod {stem};")) && !all_src.contains(&format!("mod {stem} ")) {
                        missing.entry(version.clone()).or_default().push(format!("{place}: in the tree but no `mod {stem}` declares it"));
                        continue;
                    }
                }
                there
            }
            "item" => {
                let (k, n) = name.split_once(' ').unwrap_or(("fn", name.as_str()));
                if defines(&text, k, n) {
                    true
                } else if defines(&all_src, k, n) {
                    moved += 1;
                    true
                } else {
                    false
                }
            }
            "test" => {
                if has_fn(&text, &name) {
                    true
                } else if has_fn(&all_tests, &name) || has_fn(&all_src, &name) {
                    moved += 1;
                    true
                } else {
                    false
                }
            }
            "intent" => commands.lines().any(|l| l.trim_start().trim_start_matches('-').trim() == format!("intent: {name}")),
            "command" => main_rs.contains(&format!("Some(\"{name}\")")),
            "page" => defines(&hub_rs, "", &name) || hub_rs.contains(&format!("Page::{name}")),
            other => panic!("unknown kind {other} in the inventory"),
        };
        if !found {
            let what = if name.is_empty() { place.clone() } else { format!("{place}: {kind} {name}") };
            missing.entry(version.clone()).or_default().push(what);
        }
    }

    let total: usize = missing.values().map(Vec::len).sum();
    let mut report = format!(
        "{total} of {} things the three versions built are missing from the merged tree \
         ({moved} found moved; {} deliberately dropped with a reason).\n",
        inventory.len(),
        dropped.len()
    );
    for (version, list) in &missing {
        report.push_str(&format!("\n{version}: {} missing\n", list.len()));
        for m in list {
            report.push_str(&format!("  - {m}\n"));
        }
    }
    report.push_str(
        "\nEach one is either put back, or listed in tests/fixtures/what_each_version_built_dropped.tsv \
         with the reason it was left out.",
    );
    println!("{report}");
    assert_eq!(total, 0, "{report}");
}
