//! A guide that tells you to set something that does nothing.
//!
//! The tree guards code against code in a dozen ways. Nothing guarded the
//! **documentation** against the code, and the docs are what a person actually
//! follows.
//!
//! `docs/VOICE_SETUP.md` described push-to-talk in detail — Tab, tap passes
//! through, hold past 350ms starts recording — and said:
//!
//! > The threshold is `push_to_talk.hold_ms`.
//!
//! Nothing reads `push_to_talk.hold_ms`. Nothing constructs the state machine
//! outside its own tests. And the same page said the Windows keyboard hook
//! that feeds it "is written but unverified" — **there is no keyboard hook
//! anywhere in this tree**, on any platform. A reader following that page sets
//! a number, presses Tab, and nothing happens.
//!
//! This measures the class: a **guide** may not name a config path
//! `<section>.<key>` whose section is on the list of sections that do nothing,
//! unless it is listed below with the reason.
//!
//! Records are exempt — a handover describing what was true in September is
//! supposed to say so. Only the pages someone follows are held to this.

use std::collections::BTreeSet;
use std::fs;

/// Docs that are a record of a moment, not instructions to follow.
///
/// A handover naming a dead module is doing its job. Matching by prefix rather
/// than listing them one by one, because new ones arrive every few days and a
/// guard nobody can keep up with gets switched off.
const RECORDS: &[&str] = &[
    "HANDOVER",
    "HANDOFF",
    "MODULE_REFERENCE",
    "OUTSTANDING_TASKS",
    "BUILD_PLAN",
    "AUDIT",
    "GAPS",
    "IDEAS",
    "ROADMAP",
];

/// Guides allowed to name a dead setting, with why.
///
/// The bar: the page must **say plainly, where it names it, that it does not
/// work yet.** Documenting an unfinished thing honestly is useful. Describing
/// it as though it works is the defect.
const SAYS_IT_DOES_NOT_WORK: &[(&str, &str, &str)] = &[
    // VOICE_SETUP.md's push_to_talk.hold_ms came off on 25 Sep 2026: the
    // keyboard hook exists now (`hotkeys`, Eric's ruling H1), the setting is
    // read, and the page says what it does rather than that it does nothing.
    (
        "SELF_REPAIR.md",
        "strategy.max_angles",
        "Named as a number that is not yours to tune yet. `strategy.rs` reads \
         `max_angles` correctly, but `ToolsConfig.strategy` never reaches it \
         and `selfwork`'s `begin`/`after` have no production caller, so the \
         ladder is not driven. Wiring it means driving the self-repair loop, \
         which wants a ruling first.",
    ),
];

fn dead_sections() -> BTreeSet<String> {
    atlas::config::PARSED_AND_NEVER_READ
        .iter()
        .chain(atlas::config::NO_FIELD_TO_LAND_IN.iter())
        .map(|(k, _)| k.to_string())
        .collect()
}

fn is_record(name: &str) -> bool {
    RECORDS.iter().any(|r| name.starts_with(r))
}

/// Every `section.key` a doc names, excluding `module.rs` filenames.
fn config_paths_in(text: &str, sections: &BTreeSet<String>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for section in sections {
        let pat = format!("{section}.");
        for (i, _) in text.match_indices(&pat) {
            // Whole word only. `github.com` contains "hub." and is not a
            // setting -- this scan's own first false positive.
            let before = text[..i].chars().next_back().unwrap_or(' ');
            if before.is_alphanumeric() || before == '_' {
                continue;
            }
            let rest: String = text[i + pat.len()..]
                .chars()
                .take_while(|c| c.is_ascii_lowercase() || *c == '_')
                .collect();
            // `strategy.rs` is a filename, not a setting.
            if rest.is_empty() || rest == "rs" || rest == "json" || rest == "yaml" {
                continue;
            }
            out.insert(format!("{section}.{rest}"));
        }
    }
    out
}

#[test]
fn no_guide_tells_you_to_set_something_that_does_nothing() {
    let sections = dead_sections();
    let allowed: BTreeSet<(String, String)> = SAYS_IT_DOES_NOT_WORK
        .iter()
        .map(|(doc, path, _)| (doc.to_string(), path.to_string()))
        .collect();

    let mut found: Vec<String> = Vec::new();
    let dir = std::path::Path::new("docs");
    let Ok(entries) = fs::read_dir(dir) else {
        panic!("docs/ is gone");
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("md") {
            continue;
        }
        let name = p.file_name().and_then(|x| x.to_str()).unwrap_or("").to_string();
        if is_record(&name) {
            continue;
        }
        let Ok(text) = fs::read_to_string(&p) else { continue };
        for path in config_paths_in(&text, &sections) {
            if !allowed.contains(&(name.clone(), path.clone())) {
                found.push(format!("{name}: {path}"));
            }
        }
    }

    found.sort();
    assert!(
        found.is_empty(),
        "these guides name a setting whose section does nothing, so a reader \
         following them changes a number and gets no behaviour:\n  {}\n\nWire \
         the setting, stop instructing it, or add it to SAYS_IT_DOES_NOT_WORK \
         once the page states plainly that it does not work yet.",
        found.join("\n  ")
    );
}

#[test]
fn each_allowed_page_actually_admits_it() {
    // The allow-list is not an exemption, it is a claim about what the page
    // says. If the page stops admitting it, the entry stops being true and
    // this fails — which is the only thing keeping the list from becoming a
    // place to hide things.
    for (doc, path, _) in SAYS_IT_DOES_NOT_WORK {
        let text = fs::read_to_string(format!("docs/{doc}"))
            .unwrap_or_else(|_| panic!("docs/{doc} is gone but still listed"));
        // Case-insensitively: a page may open the sentence with the phrase,
        // which is exactly what SELF_REPAIR.md does.
        let lower = text.to_lowercase();
        let admits = ["does not run", "doesn't run", "not wired", "nothing reads", "not yours to tune"]
            .iter()
            .any(|p| lower.contains(p));
        assert!(
            admits,
            "docs/{doc} is allowed to name {path} because it admits the setting \
             does nothing — and it no longer says so anywhere"
        );
    }
}

#[test]
fn every_exemption_gives_a_reason() {
    for (doc, path, why) in SAYS_IT_DOES_NOT_WORK {
        assert!(
            why.len() > 40,
            "{doc}/{path} is exempt with no useful reason"
        );
    }
}

#[test]
fn the_scan_still_works() {
    // The usual guard on the guard. A scan that finds nothing turns this file
    // into a test that passes for any tree.
    let sections: BTreeSet<String> = ["push_to_talk".to_string()].into_iter().collect();
    let found = config_paths_in("set `push_to_talk.hold_ms` to 400", &sections);
    assert!(found.contains("push_to_talk.hold_ms"), "the path scan stopped working");

    let filenames = config_paths_in("see push_to_talk.rs for the machine", &sections);
    assert!(filenames.is_empty(), "a source filename was read as a setting");
}
