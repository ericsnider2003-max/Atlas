//! Wiring guard.
//!
//! A module can compile, be covered by tests, and still be unreachable from
//! `main.rs`. `pub mod` in lib.rs suppresses dead-code analysis, so the
//! compiler will never tell you. This test does.
//!
//! It walks the module reference graph from `main.rs` and fails if anything
//! outside the accepted baseline is unreachable. The baseline is a ratchet:
//! it may shrink, never grow.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::Path;

/// Modules that compile, are tested, and cannot be reached from `main.rs`.
///
/// This list is longer than the original audit's because that audit's detector
/// counted a config field in `voice::ToolsConfig` as a reference. It isn't
/// one — a module whose only mention is `pub x: crate::foo::FooConfig` can
/// have its settings parsed and its behaviour never run. `Intent::Dictate`
/// has no arm in `daemon::execute` at all.
///
/// Delete a name when you wire it in. Never add one.
/// High-water mark. Lower it freely. Raising it is the deliberate act of
/// admitting more unreachable code. "Never add one" was a comment last time,
/// and comments do not fail builds.
// `voiceid` left this list on 9 Sep 2026 as a side effect of wiring `recall`,
// which calls `voiceid::cosine` for semantic search. That is real reachability
// and the ratchet is right to count it, but it is one vector-maths helper —
// speaker identification itself is still not wired to anything, and nobody
// should read the shrinking number as meaning it is.
// 34 -> 27 (14 Sep): `models` and `gguf` wired. `models.rs` is "the rest of
// what Ollama does, in Rust" -- scan the folder, read the GGUF metadata, work
// out what fits, run llama-server directly -- and `gguf` came with it, since
// nothing but `models` reads model files. The ceiling is set to the list's
// actual length rather than left with slack: the slack is what let it drift
// above reality before, which `tests/ceiling.rs` caught from outside.
// 27 -> 26 (14 Sep): `mend` wired. Its Question half is what turns "I need a
// decision from you" into a backlog item that survives you not being there.
// 26 -> 20 (14 Sep, later): the away-from-the-laptop cluster wired --
// `workingset`, `remote`, `companion`, `ios`, `android`, `cloudsync`. Each was
// the thinking half of something whose other half is a phone app nobody has
// built; that is a reason for the phone side to be missing and no reason for
// the laptop side to be unreachable. `atlas carry`, `atlas remote`,
// `atlas mobile` and `atlas sync-setup <provider>` are the callers.
// 20 -> 14 (14 Sep, later still): the production cluster wired -- `edit`,
// `grade`, `plainly`, `publishing`, `voiceover`, `editors`. One missing piece
// held all six back rather than six separate gaps: nothing ever measured a
// real file, so `grade` had nothing to judge and the rest had nothing to hang
// off. `src/measure.rs` is that piece and `atlas video` is the caller.
// 14 -> 11 (14 Sep, later still): `content`, `reach` and `budget` wired.
// None of the three was waiting on a ruling -- `content`/`reach` were waiting
// on somewhere for a post's real numbers to live (a file Eric fills in, since
// no analytics connector exists), and `budget` was waiting on a hosted call
// that does not exist yet, which is exactly when its question is worth asking.
// 11 -> 10 (14 Sep, later still): `dictate` wired. This one needed more than
// a caller: `Intent::Dictate` did not exist at all -- the note at the head of
// the list below says it "has no arm in `daemon::execute`", and the variant
// it describes had been removed from `intent.rs` at some point while the note
// stayed. Adding it named five exhaustive matches in five files, each of
// which is a real ruling about dictation and not a filler arm.
//
// The name of the list is deliberately not written out in this comment.
// `tests/capability_honesty.rs` finds the list by splitting this file on the
// first occurrence of that string, so an extra mention above the constant
// hands it the wrong chunk and it fails with "baseline must be a slice
// literal" -- a guard broken by a comment, which is the fourth time a ratchet
// in this tree has moved for a reason that had nothing to do with the code.
// Set to the list's actual length, which is this file's own stated policy
// above -- the slack is what let it drift above reality before.
//
// It was 10 against a list of 5, and it passed only because
// `tests/ceiling.rs` tolerates exactly five of slack: one more and the
// tolerance itself would have caught it. **Five modules could have gone
// unwired without anything saying so.** The improvements chat found the same
// drift on its own tree at 10-against-8 in the second 17 Sep merge; this side
// had wired three more modules since, so the gap here was wider, not
// narrower. Taking their reasoning and this tree's number.
//
// Not to be raised to make room. Raising it is the thing it exists to stop.
// 5 -> 3 (18 Sep). `overnight` was wired to the tick and the backlog, and
// `delegate` became reachable through it. What is left is `afterme`,
// `confirmed` and `consent`.
const UNWIRED_CEILING: usize = 3;

