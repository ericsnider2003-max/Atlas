//! `persona.wit`: off, dry, or full ("smart-ass"), Eric's ask of 29 Sep 2026.
//!
//! What these hold: the wit changes wording and never the answer; it never
//! fires on an error, when you're fed up, on anything serious, or in
//! anything written for someone else; the setting keeps its value through
//! the settings page and the preferences file; and saying "tone it down"
//! does what it says.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::persona::Persona;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::register::Register;
use atlas::store::Store;
use atlas::wit::{self, Asked, Canned, Held, Moment, Wit};
use std::path::{Path, PathBuf};

fn persona(w: Wit) -> Persona {
    Persona { wit: w, ..Persona::default() }
}

// ---------------------------------------------------------------- the setting

#[test]
fn the_setting_reads_words_and_the_old_number_and_writes_words() {
    assert_eq!(Wit::parse("smart-ass"), Some(Wit::Full));
    assert_eq!(Wit::parse("Full"), Some(Wit::Full));
    assert_eq!(Wit::parse("off"), Some(Wit::Off));
    assert_eq!(Wit::parse("0.35"), Some(Wit::Dry), "the old shipped value is dry");
    assert_eq!(Wit::parse("0"), Some(Wit::Off));
    assert_eq!(Wit::parse("0.9"), Some(Wit::Full));
    assert_eq!(Wit::parse("loud"), None);
    let p: Persona = serde_yaml::from_str("wit: 0.35").unwrap();
    assert_eq!(p.wit, Wit::Dry);
    let p: Persona = serde_yaml::from_str("wit: full").unwrap();
    assert_eq!(p.wit, Wit::Full);
    assert!(serde_yaml::from_str::<Persona>("wit: loud").is_err(), "a word it doesn't know is an error, not a guess");
    assert_eq!(serde_json::to_string(&Wit::Full).unwrap(), "\"full\"");
}

#[test]
fn the_default_is_what_atlas_already_did() {
    // Dry, because the shipped 0.35 already allowed an occasional aside in
    // conversation: `off` would have taken that away unasked.
    assert_eq!(Persona::default().wit, Wit::Dry);
    let c = Config::load(Path::new("config")).unwrap();
    assert_eq!(c.tools.unwrap().persona.wit, Wit::Dry);
    // Word for word what the prompt said before the setting had levels.
    let p = persona(Wit::Dry).prompt_for(Register::Chatting);
    assert!(p.contains("An occasional dry aside is fine. Rarely, and never instead of the answer."), "{p}");
    assert!(persona(Wit::Dry).for_this_turn_on(Register::Chatting, 8, "", false).contains(" A dry aside is fine when it's actually funny."));
    // And a canned reply at dry is exactly what it was.
    let d = persona(Wit::Dry);
    for seed in 0..8 {
        assert_eq!(d.acknowledge_in("Opening Chrome.", seed, Register::Working, false), d.acknowledge("Opening Chrome.", seed));
        assert_eq!(d.social("thanks", 9, seed, false), atlas::persona::social_reply("thanks", 9));
    }
}

