//! A ratchet over the source for hollow answers.
//!
//! Both bugs found on the first day Atlas ran on real hardware were the same
//! shape, and 2,535 tests missed both:
//!
//! * `fn readings(&self) -> Readings { Readings::default() }` — a stub whose
//!   emptiness read as health.
//! * `other => format!("parsed {other:?} — not wired to an action yet")` — a
//!   path that announced its own incompleteness to a user and to nobody else.
//!
//! Neither is a compile error. Neither fails a unit test, because a unit test
//! asks "did it answer?" and both of them answered. What catches them is
//! asking whether the answer was *made of anything*.
//!
//! This works the way `wiring.rs` does: a named baseline of what is known
//! hollow today, and a test that fails if the count grows. Fixing one means
//! deleting a line from the baseline. Nothing gets added to it without a
//! person deciding to.

use std::collections::BTreeMap;
use std::path::Path;

/// Known hollow spots, deliberately kept. Each one is a promise to come back.
///
/// Format: `module:what`. Remove a line when it is fixed; the ratchet will
/// hold you to it.
const HOLLOW_BASELINE: &[&str] = &[
    // main's fallback prompt, kept on purpose for a machine with no
    // config/tools.yaml. It answers six intents and says so honestly.
    "main:handle catch-all",
];

const CEILING: usize = 1;

fn sources() -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let dir = Path::new("src");
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|x| x.to_str()) == Some("rs") {
                let name = p.file_stem().unwrap().to_string_lossy().to_string();
                if let Ok(text) = std::fs::read_to_string(&p) {
                    out.insert(name, text);
                }
            }
        }
    }
    out
}

/// A line of real code, with comments removed.
///
/// The previous version dropped lines that *started* with `//` and nothing
/// else, which an outside audit was right to call reading the source
/// incorrectly. Three things got through:
///
/// * **Trailing comments.** `let x = 1; // not implemented yet` does not start
///   with `//`, so the comment was scanned as code.
/// * **Block comments.** Everything inside `/* ... */` was scanned, and a
///   line like `_ => "not built yet"` sitting in an explanatory block comment
///   was flagged as a real catch-all arm.
/// * It claimed to exclude "a string in a test" and never looked at strings.
///
/// Strings are deliberately **not** stripped, and that is not an oversight:
/// the original bug was `format!("parsed {other:?} — not wired to an action
/// yet")`. The phrase lives inside a string literal because that is what
/// reaches a person. A comment is discussion; a string is an answer.
fn code_lines(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut in_block = false;
    for (i, raw) in text.lines().enumerate() {
        let mut line = raw.trim().to_string();

        if in_block {
            match line.find("*/") {
                Some(end) => {
                    line = line[end + 2..].trim().to_string();
                    in_block = false;
                }
                None => continue,
            }
        }
        // Opening a block comment: keep whatever came before it.
        while let Some(start) = line.find("/*") {
            match line[start..].find("*/") {
                Some(rel) => {
                    let end = start + rel + 2;
                    line = format!("{} {}", &line[..start], &line[end..]).trim().to_string();
                }
                None => {
                    line = line[..start].trim().to_string();
                    in_block = true;
                    break;
                }
            }
        }
        // A line comment, but only when the `//` is not inside a string.
        if let Some(at) = line_comment_at(&line) {
            line = line[..at].trim().to_string();
        }
        if line.is_empty() || line.starts_with('*') {
            continue;
        }
        out.push((i + 1, line));
    }
    out
}

/// Where a `//` comment starts, ignoring one inside a string literal.
///
/// Without this, a URL or a path in a string ends the line early and the rest
/// of it stops being read at all — which is the opposite failure, and just as
/// quiet.
fn line_comment_at(line: &str) -> Option<usize> {
    let b = line.as_bytes();
    let mut in_str = false;
    let mut escaped = false;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if escaped {
            escaped = false;
        } else if c == b'\\' && in_str {
            escaped = true;
        } else if c == b'"' {
            in_str = !in_str;
        } else if !in_str && c == b'/' && i + 1 < b.len() && b[i + 1] == b'/' {
            return Some(i);
        }
        i += 1;
    }
    None
}

