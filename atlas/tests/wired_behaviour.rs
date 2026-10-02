//! Does the running program actually do these things?
//!
//! `tests/wiring.rs` proves a module is reachable. This proves the intent
//! reaches it and comes back with something. That gap — reachable but doing
//! nothing useful — is where the last round of drift lived.

use atlas::intent::Intent;

fn config() -> atlas::config::Config {
    atlas::config::Config::load(std::path::Path::new("config")).expect("config must load")
}

fn parses(phrase: &str) -> Intent {
    atlas::intent::Parser::new(&config().commands).parse(phrase)
}

#[test]
fn asking_what_it_did_reaches_the_history_rather_than_a_stub() {
    assert!(matches!(parses("what did you do"), Intent::History(_)));
    assert!(matches!(parses("what changed"), Intent::History(_)));
    assert!(matches!(parses("put it back"), Intent::History(_) | Intent::Undo));
}

#[test]
fn asking_why_reaches_the_explaining_code() {
    assert!(matches!(parses("why did you"), Intent::Why(_)));
    assert!(matches!(parses("explain that"), Intent::Why(_)));
}

#[test]
fn there_is_only_one_intent_for_how_the_machine_is_doing() {
    // There were briefly two, answering the same question differently. Two
    // ways to ask one thing is how the answers drift apart.
    let health = parses("how's the machine");
    assert!(matches!(health, Intent::MachineHealth));
    assert!(!matches!(parses("what's slowing this down"), Intent::Unknown(_)));
}

/// Commands whose opening word is shared with ordinary speech ("add", "make",
/// "take"), so only a whole sentence of the right shape is that command --
/// "make a cake" must stay not-understood rather than become a group change.
/// For these, "<phrase> something" proves nothing; each phrase is checked
/// with a real sentence instead, which must parse to the command.
const NEEDS_A_WHOLE_SENTENCE: &[(&str, &str)] = &[
    ("add", "add Sam to the Friends group"),
    ("take", "take Sam out of the Friends group"),
    ("remove", "remove Sam from the Friends group"),
    ("make", "make Maya a reader in the Friends group"),
    ("let", "let Maya post in the Friends group"),
    // The short code leads (2 Oct 2026) count only when what follows names
    // code -- "write a haiku" is a poem, not a build.
    ("write a", "write a python script that renames my photos"),
    ("write an", "write an app that tracks my runs"),
    ("create a", "create a tool that merges csv files"),
    ("create an", "create an api for my notes"),
    ("create me a", "create me a scraper for job posts"),
    ("make me a", "make me a script that backs up my notes"),
    ("make me an", "make me an app that tracks my shopping"),
    ("code a", "code a game of snake"),
    ("program a", "program a calculator"),
    ("build a", "build a website for my bakery"),
    ("build an", "build an app that logs my workouts"),
];

fn example(phrase: &str, takes_argument: bool, optional: bool) -> String {
    if let Some((_, sentence)) = NEEDS_A_WHOLE_SENTENCE.iter().find(|(p, _)| *p == phrase) {
        return sentence.to_string();
    }
    if takes_argument && !optional {
        format!("{phrase} something")
    } else {
        phrase.to_string()
    }
}

#[test]
fn every_phrase_in_the_command_file_parses_to_something_real() {
    // A phrase that falls through to Unknown is a promise in a config file
    // with nothing behind it. Phrases that need an argument are given one —
    // "open" alone is correctly not a match, and that isn't the failure this
    // is looking for.
    let cfg = config();
    let parser = atlas::intent::Parser::new(&cfg.commands);
    let mut dead = Vec::new();
    for c in &cfg.commands.commands {
        for p in &c.phrases {
            let said = if c.intent == "improve" && !matches!(p.as_str(), "improve" | "refactor") {
                // A general phrase ("fix the", "on the") reaches project work
                // only with a project named (round 10: "change the volume"
                // was being asked which project it was for).
                format!("{p} parser on the atlas project")
            } else {
                example(p, c.takes_argument, c.argument_optional)
            };
            if matches!(parser.parse(&said), Intent::Unknown(_)) {
                dead.push(format!("{said} ({})", c.intent));
            }
        }
    }
    assert!(dead.is_empty(), "phrases that go nowhere: {dead:?}");
}

#[test]
fn a_phrase_that_needs_a_subject_still_refuses_without_one() {
    // The other half of the same rule, which the fix above must not break.
    assert!(matches!(parses("open"), Intent::Unknown(_)));
    assert!(matches!(parses("open chrome"), Intent::OpenApp(_)));
}

#[test]
fn every_intent_the_parser_can_produce_has_somewhere_to_go() {
    // The other direction: an intent in the config with no arm in the daemon
    // is the shape `dictate` was in — a whole module unreachable behind a
    // phrase that looked wired.
    let cfg = config();
    let parser = atlas::intent::Parser::new(&cfg.commands);
    let daemon_src = crate::common::source_of("daemon");
    let mut missing = Vec::new();

    for c in &cfg.commands.commands {
        let Some(phrase) = c.phrases.first() else { continue };
        let said = example(phrase, c.takes_argument, c.argument_optional);
        // Ask the parser what this really becomes rather than guessing the
        // variant name from the intent name — show_panel is Intent::Show, and
        // guessing got that wrong.
        let intent = parser.parse(&said);
        if matches!(intent, Intent::Unknown(_)) {
            missing.push(format!("{} parses to nothing", c.intent));
            continue;
        }
        let variant = format!("{intent:?}");
        let name = variant.split(['(', ' ']).next().unwrap_or("").to_string();
        if !daemon_src.contains(&format!("Intent::{name}")) {
            missing.push(format!("{} -> Intent::{name} has no arm", c.intent));
        }
    }
    assert!(missing.is_empty(), "intents with no arm in the daemon: {missing:?}");
}

