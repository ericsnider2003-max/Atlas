//! Whether a module's **capability** is reached, not whether the module is.
//!
//! ## Why there are three of these and not one
//!
//! Three files keep a "not wired yet" list and they check three different
//! things: `wiring.rs` asks whether anything *names* the module, this asks
//! whether what the module is *for* is reached, and
//! `new_capabilities_are_wired.rs` names individual functions. The split is
//! deliberate and load-bearing -- the check below skips a module with no
//! public types precisely because `wiring.rs` already covers it.
//!
//! Collapsing them into one list would either over-exempt (a module excused
//! from the strict check gets excused from the weak one too) or need a scope
//! marker per entry, which is three lists wearing one name.
//!
//! What *was* wrong is that wiring one module means deleting from more than
//! one list, and each deletion was discovered a full run apart. Every failure
//! message now names the other two. An attempt to compute all three answers in
//! one place was made and deleted -- see the note in `tests/ceiling.rs`: a
//! cheaper copy of the rule got a different answer on its first run.
//!
//! `tests/wiring.rs` asks: does anything outside this module name it? That
//! caught a lot and then it gave a false green on the oldest open request in
//! the project.
//!
//! `voiceid.rs` exists to tell Eric's voice from anyone else's. Its whole
//! public surface for that — `Verdict`, `Voiceprint`, the comparison — has
//! never been called by anything. It came off the unwired list anyway, because
//! `recall.rs` calls `voiceid::cosine` to compare two note embeddings. A
//! vector-maths helper borrowed for something unrelated made the speaker
//! identification count as wired.
//!
//! That is the exact shape `hollow` exists to name, one level up: reachable,
//! tested, and does nothing. And it is worse than an ordinary hollow, because
//! a guard reported it as fine.
//!
//! ## The rule
//!
//! A module's capability lives in its **types**, and in the functions that
//! deal in them. A function taking and returning nothing but primitives is a
//! helper anyone can borrow — `cosine(&[f32], &[f32]) -> f32` is arithmetic
//! that happens to live in `voiceid`. A function that takes or returns
//! `Verdict` is voice identification.
//!
//! > A module is capability-wired if something outside it either names one of
//! > its public types, or calls a function whose signature deals in them.
//!
//! The second half matters because Rust infers types: `asking::prepare` is
//! genuinely wired and its `Prepared` is never written down at the call site.
//! Requiring the name would flag working code, and a guard that cries wolf
//! gets switched off — which is worse than the false green it replaced.
//!
//! Config types are ignored deliberately. Every module's `Config` is named in
//! `voice.rs` whether or not the module does anything, so counting those would
//! reproduce the same false green one layer down — which is how the first
//! guard came to be fooled in the first place.
//!
//! ## What this does not claim
//!
//! It does not prove a capability runs. Nothing static can. It closes the one
//! specific gap that let a whole feature pass as finished, and it is a
//! ratchet: the list only shrinks.

mod common;

use std::collections::BTreeSet;
use std::fs;