// ================= the readings shape =================

/// Functions in `text` whose whole body is `T::default()`.
///
/// Extracted so it can be tested against fixtures rather than only against
/// the tree. A scanner that is only ever run over source that happens to be
/// clean is a scanner nobody has checked — which is how it went a week unable
/// to see the single-line form of the very bug it was written for.
fn default_stubs_in(module: &str, text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let code = code_lines(text);
    for (idx, (line_no, t)) in code.iter().enumerate() {
        let Some(arrow) = t.find("-> ") else { continue };
        let after = &t[arrow + 3..];
        let ret = after.split('{').next().unwrap_or("").trim();
        if ret.is_empty() || !ret.chars().next().unwrap().is_ascii_uppercase() {
            continue;
        }
        let stub = format!("{ret}::default()");
        if let Some(brace) = after.find('{') {
            let body = after[brace + 1..].trim().trim_end_matches('}').trim();
            if body == stub {
                found.push(format!("{module}:{line_no} {t}"));
                continue;
            }
        }
        if code.get(idx + 1).map(|(_, n)| n.as_str()) == Some(stub.as_str()) {
            found.push(format!("{module}:{line_no} {t}"));
        }
    }
    found
}


#[test]
fn no_function_returns_a_bare_default_of_its_own_measurement_type() {
    // `fn x(&self) -> T { T::default() }` on a type whose job is to carry
    // measurements. This is exactly what `Daemon::readings` was.
    // Both shapes, because this could not catch the one it was written about.
    //
    // The bug in the docstring above is `fn readings(&self) -> Readings {
    // Readings::default() }` — all on one line. This check only ever looked at
    // the *next* line, so the single-line form, which is how such a stub is
    // almost always written, passed straight through. Verified by probe: the
    // multi-line version was flagged and the one-line version was not.
    let mut found = Vec::new();
    for (module, text) in sources() {
        found.extend(default_stubs_in(&module, &text));
    }
    assert!(
        found.is_empty(),
        "these return an empty value of their own type, which reads as healthy \
         and measures nothing — the exact shape of the readings bug:\n  {}",
        found.join("\n  ")
    );
}

// ================= the "not wired" shape =================

#[test]
fn nothing_new_tells_the_user_it_is_not_built() {
    let mut found = Vec::new();
    for (module, text) in sources() {
        for (line_no, l) in code_lines(&text) {
            // Only a *catch-all* arm counts.
            //
            // The first version flagged four lines and every one was correct
            // code: `capability` reports whether a thing is planned, `knowhow`
            // describes Discord's updater stub, `cloudsync` describes a
            // OneDrive placeholder. Those modules say "not built" *about
            // something else*, which is their job.
            //
            // The bug was different in a way that can be checked: it lived in
            // a wildcard arm. `_ =>` and `other =>` mean "everything I did not
            // think about", and telling a person that everything you did not
            // think about is unbuilt is the failure. A named variant saying so
            // is a fact.
            let catch_all = l.starts_with("_ =>")
                || l.starts_with("other =>")
                || l.starts_with("_ if ")
                || l.contains("=> unreachable");
            if !catch_all {
                continue;
            }
            let lower = l.to_lowercase();
            for phrase in atlas::hollow::ADMITS_INCOMPLETE {
                if lower.contains(phrase) {
                    found.push(format!("{module}:{line_no} {}", l.chars().take(70).collect::<String>()));
                    break;
                }
            }
        }
    }
    let known = |f: &String| {
        HOLLOW_BASELINE.iter().any(|b| {
            let m = b.split(':').next().unwrap_or("");
            f.starts_with(&format!("{m}:"))
        })
    };
    let unexpected: Vec<&String> = found.iter().filter(|f| !known(f)).collect();
    assert!(
        unexpected.is_empty(),
        "new places that tell a person something is not built. Either build it \
         or add it to HOLLOW_BASELINE on purpose:\n  {}",
        unexpected.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n  ")
    );
}

