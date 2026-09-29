use atlas::tts::{
    adjust, audition_line, catalogue, find, interpret, Change, Quality,
    VoiceSettings,
};

fn now() -> VoiceSettings {
    VoiceSettings::default()
}

// ================= none of this costs anything =================

#[test]
fn the_choice_is_short_enough_to_actually_be_a_choice() {
    // A list of ninety voices is a chore, not an option.
    let c = catalogue();
    assert!(c.len() <= 8 && c.len() >= 5);
    assert!(c.iter().any(|v| v.accent == "British"));
    assert!(c.iter().any(|v| v.accent == "American"));
    assert!(c.iter().all(|v| !v.character.is_empty()), "each one says how it reads");
}

#[test]
fn better_voices_are_bigger_and_that_is_stated() {
    assert!(Quality::High.megabytes() > Quality::Medium.megabytes());
    assert!(Quality::Low.megabytes() < Quality::Medium.megabytes());
}

// ================= changing it by asking =================

#[test]
fn slower_and_faster_do_what_they_say() {
    let (next, said) = adjust(&now(), "slower");
    assert!(next.speed > now().speed);
    assert!(said.starts_with("Slower"));

    let (faster, _) = adjust(&now(), "faster");
    assert!(faster.speed < now().speed);
}

#[test]
fn adjustments_are_small_because_you_will_just_say_it_again() {
    // Overshooting is more annoying than undershooting.
    let (next, _) = adjust(&now(), "slower");
    assert!((next.speed - now().speed).abs() < 0.2);
}

#[test]
fn nothing_can_be_driven_into_being_unusable() {
    let mut v = now();
    for _ in 0..20 {
        v = adjust(&v, "slower").0;
    }
    assert!(v.speed <= 1.6);
    let mut f = now();
    for _ in 0..20 {
        f = adjust(&f, "faster").0;
    }
    assert!(f.speed >= 0.6);
    assert!(adjust(&v, "slower").1.contains("as slow as I go"));
}

#[test]
fn calling_it_robotic_gives_it_more_life() {
    match interpret("you sound really robotic", &now()) {
        Change::Adjust { what } => assert_eq!(what, "more variation"),
        o => panic!("{o:?}"),
    }
    let (next, said) = adjust(&now(), "more variation");
    assert!(next.variation > now().variation);
    assert!(said.contains("more life"));
}

#[test]
fn too_much_gives_it_less() {
    match interpret("that's a bit dramatic, calm down", &now()) {
        Change::Adjust { what } => assert_eq!(what, "less variation"),
        o => panic!("{o:?}"),
    }
}

#[test]
fn asking_for_a_british_voice_gets_one() {
    match interpret("use a British voice", &now()) {
        Change::Switch { to, .. } => assert_eq!(to.accent, "British"),
        o => panic!("{o:?}"),
    }
}

