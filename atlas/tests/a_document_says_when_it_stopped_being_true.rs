//! Every document in `docs/` is either current or says it is not.
//!
//! # The failure this is for
//!
//! On 19 September 2026 the doc folder held forty-seven markdown files. Four
//! of them were true. `BRIEF.md` opened with "64 modules · 13,900 lines of
//! Rust · 596 tests"; the tree had 281 modules, 127,245 lines and 5,637
//! tests. `THE_RETROSPECTIVE.md` described a hand review of 145 modules.
//! `WHERE_IT_STANDS.md` said 108 modules and 1,261 tests. `METRICS.md`, whose
//! own header says *do not restate these figures anywhere else — link here
//! instead*, said 139 modules; it has since been regenerated, and regenerating
//! it found a parser bug that had been filling its "not reachable" section
//! with fragments of chopped-up comments (see `tests/metrics.rs`).
//!
//! None of those files was wrong when it was written. That is the whole
//! problem: a document that was true in early September and a document that
//! is true today look identical from the outside, so the way you find out
//! which one you are reading is to check it against the code — which is the
//! work the document was supposed to save you.
//!
//! `tests/bug_sweep.rs` already catches one narrow shape of this: a doc
//! claiming something is unbuilt which is wired. It cannot catch a doc that
//! is merely *old*, because being old is not a claim. So it is said out loud
//! instead, at the top of the file, where the next reader sees it before the
//! first paragraph.
//!
//! # Why a banner rather than a delete
//!
//! These files are the record of how the tree got here, and several of them
//! carry reasoning that is still the best statement of why something is the
//! way it is — `AUDIT.md` argues for deleting the vault, and the vault was
//! built instead; the argument is still worth reading and the conclusion is
//! still wrong. Editing them to match later work would destroy exactly the
//! thing that makes them worth keeping. So nothing is rewritten and nothing
//! is deleted; each one says which date it was last true on, and where the
//! current answer lives.
//!
//! # Why a list rather than a count
//!
//! Same argument as `STALE_DOC_BASELINE` in `bug_sweep.rs`, which replaced
//! `STALE_DOC_CLAIMS: usize = 7`. A number tells you some document is wrong
//! and names none of them, so the cheap way to make it pass is to write the
//! next number. A named list puts the file you are excusing in the diff.

use std::collections::BTreeSet;

/// Documents held to being true right now.
///
/// Two kinds, and the distinction matters:
///
/// * **Generated.** `CAPABILITIES.md` is written by
///   `capability::as_markdown()` and `tests/catalogue.rs` fails if it and
///   `capability::all()` disagree. `MODULE_REFERENCE_2026-09-26.md` is a
///   dump of every module's own doc comment. Neither can fall behind the
///   code without a test failing, so neither needs watching here. `METRICS.md`
///   is written by `atlas metrics`; `tests/metrics.rs` and `tests/guards.rs`
///   both hold it, the second because the dispatch that writes it has been
///   lost in a merge twice.
/// * **Evergreen.** How to install it, how to set up a voice, what runs on
///   which platform. These describe mechanisms rather than the state of the
///   tree, and a mechanism does not go stale when a module is added. They
///   were checked by hand on 19 Sep 2026 and carry no tree-size numbers --
///   which is the property that earns the place on this list, and the
///   property the second test below actually enforces.
const CURRENT: &[&str] = &[
    // The accessibility statement and conformance report (26 Sep), kept with
    // the Help page's own statement.
    "ACCESSIBILITY.md",
    "CAPABILITIES.md",
    "FIRST_BOOT.md",
    "GETTING_IT_ON_A_PHONE.md",
    "HANDOFF_2026-09-19.md",
    "HOW_IT_WORKS_ON_ITSELF.md",
    "INSTALLING.md",
    "JARVIS.md",
    "MEDIA.md",
    "METRICS.md",
    "MODEL_STACK.md",
    "MODULE_REFERENCE_2026-09-26.md",
    "NOT_GETTING_STUCK.md",
    "OFFLINE.md",
    "ON_YOUR_PHONE.md",
    "REFERENCES.md",
    "SENDING_FROM_YOUR_PHONE.md",
    "SETTING_UP.md",
    // Checked against the code at each courier build step; §20 is its log.
    "UPDATE_COURIER_SPEC.md",
    "VOICE_SETUP.md",
    "WHAT_RUNS_WHERE.md",
];