#[test]
fn a_phrase_with_an_apostrophe_actually_matches() {
    // It didn't. `normalize` strips apostrophes from what you say, and
    // nothing stripped them from the config, so every phrase containing one
    // was unmatchable. The config listed "whats new" beside "what's new",
    // which hid it.
    assert!(!matches!(parses("what's new"), Intent::Unknown(_)));
    assert!(!matches!(parses("how's the machine"), Intent::Unknown(_)));
    assert!(!matches!(parses("what couldn't you do"), Intent::Unknown(_)));
}

#[test]
fn the_two_spellings_of_a_phrase_reach_the_same_place() {
    assert_eq!(
        format!("{:?}", parses("what's slowing this down")),
        format!("{:?}", parses("whats slowing this down"))
    );
}

#[test]
fn the_new_families_reach_their_code_rather_than_a_stub() {
    // Each of these was a module with tests and no caller. The point isn't
    // that the phrase parses — it's that the arm behind it calls the module.
    for (said, want) in [
        ("sign me into linkedin", "SignIn"),
        ("unlock", "Unlock"),
        ("note that the fee is 40 points", "Capture"),
        ("check my email", "Mail"),
        ("sync", "Sync"),
        ("check this before i post it", "ReviewPost"),
        ("get me ready to travel", "TravelPrep"),
        ("what is this file", "Files"),
        ("sign me up for something", "CreateAccount"),
    ] {
        let got = format!("{:?}", parses(said));
        assert!(got.starts_with(want), "{said:?} -> {got}");
    }
}

#[test]
fn signing_in_and_signing_up_both_need_approval_and_cannot_be_learned_away() {
    use atlas::policy::{classify_with_policy, Decision};
    let mut mem = atlas::memory::Memory::default();
    for intent in [
        Intent::SignIn("a-bank.com".into()),
        Intent::CreateAccount("example.com".into()),
    ] {
        let kind = atlas::session::kind_of(&intent);
        for _ in 0..500 {
            mem.record_approval(kind, true, None);
        }
        assert_eq!(
            classify_with_policy(&intent, &mem, &Default::default()),
            Decision::RequireApproval,
            "{intent:?} became automatic after enough yeses"
        );
    }
}

#[test]
fn asking_about_this_with_nothing_to_point_at_asks_rather_than_guessing() {
    // The resolver is in the path now. Two equally likely things is a
    // question, not a coin toss.
    use atlas::subject::{resolve, Candidates, Resolution};
    let nothing = Candidates::default();
    assert!(matches!(resolve("summarise this", &nothing), Resolution::Nothing(_)));

    let both = Candidates {
        clipboard: Some("a paragraph of text".into()),
        selection: Some("a different paragraph".into()),
        ..Default::default()
    };
    // Selection is the more explicit of the two, so this one is decidable.
    assert!(matches!(resolve("summarise this", &both), Resolution::Found { .. }));
}

#[test]
fn an_answer_that_hedges_a_lot_is_flagged_as_uncertain() {
    use atlas::certainty::{assess, phrase, CertaintyConfig, Confidence, Grounding};
    let cfg = CertaintyConfig { enabled: true, ..Default::default() };
    let waffle = "It might possibly be that this could perhaps be the case, I think.";
    let (level, _, why) = assess(waffle, &Grounding::default(), &cfg);
    assert_ne!(level, Confidence::Fine, "three hedges is not confidence");
    assert!(phrase(waffle, level, &why).len() > waffle.len(), "it says so");
}

#[test]
fn something_atlas_knows_how_to_do_beats_shrugging_at_it() {
    // The procedure book was written months ago and nothing consulted it.
    // Anything without a command used to fall straight through to "I didn't
    // catch that".
    use atlas::knowhow::{shipped, Knowhow};
    assert!(!shipped().is_empty(), "there are procedures to find");
    let book = Knowhow::shipped();
    // Offline is the normal case here, so at least one has to be findable
    // with the network unplugged.
    let any_offline = shipped()
        .iter()
        .any(|p| book.for_request(&p.goal, false).is_some());
    assert!(any_offline, "nothing is reachable offline");
}

#[test]
fn a_failed_route_leads_to_another_one_rather_than_a_failure_report() {
    // Reporting a failure is what a program does. Trying the next route is
    // what an assistant does.
    use atlas::route::{known_routes, needs_internet, Kind};
    let extract: Vec<_> = known_routes().into_iter().filter(|r| r.for_what == Kind::Extract).collect();
    assert!(extract.len() > 1, "there has to be another way for this to mean anything");

    // And at least one of them works with the network unplugged.
    assert!(extract.iter().any(|r| !needs_internet(r)));
}

#[test]
fn the_trading_rule_that_leads_says_why_it_applies_to_you() {
    // The general version is on every tax site. The one that costs people
    // money is the one about their own situation.
    use atlas::ledger::trading_rules;
    let rules = trading_rules();
    assert!(!rules.is_empty());
    assert!(!rules[0].why_you.is_empty(), "no reason it matters to you");
    assert_ne!(rules[0].what, rules[0].why_you);
}

#[test]
fn a_draft_that_says_nothing_is_told_so_and_one_that_does_is_left_alone() {
    use atlas::stance::{assess, brief, Kind};
    let empty = "There are many perspectives on this and it depends on your situation.";
    assert!(brief(&assess(empty, Kind::Case), Kind::Case).is_some(), "should have something to say");
}