#[test]
fn asking_for_a_deeper_one_gets_a_deeper_one() {
    match interpret("try something deeper", &now()) {
        Change::Switch { to, why } => {
            assert!(to.character.contains("low") || to.character.contains("dry"), "{to:?}");
            assert!(!why.is_empty(), "and says what it's like");
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn naming_a_voice_switches_straight_to_it() {
    match interpret("use Amy", &now()) {
        Change::Switch { to, .. } => assert_eq!(to.name, "Amy"),
        o => panic!("{o:?}"),
    }
}

#[test]
fn asking_for_better_offers_the_best_one_with_its_cost() {
    match interpret("can you sound more natural", &now()) {
        Change::Switch { to, why } => {
            assert_eq!(to.quality, Quality::High);
            assert!(why.contains("MB") && why.contains("slower"), "the honest trade: {why}");
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn asking_for_a_different_voice_plays_a_few_rather_than_picking_one() {
    match interpret("use a different voice", &now()) {
        Change::Audition(choices) => {
            assert_eq!(choices.len(), 3);
            assert!(choices.iter().all(|v| v.id != now().voice), "not the one you have");
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn the_audition_says_something_it_would_actually_say() {
    // You're choosing how it will sound saying the things it says, not
    // "testing one two three".
    let line = audition_line(&catalogue()[0]);
    assert!(line.contains("Ryan"), "it names itself");
    assert!(line.contains("nine o'clock post"), "in a real sentence");
}

#[test]
fn something_it_cannot_do_says_so_rather_than_picking_at_random() {
    match interpret("give me a Scottish voice", &now()) {
        Change::Switch { .. } => panic!("there isn't one"),
        Change::Unclear(_) | Change::Audition(_) => {}
        o => panic!("{o:?}"),
    }
}

#[test]
fn an_unrelated_sentence_changes_nothing() {
    assert!(matches!(interpret("what's the weather", &now()), Change::Unclear(_)));
}

// ================= what it is now =================

#[test]
fn atlas_can_say_what_voice_it_is_using() {
    // Named explicitly rather than taken from the default. The default voice
    // is now a Chatterbox reference clip, which has no catalogue entry and so
    // no accent to describe — this test is about `describe`, not about which
    // voice happens to ship.
    let ryan = VoiceSettings { voice: "en_US-ryan-medium".into(), ..now() };
    let said = ryan.describe();
    assert!(said.contains("Ryan") && said.contains("American"));
    assert!(said.contains("normal pace"));

    let slow = VoiceSettings { speed: 1.4, ..ryan };
    assert!(slow.describe().contains("slow"));
}

#[test]
fn the_settings_reach_piper_as_plain_arguments() {
    // `args()` builds piper's command line specifically, so it is given a
    // piper voice. The live command comes from `tools.yaml` now, and this
    // stays as the check that piper's own argument shape is still right for
    // when you switch back to it.
    let ryan = VoiceSettings { voice: "en_US-ryan-medium".into(), ..now() };
    let a = ryan.args("models", "out.wav");
    assert!(a.contains(&"models/en_US-ryan-medium.onnx".to_string()));
    assert!(a.contains(&"--length_scale".to_string()));
    assert!(a.contains(&"--noise_scale".to_string()));
    assert!(!a.iter().any(|x| x.contains("api") || x.contains("key")), "nothing to sign up for");
}

#[test]
fn a_voice_can_be_found_by_name_or_by_id() {
    assert_eq!(find("Alan").unwrap().accent, "British");
    assert_eq!(find("en_US-amy-medium").unwrap().name, "Amy");
    assert!(find("nonsense").is_none());
}

// ===========================================================================
// The engine, and the settings that never reached it.
//
// `tts.rs` had `Engine`, `EngineConfig`, `voice_extension`, `speed_value` and
// `is_consistent` — a complete pluggable abstraction — and `EngineConfig` was
// not part of any config struct. Nothing constructed it and nothing read it.
//
// Downstream of that, three things were quietly disconnected:
//
//   1. `tts.args` was `["-m", "{tts_model}", "-f", "{out_wav}"]`, so `speed`,
//      `variation` and `sentence_gap` stored numbers and changed nothing you
//      could hear.
//   2. `{tts_model}` named a *different voice* than `voice_settings.voice`,
//      so the voice you chose lost to a variable you never edited.
//   3. `speed_value` — the conversion that stops a speed setting inverting
//      when you change engine — was never applied anywhere.
// ===========================================================================

use atlas::tts::{Engine, EngineConfig};

#[test]
fn the_shipped_config_names_an_engine_and_agrees_with_itself() {
    // An engine and an executable that disagree is a fault that only shows up
    // as silence.
    let c = atlas::config::Config::load(std::path::Path::new("config")).unwrap();
    let t = c.tools.as_ref().expect("tools.yaml loads");
    assert!(
        t.tts_engine.is_consistent(),
        "engine {:?} and exe {:?} disagree",
        t.tts_engine.engine,
        t.tts_engine.exe
    );
}

#[test]
fn the_speech_command_uses_the_voice_you_chose() {
    // The specific bug: the command took its voice from `{tts_model}` while
    // Settings wrote to `voice_settings.voice`, so changing your voice did
    // nothing and the reason was invisible.
    let c = atlas::config::Config::load(std::path::Path::new("config")).unwrap();
    let t = c.tools.as_ref().unwrap();
    let joined = t.tts.args.join(" ");
    // Either form counts. An engine that copies a voice from a recording needs
    // the file (`{voice_file}`); one with preset voices needs the name
    // (`{voice_id}`). What must never come back is a separate model var that
    // ignores your choice entirely.
    assert!(
        joined.contains("{voice_file}") || joined.contains("{voice_id}"),
        "the voice isn't in the command: {joined}"
    );
    assert!(
        !joined.contains("{tts_model}"),
        "the command still uses the old model var, which ignores your chosen voice: {joined}"
    );
}

#[test]
fn the_speech_command_carries_the_settings_that_exist_to_change_it() {
    let c = atlas::config::Config::load(std::path::Path::new("config")).unwrap();
    let t = c.tools.as_ref().unwrap();
    let joined = t.tts.args.join(" ");
    // Speed and sentence gap mean something to every engine here.
    for v in ["{speed}", "{sentence_gap}"] {
        assert!(joined.contains(v), "{v} never reaches the engine: {joined}");
    }
    // `variation` is expressiveness, and only an engine that has such a
    // control can use it. Kokoro is decoder-only with no expressive knob, so
    // demanding it here would force a flag that does nothing — which is the
    // fault this whole area was fixing, in the other direction.
    if t.tts_engine.engine.can_clone() {
        assert!(joined.contains("{variation}"), "a cloning engine ignores variation: {joined}");
    }
}

#[test]
fn speed_is_converted_for_whichever_engine_is_named() {
    // piper takes a length scale, where larger is slower -- the same as the
    // setting, which is a pace (`adjust` adds to it for "slower"). Everything
    // else takes a multiplier, where larger is faster. Handing one to the
    // other silently inverts every speed you have ever chosen.
    //
    // Updated 28 Sep 2026: this asserted piper got the reciprocal (0.8 ->
    // 1.25) under a comment calling a *longer* scale faster. A longer piper
    // length scale is slower, so "a bit slower" was making piper faster.
    let faster = 0.8_f32;
    let piper = Engine::Piper.speed_value(faster);
    let kokoro = Engine::Kokoro.speed_value(faster);
    assert_eq!(piper, faster, "piper's length scale is already a pace");
    assert!(kokoro > 1.0, "a multiplier engine needs a larger number to speak faster: {kokoro}");
    assert_ne!(piper, kokoro, "the same setting reached both engines unchanged");
}

#[test]
fn a_voice_file_takes_the_extension_its_engine_expects() {
    for (engine, ext) in [(Engine::Piper, "onnx"), (Engine::Kokoro, "pt"), (Engine::Chatterbox, "wav")] {
        let cfg = EngineConfig { engine, exe: String::new(), voices_dir: "models".into() };
        let f = cfg.voice_file_for("some-voice");
        assert!(f.ends_with(ext), "{engine:?} asked for {f}");
    }
}

#[test]
fn a_voice_you_downloaded_yourself_still_resolves() {
    // The id in your settings is not always a catalogue entry. Requiring one
    // would mean silently falling back to a different voice, which is the
    // fault being fixed.
    let cfg = EngineConfig::default();
    let f = cfg.voice_file_for("something-not-in-the-catalogue");
    assert!(f.contains("something-not-in-the-catalogue"), "{f}");
}

#[test]
fn a_mismatched_engine_and_executable_is_caught() {
    let bad = EngineConfig {
        engine: Engine::Kokoro,
        exe: "tools/piper/piper.exe".into(),
        voices_dir: "models".into(),
    };
    assert!(!bad.is_consistent(), "a kokoro engine driving piper was accepted");
    // And voicebox counts as chatterbox, which is what it is.
    let vb = EngineConfig {
        engine: Engine::Chatterbox,
        exe: "tools/voicebox/voicebox.exe".into(),
        voices_dir: "models".into(),
    };
    assert!(vb.is_consistent());
}

#[test]
fn piper_stays_the_default_until_something_else_is_proven() {
    // Defaulting to the better engine and failing is worse than defaulting to
    // the plainer one and working. Atlas does not pick the upgrade for you.
    assert_eq!(EngineConfig::default().engine, Engine::Piper);
}

#[test]
fn doctor_names_the_voice_that_will_actually_be_used() {
    // It used to check `tts_model` — a file nothing read once the settings
    // were wired — so it would tell you to download amy while ryan was the
    // voice you had chosen.
    let c = atlas::config::Config::load(std::path::Path::new("config")).unwrap();
    let p = atlas::platform::mock::MockPlatform::new(vec![]);
    let findings = atlas::doctor::run(&c, c.tools.as_ref(), &p);
    let e = findings
        .iter()
        .find(|f| f.label == "speech engine")
        .expect("doctor never says which engine is driving speech");
    let chosen = &c.tools.as_ref().unwrap().voice_settings.voice;
    assert!(
        e.detail.contains(chosen.as_str()) || e.detail.contains("disagree"),
        "it doesn't name the chosen voice: {}",
        e.detail
    );
    assert!(
        !findings.iter().any(|f| f.label == "model 'tts_model'"),
        "doctor still checks a voice file nothing reads"
    );
}
