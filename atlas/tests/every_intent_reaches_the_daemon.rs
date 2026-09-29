//! The gap between "the capability is tested" and "the daemon calls it right".
//!
//! # How this was found
//!
//! Not by reading. By planting a defect.
//!
//! `daemon::mute_topic` consults the unmute reader before the mute reader,
//! because "unmute the backups" contains the word "mute" and would otherwise
//! be read as a request to silence the very thing being asked for. The
//! function carries a comment saying exactly that. **Swapping the two blocks
//! left the entire 4,900-test suite green**, because nothing in the tree ever
//! executed that function.
//!
//! `interrupt::mute_from` had tests. `Muted::mute` had tests. The branch that
//! joins them — which one to ask first — had none, and it is the only part of
//! the arrangement anybody could get wrong.
//!
//! # Why the other nine guards do not catch it
//!
//! Every reachability guard in this tree asks *"is this called?"*. `mute_from`
//! was called, by `daemon::mute_topic`, so it counted as wired and came off
//! `KNOWN` the same day. The guards measure the edge from the daemon to the
//! capability. **Nothing measures whether that edge was ever traversed.**
//!
//! That is a second kind of deadness, and it is the more expensive one: dead
//! code does nothing, whereas a wrong branch does the wrong thing confidently
//! and reports success.
//!
//! # What this measures
//!
//! For every `Intent` variant, whether any test drives it through the daemon —
//! either `Daemon::execute(&Intent::X)` directly, or `Daemon::turn("...")`
//! with a sentence that the real parser turns into that intent.
//!
//! `turn` is the front door and is what most of the suite uses, so a guard
//! reading only `execute` would report almost everything as uncovered and be
//! wrong about most of it.
//!
//! **The measurement is deliberately conservative.** A `turn` call whose
//! argument is a loop variable cannot be resolved by reading the file, so it
//! does not count as coverage. File-local `const NAME: &str = "..."` is
//! resolved, because several tests use that shape. The error direction is
//! toward listing something as uncovered when a loop does in fact cover it —
//! which shows up as a nuisance entry, not as a false green.

use atlas::config::Config;
use atlas::intent::{Intent, Parser};
use std::collections::{BTreeMap, BTreeSet};

/// `Intent::Foo` -> the name `session::kind_of` gives it.
///
/// Read out of `kind_of` itself rather than guessed by lower-casing the
/// variant name. `kind_of` is the tree's existing identity for an intent —
/// it is what `profiles.rs`'s two restriction lists are written in, and what
/// the exhaustive classification guard checks against. Two spellings of the
/// same idea is how two lists drift apart.
fn variant_names() -> BTreeMap<String, String> {
    let src = std::fs::read_to_string("src/session.rs").expect("src/session.rs");
    let start = src.find("pub fn kind_of").expect("kind_of");
    let body = &src[start..];
    let end = body.find("\n}\n").unwrap_or(body.len());
    let mut out = BTreeMap::new();
    for line in body[..end].lines() {
        let Some(rest) = line.split("Intent::").nth(1) else { continue };
        let variant: String = rest.chars().take_while(|c| c.is_alphanumeric()).collect();
        let Some(q) = line.split("=> \"").nth(1) else { continue };
        let name: String = q.chars().take_while(|c| *c != '"').collect();
        if !variant.is_empty() && !name.is_empty() {
            out.insert(variant, name);
        }
    }
    assert!(
        out.len() > 50,
        "the kind_of parse found only {} variants, so it has stopped working \
         and this guard would pass for anything",
        out.len()
    );
    out
}

/// Every variant of `Intent`, by its `kind_of` name.
fn every_intent() -> BTreeSet<String> {
    variant_names().into_values().collect()
}

fn string_literal_after(text: &str, at: usize) -> Option<String> {
    let rest = &text[at..];
    let open = rest.find('"')?;
    // Nothing between the paren and the quote but whitespace, or it is an
    // expression rather than a literal argument.
    if rest[..open].contains(|c: char| !c.is_whitespace() && c != '(') {
        return None;
    }
    let after = &rest[open + 1..];
    let close = after.find('"')?;
    Some(after[..close].to_string())
}

