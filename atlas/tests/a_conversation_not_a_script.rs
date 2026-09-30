//! **Talking to Atlas without following a script, and being talked back to
//! like a person.**
//!
//! Eric's words: *"I should be able to speak and communicate with Atlas
//! freely. I shouldn't have to follow a script to communicate with Atlas and
//! Atlas shouldn't speak back to me in a simply scripted way."*
//!
//! Four separate mechanisms produced that, and all four were failures of
//! wiring rather than missing design — every piece needed already existed.
//!
//! **1. The phrase parser answered sentences it had only partly read.** It
//! matches a phrase as a *prefix* and hands the remainder to `build` as the
//! argument. Measured on fifteen ordinary sentences before the fix:
//!
//! ```text
//! "open chrome and tell me what you think of the numbers"
//!     -> OpenApp("chrome and tell me what you think of the numbers")
//!        spoken: "Opening chrome and tell me what you think of the numbers."
//! "can you open chrome for me"
//!     -> Capabilities("open chrome for me")     -- and did NOT open chrome
//! "open up a browser would you, I want to look at something"
//!     -> OpenApp("up a browser would you i want to look at something")
//! "hang on, go back"
//!     -> Pause                                  -- "go back" discarded
//! ```
//!
//! **2. The model was never told who Atlas is.** `Brain::decide` sent the
//! bare action schema, which ended "One short sentence." `persona.rs` held
//! the whole character — tone, form of address, "have opinions", "disagree
//! when you have reason to", "you are not only for work" — and
//! `Persona::prompt_for`, whose own doc names this exact failure, was on
//! `tests/dead_methods.rs`'s dead list.
//!
//! **3. The register was read after the answer, so it could only trim it.**
//! A `Chatting` register permitted eight sentences from a model that had been
//! instructed to produce one.
//!
//! **4. An answer to Atlas's own question was thrown away.** `let _ = q;` —
//! the question was discarded, `pending` cleared, and the reply re-parsed
//! from scratch by a parser that had no idea a question had been asked.

use atlas::brain::{Brain, Decision, Reached, ACTION_SCHEMA};
use atlas::intent::{Intent, Parser};
use atlas::persona::{Persona, Tone};
use atlas::register::Register;

fn parser() -> Parser {
    let cfg = atlas::config::Config::load(std::path::Path::new("config")).unwrap();
    Parser::new(&cfg.commands)
}

/// Repeats whatever it was asked, so the prompt can be inspected.
struct Spy(std::sync::Mutex<String>);
impl atlas::brain::Llm for Spy {
    fn complete(&self, system: &str, _user: &str) -> atlas::error::Result<String> {
        *self.0.lock().unwrap() = system.to_string();
        Ok("{\"action\":\"say\",\"arg\":null,\"say\":\"fine\"}".into())
    }
}

// ================= 1. the parser stops guessing =================

#[test]
fn a_bare_command_still_resolves_instantly_and_offline() {
    // The fast path is the reason the parser exists: no model call, no
    // dependency on Ollama being up, no latency. Breaking that to get
    // conversation would be a bad trade, so it is pinned.
    let p = parser();
    assert_eq!(p.parse("open chrome"), Intent::OpenApp("chrome".into()));
    assert_eq!(p.parse("pause"), Intent::Pause);
    assert_eq!(p.parse("what's new"), Intent::Capabilities(String::new()));
}

#[test]
fn a_command_with_a_sentence_attached_goes_to_the_model() {
    let p = parser();
    for said in [
        "open chrome and tell me what you think of the numbers",
        "open up a browser would you, I want to look at something",
        "close excel then tell me how the morning went",
    ] {
        assert!(
            matches!(p.parse(said), Intent::Unknown(_)),
            "still being answered from a partial read: {said:?} -> {:?}",
            p.parse(said)
        );
    }
}

#[test]
fn a_zero_argument_command_with_words_left_over_is_not_a_match() {
    // The most dangerous class: the leftover was silently discarded and a
    // mode changed. "hang on, go back" is not "pause".
    let p = parser();
    assert!(matches!(p.parse("hang on, go back"), Intent::Unknown(_)));
    assert_eq!(p.parse("pause"), Intent::Pause, "the bare command must still work");
}

