//! Every capability Atlas lists is reached by a test (Q8, 8 Oct 2026).
//!
//! `capability::all()` is the list of what Atlas can do. A capability whose
//! modules no test file calls into and which has no test of its own is a
//! feature that has never been run by anything: it can be broken and the suite
//! stays green. This is the floor for "one test per feature".
//!
//! What it proves is *reach*, not correctness: some test imports the module or
//! the module tests itself. It does not prove the test checks the right thing
//! (that is what the mutation runs are for), and it does not prove the feature
//! works from the front door (`every_command_end_to_end` does that for the
//! 162 spoken commands). A capability with no reach at all is named here and
//! the list may only shrink.

use atlas::capability;
use std::path::{Path, PathBuf};

/// Capabilities allowed to have no reaching test, each with what they wait on.
/// Empty on 8 Oct 2026: every capability is reached. A new entry needs a reason.
const UNREACHED: &[(&str, &str)] = &[];

/// Test files that read source text instead of running behaviour; they mention
/// every module and prove nothing about any of them.
const GUARDS: &[&str] = &[
    "catalogue", "capability_wiring", "dead_capabilities", "new_capabilities_are_wired", "name_collisions",
    "dead_methods", "dead_config", "wiring", "guards", "hollowcode",
];

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// The source files of a module wherever they live: `src/name.rs`,
/// `src/name/**`, or the same under a parent folder (`src/market/claims.rs`).
fn files_of(name: &str, src: &[PathBuf]) -> Vec<PathBuf> {
    src.iter()
        .filter(|p| {
            let stem_is = p.file_stem().is_some_and(|s| s == name);
            let under = p.components().any(|c| c.as_os_str() == name);
            stem_is || under
        })
        .cloned()
        .collect()
}

#[test]
fn every_capability_is_reached_by_some_test() {
    let mut src = Vec::new();
    rs_files(Path::new("src"), &mut src);
    let mut tests: Vec<(String, String)> = Vec::new();
    let mut files = Vec::new();
    rs_files(Path::new("tests"), &mut files);
    for f in files {
        let stem = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        if GUARDS.contains(&stem.as_str()) || stem.starts_with("every_") && stem != "every_command_end_to_end" || stem == "mod" {
            continue;
        }
        if let Ok(t) = std::fs::read_to_string(&f) {
            tests.push((stem, t));
        }
    }

    let reached = |module: &str| -> bool {
        let inline = files_of(module, &src)
            .iter()
            .any(|f| std::fs::read_to_string(f).map(|t| t.contains(concat!("#[te", "st]"))).unwrap_or(false));
        let pats = [format!("::{module}::"), format!("::{module};"), format!("::{module}}}"), format!("::{module},"), format!("::{module} as")];
        inline || tests.iter().any(|(_, t)| pats.iter().any(|p| t.contains(p.as_str())))
    };

    let mut unreached = Vec::new();
    for c in capability::all() {
        if !c.modules.iter().any(|m| reached(m)) {
            unreached.push(c.id);
        }
    }
    let allowed: Vec<&str> = UNREACHED.iter().map(|(id, _)| *id).collect();
    let new: Vec<&&str> = unreached.iter().filter(|id| !allowed.contains(*id)).collect();
    assert!(
        new.is_empty(),
        "these capabilities are listed in capability::all() but no test calls into their modules and the modules have no tests of their own: {new:?}. Write the test; do not add them to UNREACHED without saying what they wait on."
    );
    let gone: Vec<&&str> = allowed.iter().filter(|id| !unreached.contains(*id)).collect();
    assert!(gone.is_empty(), "these are in UNREACHED but are reached now; take them off the list: {gone:?}");
}

#[test]
fn a_capability_with_no_module_would_be_caught() {
    // The check can fail: a module nothing mentions is not "reached".
    let mut src = Vec::new();
    rs_files(Path::new("src"), &mut src);
    assert!(files_of("claims", &src).iter().any(|p| p.to_string_lossy().contains("market")), "nested modules are found where they live");
    assert!(files_of("no_such_module_anywhere", &src).is_empty());
}