const UNWIRED_BASELINE: &[&str] = &[
    // Landed tonight from an outside audit -- present, tested (28, 19, 13,
    // and 14 tests respectively), not yet wired to a caller. Each is a
    // standalone capability (question-parking that refuses shortcuts,
    // offline toolchain scaffolding, answer-effort sizing, long-horizon
    // success criteria) rather than something the existing daemon loop
    // already had a slot for.
    // afterme was wired 19 Sep 2026. It was complete, tested and
    // unreachable for one reason: `gaps` wants a place, a list of people told
    // and something counting the days, and nothing in the tree held any of
    // those. `afterme::Arrangement` is that missing piece; `atlas afterme`
    // sets it and the daemon nudges on `review_every_days`.
    // confirmed was wired 19 Sep 2026. It was written for Atlas making a
    // security change itself, which nothing here does -- but the read-back is
    // the half that does the work ("turn it off on Instagram" and "on
    // Instagram and Facebook" sound alike, and that page changes both), and
    // it is worth the same when you are the one about to click. `atlas
    // walkthrough turn-off` is the one place in this tree where a security
    // change gets started, and it started with no read-back and no yes.
    // delegate came with overnight on 18 Sep 2026: `Session::delegation_for`
    // builds a `delegate::Delegation`, so the module is now reached from
    // main. Reached is not the same as exercised -- `delegation_for`
    // returns None unless the night's brain is `delegate`, which is not the
    // shipped default and is not wired. Delisted because the ratchet
    // measures reachability, and named here so nobody reads it as a live
    // capability.
    // overnight was wired 18 Sep 2026: `Daemon::run_the_night` advances the
    // night a step an hour inside the window, works the backlog under the
    // `ask_you_later` brain, and holds the brief until you are actually up.
    // See tests/the_night_actually_runs.rs.
];
// 10 -> 8 (14 Sep): `walkthrough` and `quickinput` wired as `atlas
// walkthrough` and `atlas type`.
//
// These two were sitting in the same list as `confirmed`, `consent`,
// `delegate` and `afterme` and were being held back for the same reason --
// "acts on his accounts, needs his ruling". That was true of the other four
// and not of these. `walkthrough` is the half that *doesn't* act: it opens
// the page and says where the switch is. (`atlas_clicks` was nailed to false
// then; on 24 Sep 2026 Eric ruled Atlas may press it after his yes, and it is
// now a setting, off by default.) `quickinput` is
// a text box. Neither needed a ruling; they needed someone to notice they
// were in the wrong pile.

