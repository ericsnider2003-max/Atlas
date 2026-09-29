//! Nothing on a page may look like code.
//!
//! `{:?}` on an internal enum reaches the screen as `LookingBack` or
//! `Kind::Upkeep` — a variable name printed into a product. It is the single
//! cheapest way to make a tool look unfinished, and it happens by accident
//! every time someone formats a value they have rather than writing the
//! sentence a person would read.
//!
//! This scans the rendered pages for the shapes that give it away.

use atlas::dash::{Card, Layout};
use atlas::hub::{self, Page};

/// Things that only ever appear in source, never in a sentence.
///
/// `_` catches snake_case identifiers, `::` catches paths, and the bracket
/// pairs catch a struct or list printed with debug formatting.
fn looks_like_code(html: &str) -> Vec<String> {
    // Only the visible text, not attributes or the stylesheet — a CSS class
    // name is allowed to be snake_case, a sentence is not.
    let mut offenders = Vec::new();
    let mut visible = String::new();
    let mut in_tag = false;
    let mut in_style = false;
    let mut in_script = false;
    let lower = html.to_lowercase();
    let mut idx = 0;
    for ch in html.chars() {
        if lower[idx..].starts_with("<style") {
            in_style = true;
        }
        if lower[idx..].starts_with("</style") {
            in_style = false;
        }
        if lower[idx..].starts_with("<script") {
            in_script = true;
        }
        if lower[idx..].starts_with("</script") {
            in_script = false;
        }
        idx += ch.len_utf8();
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag && !in_style && !in_script => visible.push(ch),
            _ => {}
        }
    }

    for word in visible.split_whitespace() {
        let w = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != ':');
        if w.is_empty() {
            continue;
        }
        if w.contains("::") {
            offenders.push(format!("path-like: {w}"));
        }
        // snake_case: an underscore between two letters, in visible prose.
        if w.contains('_') && w.chars().any(|c| c.is_alphabetic()) {
            offenders.push(format!("identifier-like: {w}"));
        }
    }
    if visible.contains("Some(") || visible.contains("None)") || visible.contains("Err(") {
        offenders.push("debug-formatted option or result".into());
    }
    offenders
}

fn bodies() -> Vec<(Card, String)> {
    Card::all()
        .into_iter()
        .map(|c| {
            (
                c,
                format!("<p>Two things moved on {} today.</p>", c.title()),
            )
        })
        .collect()
}

#[test]
fn the_dashboard_reads_as_english() {
    for arranging in [false, true] {
        let html = hub::dashboard_page(&Layout::default(), &bodies(), arranging);
        let bad = looks_like_code(&html);
        assert!(bad.is_empty(), "arranging={arranging}: {bad:?}");
    }
}

#[test]
fn an_empty_dashboard_reads_as_english() {
    let html = hub::dashboard_page(&Layout::default(), &[], false);
    assert!(looks_like_code(&html).is_empty());
}

#[test]
fn every_card_is_named_in_words_a_person_would_use() {
    for c in Card::all() {
        let t = c.title();
        assert!(!t.contains('_'), "{t} is an identifier, not a name");
        assert!(
            t.chars().next().is_some_and(|ch| ch.is_uppercase()),
            "{t} does not read as a heading"
        );
        assert_ne!(t, c.key(), "{t} is the form value, not a label");
    }
}

/// The naming rule, enforced.
///
/// A label is a **name**: at most three words, no trailing punctuation, no
/// sentence doing the description's job. The sentence still exists — it is
/// `note()` — and every name must have one, because a short name is only
/// clearer than a long one when the long one is still there when you arrive.
#[test]
fn every_page_is_named_in_words_a_person_would_use() {
    for (_, pages) in hub::NAV {
        for p in *pages {
            let l = p.label();
            assert!(!l.contains('_') && !l.contains("::"), "{l} is an identifier");
            assert!(
                l.split_whitespace().count() <= 3,
                "{l:?} is a sentence — that is what the note is for"
            );
            assert!(
                !l.ends_with('.') && !l.contains(','),
                "{l:?} is punctuated like prose"
            );
            assert!(
                !l.starts_with("What ") && !l.starts_with("How "),
                "{l:?} describes rather than names"
            );

            let note = p.note();
            assert!(
                note.split_whitespace().count() >= 4,
                "{l} has no real explanation, so the short name costs you the \
                 meaning of it"
            );
            assert!(
                note.ends_with('.'),
                "{l}'s note is a fragment, not a sentence: {note:?}"
            );
            assert_ne!(note, l, "{l} explains itself with its own name");
        }
    }
}

