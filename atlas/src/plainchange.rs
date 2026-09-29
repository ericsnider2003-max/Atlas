//! Explaining a change without showing you code.
//!
//! Atlas working on itself is worth nothing if the only way to check it is to
//! read a diff. You need to know **what will be different**, not what lines
//! moved.
//!
//! There is a good hook for this in how the tests are written. Every test in
//! this project is named as a sentence about behaviour —
//! `a_dangling_word_means_you_are_not_finished` — so the tests that were added
//! or removed describe the change better than the code does. That's what this
//! reads.

use serde::{Deserialize, Serialize};

/// What a change does, in the terms you care about.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Effect {
    /// One sentence: what will be different.
    pub headline: String,
    /// New behaviour, in plain language.
    pub now_does: Vec<String>,
    /// Behaviour that has gone.
    pub no_longer: Vec<String>,
    /// What it touched, named the way you'd name it.
    pub areas: Vec<String>,
    /// Anything worth being careful about.
    pub watch_out: Vec<String>,
    /// How sure Atlas is that this is the whole story.
    pub certain: bool,
}

/// Turn a test name into a sentence.
///
/// `a_dangling_word_means_you_are_not_finished` becomes
/// "a dangling word means you are not finished".
pub fn sentence_from_test(name: &str) -> String {
    let words = name.trim_start_matches("test_").replace('_', " ");
    let mut c = words.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => words,
    }
}

/// Which part of Atlas a file belongs to, in words you'd use.
pub fn area_of(path: &str) -> &'static str {
    let p = path.to_lowercase();
    let name = p.rsplit('/').next().unwrap_or(&p);
    match name.trim_end_matches(".rs") {
        "voice" | "tts" | "endpoint" | "hearing" | "audio" | "language" => "how it hears and speaks",
        "persona" | "register" | "brain" | "certainty" => "how it talks to you",
        "panel" | "look" | "overlay" | "hub" | "mind" => "what you see",
        "workspace" | "layout" | "platform" | "win" => "arranging your windows",
        "publish" | "delivery" | "browser" | "cdp" => "posting and the browser",
        "finance" | "ledger" => "money",
        "policy" | "grants" | "identity" | "consent" => "permissions",
        "index" | "recall" | "research" => "finding things",
        "backup" | "safety" | "retention" | "store" => "looking after your data",
        "scheduler" | "lanes" | "overnight" | "anticipate" => "when things happen",
        "sandbox" | "selfwork" | "strategy" | "handoff" => "how it works on itself",
        _ => "the general workings",
    }
}

/// What changed, mechanically.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Diff {
    pub files: Vec<String>,
    /// Test names added.
    pub tests_added: Vec<String>,
    /// Test names removed.
    pub tests_removed: Vec<String>,
    /// Test names whose expected value changed.
    pub tests_changed: Vec<String>,
    pub lines_added: usize,
    pub lines_removed: usize,
    /// Config keys added or changed, which are the settings you'd notice.
    pub settings_touched: Vec<String>,
}

/// Build a behavioural diff from the file contents before and after a change —
/// the shape a staged self-fix or a queued edit hands over.
///
/// It reads test names, because in this tree a test name *is* a sentence about
/// behaviour: the tests a change adds are what it now does, the ones it removes
/// are what it no longer promises. Everything else here — the files it touched,
/// roughly how many lines moved — is mechanical. It does not read the code's
/// meaning; that is the whole point of pairing it with the test names, which
/// already carry it.
pub fn diff_of(before: &[(String, String)], after: &[(String, String)]) -> Diff {
    use std::collections::BTreeSet;
    let tests_in = |files: &[(String, String)]| -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for (_path, content) in files {
            for name in test_names(content) {
                out.insert(name);
            }
        }
        out
    };
    let before_tests = tests_in(before);
    let after_tests = tests_in(after);
    let tests_added: Vec<String> = after_tests.difference(&before_tests).cloned().collect();
    let tests_removed: Vec<String> = before_tests.difference(&after_tests).cloned().collect();

    // A plain line count, enough to tell a one-line fix from a rewrite. Lines
    // present in the after and not the before count as added, and the reverse
    // as removed — the same symmetric measure `selfwork::lines_touched` uses.
    let mut lines_added = 0;
    let mut lines_removed = 0;
    for (path, after_content) in after {
        let before_content =
            before.iter().find(|(p, _)| p == path).map(|(_, c)| c.as_str()).unwrap_or("");
        let before_lines: Vec<&str> = before_content.lines().collect();
        let after_lines: Vec<&str> = after_content.lines().collect();
        lines_added += after_lines.iter().filter(|l| !before_lines.contains(l)).count();
        lines_removed += before_lines.iter().filter(|l| !after_lines.contains(l)).count();
    }

    let files: Vec<String> = after.iter().map(|(p, _)| p.clone()).collect();

    Diff {
        files,
        tests_added,
        tests_removed,
        tests_changed: Vec::new(),
        lines_added,
        lines_removed,
        settings_touched: Vec::new(),
    }
}

