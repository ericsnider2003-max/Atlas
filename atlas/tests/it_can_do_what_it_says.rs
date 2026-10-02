//! Everything Atlas tells you to say or run has to exist.
//!
//! # The class of bug this is for
//!
//! Atlas talks. It tells you to run `atlas vault recovery`, or to say "I'm
//! back", and every one of those sentences is a promise that something on the
//! other end will answer. A sentence naming a command that was never built,
//! or a phrase the parser has never heard of, is this codebase's signature
//! failure wearing its most convincing disguise: it reads as a finished
//! feature, it is *documented in Atlas's own voice*, and the only way anybody
//! finds out is by trying it.
//!
//! Two were found by hand on 17 September, both in code written days apart by
//! somebody who had just read the surrounding file:
//!
//! * `handover::Hint::offer` says *"Say \"hand over\" if someone else has the
//!   laptop"*, and the module doc says "Anyone may enter — your friend can say
//!   'guest mode' themselves". Neither phrase parsed to anything. Entering a
//!   handover was command-line only, so the asymmetry the whole design rests
//!   on — free to enter, costly to leave — had no free half.
//! * `config/tools.yaml` says *"`atlas mail setup` prints the steps"* for the
//!   Azure registration. There was no `mail` command at all.
//!
//! Finding those by reading is luck. This finds them by construction.
//!
//! # What it does not do
//!
//! It does not check that the command does anything *useful* — that is what
//! the rest of the suite is for. It checks the narrower thing that can be
//! checked mechanically: the door Atlas names is a door that opens.

use std::collections::BTreeSet;

/// Every file that speaks to a person, or that a person reads while setting
/// Atlas up.
fn files_that_talk() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for dir in ["src", "config"] {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !matches!(ext, "rs" | "yaml") {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(&path) {
                out.push((path.display().to_string(), text));
            }
        }
    }
    out.push(("ATLAS.bat".into(), std::fs::read_to_string("ATLAS.bat").unwrap_or_default()));
    out
}

/// Subcommands `main.rs` actually dispatches on.
///
/// Read from the dispatch itself — `words.first()... == Some("x")` — rather
/// than from every `Some("...")` in the file, which would also collect string
/// matches from a dozen unrelated `match` arms and quietly make this guard
/// pass for commands that do not exist.
fn commands_that_exist() -> BTreeSet<String> {
    let main = crate::common::source_of("main");
    let mut out = BTreeSet::new();
    for line in main.lines() {
        let code = line.split("//").next().unwrap_or("");
        if !code.contains("words.first()") {
            continue;
        }
        let mut rest = code;
        while let Some(i) = rest.find("Some(\"") {
            rest = &rest[i + 6..];
            if let Some(end) = rest.find('"') {
                out.insert(rest[..end].to_string());
            }
        }
    }
    assert!(
        out.len() > 20,
        "the dispatch parse found {} commands, so it has stopped working and this \
         guard would pass for anything",
        out.len()
    );
    assert!(out.contains("vault"), "the dispatch parse is not finding real commands");
    out
}

/// Every **backticked** `atlas <word>` Atlas says out loud or ships in a
/// config a person reads.
///
/// Backticks, rather than every occurrence of the word "atlas" followed by
/// something. That was the first version and it collected the startup banner
/// ("atlas ready. type a command"), the spoken phrase list in `attention.rs`
/// ("shut it down", "kill it", "atlas halt"), and half the prose in the tree
/// — sixteen findings, three of them real. A guard whose output is mostly
/// noise gets an exemption list bolted on until it means nothing.
///
/// This codebase writes a command it wants you to run in backticks, every
/// time. That is the convention, so that is what this reads.
fn commands_atlas_names() -> Vec<(String, String)> {
    let mut found = Vec::new();
    for (file, text) in files_that_talk() {
        let mut rest = text.as_str();
        while let Some(i) = rest.find("`atlas ") {
            rest = &rest[i + 7..];
            let word: String = rest
                .chars()
                .take_while(|c| c.is_ascii_lowercase() || *c == '-')
                .collect();
            if word.is_empty() {
                continue;
            }
            found.push((file.clone(), word));
        }
    }
    found
}

