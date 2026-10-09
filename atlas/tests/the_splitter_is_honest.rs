//! The production/test splitter, checked against every file it will be used on.
//!
//! `common::split_production_and_tests` decides what counts as the program and
//! what counts as its own inline tests. Every number in
//! `dead_capabilities.rs` and `new_capabilities_are_wired.rs` now rests on it,
//! so it gets checked rather than trusted.
//!
//! The risk being guarded against is specific. The splitter uses a simple
//! textual rule — `#[cfg(test)]` at column zero, closing `}` at column zero —
//! chosen over a brace matcher because Rust needs a real lexer to count braces
//! safely and a subtly wrong lexer mis-slices a file in silence. A simple rule
//! is only safe while it is *true of this tree*, which is what
//! `every_inline_test_block_has_the_shape_the_splitter_assumes` asserts, on
//! all 36 files, every run.
//!
//! If someone writes an indented `#[cfg(test)]`, or a block that closes on an
//! indented brace, that test fails and says so — rather than the deadness
//! numbers moving for a reason nobody can see.

mod common;

use std::collections::BTreeSet;

#[test]
fn a_test_only_child_file_cannot_be_counted_as_a_production_writer() {
    let text = "//! Private test fixtures.\n#![cfg(test)]\nfn check() { store.save(\"test\", &value); }\n";
    let (production, tests) = common::split_production_and_tests(text);
    assert!(production.is_empty());
    assert_eq!(tests, text);
    let text = "// #![cfg(test)] is mentioned here.\nfn save() {}\n";
    let (production, tests) = common::split_production_and_tests(text);
    assert_eq!(production, text);
    assert!(tests.is_empty());
}

fn src_files() -> Vec<(String, String)> {
    fn walk(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        paths.sort();
        for p in paths {
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().and_then(|x| x.to_str()) == Some("rs") {
                if let Ok(t) = std::fs::read_to_string(&p) {
                    out.push((p.to_string_lossy().to_string(), t));
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(std::path::Path::new("src"), &mut out);
    out
}

#[test]
fn a_file_with_no_inline_tests_is_returned_whole() {
    let text = "pub fn a() {}\npub fn b() {}\n";
    let (prod, tests) = common::split_production_and_tests(text);
    // Byte-identical, trailing newline included: a file with no inline tests
    // must come back out exactly as it went in.
    assert_eq!(prod, text, "a file with no test block was altered");
    assert!(tests.trim().is_empty(), "invented a test block: {tests:?}");
}

#[test]
fn the_block_goes_to_tests_and_the_rest_stays() {
    // The attribute is assembled rather than written out, and that is not
    // fussiness. `retrospective.rs::no_test_asserts_nothing` finds the end of
    // a test body by looking for the next `#[test]`, so a literal one inside
    // this fixture's string truncates the body it scans and this test reads
    // as asserting nothing. It caught exactly that, correctly, on the run
    // this comment was written. Leave it assembled.
    let tag = concat!("#[", "test]");
    let text = format!(
        "pub fn a() {{}}\n\
         #[cfg(test)]\n\
         mod tests {{\n\
             use super::*;\n\
             {tag}\n\
             fn t() {{ a(); }}\n\
         }}\n\
         pub fn after() {{}}\n"
    );
    let (prod, tests) = common::split_production_and_tests(&text);
    assert!(prod.contains("pub fn a()"), "lost production code: {prod}");
    assert!(
        prod.contains("pub fn after()"),
        "dropped everything after the block, which would hide real code: {prod}"
    );
    assert!(!prod.contains(tag), "test code left in production: {prod}");
    assert!(tests.contains("fn t()"), "test body not captured: {tests}");
    assert!(
        !tests.contains("pub fn after()"),
        "the block swallowed code that follows it: {tests}"
    );
}

#[test]
fn a_doc_comment_mentioning_the_attribute_is_not_a_block() {
    // `src/market/fixtures.rs` line 3 says "not `#[cfg(test)]`" in prose. A
    // rule that matched anywhere in the line would delete that whole file
    // from the measurement.
    let text = "//! Not scaffolding, and not `#[cfg(test)]`. Real bars.\npub fn a() {}\n";
    let (prod, tests) = common::split_production_and_tests(text);
    assert!(tests.trim().is_empty(), "a doc comment was read as a test block: {tests:?}");
    assert!(prod.contains("pub fn a()"));
}

#[test]
fn every_inline_test_block_has_the_shape_the_splitter_assumes() {
    let mut checked = 0;
    let mut wrong: Vec<String> = Vec::new();

    for (path, text) in src_files() {
        // Indented attributes would be missed silently by the splitter, so
        // they are rejected here rather than mis-sliced there.
        for (n, line) in text.split('\n').enumerate() {
            let t = line.trim_start();
            if t.starts_with("#[cfg(test)]") && !line.starts_with("#[cfg(test)]") {
                wrong.push(format!("{path}:{}: indented `#[cfg(test)]`", n + 1));
            }
        }

        let (_, tests) = common::split_production_and_tests(&text);
        if tests.trim().is_empty() {
            continue;
        }
        if text.lines().take_while(|line| line.trim().is_empty() || line.starts_with("//") || line.starts_with("#!"))
            .any(|line| line.trim_end() == "#![cfg(test)]") {
            let (production, tests) = common::split_production_and_tests(&text);
            assert!(production.is_empty(), "{path}: test-only module leaked into production");
            assert_eq!(tests, text, "{path}: test-only module changed during split");
            continue;
        }
        checked += 1;
        let lines: Vec<&str> = tests.split('\n').collect();
        if !lines[0].starts_with("#[cfg(test)]") {
            wrong.push(format!("{path}: block does not start with the attribute"));
        }
        match lines.get(1) {
            Some(l) if l.contains("mod ") && l.trim_end().ends_with('{') => {}
            Some(l) => wrong.push(format!("{path}: second line is not `mod ... {{`: {l}")),
            None => wrong.push(format!("{path}: block is one line long")),
        }
        if lines.last().map(|l| l.trim_end()) != Some("}") {
            wrong.push(format!("{path}: block does not end at a column-zero `}}`"));
        }
    }

    assert!(
        wrong.is_empty(),
        "the splitter's rule no longer describes this tree:\n  {}\n\nEvery deadness number \
         depends on this rule holding. Fix the rule and this test together.",
        wrong.join("\n  ")
    );
    assert!(
        checked > 30,
        "only {checked} inline test blocks were found, so this check has stopped working \
         and would pass for a tree with none at all"
    );
}

#[test]
fn splitting_loses_nothing() {
    // Every line must end up on exactly one side. A splitter that drops lines
    // would shrink the measurement invisibly, which is the failure this whole
    // file exists to prevent.
    for (path, text) in src_files() {
        let (prod, tests) = common::split_production_and_tests(&text);
        let before = text.split('\n').count();
        let after = if prod.is_empty() { 0 } else { prod.split('\n').count() }
            + if tests.is_empty() { 0 } else { tests.split('\n').count() };
        assert_eq!(
            before, after,
            "{path}: {before} lines in, {after} out -- the splitter is losing or duplicating code"
        );

        let all: BTreeSet<&str> = text.split('\n').filter(|l| !l.trim().is_empty()).collect();
        let kept: BTreeSet<&str> = prod
            .split('\n')
            .chain(tests.split('\n'))
            .filter(|l| !l.trim().is_empty())
            .collect();
        assert_eq!(all, kept, "{path}: a line changed or vanished in the split");
    }
}