#[test]
fn no_internal_type_name_leaks_into_something_a_person_reads() {
    // `parsed {other:?}` printed a Rust enum at a user. Debug-formatting a
    // value into user-facing text is how that happens.
    let mut found = Vec::new();
    for (module, text) in sources() {
        for (line_no, l) in code_lines(&text) {
            if !l.contains("format!(") {
                continue;
            }
            // `{x:?}` inside a format! that is being returned as an answer.
            if l.contains(":?}") && (l.contains("parsed") || l.contains("Intent")) {
                found.push(format!("{module}:{line_no}"));
            }
        }
    }
    let known: Vec<&str> = HOLLOW_BASELINE.iter().map(|b| b.split(':').next().unwrap()).collect();
    let unexpected: Vec<&String> =
        found.iter().filter(|f| !known.iter().any(|m| f.starts_with(&format!("{m}:")))).collect();
    assert!(unexpected.is_empty(), "debug output reaching a person: {unexpected:?}");
}

// ================= the ratchet itself =================

#[test]
fn the_hollow_baseline_only_shrinks() {
    assert!(
        HOLLOW_BASELINE.len() <= CEILING,
        "the baseline grew to {} — lower the ceiling deliberately or fix one",
        HOLLOW_BASELINE.len()
    );
}

#[test]
fn every_baseline_entry_names_a_module_that_exists() {
    let src = sources();
    for b in HOLLOW_BASELINE {
        let m = b.split(':').next().unwrap();
        assert!(src.contains_key(m), "HOLLOW_BASELINE names '{m}', which is not a module");
    }
}

// ================= the runtime judge =================

#[test]
fn the_judge_catches_both_bugs_that_got_through() {
    use atlas::hollow::{judge, judge_readings, Why};
    // Bug one, as it actually appeared on screen.
    let h = judge("what's outstanding", "parsed Outstanding — not wired to an action yet")
        .expect("the exact sentence that shipped should be caught");
    assert_eq!(h.why, Why::SaysSoItself);
    // Bug two.
    let h = judge_readings(&atlas::health::Readings::default())
        .expect("an all-zero reading is an unread instrument");
    assert_eq!(h.why, Why::NothingMeasured);
}

#[test]
fn a_real_answer_is_left_alone() {
    use atlas::hollow::judge;
    assert!(judge("how's the machine", "12 gigabytes free, memory at 44 percent.").is_none());
    assert!(judge("what's outstanding", "Nothing outstanding.").is_none());
}

#[test]
fn silence_counts_as_hollow() {
    use atlas::hollow::{judge, Why};
    assert_eq!(judge("anything", "   ").unwrap().why, Why::Silent);
}

#[test]
fn a_partly_read_machine_names_which_instrument_failed() {
    use atlas::health::Readings;
    use atlas::hollow::{judge_readings, unread_instruments};
    let r = Readings { ram_total_gb: 16.0, ram_used_gb: 8.0, ..Default::default() };
    // This used to assert `judge_readings(&r).is_none()` — "not wholly hollow,
    // something was read". That was the bug written down as a rule. It is
    // exactly the case that shipped: off Windows `read_disk` was an empty stub
    // while memory read fine, so disk reported zero forever and the detector
    // stayed quiet because the machine was only half broken. Half broken is
    // the normal way things break.
    let found = judge_readings(&r).expect("a machine with no disk reading was called healthy");
    assert!(found.answer.contains("disk"), "it didn't name which one: {}", found.answer);
    // And it is named rather than glossed.
    assert_eq!(unread_instruments(&r), vec!["disk"]);
}

#[test]
fn the_audit_asks_only_questions_that_change_nothing() {
    use atlas::hollow::SELF_QUESTIONS;
    // A self-audit with side effects is one nobody dares run on a schedule.
    for q in SELF_QUESTIONS {
        let l = q.to_lowercase();
        for verb in ["open", "close", "send", "post", "delete", "boot", "run", "buy"] {
            assert!(!l.contains(verb), "'{q}' does something — an audit must not");
        }
    }
}