/// Modules whose capability is genuinely not reached yet.
///
/// Not a to-do list — a record of what is honestly unfinished, held so it can
/// only get shorter. Anything wired must be deleted from here, which is what
/// makes the ratchet bite.
const CAPABILITY_UNWIRED: &[&str] = &[
    // --- 18 Sep: mesh, exposed by wiring sync -------------------------------
    //
    // `mesh` is about reaching your laptop *directly* from another network --
    // a private network like Tailscale -- as opposed to leaving a file
    // somewhere for the other side to pick up later.
    //
    // It looked wired because `Intent::Sync` called `mesh::choose`. That call
    // passed four hardcoded literals rather than observations, and threw the
    // answer away; the daemon's own comment said so. When sync was given a
    // real carrier (a folder both machines can see) that call went, and what
    // was left is the truth: nothing in this tree can reach another device
    // directly, and `Path::SameNetwork`, `Path::Mesh` and `Path::Cable` have
    // never been reachable.
    //
    // Listed rather than quietly re-decorated. The folder carrier covers the
    // relay case; direct is a different transport and is not built.

    // `look`'s MODULE is now reachable (its palette and catenary constants are
    // the design spec that `look_paint` implements and pins itself against, so
    // it is off UNWIRED_BASELINE in wiring.rs). But `look`'s own CAPABILITY —
    // rendering the panels as HTML — is deliberately NOT wired: an HTML panel
    // needs an external browser or a bundled web engine, and the ruling for
    // this system is in-house and self-contained, so the design is painted
    // natively by `look_paint`/`window` instead. The two guards check
    // different things and the honest answer differs between them.
    //
    // Corrected 17 Sep: this note used to say `look` "stays here as the
    // superseded HTML renderer, kept as the spec rather than deleted." It is
    // not that any more -- the nine HTML functions were deleted and the file
    // is 77 lines holding `TOKENS`, `TRACE_LINES` and `resting_sag`, which
    // `look_paint` implements and pins itself against. So what is unwired is
    // narrower than the note claimed: a palette and a catenary constant that
    // only a test reads directly. A stale comment on a guard list is the one
    // kind of rot these lists cannot catch themselves, which is why it was
    // carried as a standing correction until now.
    "look",
    // `grading` and `messaging` added 17 Sep, and the two moved for one
    // reason. The daemon called `grading::spoken(&[])` and
    // `messaging::spoken(&[], ..)` -- each module's actual capability -- and
    // each returns a confident nothing on empty input ("Grade looks clean.",
    // "0 messages, all group chat"). So turning either tool ON changed the
    // answer from an honest refusal into a verdict about something nothing
    // had read. The 17 Sep merge replaced both calls with the refusal, which
    // is right, and which left both modules capability-unwired.
    //
    // Neither can honestly be re-wired until there is a reader: nothing in
    // this tree produces a grade node-tree, and `imap.rs` reads mail, not
    // chat. Their helpers are still borrowed elsewhere -- and that is exactly
    // what this guard exists to refuse to count as wiring.
    // plainchange came off 21 Sep 2026. The self-improvement loop stages a
    // candidate and the reply now leads with its behaviour view — what the
    // change will do and no longer promises, read from the tests it adds and
    // drops (`plainchange::diff_of` → `explain` → `spoken`), stored so "what
    // will that change do?" gives the fuller `written`/`ask` form. The daemon
    // now deals in `Diff` and `Effect`, so the capability is reached, not just
    // the module.
    // afterme was wired 19 Sep 2026 -- `atlas afterme` keeps the arrangement
    // and the daemon reads `review_every_days`. See UNWIRED_BASELINE.
    // confirmed was wired 19 Sep 2026. It was written for Atlas making a
    // security change itself, which nothing here does -- but the read-back is
    // the half that does the work ("turn it off on Instagram" and "on
    // Instagram and Facebook" sound alike, and that page changes both), and
    // it is worth the same when you are the one about to click. `atlas
    // walkthrough turn-off` is the one place in this tree where a security
    // change gets started, and it started with no read-back and no yes.
    // enrol was wired 20 Sep 2026: the daemon's CreateAccount branch now
    // goes through `Enrolment::permitted` -- one gate, not a hand-rolled
    // subset that skipped your own `never_on` list. The state machine
    // itself (step/resume/vault_write) still waits on the confirmation
    // gate its own canned reply promises, which is a ruling, not wiring.
    // overnight was wired 18 Sep 2026: `Daemon::run_the_night` advances the
    // night a step an hour inside the window, works the backlog under the
    // `ask_you_later` brain, and holds the brief until you are actually up.
    // See tests/the_night_actually_runs.rs.
    // sync was wired 18 Sep 2026: captures append to a `sync::Log`, and
    // `Intent::Sync` writes a bundle into a folder both machines can see and
    // takes in whatever the other side left, using `already_seen` so a
    // folder read twice does not double anything. See
    // tests/two_atlases_one_folder.rs.
];

/// Modules exempt from the rule, with the reason.
///
/// Only two shapes qualify, and "it hasn't been wired yet" is neither of them.
const NOT_A_CAPABILITY: &[(&str, &str)] = &[
    ("lib", "the crate root"),
    ("main", "the entry point"),
    ("mobile", "the phones' C entry points (atlas_mobile_*), called from Swift and Kotlin, which this scan of Rust sources cannot see"),
    ("error", "the error type every module returns"),
    ("store", "how everything persists; used by name everywhere"),
    ("voice", "the config tree itself"),
];