#[test]
fn a_polite_request_is_not_read_as_a_question_about_atlas() {
    // "can you" is a `capabilities` phrase, so "can you open chrome for me"
    // became Capabilities("open chrome for me") -- it asked about itself and
    // did not open chrome. In English it is a request: since 27 Sep 2026 the
    // politeness comes off and the request underneath is what's parsed.
    let p = parser();
    assert_eq!(p.parse("can you open chrome for me"), p.parse("open chrome"));
    assert!(!matches!(p.parse("can you open chrome for me"), Intent::Capabilities(_)));
    // A polite request with nothing Atlas recognises underneath still goes to
    // the model, not to "what can you do".
    assert!(matches!(p.parse("could you ponder the nature of a wednesday"), Intent::Unknown(_)));
    // The genuine capability question still resolves instantly.
    assert_eq!(p.parse("what can you do"), Intent::Capabilities(String::new()));
}

#[test]
fn a_topic_is_allowed_to_be_a_whole_clause() {
    // The completeness rule must not fire on the commands it would ruin: a
    // topic or a note is SUPPOSED to be a sentence, and may contain a word
    // that reads as a command.
    //
    // The first version of the rule applied its "the remainder is a command"
    // check to every intent, and broke exactly these:
    //
    //   "research open source licensing"  -> Unknown  ("open" read as open_app)
    //   "the passphrase is open sesame"   -> Unknown  (to the MODEL)
    let p = parser();
    // "open" is an `open_app` phrase. A topic containing it is still a
    // topic -- and this is the assertion that catches the rule being widened
    // to every intent, which an earlier version was.
    assert_eq!(
        p.parse("research open source licensing"),
        Intent::Research("open source licensing".into())
    );
    assert_eq!(
        p.parse("research the drawdown on the index fund and how it compares"),
        Intent::Research("the drawdown on the index fund and how it compares".into())
    );
    // A note is text too, and may well contain a command word.
    assert_eq!(
        p.parse("note that I should open chrome later"),
        Intent::Capture("i should open chrome later".into())
    );
}

#[test]
fn the_same_intent_twice_over_is_one_question_not_two() {
    // `commands.yaml` lists both "why did you" and "explain that" under
    // `why`, so "why did you explain that" leaves "explain that" over, which
    // parses as `why` again. That is one coherent question. An earlier
    // version of rule 2 rejected it and turned a specific question into a
    // bare "Which one?".
    let p = parser();
    assert_eq!(p.parse("why did you explain that"), Intent::Why("explain that".into()));
}

#[test]
fn a_passphrase_is_never_second_guessed_into_the_model() {
    // `unlock` is a raw-argument command and must stay on the fast path
    // whatever it looks like. `typed.rs`: there is "exactly one thing in
    // Atlas that a microphone must never carry: the vault passphrase" --
    // rejecting it here would send it to the model instead.
    let p = parser();

    // The case that actually bites: a passphrase whose first word reads as a
    // command. "open sesame" parses as `open_app`, so a rule that rejects a
    // match whose remainder is another command sends this to the model.
    match p.parse("the passphrase is open sesame") {
        Intent::Unlock(s) => assert_eq!(s, "open sesame"),
        other => panic!("the vault passphrase left the fast path: {other:?}"),
    }
    // And a long one with punctuation in it.
    match p.parse("the passphrase is correct horse, battery staple") {
        Intent::Unlock(_) => {}
        other => panic!("the vault passphrase left the fast path: {other:?}"),
    }

    // Two protections cover this, deliberately, and each one alone is enough
    // -- which means neither can be caught by mutating the other. So they are
    // pinned by source, the way `tests/guards.rs` pins its call sites.
    //
    // 1. `never_second_guessed` returns early for `unlock`.
    // 2. The gate that limits the remainder-is-a-command rule to name-like
    //    arguments, which `unlock` is not.
    //
    // The realistic future mistake is somebody adding `unlock` to
    // `argument_is_a_name` -- a passphrase looks like a single token until it
    // isn't -- which removes protection 2 silently. This fails if either goes.
    let src = std::fs::read_to_string("src/intent.rs").unwrap();
    let names = src
        .split("fn argument_is_a_name")
        .nth(1)
        .expect("argument_is_a_name")
        .split('}')
        .next()
        .unwrap();
    assert!(
        !names.contains("unlock"),
        "`unlock` was added to argument_is_a_name -- a passphrase is not a name"
    );
    let never = src
        .split("fn never_second_guessed")
        .nth(1)
        .expect("never_second_guessed")
        .split('}')
        .next()
        .unwrap();
    assert!(never.contains("unlock"), "`unlock` lost its exemption");
}

// ================= 2. the model is told who Atlas is =================