#[test]
fn the_audit_reports_what_it_found_rather_than_a_count() {
    use atlas::hollow::{audit, spoken};
    let found = audit(|q| match q {
        "how's the machine" => "not built yet".into(),
        _ => "a real answer".into(),
    });
    assert_eq!(found.len(), 1);
    let said = spoken(&found, &atlas::judgment::JudgmentConfig::default());
    assert!(said.contains("how's the machine"), "{said}");
    assert!(said.contains("isn't wired up"), "{said}");
}

#[test]
fn a_clean_audit_says_so_without_hedging() {
    use atlas::hollow::{audit, spoken};
    let found = audit(|_| "a real answer".into());
    assert!(found.is_empty());
    assert_eq!(
        spoken(&found, &atlas::judgment::JudgmentConfig::default()),
        "Everything I asked myself came back with a real answer."
    );
}

// --- the detector's own real-world audit ------------------------------------
//
// Eric had an outside audit run against `hollow` and reported it failing in
// real situations. Reproduced here as a fixed table of answers Atlas actually
// produces, so the detector is measured rather than assumed. Six of these
// fourteen passed undetected before the ZeroDressedAsFine and NullAsAnAnswer
// checks existed — and every miss had the same shape: fluent, confident, and
// made entirely of zeros.

#[test]
fn the_detector_catches_real_hollow_answers_and_leaves_real_ones_alone() {
    use atlas::hollow::judge;
    let cases: &[(&str, &str, bool)] = &[
        ("how's the machine", "All fine. 0 gigabytes free, memory at 0 percent.", true),
        ("how's the machine", "All fine. 300 gigabytes free, memory at 50 percent.", false),
        ("what's outstanding", "Nothing outstanding.", false),
        ("what can you do", "", true),
        ("research tides", "Research isn't built yet, so I can't look into tides.", true),
        ("what did you do today", "You did nothing today.", false),
        ("are the connections ok", "Everything's connected.", true),
        ("what's queued", "0 scheduled, 0 awaiting you, 0 queued.", true),
        ("summarise my notes", "I can't find anything about that.", false),
        ("what's my disk", "Disk: 0 GB free of 0 GB.", true),
        ("run the checks", "Ran 0 checks.", true),
        ("who is at the desk", "Unknown", true),
        ("what's my calendar", "You have no events. (I couldn't reach the calendar.)", false),
    ];
    let mut wrong = Vec::new();
    for (q, a, should) in cases {
        let got = judge(q, a).is_some();
        if got != *should {
            wrong.push(format!(
                "{q}: expected {}, got {} — {a:?}",
                if *should { "hollow" } else { "fine" },
                if got { "hollow" } else { "fine" }
            ));
        }
    }
    assert!(wrong.is_empty(), "the detector got these wrong:\n  {}", wrong.join("\n  "));
}

#[test]
fn a_correct_answer_of_zero_is_not_flagged() {
    // The crying-wolf failure this module explicitly avoids. "Nothing
    // outstanding" is a true answer whose real value is zero, and flagging it
    // would get the whole audit switched off.
    use atlas::hollow::judge;
    for good in [
        "Nothing outstanding.",
        "You did nothing today.",
        "0 messages waiting.",
        "No events tomorrow.",
    ] {
        assert!(judge("q", good).is_none(), "flagged a correct answer: {good}");
    }
}

#[test]
fn a_half_read_machine_is_caught_not_only_a_completely_unread_one() {
    // `judge_readings` required *every* field to be zero, which made it blind
    // to what actually shipped: off Windows `read_disk` was an empty stub
    // while memory read fine. A detector that only fires when everything is
    // broken cannot see a system that is half broken.
    use atlas::health::Readings;
    use atlas::hollow::judge_readings;
    let half = Readings { ram_total_gb: 16.0, ram_used_gb: 8.0, ..Default::default() };
    let found = judge_readings(&half).expect("a machine with no disk reading was called healthy");
    assert!(found.answer.contains("disk"), "it didn't name which one: {}", found.answer);

    let whole = Readings {
        ram_total_gb: 16.0,
        ram_used_gb: 8.0,
        disk_total_gb: 500.0,
        disk_free_gb: 200.0,
        ..Default::default()
    };
    assert!(judge_readings(&whole).is_none(), "a fully read machine was flagged");
}