/// How a stale document opens.
const SAYS_SO: &str = "> **STALE";

fn docs() -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir("docs") else { return out };
    for e in dir.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("md") {
            continue;
        }
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        if let Ok(t) = std::fs::read_to_string(&p) {
            out.push((name, t));
        }
    }
    out.sort();
    out
}

#[test]
fn a_document_that_is_not_current_says_so_in_its_first_lines() {
    let mut silent = Vec::new();
    for (name, text) in docs() {
        if CURRENT.contains(&name.as_str()) {
            continue;
        }
        // The first six lines, not the whole file: a warning buried at the
        // bottom is a warning you read after acting on the document.
        let top: String = text.lines().take(6).collect::<Vec<_>>().join("\n");
        if !top.contains(SAYS_SO) {
            silent.push(name);
        }
    }
    assert!(
        silent.is_empty(),
        "these documents are not on the current list and do not say they are out of \
         date, so a reader has no way to tell which they are holding:\n  {}\n\nEither \
         add the stale banner, or put the file on CURRENT once it has been checked \
         against the code.",
        silent.join("\n  ")
    );
}

#[test]
fn a_document_called_current_carries_no_tree_size_number() {
    // What actually went wrong: not that the evergreen files were wrong, but
    // that a file describing a mechanism picked up a sentence of state on the
    // way past. `COST.md` is about what Atlas costs to run and had "891
    // tests" in it; `RESOURCES.md` is about what a laptop needs and had
    // "195 tests". One sentence each, and each one turned an evergreen
    // document into a dated one without anybody deciding to.
    //
    // So the rule is not "do not be wrong", which cannot be tested. It is
    // "do not carry the kind of number that goes stale", which can.
    // `UPDATE_COURIER_SPEC.md` is a live spec with a build log at its foot,
    // one line per step and the tests that step added -- a handoff by
    // another name, kept by the main chat as each step lands (round 9).
    let generated = [
        "CAPABILITIES.md",
        "METRICS.md",
        "MODULE_REFERENCE_",
        "HANDOFF_",
        "UPDATE_COURIER_SPEC.md",
    ];
    let mut carrying = Vec::new();
    for (name, text) in docs() {
        if !CURRENT.contains(&name.as_str()) {
            continue;
        }
        // A generated file and the handoff are *supposed* to state the
        // numbers -- that is what they are for, and each is regenerated or
        // rewritten rather than left to drift.
        if generated.iter().any(|g| name.starts_with(g)) {
            continue;
        }
        for (n, line) in text.lines().enumerate() {
            let l = line.to_lowercase();
            for unit in ["tests", "modules", "lines of rust", "lines of source"] {
                if let Some(at) = l.find(unit) {
                    // A number immediately before the unit. "five tests" and
                    // "the tests" are prose; "891 tests" is a measurement
                    // that was taken once.
                    let before: String = l[..at].chars().rev().take(12).collect();
                    if before.trim_start().starts_with(|c: char| c.is_ascii_digit()) {
                        carrying.push(format!("{name}:{}: {}", n + 1, line.trim()));
                    }
                }
            }
        }
    }
    assert!(
        carrying.is_empty(),
        "these are on the current list and state a number that goes stale on its \
         own:\n  {}\n\nLink to METRICS.md or the handoff instead of restating a \
         count, or take the file off CURRENT.",
        carrying.join("\n  ")
    );
}

#[test]
fn every_name_on_the_current_list_is_a_file_that_exists() {
    // A list of filenames rots the same way a document does: a file gets
    // renamed, its entry here keeps passing because nothing checks it, and
    // the guard quietly stops covering the file it was written for.
    let there: BTreeSet<String> = docs().into_iter().map(|(n, _)| n).collect();
    let gone: Vec<&&str> = CURRENT.iter().filter(|c| !there.contains(**c)).collect();
    assert!(
        gone.is_empty(),
        "CURRENT names documents that are not in docs/: {gone:?}. A name on this \
         list that points at nothing is a guard covering nothing."
    );
}

#[test]
fn the_current_list_is_sorted_and_free_of_duplicates() {
    let mut sorted = CURRENT.to_vec();
    sorted.sort_unstable();
    assert_eq!(sorted, CURRENT.to_vec(), "CURRENT is not in sorted order");
    let unique: BTreeSet<&&str> = CURRENT.iter().collect();
    assert_eq!(unique.len(), CURRENT.len(), "CURRENT has a duplicate");
}