/// Intents some test actually drives through the daemon.
fn reached_by_a_test(parser: &Parser) -> BTreeSet<String> {
    let names = variant_names();
    let mut out = BTreeSet::new();
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir("tests")
        .expect("tests/")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x == "rs").unwrap_or(false))
        .collect();
    files.sort();

    for path in files {
        let text = std::fs::read_to_string(&path).unwrap_or_default();

        // `const A_REAL_COMMAND: &str = "what's outstanding";`
        let mut consts: BTreeMap<String, String> = BTreeMap::new();
        for line in text.lines() {
            let t = line.trim();
            if !t.starts_with("const ") || !t.contains("&str") {
                continue;
            }
            let Some(name) = t[6..].split(':').next().map(|s| s.trim().to_string()) else {
                continue;
            };
            let Some(eq) = t.find('=') else { continue };
            if let Some(lit) = string_literal_after(t, eq) {
                consts.insert(name, lit);
            }
        }

        for line in text.lines() {
            let code = line.split("//").next().unwrap_or("");

            // Direct: `execute(&Intent::Foo` / `execute_timed(&Intent::Foo`.
            let mut rest = code;
            while let Some(i) = rest.find("Intent::") {
                rest = &rest[i + 8..];
                let variant: String = rest.chars().take_while(|c| c.is_alphanumeric()).collect();
                // Only when this line is actually executing it. Matching every
                // `Intent::` would count the parser tests, which is precisely
                // the coverage this guard exists to distinguish from.
                if code.contains("execute(") || code.contains("execute_timed(") {
                    if let Some(name) = names.get(&variant) {
                        out.insert(name.clone());
                    }
                }
            }

            // The front door: `turn("...")` / `turn_from("...")`.
            for marker in [".turn(", ".turn_from("] {
                let Some(i) = code.find(marker) else { continue };
                let at = i + marker.len() - 1;
                let said = string_literal_after(code, at).or_else(|| {
                    let arg: String = code[at + 1..]
                        .trim_start()
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    consts.get(&arg).cloned()
                });
                if let Some(said) = said {
                    out.insert(atlas::session::kind_of(&parser.parse(&said)).to_string());
                }
            }
        }
    }

    assert!(
        out.len() > 10,
        "the coverage scan found only {} intents reached by a test, so it has \
         stopped working and this guard would pass for anything",
        out.len()
    );
    out
}

fn parser(cfg: &Config) -> Parser {
    Parser::new(&cfg.commands)
}

/// Intents no test drives through the daemon, each with why it is tolerable.
///
/// On the same rule as `ORPHANS` and `KNOWN` elsewhere in this suite: **the
/// list may grow; it may not grow silently.** Every entry carries a reason,
/// and a reason of the shape "nobody got to it" is not one — it is the
/// admission that the entry should not be here.
///
/// The rule that decided which of the original 27 got a test and which got a
/// line: **if the branch being wrong would hand somebody your secrets, act as
/// you, or silently lose something, it gets a test.** Everything below fails
/// visibly and immediately when it fails at all. A wrong `which model fits
/// here` is a bad answer on the screen in front of you; a wrong `capture` was
/// a thought quietly dropped, which is why that one is not on this list any
/// more — it was tested, and it was broken.
// `apply_lesson` was removed on 17 Sep. Its reason here said a wrong branch
// "shows up the next time you ask what it learned" -- and it ended
// `let _ = save(...)` before saying "I'll keep to it", so a failed write lost
// the correction *and* told the person it was kept, which stops them
// repeating it. Not visible, and worse than losing it quietly.
//
// `travel_prep` is still listed below and is the next one to look at, for the
// same reason in a different dress: "reads back a list you are looking at"
// assumes the reader can tell a right list from a wrong one by looking. The
// recovery-codes flag it depends on was unsettable until this same day, so
// that list was wrong for as long as it existed and nobody saw it.
const NO_DAEMON_TEST: &[(&str, &str)] = &[
    // `capabilities` came off 21 Sep: `tests/what_works_offline.rs` now drives
    // `Intent::Capabilities` through the daemon (the offline-count wiring), so
    // the branch has a real end-to-end test.
    (
        "capture_webcam",
        "needs a camera. Everything below the branch is `frames`/`gaze`, which \
         are tested against fixtures; the branch itself cannot run headless.",
    ),
    (
        "gestures",
        "needs a hand in front of a camera. `handshape` has 55 tests of its \
         own against recorded geometry.",
    ),
    // `history` came off 21 Sep 2026: `reviewing_what_atlas_did_unprompted.rs`
    // now drives the intent through the daemon end to end -- "what did you do
    // on your own" is answered from `undo::on_its_own`, and the plain question
    // still returns the whole log -- so the branch has a daemon-level test.
    (
        "machine_health",
        "reports what the machine is doing. Wrong numbers are wrong on the \
         screen, and `health.rs` produces them under its own tests.",
    ),
    (
        "name_this",
        "the owner-side twin of `this_is_me`, which IS tested at the daemon \
         because a guest reaching it is the dangerous case. Naming something \
         as yourself, on your own machine, is not.",
    ),
    // `recommend` gained a daemon-level test 21 Sep 2026
    // (`recommend_names_the_slowest_stage.rs`): wiring `wants::slowest` into
    // its reply meant proving, through the daemon, that the self-report names
    // the measured bottleneck -- so this is no longer an intent with no test.
    (
        "say",
        "speaks a line. `tts` and `speech` are tested, including interruption, \
         and a wrong branch here is audible immediately.",
    ),
    // set_mode came off this list on 22 Sep 2026: wiring modes::leave gave it a
    // daemon-level test (tests/leaving_a_mode.rs drives "mode off" through a
    // real Daemon), so it is no longer untested at that level.
    (
        "travel_prep",
        "on `THE_OWNERS_OWN`, so the guest case is covered by the exhaustive \
         classification guard rather than left open; what is untested is the \
         happy path, which reads back a list you are looking at.",
    ),
    // `view_display` came off on 28 Sep 2026: "look at my screen" now reads
    // the window's words when the picture reader can't, and
    // `tests/reading_without_the_picture_reader.rs` drives it through the
    // daemon.
    (
        "whats_there",
        "describes what is on screen. `vision` and `words` have 39 and 38 \
         tests; the branch adds no decision.",
    ),
    (
        "whats_this",
        "the same as `whats_there`, scoped to one thing rather than the whole \
         screen. `subject.rs` works out what 'this' refers to and is tested \
         separately; a wrong answer is a wrong description said back to you \
         about something you are looking at.",
    ),
    (
        "which_model",
        "answers which model fits this machine. `models` and `fit` are tested; \
         the answer is the output and you can read it.",
    ),
];