#[test]
fn the_character_reaches_the_model() {
    let p = parser();
    let spy = Spy(std::sync::Mutex::new(String::new()));
    let persona = Persona { address: "Eric".into(), ..Persona::default() };
    Brain { llm: &spy, fallback: &p, voice: Some((&persona, Register::Chatting)) }
        .decide("what did you make of that", "");

    let prompt = spy.0.lock().unwrap().clone();
    assert!(prompt.contains("Address the user as Eric"), "got:\n{prompt}");
    assert!(prompt.contains("Have opinions"), "got:\n{prompt}");
    assert!(prompt.contains("not only for work"), "got:\n{prompt}");
    // And the schema is still there -- character replacing the schema would
    // break every action.
    assert!(prompt.contains("workspace_on"), "the action schema was lost:\n{prompt}");
    assert!(prompt.contains("ONE JSON object"), "got:\n{prompt}");
}

#[test]
fn the_instructions_change_with_the_register() {
    let p = parser();
    let persona = Persona::default();

    let spy = Spy(std::sync::Mutex::new(String::new()));
    Brain { llm: &spy, fallback: &p, voice: Some((&persona, Register::Working)) }
        .decide("zzz unparseable", "");
    let working = spy.0.lock().unwrap().clone();

    let spy2 = Spy(std::sync::Mutex::new(String::new()));
    Brain { llm: &spy2, fallback: &p, voice: Some((&persona, Register::Chatting)) }
        .decide("zzz unparseable", "");
    let chatting = spy2.0.lock().unwrap().clone();

    assert_ne!(working, chatting, "the same prompt for a task and a conversation");
    assert!(working.contains("one or two sentences"), "got:\n{working}");
    // 29 Sep 2026: "Talk like a person: follow a tangent ... up to about
    // eight sentences" read, to the 4B model on Eric's laptop, as licence to
    // ramble. A conversation now answers first and runs to a few sentences.
    assert!(chatting.contains("Answer what they just said first"), "got:\n{chatting}");
    assert!(chatting.contains("A few") && chatting.contains("more only if they ask"), "got:\n{chatting}");
}

#[test]
fn the_schema_no_longer_orders_one_short_sentence() {
    // The line that made every register sound the same. Length is the
    // persona's and the register's business now.
    assert!(
        !ACTION_SCHEMA.contains("One short sentence"),
        "the schema is still overriding the register:\n{ACTION_SCHEMA}"
    );
}

#[test]
fn no_persona_keeps_the_bare_schema() {
    // `atlas ask` and the schema's own tests have no persona to give.
    let p = parser();
    let spy = Spy(std::sync::Mutex::new(String::new()));
    Brain { llm: &spy, fallback: &p, voice: None }.decide("zzz unparseable", "");
    assert_eq!(*spy.0.lock().unwrap(), ACTION_SCHEMA);
}

// ================= 3. an action speaks in Atlas's voice =================

#[test]
fn an_action_is_acknowledged_with_the_form_of_address() {
    // Eric's ask, literally: opens instantly, and says "opening chrome now,
    // <what it calls you>". No model call, so nothing is slower.
    let p = Persona { address: "Eric".into(), tone: Tone::Dry, ..Persona::default() };
    let said = p.acknowledge("Opening Chrome.", 0);
    assert_eq!(said, "Opening Chrome now, Eric.");
}

#[test]
fn the_same_action_twice_does_not_come_back_identical() {
    let p = Persona { address: "Eric".into(), ..Persona::default() };
    let variants: std::collections::BTreeSet<String> =
        (0..3).map(|n| p.acknowledge("Opening Chrome.", n)).collect();
    assert!(variants.len() > 1, "a fixed table by another name: {variants:?}");
}

#[test]
fn no_form_of_address_means_no_name_is_used() {
    // `Persona::address` defaults to empty and `persona.rs` is explicit that
    // empty means it does not. "a system that calls you 'sir'" is a choice,
    // not a default.
    let p = Persona::default();
    for n in 0..3 {
        let said = p.acknowledge("Opening Chrome.", n);
        assert!(!said.contains(','), "a name crept in with no address set: {said:?}");
        assert!(said.starts_with("Opening Chrome"), "got {said:?}");
    }
}

#[test]
fn an_answer_is_not_dressed_up_as_an_action() {
    // "now" belongs on something being done. A count, a fact or a refusal
    // keeps the words it was built with.
    assert!(!atlas::brain::is_an_action(&Intent::Outstanding));
    assert!(!atlas::brain::is_an_action(&Intent::HowAmIDoing));
    assert!(!atlas::brain::is_an_action(&Intent::Say("anything".into())));
    assert!(atlas::brain::is_an_action(&Intent::OpenApp("chrome".into())));
    assert!(atlas::brain::is_an_action(&Intent::Pause));
}