/// Words that follow "atlas " in ordinary prose and are not commands.
///
/// Kept short and specific on purpose. Every entry here is a hole in the
/// guard, so each one has to be a word that could not plausibly become a
/// command, rather than a convenient way to silence a failure.
const NOT_A_COMMAND: &[&str] = &[
    // Backticked prose that is genuinely not a command. Kept as short as it
    // can be: every entry is a hole, and this list is where a guard like this
    // goes to die.
    "instance",
    "install",
];

/// A flag rather than a subcommand: `--daemon`, `--voice`, `--wake`.
///
/// Dispatched before `words` exists — `main` filters anything starting with a
/// dash out of `words` and handles it separately — so the subcommand parse
/// above genuinely cannot see them, and `tests/cli_args.rs` is what covers
/// that path. Recognised here rather than listed one by one, so a new flag
/// does not need adding to an exemption list.
fn is_a_flag(word: &str) -> bool {
    word.starts_with('-')
}

#[test]
fn every_command_atlas_names_is_a_command_that_exists() {
    let exists = commands_that_exist();
    let mut missing: Vec<String> = Vec::new();

    for (file, word) in commands_atlas_names() {
        if NOT_A_COMMAND.contains(&word.as_str()) || is_a_flag(&word) || exists.contains(&word) {
            continue;
        }
        missing.push(format!("{file}: `atlas {word}`"));
    }
    missing.sort();
    missing.dedup();

    assert!(
        missing.is_empty(),
        "Atlas names these commands and they do not exist. Each one is a \
         sentence that reads as a finished feature and answers nothing:\n  {}\n\n\
         Build the command, or stop saying it. If a word here is ordinary prose \
         rather than a command, add it to NOT_A_COMMAND -- but read it twice \
         first, because that list is where this guard goes to die.",
        missing.join("\n  ")
    );
}

/// Every phrase Atlas tells you to *say*, extracted from its own sentences.
///
/// Only the `say "..."` shape, deliberately. Widening it to any quoted string
/// would collect half the codebase and force an exemption list longer than
/// the guard, which is how a check stops meaning anything.
fn phrases_atlas_tells_you_to_say() -> Vec<(String, String)> {
    let mut found = Vec::new();
    for (file, text) in files_that_talk() {
        // Comment lines are dropped first. A doc comment saying "he's back"
        // mid-sentence is prose about the feature, not Atlas telling somebody
        // to say something, and including them buried the two real findings
        // under twenty explanations of them.
        let speech: Vec<&str> = text
            .lines()
            .filter(|l| {
                let t = l.trim_start();
                !(t.starts_with("//") || t.starts_with('#'))
            })
            .collect();
        // Joined, because these sentences are written across line breaks in
        // string literals and the phrase lands on the far side of the wrap.
        let joined = speech.join(" ").replace("\\ ", "");
        let lower = joined.to_lowercase();
        let mut from = 0usize;
        while let Some(i) = lower[from..].find("say ") {
            let at = from + i + 4;
            from = at;
            let after = &joined[at..];
            // The quote can be escaped (inside a Rust string) or plain.
            let opened = if after.starts_with("\\\"") {
                Some(2)
            } else if after.starts_with('"') {
                Some(1)
            } else {
                None
            };
            let Some(skip) = opened else { continue };
            let body = &after[skip..];
            let end = body.find(['"', '\\']).unwrap_or(0);
            if end == 0 {
                continue;
            }
            let phrase = body[..end].trim().to_string();
            // A format placeholder is not a phrase anybody says.
            if phrase.is_empty() || phrase.len() > 40 || phrase.contains('{') {
                continue;
            }
            found.push((file.clone(), phrase));
        }
    }
    found
}