#[test]
fn every_card_is_named_and_then_explained() {
    for c in Card::all() {
        // The command deck (Eric's design, 23 Sep 2026) names its cards the
        // way a person would say them — "Waiting on you", "What I did without
        // being asked" — so the ceiling is six words, not two. Still a name:
        // no full stop, and the explanation still lives in `note()`.
        assert!(
            c.title().split_whitespace().count() <= 6 && !c.title().ends_with('.'),
            "{:?} is a sentence on a card heading",
            c.title()
        );
        assert!(
            c.note().split_whitespace().count() >= 4,
            "{} has no explanation under it",
            c.title()
        );
        assert!(
            !c.note().ends_with('.'),
            "{}'s note is a caption, not a sentence — it sits under a heading",
            c.title()
        );
    }
}

#[test]
fn no_two_pages_or_cards_share_a_name() {
    // Short names collide far more easily than sentences did.
    let mut names: Vec<&str> = hub::NAV
        .iter()
        .flat_map(|(_, p)| p.iter().map(|p| p.label()))
        .collect();
    // A card may carry the name of the page it summarises and links to — the
    // deck's Projects card opens the Projects page, its Health card shows what
    // the Health page does. Two different things sharing a name is the
    // problem; a summary and its page are one thing.
    let summarises = [(Card::Projects, Page::Workshop), (Card::Machine, Page::Status)];
    names.extend(
        Card::all()
            .iter()
            .filter(|c| !summarises.iter().any(|(k, p)| k == *c && p.label() == c.title()))
            .map(|c| c.title()),
    );
    let n = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(n, names.len(), "two things share a name");
}

/// Every file that speaks to you in words.
///
/// `daemon.rs` was missing from this list, which is how Atlas came to say,
/// out loud, *"I don't know how to rehearse ReviewPost."* — the exact bug
/// this guard exists to prevent, in the one module that does most of the
/// talking. If a module can put a sentence in front of a person, it belongs
/// here.
const SPEAKS_TO_YOU: &[&str] = &[
    "src/hub.rs",
    "src/main.rs",
    "src/hublive.rs",
    "src/daemon.rs",
    "src/panel.rs",
    "src/window.rs",
    "src/mind.rs",
    "src/brief.rs",
    "src/dash.rs",
    // `atlas doctor` is read by a person trying to work out why something
    // is broken. It printed `{:?}` on the speech engine.
    "src/doctor.rs",
];