#[test]
fn the_gap_is_named_rather_than_forgotten() {
    let cfg = Config::load(std::path::Path::new("config")).expect("config");
    let reached = reached_by_a_test(&parser(&cfg));
    let all = every_intent();
    let missing: BTreeSet<String> = all.difference(&reached).cloned().collect();
    let listed: BTreeSet<String> = NO_DAEMON_TEST.iter().map(|(n, _)| n.to_string()).collect();

    let unexplained: Vec<&String> = missing.difference(&listed).collect();
    assert!(
        unexplained.is_empty(),
        "these intents are dispatched by the daemon and no test ever drives \
         them through it:\n  {}\n\nThe capability may well be tested; the \
         branch that reaches it is not, and that is where the decision lives. \
         Write a test in `tests/the_branches_that_do_damage.rs`, or add it to \
         NO_DAEMON_TEST with why a wrong branch there would be noticed.",
        unexplained.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n  ")
    );

    let fixed: Vec<&String> = listed.difference(&missing).collect();
    assert!(
        fixed.is_empty(),
        "these are listed as having no daemon-level test and now have one:\n  \
         {}\n\nGood -- delete those lines, so the list keeps meaning something.",
        fixed.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n  ")
    );
}

#[test]
fn every_entry_says_why_a_wrong_branch_would_be_noticed() {
    // A list of names with no reasons is a ceiling again, just longer. The
    // length floor is doing real work: "not risky" fits in fewer characters
    // than any argument for why it is not risky.
    for (name, why) in NO_DAEMON_TEST {
        assert!(
            why.len() > 60,
            "{name} is exempted without a real reason: {why:?}"
        );
        assert!(
            !why.to_lowercase().contains("todo") && !why.to_lowercase().contains("not yet"),
            "{name}'s reason is a note to self rather than an argument: {why:?}"
        );
    }
}

#[test]
fn most_of_the_daemon_is_actually_exercised() {
    // A blunt floor under the whole measurement. If a refactor breaks the
    // scan, the two tests above start passing for the wrong reason -- an
    // empty `missing` set agrees with an empty list. This one does not.
    let cfg = Config::load(std::path::Path::new("config")).expect("config");
    let reached = reached_by_a_test(&parser(&cfg));
    let all = every_intent();
    assert!(
        reached.len() * 2 > all.len(),
        "only {} of {} intents are driven through the daemon by any test",
        reached.len(),
        all.len()
    );
    // And `Unknown` is reached, because "Atlas did not understand" is a
    // branch like any other and the one a person meets most often.
    assert!(
        reached.contains("unknown"),
        "nothing tests what happens when Atlas does not understand you"
    );
    let _ = Intent::Unknown;
}
