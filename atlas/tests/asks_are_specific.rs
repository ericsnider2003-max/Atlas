//! Atlas never asks a generic question (Eric, 21 Oct 2026).
//!
//! "Atlas should ask and think with context. Atlas shouldn't ask and be
//! guessing." A reply that is only "Which one?" or "I didn't catch that."
//! gives the person nothing to answer with: it names no slot, offers no
//! candidates, and does not say what was heard. `understood::resolve` and
//! `understood::ask_with_options` build the specific version from context.
//!
//! This scans every `src/**/*.rs` file for a string literal that is *only* a
//! generic question. A new one fails the test. The few that remain sit in
//! files another session owns and are listed below with a count, exactly, so
//! fixing one forces the number down (a ratchet, not a ceiling).

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

const GENERIC: &[&str] = &[
    "Which one?",
    "which one?",
    "I didn't catch that.",
    "I didn't catch that",
    "Can you clarify?",
    "What do you mean?",
    "Could you be more specific?",
    "I didn't catch that. Go ahead?",
];

const WHY: &str =
    "owned by the reliability session; see docs/HANDOFF_ONE_COMPLETION_PATH_2026-10-21.md";

/// (file under src/, how many generic literals it still holds, why).
const KNOWN: &[(&str, usize, &str)] = &[];

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Literals that are exactly a generic question, one per hit. Comment lines
/// are skipped: the history of a bug may quote the old wording.
fn hits(text: &str) -> usize {
    let mut n = 0;
    for line in text.lines() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        for g in GENERIC {
            let lit = format!("\"{g}\"");
            let mut from = 0;
            while let Some(i) = line[from..].find(&lit) {
                let at = from + i;
                // `\"Which one?\"` inside a longer string is not the literal.
                if !line[..at].ends_with('\\') {
                    n += 1;
                }
                from = at + lit.len();
            }
        }
    }
    n
}

fn found() -> Vec<(String, usize)> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rs_files(&src, &mut files);
    files.sort();
    let mut out = Vec::new();
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else {
            continue;
        };
        let n = hits(&text);
        if n > 0 {
            let rel = f
                .strip_prefix(&src)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            out.push((rel, n));
        }
    }
    out
}

#[test]
fn no_new_question_is_only_a_generic_question() {
    let found = found();
    let mut problems = Vec::new();
    for (file, n) in &found {
        let allowed = KNOWN.iter().find(|k| k.0 == file).map(|k| k.1).unwrap_or(0);
        if *n > allowed {
            problems.push(format!(
                "{file}: {n} generic question literal(s), {allowed} allowed. Build the question \
                 from context instead: `Daemon::resolution_context()` with \
                 `understood::resolve` / `understood::ask_with_options`."
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn the_known_list_is_exact_so_a_fix_lowers_it() {
    let found = found();
    let mut problems = Vec::new();
    for (file, allowed, _why) in KNOWN {
        let n = found
            .iter()
            .find(|f| f.0 == *file)
            .map(|f| f.1)
            .unwrap_or(0);
        if n < *allowed {
            problems.push(format!(
                "{file}: now holds {n}, KNOWN says {allowed}. Lower the number (or remove the row)."
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
    assert!(!WHY.is_empty());
}

#[test]
fn the_scan_sees_a_generic_literal_and_ignores_the_rest() {
    assert_eq!(hits("let a = \"Which one?\";"), 1);
    assert_eq!(
        hits("x(\"I didn't catch that.\", \"What do you mean?\")"),
        2
    );
    // A comment quoting the old wording, a longer string, an escaped quote.
    assert_eq!(hits("// asked \"Which one?\" once"), 0);
    assert_eq!(hits("let a = \"Which one? Say its number.\";"), 0);
    assert_eq!(hits("let a = \"asked \\\"Which one?\\\" here\";"), 0);
}

/// `Daemon::context_question`: "close it" with nothing said before it used to
/// get a bare "Which app?"; with a window in front it names that window.
#[test]
fn context_question_offers_the_window_you_have_open() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![]);
    p.focus_on("notepad.exe", "Draft - Notepad");
    let root = std::env::temp_dir().join("atlas-asks-specific");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let mut d = Daemon::new(
        &c,
        &p,
        None,
        Store::new(root),
        Proactive::new(ProactiveConfig::default()),
    );
    let said = d.turn("close it", 1_000);
    assert!(said.contains("Notepad"), "{said}");
    assert!(said.contains("the one you have open"), "{said}");
    // And with nothing in front it is still the plain question, not worse.
    p.focus_on("", "");
    let mut d2 = Daemon::new(
        &c,
        &p,
        None,
        Store::new(std::env::temp_dir().join("atlas-asks-specific-2")),
        Proactive::new(ProactiveConfig::default()),
    );
    assert_eq!(d2.turn("close it", 2_000), "Which app?");
}
