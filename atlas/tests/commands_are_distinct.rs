//! No two commands may share a phrase.
//!
//! ## Why this guard exists
//!
//! The parser sorts phrases longest first, which is the whole trick that lets
//! "open workspace" beat "open". It works because a longer phrase is a more
//! specific one.
//!
//! Two *identical* phrases leave it nothing to sort by. One of them wins by
//! position in the file and the other can never fire — not rarely, never. The
//! command still appears in the config, still appears in the docs, still looks
//! implemented. This is the codebase's signature failure wearing a different
//! hat: a thing that is written down, reads as finished, and is unreachable.
//!
//! Three of these were sitting in `commands.yaml` and were found by writing
//! this test, not by reading:
//!
//! - `switch to` was claimed by both `focus_app` and `set_mode`, so
//!   "switch to focus" opened an application called "focus"
//! - `undo`, `undo that` and `put it back` were listed under both `undo` and
//!   `history`
//!
//! ## Watched failing
//!
//! Put any of those phrases back in both places and this test names the pair.
//! It was proven that way rather than assumed: the first version compared
//! phrases *within* each command rather than across them, and passed happily
//! with every one of the clashes above still in the file.

use std::collections::BTreeMap;

/// Read the shipped command list without a YAML parser.
///
/// Deliberately not going through `Config`: this test has to see what is in
/// the file, and loading it through the same code the program uses would hide
/// a phrase the loader silently drops.
fn phrases() -> Vec<(String, String)> {
    let text = include_str!("../config/commands.yaml");
    let mut out = Vec::new();
    let mut intent = String::new();
    let mut collecting = false;
    let mut buffer = String::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("- intent:") {
            intent = rest.trim().to_string();
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("phrases:") {
            buffer = rest.trim().to_string();
            collecting = true;
        } else if collecting {
            buffer.push(' ');
            buffer.push_str(trimmed);
        }
        if collecting && buffer.contains(']') {
            let inside = buffer
                .trim_start_matches('[')
                .split(']')
                .next()
                .unwrap_or_default()
                .trim_start_matches('[')
                .to_string();
            for raw in inside.split(',') {
                let p = raw.trim().trim_matches('"').trim().to_string();
                if !p.is_empty() {
                    out.push((p, intent.clone()));
                }
            }
            collecting = false;
            buffer.clear();
        }
    }
    out
}

/// The same tidying the parser does before it compares anything.
fn normalise(s: &str) -> String {
    s.trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '_')
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn the_command_list_was_actually_read() {
    // A guard that reads nothing passes forever. This is the check that this
    // one is looking at something.
    let all = phrases();
    assert!(all.len() > 150, "only found {} phrases", all.len());
    assert!(
        all.iter().any(|(p, i)| p == "boot workspace" && i == "workspace_on"),
        "the parsing is wrong: a known phrase is missing"
    );
    assert!(
        all.iter().any(|(_, i)| i == "whats_this"),
        "seeing's commands should be in here"
    );
}

#[test]
fn no_two_commands_claim_the_same_phrase() {
    let mut owner: BTreeMap<String, String> = BTreeMap::new();
    let mut clashes: Vec<String> = Vec::new();
    for (phrase, intent) in phrases() {
        let key = normalise(&phrase);
        match owner.get(&key) {
            Some(first) if *first != intent => {
                clashes.push(format!("\"{key}\" is claimed by both {first} and {intent}"));
            }
            _ => {
                owner.insert(key, intent);
            }
        }
    }
    assert!(
        clashes.is_empty(),
        "one of each pair below can never fire — the longest-phrase rule has nothing left to \
         separate them by:\n  {}",
        clashes.join("\n  ")
    );
}

#[test]
fn every_phrase_survives_tidying() {
    // An apostrophe is stripped from what you say and from nothing in the
    // config, so "what's new" listed with one was dead while "whats new"
    // beside it hid that. Same shape, different step.
    for (phrase, intent) in phrases() {
        let tidy = normalise(&phrase);
        assert!(
            !tidy.is_empty(),
            "{intent} lists a phrase that tidies away to nothing: {phrase:?}"
        );
    }
}