// ================= the scanner, checked against fixtures =================
//
// An outside audit reported this file reading the source incorrectly. It was
// right, in three ways, and none of them would ever surface by running it over
// a tree that happens to be clean. These fixtures are the check.

#[test]
fn a_one_line_default_stub_is_caught() {
    // The exact bug in this file's own docstring, written the way anybody
    // would actually write it. It went undetected because the scanner only
    // ever looked at the *next* line.
    let text = "pub fn readings(&self) -> Readings { Readings::default() }\n";
    let found = default_stubs_in("probe", text);
    assert_eq!(found.len(), 1, "the single-line form was missed: {found:?}");
}

#[test]
fn a_multi_line_default_stub_is_still_caught() {
    let text = "fn other(&self) -> Readings {\n    Readings::default()\n}\n";
    assert_eq!(default_stubs_in("probe", text).len(), 1);
}

#[test]
fn a_function_that_actually_measures_something_is_not_flagged() {
    // The crying-wolf direction. A real body must not look like a stub.
    let text = "fn real(&self) -> Readings {\n    let mut r = Readings::default();\n    read_memory(&mut r);\n    r\n}\n";
    assert!(default_stubs_in("probe", text).is_empty(), "flagged a real reader");
}

#[test]
fn a_trailing_comment_is_not_read_as_code() {
    // `let x = 1; // not implemented yet` does not start with `//`, so the
    // comment was being scanned.
    let code = code_lines("let x = 1; // this is not implemented yet\n");
    assert_eq!(code.len(), 1);
    assert_eq!(code[0].1, "let x = 1;", "the comment survived: {:?}", code[0].1);
}

#[test]
fn a_block_comment_is_not_read_as_code() {
    // A catch-all arm quoted inside an explanatory block comment was being
    // flagged as a real one. This file is full of such comments.
    let text = "let a = 1;\n/* explaining:\n_ => \"not built yet\"\n*/\nlet b = 2;\n";
    let code = code_lines(text);
    let joined: Vec<&str> = code.iter().map(|(_, l)| l.as_str()).collect();
    assert!(!joined.iter().any(|l| l.contains("not built yet")), "got: {joined:?}");
    assert_eq!(joined, vec!["let a = 1;", "let b = 2;"]);
}

#[test]
fn a_slash_inside_a_string_does_not_end_the_line_early() {
    // The opposite failure, and just as quiet: treating the `//` in a URL as
    // a comment truncates the line and stops the rest being read at all.
    let code = code_lines("let u = \"https://example.com\"; let v = 2;\n");
    assert_eq!(code[0].1, "let u = \"https://example.com\"; let v = 2;");
}

#[test]
fn a_phrase_inside_a_string_is_still_read() {
    // Strings are deliberately not stripped. The original bug lived in one:
    // `format!("parsed {other:?} — not wired to an action yet")`. A comment is
    // discussion; a string is what reaches a person.
    let code = code_lines("_ => \"not wired to an action yet\".into(),\n");
    assert_eq!(code.len(), 1);
    assert!(code[0].1.contains("not wired to an action"));
}

#[test]
fn the_scanner_only_reads_rust() {
    // Stated plainly because it has been asked. `sources()` filters on the
    // `.rs` extension, so this ratchet covers Atlas's own Rust and nothing
    // else. Pointed at a Python or C++ tree it would read zero files and pass
    // silently — which would look exactly like a clean bill of health.
    //
    // `craft.rs` is the part that knows other languages, and it runs their
    // toolchains rather than reading them. If this ratchet is ever wanted for
    // another language, that is a real piece of work and not a config change.
    let found = sources();
    assert!(!found.is_empty(), "no source was read at all");
    assert!(found.contains_key("hollow"), "the tree it is pointed at is Atlas's own src/");
}