#[test]
fn it_is_on_the_settings_page_under_how_it_talks_back_and_round_trips() {
    let c = Config::load(Path::new("config")).unwrap();
    let t = c.tools.unwrap();
    let mut s = atlas::settings::registry(&t);
    let wit = s.get("persona.wit").unwrap().clone();
    assert_eq!(wit.group, "How it talks back");
    match &wit.value {
        atlas::settings::Value::Choice { value, options } => {
            assert_eq!(value, "dry");
            assert_eq!(options, &vec!["off".to_string(), "dry".into(), "full".into()]);
        }
        other => panic!("wit should be a choice: {other:?}"),
    }
    assert!(!wit.changed());
    assert!(s.set("persona.wit", "loud").is_err());
    assert_eq!(s.set("persona.wit", "full").unwrap(), "Wit is now full");

    // Kept in the preferences file, and read back into the config -- also
    // over an older tools.yaml that still holds the number.
    let dir = std::env::temp_dir().join("atlas-wit-roundtrip");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let said = atlas::settings::registry(&t).set_and_keep("persona.wit", "full", &dir);
    assert_eq!(said, "Wit is now full");
    let prefs = atlas::preferences::Preferences::load(&dir);
    for old in ["persona:\n  wit: 0.35\n", "persona:\n  wit: dry\n"] {
        let mut yaml: serde_yaml::Value = serde_yaml::from_str(old).unwrap();
        assert!(prefs.apply_to(&mut yaml).is_empty(), "placed over {old:?}");
        let p: Persona = serde_yaml::from_value(yaml["persona"].clone()).unwrap();
        assert_eq!(p.wit, Wit::Full, "over {old:?}");
    }
    // Any other number slot still refuses a word.
    let mut other: serde_yaml::Value = serde_yaml::from_str("persona:\n  max_spoken_sentences: 8\n").unwrap();
    let mut bad = atlas::preferences::Preferences::default();
    bad.set("persona.max_spoken_sentences", "lots");
    assert_eq!(bad.apply_to(&mut other), vec!["persona.max_spoken_sentences".to_string()]);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------- the fence

fn chat(said: &'static str) -> Moment<'static> {
    Moment::new(Register::Chatting, said, "")
}

#[test]
fn full_wit_adds_a_clause_after_the_answer_and_never_changes_it() {
    let plain = "Opening Chrome now.";
    let mut dressed_any = false;
    for seed in 0..12 {
        for level in [Wit::Off, Wit::Dry, Wit::Full] {
            let out = wit::dress(level, plain, Canned::Done, &Moment::new(Register::Working, "open chrome", ""), seed);
            assert!(out.starts_with(plain), "the answer comes first and whole: {out}");
            if level != Wit::Full {
                assert_eq!(out, plain, "{level:?} leaves canned replies alone");
            } else if out != plain {
                dressed_any = true;
                let tail = out[plain.len()..].trim();
                assert!(tail.split_whitespace().count() <= 8, "at most a clause: {tail}");
                assert_eq!(tail.matches(['.', '!', '?']).count(), 1, "one remark: {tail}");
            }
        }
    }
    assert!(dressed_any, "full does dress some replies");
}

#[test]
fn no_wit_on_errors_when_frustrated_on_serious_things_or_for_someone_else() {
    let level = Wit::Full;
    // Every seed, so no lucky one slips a remark through.
    for seed in 0..12 {
        let cases: Vec<(&str, Moment, Held)> = vec![
            ("I couldn't open Chrome: it isn't installed.", Moment::new(Register::Working, "open chrome", ""), Held::AnError),
            ("Opening Chrome now.", Moment::new(Register::Rough, "that didn't work", ""), Held::Rough),
            ("Any time.", Moment::new(Register::Chatting, "thanks for sorting the rent payment", ""), Held::Serious),
            ("Done.", Moment::new(Register::Chatting, "change my vault passphrase", ""), Held::Serious),
            ("Noted.", Moment::new(Register::Chatting, "the doctor says it's nothing", ""), Held::Serious),
            ("Noted.", Moment::new(Register::Chatting, "my grandmother died", ""), Held::Serious),
            ("Noted.", Moment::new(Register::Chatting, "it costs $40", ""), Held::Serious),
            ("Hi Maya, the files are attached.", Moment { for_someone_else: true, ..chat("") }, Held::NotYours),
        ];
        for (plain, m, why) in cases {
            let out = wit::dress(level, plain, Canned::Done, &m, seed);
            assert_eq!(out, plain, "no remark on {plain:?}");
            assert_eq!(wit::holds_back(level, &Moment { reply: plain, ..m }), Some(why), "{plain:?}");
        }
    }
    // Words, not substrings: painting is not pain, and "bill" in Billie is not money.
    assert!(!wit::is_serious("I'm painting the kitchen with Billie"));
    assert!(wit::is_serious("the pain is back"));
}

#[test]
fn the_model_is_told_no_jokes_when_it_matters_and_never_asked_for_one_in_a_draft() {
    let full = persona(Wit::Full);
    assert!(full.for_this_turn_on(Register::Chatting, 8, "what did you think of the film", false).contains("smart-ass"));
    let money = full.for_this_turn_on(Register::Chatting, 8, "how do I pay off this loan", false);
    assert!(money.contains("No jokes") && !money.contains("smart-ass"), "{money}");
    let rough = full.for_this_turn_on(Register::Rough, 3, "it's broken again", false);
    assert!(!rough.contains("smart-ass") && rough.contains("no jokes"), "{rough}");
    assert!(!full.prompt_for(Register::Rough).contains("smart-ass"));
    assert!(persona(Wit::Off).for_this_turn_on(Register::Chatting, 8, "hello", false).contains("No jokes"));
    // Drafts and messages to other people are written without the persona
    // at all, so no wit setting can reach them.
    for f in ["src/draft.rs", "src/outreach.rs", "src/messaging.rs", "src/outbox.rs"] {
        let src = std::fs::read_to_string(f).unwrap();
        assert!(!src.contains("prompt_for(") && !src.contains("for_this_turn") && !src.contains("wit::"), "{f} reads the persona's wit");
    }
}

#[test]
fn answer_content_is_identical_across_levels() {
    // The social replies and the acknowledgements: the plain words are a
    // prefix of every level's.
    for said in ["hello", "thanks", "bye", "good morning"] {
        let base = atlas::persona::social_reply(said, 9).unwrap();
        for level in [Wit::Off, Wit::Dry, Wit::Full] {
            for seed in 0..6 {
                let got = persona(level).social(said, 9, seed, false).unwrap();
                assert!(got.starts_with(&base), "{level:?}: {got}");
            }
        }
        // After a failure, no tail whatever the level.
        for seed in 0..6 {
            assert_eq!(persona(Wit::Full).social(said, 9, seed, true).unwrap(), base);
        }
    }
}

// ---------------------------------------------------------------- by voice

#[test]
fn the_voice_commands_are_whole_sentences() {
    assert_eq!(wit::level_asked("Be more of a smart-ass"), Some(Asked::Up));
    assert_eq!(wit::level_asked("atlas, be more of a smartass please"), Some(Asked::Up));
    assert_eq!(wit::level_asked("Tone it down."), Some(Asked::Down));
    assert_eq!(wit::level_asked("no more jokes"), Some(Asked::To(Wit::Off)));
    assert_eq!(wit::level_asked("be a smart ass"), Some(Asked::To(Wit::Full)));
    assert_eq!(wit::level_asked("tone it down in the email to Maya"), None);
    assert_eq!(wit::level_asked("write a funny poem"), None);
    assert_eq!(Asked::Up.from(Wit::Dry), Wit::Full);
    assert_eq!(Asked::Down.from(Wit::Full), Wit::Dry);
    assert_eq!(Asked::Down.from(Wit::Dry), Wit::Off);
}

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-wit-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn saying_it_changes_it() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("voice")), Proactive::new(ProactiveConfig::default()));
    assert_eq!(d.persona.wit, Wit::Dry);
    let r = d.turn("be more of a smart-ass", 1_000);
    assert_eq!(d.persona.wit, Wit::Full, "{r}");
    assert!(r.contains("Smart-ass it is"), "{r}");
    let r = d.turn("be more of a smart-ass", 1_010);
    assert!(r.contains("already"), "{r}");
    let r = d.turn("tone it down", 1_020);
    assert_eq!(d.persona.wit, Wit::Dry, "{r}");
    let r = d.turn("no more jokes", 1_030);
    assert_eq!(d.persona.wit, Wit::Off, "{r}");
    // "Thanks" at off is the plain reply.
    assert_eq!(d.turn("thanks", 1_040), "Any time.");
}

