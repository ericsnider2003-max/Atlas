//! The checkable half of design taste.
//!
//! `taste::review` doesn't claim to know whether a page looks good — nothing a
//! machine runs offline can. What it holds to is the part that *is* mechanical:
//! spacing on the scale, colours from tokens not typed-in hex, and the
//! accessibility floors (alt text, a page language, a name on every control).
//! These defend that it catches those, that it separates "fix this" from
//! "worth a look", and that it never overstates what a clean page means.

use atlas::brain::MockLlm;
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use atlas::taste::{
    blocking, build_web, review, spoken, wants_web_page, Finding, Outcome, Rules, Severity,
};
use std::path::Path;

fn rules() -> Rules {
    Rules::default()
}

fn has(findings: &[atlas::taste::Finding], rule: &str) -> bool {
    findings.iter().any(|f| f.rule == rule)
}

// --- accessibility floors (blocking) ---------------------------------------

#[test]
fn an_image_without_alt_text_is_flagged() {
    let f = review(r#"<img src="logo.png">"#, &rules());
    assert!(has(&f, "alt text"), "{f:?}");
    assert_eq!(f.iter().find(|x| x.rule == "alt text").unwrap().severity, Severity::Blocking);

    // With alt text, nothing.
    let ok = review(r#"<img src="logo.png" alt="Company logo">"#, &rules());
    assert!(!has(&ok, "alt text"), "{ok:?}");
}

#[test]
fn a_page_with_no_language_is_flagged() {
    assert!(has(&review("<html><body>hi</body></html>", &rules()), "page language"));
    assert!(!has(&review(r#"<html lang="en"><body>hi</body></html>"#, &rules()), "page language"));
}

#[test]
fn a_control_with_no_name_is_flagged() {
    assert!(has(&review(r#"<input type="text">"#, &rules()), "control name"));
    // Named by aria-label, or by an id a label could point at — not flagged.
    assert!(!has(&review(r#"<input type="text" aria-label="Search">"#, &rules()), "control name"));
    assert!(!has(&review(r#"<input type="text" id="q">"#, &rules()), "control name"));
}

// --- colour tokens (blocking) ----------------------------------------------

#[test]
fn a_hex_colour_typed_into_an_inline_style_is_flagged() {
    let f = review(r#"<div style="color:#3a7bd5">hi</div>"#, &rules());
    assert!(has(&f, "colour token"), "{f:?}");
    assert!(f.iter().find(|x| x.rule == "colour token").unwrap().detail.contains("#3a7bd5"));

    // A token is the right shape — not flagged.
    assert!(!has(&review(r#"<div style="color:var(--accent)">hi</div>"#, &rules()), "colour token"));
}

// --- spacing scale (advisory) ----------------------------------------------

#[test]
fn spacing_off_the_grid_is_advisory_on_the_grid_is_clean() {
    let off = review(r#"<div style="margin:13px">x</div>"#, &rules());
    let s = off.iter().find(|f| f.rule == "spacing scale").expect("13 is off a 4px grid");
    assert_eq!(s.severity, Severity::Advisory);

    // 16 is a multiple of 4 — clean.
    assert!(!has(&review(r#"<div style="margin:16px">x</div>"#, &rules()), "spacing scale"));
}

// --- structure (advisory) --------------------------------------------------

#[test]
fn a_skipped_heading_level_is_advisory() {
    assert!(has(&review("<h1>Title</h1><h3>Sub</h3>", &rules()), "heading order"));
    // A normal step down is fine.
    assert!(!has(&review("<h1>Title</h1><h2>Sub</h2>", &rules()), "heading order"));
}

// --- the honest summary ----------------------------------------------------

#[test]
fn a_clean_page_is_called_consistent_not_good() {
    let clean = r#"<html lang="en"><body><h1>Hi</h1><img src="a.png" alt="A"></body></html>"#;
    let f = review(clean, &rules());
    assert!(f.is_empty(), "should be clean: {f:?}");
    let said = spoken(&f);
    assert!(said.to_lowercase().contains("consistent"), "{said}");
    // The load-bearing honesty: never claims the design is good/right.
    assert!(!said.to_lowercase().contains("good design"), "{said}");
    assert!(said.to_lowercase().contains("not whether it's the right design"), "{said}");
}

#[test]
fn blocking_and_advisory_are_separated() {
    // A page with one of each.
    let html = r#"<img src="a.png"><div style="margin:13px">x</div>"#;
    let f = review(html, &rules());
    let b = blocking(&f);
    assert!(b.iter().any(|x| x.rule == "alt text"), "alt text blocks: {f:?}");
    assert!(b.iter().all(|x| x.severity == Severity::Blocking));
    assert!(f.iter().any(|x| x.severity == Severity::Advisory), "spacing is advisory");
}

// --- the house style is configurable, with neutral defaults ----------------

#[test]
fn the_spacing_grid_follows_the_configured_base_unit() {
    // Default grid is 4px, so 8px is clean. Move the house style to an 8px
    // grid and the same 12px value goes off-grid while 16px stays on.
    let eight = Rules { base_unit: 8, ..Rules::default() };
    assert!(!has(&review(r#"<div style="margin:8px">x</div>"#, &Rules::default()), "spacing scale"));
    assert!(has(&review(r#"<div style="margin:12px">x</div>"#, &eight), "spacing scale"), "12 is off an 8px grid");
    assert!(!has(&review(r#"<div style="margin:16px">x</div>"#, &eight), "spacing scale"), "16 is on an 8px grid");
}

#[test]
fn a_rule_can_be_turned_off_in_the_house_style() {
    // Colour-token enforcement off: a typed-in hex no longer blocks.
    let no_tokens = Rules { colours_from_tokens: false, ..Rules::default() };
    assert!(has(&review(r#"<div style="color:#abc">x</div>"#, &Rules::default()), "colour token"));
    assert!(!has(&review(r#"<div style="color:#abc">x</div>"#, &no_tokens), "colour token"));
}

#[test]
fn the_default_house_style_is_neutral_not_personal() {
    // The shipped default is generic best-practice, not a personal palette or
    // font: a 4px grid, tokens, accessibility, classes — nothing that presumes
    // whose page it is.
    let d = Rules::default();
    assert_eq!(d.base_unit, 4);
    assert!(d.colours_from_tokens && d.accessibility && d.prefer_classes);
}

// --- routing a build to the web/taste gate ---------------------------------

#[test]
fn web_words_route_to_the_taste_gate_and_scripts_do_not() {
    assert!(wants_web_page("build me a landing page for my band"));
    assert!(wants_web_page("make a website for the bakery"));
    assert!(wants_web_page("a simple html page that lists the menu"));
    // A script or program is not a web page.
    assert!(!wants_web_page("write a script that renames files by date"));
    assert!(!wants_web_page("a program to back up my photos"));
    // Whole-word: "html" inside another word doesn't fire.
    assert!(!wants_web_page("parse this htmlish blob into records"));
}

// --- the taste-gated build loop --------------------------------------------

const CLEAN_PAGE: &str = r#"<!doctype html><html lang="en"><head><style>:root{--bg:#fff}.box{margin:16px}</style></head><body><h1>Hi</h1><img src="a.png" alt="A"></body></html>"#;

fn a_blocker() -> Vec<Finding> {
    vec![Finding { severity: Severity::Blocking, rule: "alt text".into(), detail: "an <img> has no alt text".into() }]
}

#[test]
fn the_web_loop_iterates_until_the_page_clears_the_floors() {
    let llm = MockLlm(format!("```\n{CLEAN_PAGE}\n```"));
    let calls = std::cell::Cell::new(0u32);
    let outcome = build_web("a page", &llm, 3, |_html| {
        let n = calls.get();
        calls.set(n + 1);
        if n == 0 { a_blocker() } else { vec![] }
    });
    match outcome {
        Outcome::Built { rounds, .. } => assert_eq!(rounds, 1, "one fix pass"),
        other => panic!("expected Built, got {other:?}"),
    }
}

#[test]
fn a_page_that_never_clears_is_a_struggle_with_the_problems_attached() {
    let llm = MockLlm(format!("```\n{CLEAN_PAGE}\n```"));
    let outcome = build_web("a page", &llm, 2, |_| a_blocker());
    match outcome {
        Outcome::Struggled { rounds, findings, html } => {
            assert_eq!(rounds, 2);
            assert!(findings.iter().any(|f| f.severity == Severity::Blocking));
            assert!(!html.is_empty(), "the best draft is still handed back");
            // Honest: never called good, always shows what's wrong.
            let said = outcome_spoken(&Outcome::Struggled { rounds, findings, html });
            assert!(said.to_lowercase().contains("couldn't get it past"), "{said}");
        }
        other => panic!("expected Struggled, got {other:?}"),
    }
}

#[test]
fn no_page_shaped_reply_is_no_draft() {
    let llm = MockLlm("here is a paragraph of prose, not a page at all".into());
    assert!(matches!(build_web("a page", &llm, 3, |_| vec![]), Outcome::NoDraft(_)));
}

#[test]
fn a_built_page_is_called_consistent_not_good() {
    let llm = MockLlm(format!("```\n{CLEAN_PAGE}\n```"));
    let outcome = build_web("a page", &llm, 3, |_| vec![]);
    let said = outcome.spoken();
    assert!(said.to_lowercase().contains("consistent and accessible"), "{said}");
    assert!(!said.to_lowercase().contains("good design"), "must not claim it's good: {said}");
    assert!(said.to_lowercase().contains("not whether it's the right design"), "{said}");
}

fn outcome_spoken(o: &Outcome) -> String {
    o.spoken()
}

// --- end to end through the daemon -----------------------------------------

#[test]
fn reviewing_a_page_through_the_daemon_reports_findings() {
    let dir = std::env::temp_dir().join(format!("atlas-taste-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let mut d = Daemon::new(&c, &p, None, Store::new(dir.clone()), Proactive::new(ProactiveConfig::default()));

    // A string literal (not a `format!`) so the every-intent-reaches-the-daemon
    // guard can parse it and confirm this branch is driven end to end. The
    // markup is passed inline — the '<' tells `design_review` it's a page, not
    // a path.
    let reply = d.turn("review the design of <img src=hero.png>", 100);
    assert!(
        reply.contains("alt text") || reply.contains("screen reader"),
        "should flag the missing alt text: {reply}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