/// A module referenced only as a config type inside `voice::ToolsConfig` is
/// not wired. The review that produced this test named that camouflage; this
/// strips it so the test can actually see it.
///
/// Config presence means "its settings parse from YAML", not "its behaviour
/// runs".
fn strip_config_only(name: &str, src: &str) -> String {
    src.lines()
        .filter(|l| {
            let t = l.trim();
            !(t.starts_with("pub ") && t.contains(&format!("crate::{name}::"))
                && t.contains("Config"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn module_sources() -> HashMap<String, String> {
    let mut out = HashMap::new();
    for entry in fs::read_dir("src").expect("src/ must exist") {
        let path = entry.expect("readable dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap().to_string();
        if stem == "lib" {
            continue;
        }
        // `src/<stem>.rs` plus everything under `src/<stem>/` (27 Sep 2026,
        // ahead of splitting daemon.rs into src/daemon/*.rs: a child module
        // is part of its parent's wiring, not a module of its own). No such
        // folder exists today, so this reads exactly the one file it did.
        out.insert(stem.clone(), crate::common::source_of(&stem));
    }
    let platform = Path::new("src/platform/mod.rs");
    if platform.exists() {
        out.insert(
            "platform".into(),
            fs::read_to_string(platform).expect("readable platform mod"),
        );
    }
    out
}

/// True if `text` refers to `module::something`.
///
/// Deliberately crude: a path reference is enough to count as wiring, because
/// embedding a module's config struct is a real (if weak) form of use. This
/// biases toward *under*-reporting, so anything it does flag is unambiguous.
fn references(text: &str, module: &str) -> bool {
    let needle = format!("{module}::");
    let bytes = text.as_bytes();
    let mut from = 0;
    while let Some(rel) = text[from..].find(&needle) {
        let at = from + rel;
        let prev_ok = at == 0 || {
            let p = bytes[at - 1];
            !(p.is_ascii_alphanumeric() || p == b'_')
        };
        let after = at + needle.len();
        let next_ok = bytes
            .get(after)
            .is_some_and(|c| {
                c.is_ascii_alphanumeric() || matches!(c, b'_' | b'<' | b'{' | b'*')
            });
        if prev_ok && next_ok {
            return true;
        }
        from = at + needle.len();
    }
    false
}

fn reachable_from_main(sources: &HashMap<String, String>) -> HashSet<String> {
    let names: Vec<&String> = sources.keys().collect();
    let mut seen: HashSet<String> = HashSet::new();
    let mut stack = vec!["main".to_string()];
    seen.insert("main".to_string());

    while let Some(current) = stack.pop() {
        let Some(text) = sources.get(&current) else {
            continue;
        };
        for candidate in &names {
            let name: &str = candidate.as_str();
            if name == current || seen.contains(name) {
                continue;
            }
            // In voice.rs a bare `pub x: crate::foo::FooConfig` is config
            // wiring, not behaviour wiring — the camouflage this test exists
            // to see through.
            let looked_at;
            let text: &str = if current == "voice" {
                looked_at = strip_config_only(name, text);
                &looked_at
            } else {
                text
            };
            if references(text, name) {
                seen.insert(name.to_string());
                stack.push(name.to_string());
            }
        }
    }
    seen
}

#[test]
fn every_module_is_reachable_from_the_entrypoint() {
    let sources = module_sources();
    assert!(
        sources.contains_key("main"),
        "src/main.rs must exist for this test to mean anything"
    );

    let reachable = reachable_from_main(&sources);
    let unreachable: BTreeSet<String> = sources
        .keys()
        .filter(|m| **m != "main" && !reachable.contains(*m))
        .cloned()
        .collect();

    let baseline: BTreeSet<String> = UNWIRED_BASELINE.iter().map(|s| s.to_string()).collect();

    let newly_orphaned: Vec<&String> = unreachable.difference(&baseline).collect();
    assert!(
        newly_orphaned.is_empty(),
        "these modules compile and are tested but nothing in the running program \
         can reach them:\n  {}\n\nWire them into a caller, or add them to \
         UNWIRED_BASELINE deliberately.",
        newly_orphaned
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

#[test]
fn the_unwired_baseline_only_shrinks() {
    let sources = module_sources();
    let reachable = reachable_from_main(&sources);

    let now_wired: Vec<&str> = UNWIRED_BASELINE
        .iter()
        .copied()
        .filter(|m| reachable.contains(*m))
        .collect();

    assert!(
        now_wired.is_empty(),
        "these are wired in now — delete them from UNWIRED_BASELINE so the \
         ratchet keeps holding.\n\n\
         Check the same names in CAPABILITY_UNWIRED (tests/capability_wiring.rs) \
         and KNOWN (tests/new_capabilities_are_wired.rs) in the SAME change. \
         Wiring a module usually means deleting from more than one of the three, \
         and each of those checks a different thing — so the others do not fail \
         until this one passes, which means finding out a full run at a time.\n\n  {}",
        now_wired.join("\n  ")
    );
}

#[test]
fn baseline_names_refer_to_real_modules() {
    let sources = module_sources();
    let bogus: Vec<&str> = UNWIRED_BASELINE
        .iter()
        .copied()
        .filter(|m| !sources.contains_key(*m))
        .collect();
    assert!(
        bogus.is_empty(),
        "UNWIRED_BASELINE names modules that no longer exist: {bogus:?}"
    );
}

#[test]
fn the_ceiling_tracks_reality_rather_than_drifting_above_it() {
    // `saturating_sub` alone is blind to exactly the case this test is named
    // for: if the baseline outgrows the ceiling, the subtraction floors at
    // zero and "0 <= 5" passes regardless of how far past it the list has
    // grown. tests/ceiling.rs caught this from outside once already -- this
    // direct check closes the same gap from inside the file it happened in.
    assert!(
        UNWIRED_BASELINE.len() <= UNWIRED_CEILING,
        "the unwired list has grown to {} past its ceiling of {UNWIRED_CEILING} -- \
         raise the ceiling deliberately or wire something in",
        UNWIRED_BASELINE.len()
    );
    let slack = UNWIRED_CEILING.saturating_sub(UNWIRED_BASELINE.len());
    assert!(slack <= 5, "lower the ceiling to {} so it means something", UNWIRED_BASELINE.len());
}
