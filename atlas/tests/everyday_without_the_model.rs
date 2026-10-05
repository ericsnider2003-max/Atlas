//! The self-test, 4 Oct 2026, on Eric's laptop: the everyday sentences
//! reached the right command but only by asking the model which one -- ten
//! to sixteen seconds each ("find the tax pdf from last year" 10.7 s, "how did
//! my last youtube video do" 16.3 s, "my laptop is running slow, what's
//! eating the memory" 12.4 s). The rules read them now (`doing::rescue`),
//! and leave alone the sentences that only share a word with them.

use atlas::config::Config;
use atlas::intent::{Intent, Parser};
use std::path::Path;

fn parser() -> Parser {
    Parser::new(&Config::load(Path::new("config")).unwrap().commands)
}

fn reached(p: &Parser, s: &str) -> Option<String> {
    p.parse_named(s).1
}

#[test]
fn everyday_sentences_reach_their_command_without_the_model() {
    let p = parser();
    for (s, want) in [
        ("my laptop is running slow, what's eating the memory", "machine_health"),
        ("how much space have I got left on this thing", "machine_health"),
        ("what's using all my ram", "machine_health"),
        ("find the tax pdf from last year", "find_file"),
        ("where's that invoice from March", "find_file"),
        ("anything new come in by email", "mail"),
        ("any new mail today", "mail"),
        ("how did my last youtube video do", "social"),
    ] {
        assert_eq!(reached(&p, s).as_deref(), Some(want), "{s}");
    }
}

#[test]
fn a_shared_word_is_not_enough() {
    let p = parser();
    for (s, not) in [
        ("send an email to Sam about the invoice", "mail"),
        ("find me a flight to Denver", "find_file"),
        ("find out what time the game starts", "find_file"),
        ("the website is running slow", "machine_health"),
        ("there's not much space in the car", "machine_health"),
    ] {
        assert_ne!(reached(&p, s).as_deref(), Some(not), "{s}");
    }
}

#[test]
fn the_file_asked_for_is_kept() {
    let p = parser();
    match p.parse("find the tax pdf from last year") {
        Intent::FindFile(q) => assert!(q.contains("tax pdf"), "{q}"),
        other => panic!("{other:?}"),
    }
}