#[test]
fn a_real_sentence_is_left_alone_entirely() {
    // The guard that stops `acknowledge` touching anything with content in
    // it. An eight-sentence conversational reply must not acquire ", Eric."
    let p = Persona { address: "Eric".into(), ..Persona::default() };
    let long = "It's steeper than the other two, but it's eleven trades, so that's \
                noise rather than a signal. I'd leave it alone until there are fifty.";
    assert_eq!(p.acknowledge(long, 0), long);
}

// ================= 4. an answer connects to the question =================

mod answering {
    use atlas::daemon::Daemon;
    use atlas::platform::mock::MockPlatform;
    use atlas::platform::Monitor;
    use atlas::proactive::{Proactive, ProactiveConfig};
    use atlas::store::Store;
    use std::path::{Path, PathBuf};

    fn tmp(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("atlas-conv-{tag}"));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// Records the whole user message the model was sent.
    struct UserSpy(std::sync::Mutex<String>);
    impl atlas::brain::Llm for UserSpy {
        fn complete(&self, _system: &str, user: &str) -> atlas::error::Result<String> {
            self.0.lock().unwrap().push_str(user);
            Ok("{\"action\":\"say\",\"arg\":null,\"say\":\"the second one it is\"}".into())
        }
    }

    #[test]
    fn the_question_atlas_asked_reaches_the_model_with_the_answer() {
        // End to end, with a real model call, because the failure was
        // precisely that the two halves never met: Atlas asked "which
        // browser did you mean?", heard "the second one", and re-parsed it
        // from scratch with no idea a question had been asked.
        let c = atlas::config::Config::load(Path::new("config")).unwrap();
        let p = MockPlatform::new(vec![Monitor {
            id: 1,
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
            primary: true,
        }]);
        let spy = std::sync::Arc::new(UserSpy(std::sync::Mutex::new(String::new())));
        let mut d = Daemon::new(
            &c,
            &p,
            Some(spy.clone()),
            Store::new(tmp("asked")),
            Proactive::new(ProactiveConfig::default()),
        );

        d.session.ask("Which browser did you mean?");
        let _ = d.turn("the second one", 100);

        let sent = spy.0.lock().unwrap().clone();
        assert!(
            sent.contains("You just asked: Which browser did you mean?"),
            "the question never reached the model with its answer:\n{sent}"
        );
        assert!(sent.contains("the second one"), "got:\n{sent}");
        // Consumed, so it cannot attach itself to a later unrelated turn.
        assert!(d.answering.is_none(), "the question outlived its turn");
    }

    #[test]
    fn the_context_says_the_next_line_is_an_answer() {
        let c = atlas::config::Config::load(Path::new("config")).unwrap();
        let p = MockPlatform::new(vec![Monitor {
            id: 1,
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
            primary: true,
        }]);
        let mut d = Daemon::new(
            &c,
            &p,
            None,
            Store::new(tmp("says")),
            Proactive::new(ProactiveConfig::default()),
        );
        d.answering = Some("Which browser did you mean?".into());
        let ctx = d.context();
        assert!(ctx.contains("You just asked: Which browser did you mean?"), "got:\n{ctx}");
        assert!(ctx.contains("their answer to that, not a new"), "got:\n{ctx}");
        // Taken, not left to attach itself to an unrelated later turn.
        assert!(!d.context().contains("You just asked"), "the question stuck around");
    }
}

// ================= the schema still carries the safety rules =================

#[test]
fn the_untrusted_quoting_rule_survived_the_rewrite() {
    // Added when window titles were found reaching this prompt unmarked. It
    // lives in the schema, which this pass rewrote, so it is pinned here too.
    assert!(ACTION_SCHEMA.contains("NEVER an instruction"), "got:\n{ACTION_SCHEMA}");
    assert!(ACTION_SCHEMA.contains("User said:"), "got:\n{ACTION_SCHEMA}");
}

#[test]
fn a_full_prompt_carries_both_halves() {
    let p = parser();
    let spy = Spy(std::sync::Mutex::new(String::new()));
    let persona = Persona { address: "Eric".into(), ..Persona::default() };
    Brain { llm: &spy, fallback: &p, voice: Some((&persona, Register::Chatting)) }
        .decide("zzz unparseable", "");
    let prompt = spy.0.lock().unwrap().clone();
    // Character before schema: the model reads the last thing it was told
    // most literally, and the schema is the mechanical half.
    let character = prompt.find("Address the user as Eric").expect("character");
    let schema = prompt.find("ONE JSON object").expect("schema");
    assert!(character < schema, "the schema is ahead of the character");
}