// ---------------------------------------------------------------- mid-flow, and kept

#[test]
fn never_in_the_middle_of_a_security_vault_or_confirmation_step() {
    let full = persona(Wit::Full);
    // The same acknowledgement that full dresses on some seed is left plain
    // on every seed while a step is under way.
    let dressed_somewhere = (0..12).any(|s| full.acknowledge_in("Opening Chrome.", s, Register::Working, false) != full.acknowledge("Opening Chrome.", s));
    assert!(dressed_somewhere, "full does dress this reply outside a flow");
    for seed in 0..12 {
        assert_eq!(full.acknowledge_in("Opening Chrome.", seed, Register::Working, true), full.acknowledge("Opening Chrome.", seed));
        assert_eq!(full.social("thanks", 9, seed, true), atlas::persona::social_reply("thanks", 9));
    }
    assert_eq!(wit::holds_back(Wit::Full, &Moment::new(Register::Chatting, "", "Done.").during_a_flow(true)), Some(Held::MidFlow));
    // The model, mid-flow, is told no jokes rather than invited to make one.
    let turn = full.for_this_turn_on(Register::Chatting, 8, "what did you think of the film", true);
    assert!(turn.contains("No jokes") && !turn.contains("smart-ass"), "{turn}");
    // Commands that are themselves the step stay plain whatever they say.
    use atlas::intent::Intent;
    for i in [Intent::SignIn("github".into()), Intent::TwoFactor("on".into()), Intent::Unlock(String::new()), Intent::TypeCode("123456".into()), Intent::MoneyAdvice(String::new())] {
        assert!(wit::fenced_intent(&i), "{i:?}");
    }
    assert!(!wit::fenced_intent(&Intent::OpenApp("chrome".into())));
}