/// Can Atlas hear this at all, by any of the ways it listens?
///
/// The parser is not the only one. A plain yes or no is resolved by
/// `session::is_yes`/`is_no` before anything parses, pause and resume are
/// caught by `attention::hear` above the parser so they work mid-task, and
/// dictation reads its own stop phrase while the parser is bypassed
/// entirely. A guard that knew only about the parser would report all three
/// as dead and be wrong every time.
fn atlas_can_hear(parser: &atlas::intent::Parser, phrase: &str) -> bool {
    if !matches!(parser.parse(phrase), atlas::intent::Intent::Unknown(_)) {
        return true;
    }
    if atlas::session::is_yes(phrase) || atlas::session::is_no(phrase) {
        return true;
    }
    if atlas::attention::hear(phrase).is_some() {
        return true;
    }
    // Read at the turn's front door, before the parser (1 Oct 2026): asking
    // for an ability, watching, and being on a call.
    if atlas::growth::asks_for_an_ability(phrase).is_some()
        || atlas::growth::answer(phrase).is_some()
        || atlas::growth::asks_for_the_list(phrase)
        || atlas::camwatch::asks(phrase).is_some()
        || atlas::callmute::asks(phrase).is_some()
    {
        return true;
    }
    // While dictating, the parser is not consulted at all -- `dictate::parse`
    // reads the stop phrase itself, so a phrase that produces a `Stop` piece
    // is heard even though nothing in `commands.yaml` mentions it.
    atlas::dictate::parse(phrase, &atlas::dictate::DictateConfig::default())
        .iter()
        .any(|p| matches!(p, atlas::dictate::Piece::Stop))
}

/// Phrases Atlas says that the parser does not answer, each with why.
///
/// Two kinds, and the difference matters. One is a phrase that is only
/// meaningful as an *answer* to something Atlas just asked — those are
/// handled where the question was asked, not by the parser, and demanding a
/// global command for them would be wrong. The other is a real gap: a
/// feature Atlas describes and nobody built.
///
/// The second kind is listed here rather than fixed in the same pass because
/// each is a feature, not a phrase — but listed, named, and with the shape of
/// the missing thing written down. The list may grow; it may not grow
/// silently, which is the same rule `tests/new_capabilities_are_wired.rs`
/// applies to capabilities.
const NOT_A_COMMAND_ON_ITS_OWN: &[(&str, &str)] = &[
    (
        "always",
        "an answer to an add-on step's own \"go ahead?\" -- a yes that also means \
         don't ask about this step again (`session::is_always`, handled where the \
         flow waits for your answer). On its own there is nothing to say yes to.",
    ),
    // --- only meaningful as an answer to a question just asked ---
    (
        "skip",
        "an answer during first-run setup, handled by the question that asked \
         it. A global `skip` command would have nothing to skip.",
    ),
    (
        "all",
        "an answer to `look`'s own question about which of several things you \
         meant. Same shape as `skip`.",
    ),
    (
        "forget",
        "a prefix, not a phrase: `person.rs` says 'say \"forget\" and any of \
         that'. The command would be `forget <the thing>`, and the thing is \
         whatever Atlas just listed.",
    ),
    // --- answers to a list just shown (round 11, `workday::read_first`) ---
    (
        "keep 1",
        "an answer to the notes review's own numbered list (`note_review`), read \
         as a follow-up for ten minutes after the list is shown. On its own there \
         is no note 1.",
    ),
    (
        "done 2",
        "an answer to the waiting-for list's own numbers (`waiting_for`), read as \
         a follow-up for ten minutes after the list is shown.",
    ),
    (
        "tell me more about 1",
        "an answer to the opportunities list just shown (`hunt::understand`, read \
         by `workday::read_first` while `Follow::Opportunities` is live -- \
         tests/opportunity_hunting.rs parses \"tell me more about #3\" through the \
         parser with that list showing). On its own there is no opportunity 1. \
         Listed 29 Sep 2026, when the hunt's own sentence was changed from a bare \
         \"tell me more about\" to the phrase you actually say.",
    ),
    (
        "show",
        "an answer during a flashcard quiz -- turn the card over. Read as a \
         follow-up while a card is up (`workday::Follow::Quiz`); on its own \
         there is no card to show.",
    ),
    // --- real gaps, all three built on 17 September ---
    //
    // This block used to hold three entries: "finish setting up", "stop
    // telling me about the backups" and "this is me". Each was a feature
    // Atlas described in its own voice and nobody had built. They are built
    // now -- `FinishSetup`, `MuteTopic` and `ThisIsMe` -- and the test below
    // is what made removing them compulsory rather than optional: it fails
    // while a phrase is listed here and reachable, so a fixed thing cannot
    // sit in this list looking broken.
    //
    // The block is left with its heading and empty on purpose. The next real
    // gap goes here, with its reason, and the list stays a record of what is
    // owed rather than a place to put things down.
];