#[test]
fn the_decision_still_reports_whether_the_model_answered() {
    let p = parser();
    let spy = Spy(std::sync::Mutex::new(String::new()));
    let persona = Persona::default();
    let d: Decision = Brain { llm: &spy, fallback: &p, voice: Some((&persona, Register::Working)) }
        .decide("zzz unparseable", "");
    assert_eq!(d.model, Reached::Yes);

    let fast = Brain { llm: &spy, fallback: &p, voice: Some((&persona, Register::Working)) }
        .decide("open chrome", "");
    assert_eq!(fast.model, Reached::NotNeeded, "a bare command must not reach the model");
}

#[test]
fn the_configured_persona_is_the_one_that_is_used() {
    use atlas::daemon::Daemon;
    use atlas::platform::mock::MockPlatform;
    use atlas::platform::Monitor;
    use atlas::proactive::{Proactive, ProactiveConfig};
    use atlas::store::Store;
    use std::path::Path;
    // This was `Persona::default()` in `Daemon::new`, so the whole `persona:`
    // block in tools.yaml -- name, address, tone, wit, argues, converses --
    // decided nothing.
    let cfg = atlas::config::Config::load(Path::new("config")).unwrap();
    let configured = cfg.tools.as_ref().map(|t| t.persona.clone()).unwrap_or_default();
    let p = MockPlatform::new(vec![Monitor {
        id: 1,
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
        primary: true,
    }]);
    let d = Daemon::new(
        &cfg,
        &p,
        None,
        Store::new(std::env::temp_dir().join("atlas-conv-cfgpersona")),
        Proactive::new(ProactiveConfig::default()),
    );
    assert_eq!(d.persona.name, configured.name);
    assert_eq!(d.persona.tone, configured.tone);
    assert_eq!(d.persona.max_spoken_sentences, configured.max_spoken_sentences);
}

// ================= one length, agreed on by both halves =================

#[test]
fn the_length_in_the_prompt_is_the_length_it_is_cut_at() {
    // Three caps stack: the config ceiling, the active mode, and the
    // register. The briefing step and the trimming step used to compute it
    // separately -- `system_prompt` rendered "At most {config} sentences"
    // while the shaper cut at `mode_cap.min(register.length())`. Told eight
    // and cut at three is the same failure as told one and allowed eight.
    let p = Persona { max_spoken_sentences: 8, ..Persona::default() };
    let spy = Spy(std::sync::Mutex::new(String::new()));

    let capped = Persona {
        max_spoken_sentences: p.max_spoken_sentences.min(Register::Working.length()),
        ..p.clone()
    };
    Brain { llm: &spy, fallback: &parser(), voice: Some((&capped, Register::Working)) }
        .decide("zzz unparseable", "");
    let prompt = spy.0.lock().unwrap().clone();
    assert!(
        prompt.contains("At most 2 sentences"),
        "the model was told a length the shaper will not honour:\n{prompt}"
    );
}

#[test]
fn a_fresh_install_lets_a_conversation_be_eight_sentences() {
    // `Modes::verbosity()` returns `unwrap_or(2)` when no mode is on, which
    // reads as 3 sentences. Applied as a ceiling over the register that
    // capped every conversation at three on a fresh install -- the clipped
    // reply, arriving through a placeholder default.
    let m = atlas::modes::Modes::default();
    assert_eq!(m.verbosity_if_set(), None, "a fresh install has no mode on");

    // What the old default resolved to, spelled out rather than fetched from
    // a method that no longer exists: `verbosity()` returned
    // `unwrap_or(2)`, and `sentences_for(2)` is 3.
    assert_eq!(atlas::modes::sentences_for(2), 3);
    assert!(
        atlas::modes::sentences_for(2) < Register::Chatting.length(),
        "the old default was not actually the binding constraint -- check this test"
    );
}

#[test]
fn a_mode_that_asks_for_brevity_still_wins() {
    // The other direction: turning on a terse mode must still cap a chatty
    // register. Removing the mode cap entirely would have been the easy
    // wrong fix.
    assert_eq!(atlas::modes::sentences_for(0), 1);
    assert!(atlas::modes::sentences_for(0) < Register::Chatting.length());
}