/// Every module, and the program half of each.
///
/// Two corrections on 17 Sep, both of them about what this guard could see:
///
/// * `read_dir("src")` does not recurse, so `src/market/` and `src/platform/`
///   -- 19 files, 168 public functions -- were outside this guard entirely.
///   `market` is where the whole trading side lives.
/// * `#[cfg(test)]` blocks were read as the program, so a capability reached
///   only by its own module's unit test counted as wired. That is the same
///   correction `dead_capabilities.rs`, `new_capabilities_are_wired.rs` and
///   `name_collisions.rs` took today; this file is the last of the set.
///
/// This guard trims before matching `pub fn` and always has, so unlike the
/// other three it never had the column-zero blind spot.
fn source_files() -> Vec<(String, String)> {
    fn walk(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        let mut paths: Vec<_> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        paths.sort();
        for p in paths {
            if p.is_dir() {
                walk(&p, out);
                continue;
            }
            if p.extension().is_some_and(|x| x == "rs") {
                if let (Some(stem), Ok(text)) =
                    (p.file_stem(), fs::read_to_string(&p))
                {
                    // A `mod.rs` *is* its directory, in Rust and in every
                    // other guard here. Named by its own stem it comes out as
                    // "mod", which is not a module anyone can look up and
                    // collides between `market/` and `platform/`.
                    let name = if stem == "mod" {
                        p.parent()
                            .and_then(|d| d.file_name())
                            .map(|d| d.to_string_lossy().to_string())
                            .unwrap_or_else(|| stem.to_string_lossy().to_string())
                    } else {
                        stem.to_string_lossy().to_string()
                    };
                    // A split module's pieces are one module (27 Sep 2026).
                    common::push_module(out, &p, name, common::split_production_and_tests(&text).0);
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(std::path::Path::new("src"), &mut out);
    out
}

/// The public types a module declares, minus its config.
///
/// A type is what a module is *for*. `Verdict` is voice identification;
/// `cosine` is arithmetic that happens to live there.
fn public_types(src: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in src.lines() {
        let line = line.trim();
        for kind in ["pub struct ", "pub enum ", "pub trait "] {
            let Some(rest) = line.strip_prefix(kind) else {
                continue;
            };
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if name.is_empty() {
                continue;
            }
            // Config is named in `voice.rs` whether or not the module does
            // anything. Counting it reproduces the same false green one layer
            // down.
            if name.ends_with("Config") {
                continue;
            }
            out.insert(name);
        }
    }
    out
}

/// The public functions whose signatures deal in the module's own types.
///
/// These are the capability; everything else is a helper. The distinction is
/// the whole point: `cosine` moves floats about, `prepare` returns a
/// `Prepared`.
fn capability_fns(src: &str, types: &BTreeSet<String>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let lines: Vec<&str> = src.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        let Some(rest) = line.strip_prefix("pub fn ") else {
            i += 1;
            continue;
        };
        let name: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if name.is_empty() {
            i += 1;
            continue;
        }
        // The type names this function deals in can live anywhere between
        // `pub fn name(` and the opening `{` of its body -- a parameter list
        // or a return type is free to span several lines, and plenty in this
        // codebase do. Checking only the `pub fn` line itself is how a
        // function with a six-line parameter list (its types all past the
        // first line) read as a signature dealing in nothing but its own
        // name, and reported as a helper.
        let mut signature = String::from(rest);
        let mut j = i;
        while !signature.contains('{') && j + 1 < lines.len() && j < i + 20 {
            j += 1;
            signature.push(' ');
            signature.push_str(lines[j].trim());
        }
        if types.iter().any(|t| mentions(&signature, t)) {
            out.insert(name);
        }
        i += 1;
    }
    out
}

/// Is this function called from outside its own module?
///
/// The `src.contains(&call)` before the line walk is a prefilter, not a second
/// rule: a file whose whole text does not contain `home::f(` cannot have a
/// line that does. **This guard cost 41.6s of every verification run** because
/// both predicates here re-read every line of every file once per candidate,
/// and most files never mention most modules. Skipping those files whole took
/// it to under a second without changing a single answer -- the baselines are
/// the proof, since any semantic drift would move them.
fn called_outside(f: &str, home: &str, files: &[(String, String)]) -> bool {
    let call = format!("{home}::{f}(");
    files.iter().any(|(name, src)| {
        name != home
            && src.contains(&call)
            && src.lines().any(|l| {
                let l = l.trim();
                !l.starts_with("//") && l.contains(&call)
            })
    })
}

/// Does this line name exactly this word?
///
/// Substring matching is how this file nearly repeated the bug it was written
/// for: `VoiceId` matches inside `VoiceIdConfig`, so `voiceid` passed on the
/// strength of a config field that every module has whether or not it does
/// anything.
fn mentions(line: &str, word: &str) -> bool {
    let bytes = line.as_bytes();
    let mut from = 0;
    while let Some(at) = line[from..].find(word) {
        let start = from + at;
        let end = start + word.len();
        let before_ok = start == 0 || !is_word_byte(bytes[start - 1]);
        let after_ok = end >= bytes.len() || !is_word_byte(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Is this type named anywhere outside its own module?
fn named_outside(ty: &str, home: &str, files: &[(String, String)]) -> bool {
    let path = format!("{home}::");
    let import = format!("use crate::{home}");
    files.iter().any(|(name, src)| {
        if name == home {
            return false;
        }
        // Same prefilter as `called_outside`: `names_module` below requires one
        // of these two strings on the line, so a file containing neither
        // anywhere cannot produce a match. See that function for the timing.
        if !src.contains(&path) && !src.contains(&import) {
            return false;
        }
        src.lines().any(|l| {
            let l = l.trim();
            if l.starts_with("//") || l.starts_with("///") {
                return false;
            }
            // `crate::voiceid::Verdict`, or `use crate::voiceid::{Verdict}`,
            // or a bare `Verdict` after importing it.
            // The line has to name *this* module. An earlier version also
            // accepted any line containing `::Ty`, which meant
            // `activity::Kind` counted as a use of `infer::Kind` and
            // `recovery::Setup` as a use of `handloop::Setup`. Both modules
            // passed as wired while nothing referenced either — the same
            // false green this whole file was written to remove, reproduced
            // inside it.
            let names_module = l.contains(&format!("{home}::"))
                || l.contains(&format!("use crate::{home}"));
            names_module && (mentions(l, ty) || l.trim_end().ends_with("::*;"))
        })
    })
}

#[test]
fn every_modules_capability_is_reached_by_something() {
    let files = source_files();
    let exempt: BTreeSet<&str> = NOT_A_CAPABILITY.iter().map(|(m, _)| *m).collect();
    let known: BTreeSet<&str> = CAPABILITY_UNWIRED.iter().copied().collect();

    let mut unreached = Vec::new();
    for (name, src) in &files {
        if exempt.contains(name.as_str()) || known.contains(name.as_str()) {
            continue;
        }
        let types = public_types(src);
        // A module with no public types at all is a bag of functions —
        // `wiring.rs` already covers whether anything calls it.
        if types.is_empty() {
            continue;
        }
        let named = types.iter().any(|t| named_outside(t, name, &files));
        let used = capability_fns(src, &types)
            .iter()
            .any(|f| called_outside(f, name, &files));
        if !named && !used {
            unreached.push(name.clone());
        }
    }

    assert!(
        unreached.is_empty(),
        "these modules are reachable but none of what they are FOR is used \
         anywhere — a helper being borrowed is not the capability being \
         wired:\n  {}",
        unreached.join("\n  ")
    );
}

#[test]
fn the_known_list_only_shrinks() {
    let files = source_files();
    let mut wired_now = Vec::new();
    for name in CAPABILITY_UNWIRED {
        let Some((_, src)) = files.iter().find(|(n, _)| n == name) else {
            continue;
        };
        let types = public_types(src);
        let named = types.iter().any(|t| named_outside(t, name, &files));
        let used = capability_fns(src, &types)
            .iter()
            .any(|f| called_outside(f, name, &files));
        if named || used {
            wired_now.push(*name);
        }
    }
    assert!(
        wired_now.is_empty(),
        "these are wired in now — delete them from CAPABILITY_UNWIRED so the \
         ratchet keeps holding: {wired_now:?}\n\n\
         Check the same names in UNWIRED_BASELINE (tests/wiring.rs) and KNOWN \
         (tests/new_capabilities_are_wired.rs) in the SAME change."
    );
}

#[test]
fn every_exemption_gives_a_reason() {
    // An exemption list without reasons becomes the place things go to stop
    // being checked.
    for (module, why) in NOT_A_CAPABILITY {
        assert!(
            why.split_whitespace().count() >= 3,
            "{module} is exempt with no real reason: {why:?}"
        );
        assert!(
            !why.contains("not wired") && !why.contains("yet"),
            "{module}'s exemption is really a to-do: {why:?}"
        );
    }
}

/// The case that started this.
///
/// Pinned by name rather than left to the general rule, because the general
/// rule is new and this is the one failure it was written for. If someone
/// widens the rule until this passes for the wrong reason, this fails.
#[test]
fn a_borrowed_helper_does_not_count_as_the_capability() {
    let files = source_files();
    let voiceid = files
        .iter()
        .find(|(n, _)| n == "voiceid")
        .map(|(_, s)| s.as_str());
    let Some(src) = voiceid else {
        return; // module removed; nothing to pin
    };
    let types = public_types(src);
    assert!(
        types.contains("Verdict"),
        "voiceid's capability is its Verdict; if that has been renamed this \
         test needs updating rather than deleting"
    );

    // `cosine` is called by recall for note similarity. That is a real call
    // and it is not speaker identification.
    let recall_borrows = files
        .iter()
        .any(|(n, s)| n == "recall" && s.contains("voiceid::cosine"));
    if recall_borrows {
        let types = public_types(src);
        let capability_used = capability_fns(src, &types)
            .iter()
            .any(|f| called_outside(f, "voiceid", &files));
        assert!(
            named_outside("Verdict", "voiceid", &files) || capability_used,
            "recall borrows voiceid::cosine for note similarity, which made \
             the whole module count as wired while speaker identification had \
             no caller anywhere. That is the false green this file exists for."
        );
    }
}