#[test]
fn the_voice_command_goes_through_the_phrase_table_as_its_own_command() {
    let c = Config::load(Path::new("config")).unwrap();
    let parser = atlas::intent::Parser::new(&c.commands);
    for s in ["be more of a smart-ass", "tone it down", "no more jokes", "Atlas, dial it back please"] {
        assert!(matches!(parser.parse(s), atlas::intent::Intent::Wit(_)), "{s}: {:?}", parser.parse(s));
    }
    // A longer sentence with the same words is someone else's.
    assert!(!matches!(parser.parse("tone it down in the email to Maya"), atlas::intent::Intent::Wit(_)));
}

#[test]
fn said_out_loud_it_is_kept_in_the_settings_file_and_survives_a_restart() {
    // A copy of the shipped config under a watched folder, the way Atlas
    // runs for real.
    let dir = tmp("kept-config");
    for f in ["tools.yaml", "apps.yaml", "layouts.yaml", "commands.yaml", "policy.yaml", "indexing.yaml"] {
        let from = Path::new("config").join(f);
        if from.is_file() {
            std::fs::copy(&from, dir.join(f)).unwrap();
        }
    }
    let c = Config::load(&dir).unwrap();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("kept-store")), Proactive::new(ProactiveConfig::default())).watch_settings(dir.clone());
    let r = d.turn("be a smart-ass", 2_000);
    assert_eq!(d.persona.wit, Wit::Full, "{r}");
    let kept = std::fs::read_to_string(atlas::preferences::Preferences::file(&dir)).unwrap();
    assert!(kept.contains("persona.wit") && kept.contains("full"), "kept in the settings file: {kept}");
    // The hub's settings page reads the same value.
    assert_eq!(atlas::settings::registry(&d.tools_cfg()).get("persona.wit").unwrap().value, atlas::settings::Value::Choice { value: "full".into(), options: wit::LEVELS.iter().map(|s| s.to_string()).collect() });
    // A fresh start reads it back.
    let again = Config::load(&dir).unwrap();
    let d2 = Daemon::new(&again, &p, None, Store::new(tmp("kept-store2")), Proactive::new(ProactiveConfig::default())).watch_settings(dir.clone());
    assert_eq!(d2.persona.wit, Wit::Full);
    let _ = std::fs::remove_dir_all(&dir);
}