/// The specific bug: a `Debug` format reaching the screen.
///
/// The settings-only fallback printed the enum variant straight onto the
/// screen. It read `LookingBack needs the full Atlas running`.
///
/// This guard was green for five months for a reason that had nothing to do
/// with the code. It matched the literal `{:?}` — and Rust's inline-capture
/// form, `{other:?}`, is a different string, so twenty live instances passed
/// straight through it, including `daemon.rs:1959` saying a variant name out
/// loud. The standing caution in this tree is *"when a ratchet moves for a
/// reason that has nothing to do with the code, the ratchet is what needs
/// looking at"*; the same applies when it doesn't move.
///
/// So it now matches `:?}`, which catches both `{:?}` and `{name:?}`, and
/// `tests/hub_is_not_code.rs`'s own mutation check below proves it fails on
/// a planted one.
#[test]
fn no_source_file_debug_formats_a_value_onto_the_screen() {
    let mut offenders: Vec<String> = Vec::new();
    for file in SPEAKS_TO_YOU {
        let Some(src) = crate::common::read_source_path(file) else { continue };
        for (i, line) in src.lines().enumerate() {
            let t = line.trim_start();
            if t.starts_with("//") || t.starts_with("///") || t.starts_with('*') {
                continue;
            }
            // `{:?}` and `{anything:?}` are the same mistake. `:?}` is the
            // shortest string both share and nothing else legitimately
            // contains.
            if line.contains(":?}") {
                offenders.push(format!("{file}:{} {}", i + 1, line.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "these debug-format a value into something a person reads — a Rust \
         variant name arriving as English. Give the type a `plain()` and call \
         it:\n{}",
        offenders.join("\n")
    );
}

/// The guard's own mutation check, run in-process.
///
/// Five of the six source-text guards in this tree have needed a correction
/// because they were matching the wrong substring, and every correction was
/// a better substring. The thing that was never checked is whether the
/// matcher fires at all. This is that check: it does not read the tree, it
/// feeds the matcher both spellings and a decoy.
#[test]
fn the_matcher_catches_both_spellings_and_leaves_ordinary_code_alone() {
    let caught = |line: &str| line.contains(":?}");

    assert!(caught(r#"format!("{:?}", x)"#), "the old spelling must still be caught");
    assert!(caught(r#"format!("{other:?}")"#), "the inline-capture spelling is the one it missed");
    assert!(caught(r#"println!("{stage:?} next")"#));

    assert!(!caught(r#"format!("{x}")"#), "ordinary display formatting is fine");
    assert!(!caught(r#"format!("{:.1}", ms)"#));
    assert!(!caught(r#"let q = "who?"; "#), "a question mark in prose is not a debug format");
}

/// The `Intent` fallback that started it.
#[test]
fn every_intent_has_something_to_call_it_that_is_not_its_variant_name() {
    use atlas::intent::Intent;
    // A spread across the shapes: no argument, an argument, the catch-all.
    for i in [
        Intent::WorkspaceOn,
        Intent::ReviewPost("a post".into()),
        Intent::Dictate(String::new()),
        Intent::Unknown("blah".into()),
    ] {
        let said = i.plain();
        assert!(!said.is_empty());
        // The tell: Rust variant names are CamelCase with no spaces.
        let first = said.split_whitespace().next().unwrap_or("");
        assert!(
            !(first.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
                && first.chars().any(|c| c.is_uppercase() && first.starts_with(|f: char| f.is_uppercase()) && c != first.chars().next().unwrap())),
            "{said:?} still reads like a variant name"
        );
    }
    assert_eq!(Intent::WorkspaceOn.plain(), "starting your workspace");
}

#[test]
fn a_card_with_no_contents_still_reads_as_a_sentence() {
    let html = hub::nothing("Nothing outstanding.");
    assert!(html.contains("Nothing outstanding."));
    assert!(looks_like_code(&html).is_empty());
}

#[test]
fn a_list_with_nothing_in_it_says_which_list_is_empty() {
    let html = hub::list_page_at(Some(Page::Activity), "What I did", "intro", &[]);
    assert!(
        !html.contains(">Nothing here.<"),
        "the same three words for every empty page tells you nothing about \
         whether it is empty or broken"
    );
    assert!(looks_like_code(&html).is_empty());
}


#[test]
fn the_accounts_page_reads_as_english() {
    let mut b = atlas::accounts::Book::default();
    b.note("gmail");
    b.set_second_factor("gmail", atlas::accounts::SecondFactor::Sms);
    let advice = b.advice();
    let undescribed = b.undescribed();
    let stored = vec![("gmail".to_string(), "recovery codes".to_string())];
    let html = hub::accounts_page(&b.accounts, &advice, &undescribed, &stored, true, &[]);
    let bad = looks_like_code(&html);
    assert!(bad.is_empty(), "{bad:?}");
    for name in ["Keystone", "SecondFactor", "Sms", "TotpSeed"] {
        assert!(!html.contains(name), "{name} is a variable name");
    }
}
