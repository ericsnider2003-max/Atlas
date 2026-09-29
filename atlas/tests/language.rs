use atlas::language::{
    args, live_line, model_facts, next_size_up, notes, plan, LanguageConfig, Listening, Plan, Task,
    Turn,
};

fn cfg() -> LanguageConfig {
    LanguageConfig { my_language: "en".into(), multilingual: true, ..Default::default() }
}

// ================= what a model can actually hear =================

#[test]
fn an_english_only_model_cannot_hear_other_languages_at_all() {
    // Not badly — at all. Worth being exact about, because it looks like a
    // quality problem and isn't.
    let en = model_facts("ggml-base.en.bin");
    assert!(en.english_only);
    match plan("es", &cfg(), &en) {
        Plan::CannotHear { why, .. } => {
            assert!(why.contains("English-only"));
            assert!(why.contains("multilingual one of the same size"), "and the fix: {why}");
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn the_multilingual_model_of_the_same_size_costs_nothing_extra() {
    let en = model_facts("ggml-base.en.bin");
    let multi = model_facts("ggml-base.bin");
    assert_eq!(en.megabytes, multi.megabytes, "same size on disk");
    assert!(!multi.english_only);
}

#[test]
fn a_language_you_dont_speak_comes_back_in_english() {
    let multi = model_facts("ggml-base.bin");
    match plan("es", &cfg(), &multi) {
        Plan::Translate { from } => assert_eq!(from, "es"),
        o => panic!("{o:?}"),
    }
}

#[test]
fn your_own_language_is_just_transcribed() {
    let multi = model_facts("ggml-base.bin");
    assert!(matches!(plan("en", &cfg(), &multi), Plan::Transcribe { .. }));
    assert!(matches!(plan("", &cfg(), &multi), Plan::Transcribe { .. }), "unknown is not foreign");
}

#[test]
fn you_can_ask_for_the_original_rather_than_a_translation() {
    let no_translate = LanguageConfig { translate_others: false, ..cfg() };
    let multi = model_facts("ggml-base.bin");
    match plan("fr", &no_translate, &multi) {
        Plan::Transcribe { language } => assert_eq!(language, "fr"),
        o => panic!("{o:?}"),
    }
}

#[test]
fn translating_is_one_flag_in_the_same_pass() {
    // Whisper does this itself. There is no second model and no second run.
    let a = args("m.bin", "clip.wav", Task::Translate, "auto");
    assert!(a.contains(&"-tr".to_string()));
    assert!(a.iter().filter(|x| *x == "-m").count() == 1, "one pass");
}

#[test]
fn confidence_is_asked_for_so_a_poor_clip_can_be_recognised_as_poor() {
    let a = args("m.bin", "clip.wav", Task::Transcribe, "en");
    assert!(a.contains(&"-oj".to_string()));
    assert!(a.contains(&"-l".to_string()) && a.contains(&"en".to_string()));
}

#[test]
fn auto_detection_is_asked_for_explicitly() {
    let a = args("m.bin", "c.wav", Task::Transcribe, "auto");
    assert!(a.contains(&"auto".to_string()));
}

// ================= accents =================

#[test]
fn a_few_poor_clips_are_not_a_problem() {
    let mut l = Listening::default();
    for c in [0.9, 0.3, 0.88, 0.4, 0.91] {
        l.record(c);
    }
    assert!(!l.struggling(&cfg()));
    assert!(l.suggestion("base", &cfg()).is_none());
}

#[test]
fn consistently_mishearing_you_earns_a_suggestion_with_the_evidence() {
    let mut l = Listening::default();
    for _ in 0..8 {
        l.record(0.32);
    }
    assert!(l.struggling(&cfg()));
    let said = l.suggestion("ggml-base.en.bin", &cfg()).unwrap();
    assert!(said.contains("32%"), "the actual number: {said}");
    assert!(said.contains("small"), "and the specific fix");
    assert!(said.contains("little slower"), "with the honest cost");
}

#[test]
fn it_only_suggests_once() {
    let mut l = Listening::default();
    for _ in 0..8 {
        l.record(0.3);
    }
    assert!(l.suggestion("base", &cfg()).is_some());
    l.suggested_at = Some(100);
    assert!(l.suggestion("base", &cfg()).is_none(), "not every time");
}

#[test]
fn at_the_largest_sensible_model_it_suggests_a_microphone_instead() {
    // Medium is 1.5GB. On 3.5GB of usable memory that's most of the budget
    // for a marginal gain, so it isn't offered.
    assert_eq!(next_size_up("ggml-small.bin"), None);
    let mut l = Listening::default();
    for _ in 0..8 {
        l.record(0.3);
    }
    let said = l.suggestion("ggml-small.bin", &cfg()).unwrap();
    assert!(said.contains("closer microphone"), "got: {said}");
}

#[test]
fn the_typical_reading_ignores_one_bad_clip() {
    let mut l = Listening::default();
    for _ in 0..9 {
        l.record(0.9);
    }
    l.record(0.05);
    assert!(l.typical().unwrap() > 0.8, "median, not mean");
}

// ================= a room with several languages =================

fn conversation() -> Vec<Turn> {
    vec![
        Turn {
            speaker: "you".into(),
            language: "en".into(),
            original: "Can we push the deadline?".into(),
            english: None,
            at: 1,
        },
        Turn {
            speaker: "Marta".into(),
            language: "es".into(),
            original: "No creo que sea posible esta semana.".into(),
            english: Some("I don't think that's possible this week.".into()),
            at: 2,
        },
    ]
}

#[test]
fn what_was_actually_said_is_kept_alongside_the_translation() {
    // A translation is an interpretation. Notes that throw away the original
    // can't be checked later.
    let n = notes(&conversation());
    assert!(n.contains("No creo que sea posible"));
    assert!(n.contains("I don't think that's possible"));
}

#[test]
fn the_notes_say_which_languages_were_in_the_room() {
    let n = notes(&conversation());
    assert!(n.contains("Languages: en, es"));
}

#[test]
fn a_single_language_conversation_does_not_get_a_language_header() {
    let english_only: Vec<Turn> = conversation()
        .into_iter()
        .map(|mut t| {
            t.language = "en".into();
            t.english = None;
            t
        })
        .collect();
    assert!(!notes(&english_only).contains("Languages:"));
}

#[test]
fn a_live_line_names_the_speaker_and_the_language() {
    let turns = conversation();
    assert_eq!(live_line(&turns[1]), "Marta (es) — I don't think that's possible this week.");
    assert_eq!(live_line(&turns[0]), "you: Can we push the deadline?");
}

#[test]
fn understanding_other_languages_is_off_until_you_turn_it_on() {
    let d = LanguageConfig::default();
    assert!(!d.multilingual, "the shipped model is English-only");
    assert_eq!(d.my_language, "auto");
    assert!(d.translate_others, "but if you turn it on, it translates by default");
}

#[test]
fn template_vars_do_nothing_on_the_english_only_default() {
    // The shipped model is English-only. Even with multilingual switched on,
    // an English-only model physically can't do it, so the flags must be
    // empty — the command stays exactly what it was before language was wired.
    use atlas::language::{model_facts, template_vars, LanguageConfig};
    let english = model_facts("models/ggml-base.en.bin");
    let mut cfg = LanguageConfig::default();
    cfg.multilingual = true;
    cfg.my_language = "fr".into();
    let (lang, task) = template_vars(&cfg, &english);
    assert_eq!(lang, "", "an English-only model gets no language flag");
    assert_eq!(task, "transcribe", "and no translate");
}

#[test]
fn template_vars_activate_with_a_multilingual_model() {
    use atlas::language::{model_facts, template_vars, LanguageConfig};
    let multi = model_facts("models/ggml-base.bin"); // no .en -> multilingual
    let mut cfg = LanguageConfig::default();
    cfg.multilingual = true;
    cfg.my_language = "auto".into();
    cfg.translate_others = true;
    let (lang, task) = template_vars(&cfg, &multi);
    assert_eq!(lang, "auto", "a multilingual model lets whisper detect per clip");
    assert_eq!(task, "translate", "and bring other languages back in English");
}

#[test]
fn multilingual_off_stays_inert_even_with_a_multilingual_model() {
    // A multilingual model present but the setting off: still English-only
    // behaviour, because the person hasn't asked for other languages.
    use atlas::language::{model_facts, template_vars, LanguageConfig};
    let multi = model_facts("models/ggml-base.bin");
    let cfg = LanguageConfig::default(); // multilingual: false
    let (lang, task) = template_vars(&cfg, &multi);
    assert_eq!(lang, "");
    assert_eq!(task, "transcribe");
}