/// The names of the `#[test]` functions in a chunk of Rust source.
///
/// Deliberately not a parser: it finds a `#[test]` attribute and reads the
/// identifier of the next `fn`. Where it can't be sure it skips, the same bias
/// the rest of the tree takes — a missed test name understates the change, an
/// invented one would misdescribe it, and the first is the safer error.
fn test_names(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(rel) = content[i..].find("#[test]") {
        let at = i + rel;
        i = at + "#[test]".len();
        // The next `fn <name>` after the attribute.
        if let Some(frel) = content[i..].find("fn ") {
            let name_start = i + frel + 3;
            let name: String = content[name_start..]
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                out.push(name);
            }
        }
    }
    out
}

/// Read a change the way a person would want it read.
pub fn explain(d: &Diff, goal: &str) -> Effect {
    let now_does: Vec<String> = d.tests_added.iter().map(|t| sentence_from_test(t)).collect();
    let no_longer: Vec<String> = d.tests_removed.iter().map(|t| sentence_from_test(t)).collect();

    let mut areas: Vec<String> = d.files.iter().map(|f| area_of(f).to_string()).collect();
    areas.sort();
    areas.dedup();

    let mut watch_out = Vec::new();
    // A removed test is the one thing worth stopping on. It means something
    // Atlas used to guarantee, it no longer does.
    if !d.tests_removed.is_empty() {
        watch_out.push(format!(
            "{} thing{} Atlas used to promise it no longer does",
            d.tests_removed.len(),
            if d.tests_removed.len() == 1 { "" } else { "s" }
        ));
    }
    if !d.tests_changed.is_empty() {
        watch_out.push(format!(
            "{} thing{} now behaves differently than before",
            d.tests_changed.len(),
            if d.tests_changed.len() == 1 { "" } else { "s" }
        ));
    }
    if d.lines_added + d.lines_removed > 300 {
        watch_out.push("this is a big change to read in one go".into());
    }
    if !d.settings_touched.is_empty() {
        watch_out.push(format!("new settings: {}", d.settings_touched.join(", ")));
    }

    // Without any test changes there is nothing to describe behaviourally —
    // and Atlas should say so rather than invent a summary.
    let certain = !d.tests_added.is_empty() || !d.tests_removed.is_empty();

    let headline = if !now_does.is_empty() {
        format!("{goal}. The difference: {}", now_does[0].to_lowercase())
    } else if !areas.is_empty() {
        format!("{goal}. It touched {}, but nothing about the behaviour is testably different.", areas.join(" and "))
    } else {
        goal.to_string()
    };

    Effect { headline, now_does, no_longer, areas, watch_out, certain }
}

/// The spoken version. Short — this is the bit you hear before deciding.
pub fn spoken(e: &Effect) -> String {
    let mut s = e.headline.clone();
    if !s.ends_with('.') {
        s.push('.');
    }
    if e.now_does.len() > 1 {
        s.push_str(&format!(" And {} other change{}.", e.now_does.len() - 1,
            if e.now_does.len() == 2 { "" } else { "s" }));
    }
    if let Some(first) = e.watch_out.first() {
        s.push_str(&format!(" Worth knowing: {first}."));
    }
    if !e.certain {
        s.push_str(" I can't tell you exactly what changed about the behaviour, so read it carefully or leave it.");
    }
    s
}

/// The fuller version, still with no code in it.
pub fn written(e: &Effect) -> String {
    let mut s = format!("{}\n\n", e.headline);

    if !e.now_does.is_empty() {
        s.push_str("It will now:\n");
        for n in &e.now_does {
            s.push_str(&format!("  · {n}\n"));
        }
        s.push('\n');
    }
    if !e.no_longer.is_empty() {
        s.push_str("It will no longer:\n");
        for n in &e.no_longer {
            s.push_str(&format!("  · {n}\n"));
        }
        s.push('\n');
    }
    if !e.areas.is_empty() {
        s.push_str(&format!("Affects: {}\n\n", e.areas.join(", ")));
    }
    if !e.watch_out.is_empty() {
        s.push_str("Worth knowing:\n");
        for w in &e.watch_out {
            s.push_str(&format!("  · {w}\n"));
        }
    }
    s.push_str("\nSay yes to keep it, or no and I'll throw it away.\n");
    s
}

/// The question Atlas asks. Never "apply the diff?".
pub fn ask(e: &Effect) -> String {
    if e.watch_out.is_empty() {
        "Want me to keep that?".into()
    } else {
        "Want me to keep that, given the above?".into()
    }
}