#[test]
fn the_phrases_that_do_not_parse_are_named_rather_than_forgotten() {
    // The list above is only worth having if it stays true. A phrase that
    // gets built must leave it, or the list becomes a place where fixed
    // things go to look broken.
    let cfg = atlas::config::Config::load(std::path::Path::new("config")).expect("config");
    let parser = atlas::intent::Parser::new(&cfg.commands);
    for (phrase, why) in NOT_A_COMMAND_ON_ITS_OWN {
        assert!(
            !atlas_can_hear(&parser, phrase),
            "\"{phrase}\" is listed as unreachable and Atlas can hear it now. \
             Delete the line -- the reason given was: {why}"
        );
        assert!(why.len() > 40, "\"{phrase}\" is exempted without a real reason");
    }
}

#[test]
fn every_phrase_atlas_tells_you_to_say_parses_to_something() {
    // The `hand over` case, generalised. A phrase Atlas puts in your mouth
    // has to reach a command, or the sentence is an instruction to talk to
    // something that is not listening.
    let cfg = atlas::config::Config::load(std::path::Path::new("config")).expect("config");
    let parser = atlas::intent::Parser::new(&cfg.commands);

    let mut dead: Vec<String> = Vec::new();
    for (file, phrase) in phrases_atlas_tells_you_to_say() {
        if NOT_A_COMMAND_ON_ITS_OWN.iter().any(|(p, _)| *p == phrase) {
            continue;
        }
        if !atlas_can_hear(&parser, &phrase) {
            dead.push(format!("{file}: \"{phrase}\""));
        }
    }
    dead.sort();
    dead.dedup();

    assert!(
        dead.is_empty(),
        "Atlas tells you to say these and the parser has never heard of them:\n  \
         {}\n\nAdd the phrase to config/commands.yaml, or change what Atlas says. \
         A phrase Atlas puts in your mouth that reaches nothing is worse than \
         silence -- you try it, nothing happens, and you assume you said it wrong.",
        dead.join("\n  ")
    );
}

#[test]
fn the_extractors_actually_find_things() {
    // The failure mode of a guard like this is finding nothing and passing
    // forever. Both extractors are asserted to be returning real material,
    // so a change that breaks the parsing fails here rather than going quiet.
    let commands = commands_atlas_names();
    assert!(
        commands.len() > 20,
        "the command extractor found {} mentions -- it has stopped working",
        commands.len()
    );
    assert!(
        commands.iter().any(|(_, w)| w == "vault"),
        "the command extractor no longer sees `atlas vault`, which is all over this tree"
    );

    let phrases = phrases_atlas_tells_you_to_say();
    assert!(
        !phrases.is_empty(),
        "the phrase extractor found nothing, so the second guard is checking an \
         empty list and will pass whatever happens"
    );
}
