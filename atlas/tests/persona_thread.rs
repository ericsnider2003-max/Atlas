use atlas::anticipate::{matches, ready_line, suggested, Anticipator, Moment, Rule, Trigger};
use atlas::persona::{strip_filler, trim_to_sentences, Persona, Tone};
use atlas::store::Store;
use atlas::thread::{Thread, ThreadConfig};

// ================= the voice =================

#[test]
fn atlas_never_opens_with_filler() {
    for s in [
        "Great question! Chrome is open.",
        "Absolutely, Chrome is open.",
        "I'd be happy to help. Chrome is open.",
        "Certainly! Chrome is open.",
    ] {
        let out = strip_filler(s);
        assert_eq!(out, "Chrome is open.", "from {s:?}");
    }
}

#[test]
fn stacked_openers_are_all_removed() {
    // Models stack these — "Absolutely! Great question. Here you go."
    assert_eq!(strip_filler("Absolutely! Great question. Chrome is open."), "Chrome is open.");
}

#[test]
fn mid_sentence_padding_is_removed() {
    let out = strip_filler("It's worth noting that the build failed.");
    assert_eq!(out, "The build failed.");
}

#[test]
fn a_reply_with_no_filler_is_left_alone() {
    assert_eq!(strip_filler("Workspace online."), "Workspace online.");
    assert_eq!(strip_filler("I can't do that yet."), "I can't do that yet.");
}

#[test]
fn spoken_replies_are_capped_in_length() {
    let p = Persona::default();
    let long = "One. Two. Three. Four. Five. Six.";
    assert_eq!(p.shape(long), "One. Two. Three.");
}

#[test]
fn markdown_is_flattened_because_it_gets_read_aloud() {
    let p = Persona::default();
    let out = p.spoken("## Results\n- First item\n- Second item\n1. Third");
    assert!(!out.contains('#') && !out.contains('-'), "got: {out}");
    assert!(out.contains("Results"));
}

#[test]
fn code_fences_never_reach_the_speaker() {
    let p = Persona::default();
    let out = p.spoken("Here it is.\n```rust\nfn main() {}\n```");
    assert!(!out.contains("```"), "got: {out}");
}

#[test]
fn the_character_rules_are_actually_in_the_prompt() {
    let p = Persona::default();
    let prompt = p.system_prompt();
    for expected in ["Never flatter", "No greeting", "Disagree", "state limits", "3 sentences"] {
        assert!(
            prompt.to_lowercase().contains(&expected.to_lowercase()),
            "prompt is missing {expected:?}"
        );
    }
}

#[test]
fn atlas_does_not_greet_by_default() {
    let p = Persona::default();
    assert!(!p.greet, "Jarvis doesn't say hello");
    assert_eq!(p.resume(None), "", "silence is the default continuation");
    assert_eq!(p.resume(Some("the video edit")), "We were on the video edit.");
}

#[test]
fn tone_and_voice_are_configurable() {
    let warm = Persona { tone: Tone::Warm, ..Default::default() };
    assert!(warm.system_prompt().contains("Friendly"));
    assert!(Persona::default().system_prompt().contains("dry"));
    assert!(!Persona::default().voice_model.is_empty());
}

#[test]
fn trimming_handles_text_with_no_punctuation() {
    assert_eq!(trim_to_sentences("no full stop here", 2), "no full stop here");
}

// ================= one continuous conversation =================

fn cfg() -> ThreadConfig {
    ThreadConfig::default()
}

#[test]
fn there_is_no_session_boundary_only_a_gap() {
    let mut t = Thread::default();
    t.append("boot workspace", "Workspace online.", Some("the workspace".into()), 100);
    // Days later.
    assert!(t.resume_line(&cfg(), 100 + 3 * 86_400).unwrap().contains("been a while"));
    assert!(t.resume_line(&cfg(), 100 + 60).is_none(), "no ceremony for a short gap");
}

#[test]
fn coming_back_names_what_you_were_doing_rather_than_greeting() {
    let mut t = Thread::default();
    t.append("edit that video", "Three cuts, 120s down to 39s.", Some("the video edit".into()), 0);
    let line = t.resume_line(&cfg(), 7200).unwrap();
    assert_eq!(line, "We were on the video edit.");
    assert!(!line.to_lowercase().contains("hello"));
    assert!(!line.to_lowercase().contains("how can i help"));
}

#[test]
fn old_exchanges_are_compressed_not_discarded() {
    // Dropping old turns is what makes an assistant feel amnesiac.
    let mut t = Thread::default();
    for i in 0..30 {
        t.append(&format!("thing {i}"), "ok", None, i);
    }
    assert!(t.needs_folding(&cfg()));
    let to_fold = t.foldable(&cfg()).len();
    assert_eq!(to_fold, 30 - cfg().verbatim);

    t.fold("Discussed 18 things; nothing outstanding.", &cfg());
    assert_eq!(t.recent.len(), cfg().verbatim);
    assert_eq!(t.folded, 18);
    assert_eq!(t.len(), 30, "the thread still knows how long it is");
    assert!(t.context(&cfg()).contains("Discussed 18 things"));
}

#[test]
fn the_summary_and_the_recent_turns_both_reach_the_model() {
    let mut t = Thread::default();
    for i in 0..30 {
        t.append(&format!("thing {i}"), "ok", None, i);
    }
    t.fold("Earlier work on the trading audit.", &cfg());
    let c = t.context(&cfg());
    assert!(c.contains("Earlier: Earlier work on the trading audit."));
    assert!(c.contains("thing 29"), "the newest exchange must be there");
}

#[test]
fn context_stays_inside_its_character_budget() {
    let mut t = Thread::default();
    for i in 0..40 {
        t.append(&"x".repeat(400), &format!("reply {i}"), None, i);
    }
    let small = ThreadConfig { context_chars: 1500, ..cfg() };
    assert!(t.context(&small).len() < 2200, "budget must hold");
}

#[test]
fn the_newest_exchange_is_last_so_the_model_reads_it_as_now() {
    let mut t = Thread::default();
    t.append("first", "a", None, 1);
    t.append("second", "b", None, 2);
    let c = t.context(&cfg());
    assert!(c.find("first").unwrap() < c.find("second").unwrap());
}

#[test]
fn folding_input_includes_the_previous_summary_so_nothing_is_lost() {
    let mut t = Thread::default();
    for i in 0..30 {
        t.append(&format!("a{i}"), "ok", None, i);
    }
    t.fold("First summary.", &cfg());
    for i in 0..30 {
        t.append(&format!("b{i}"), "ok", None, 100 + i);
    }
    let input = t.fold_input(&cfg());
    assert!(input.contains("First summary."), "the old summary must carry forward");
    assert!(input.contains("b0"));
}

#[test]
fn asking_the_same_thing_twice_is_noticed() {
    let mut t = Thread::default();
    t.append("what is the homelab audit about", "It's a production readiness review.", None, 1);
    t.append("something else entirely", "ok", None, 2);
    t.append("What is the HOMELAB audit about?", "...", None, 3);
    assert!(t.asked_before("what is the homelab audit about").is_some());
    assert!(t.asked_before("hi").is_none(), "too short to judge");
}

#[test]
fn the_thread_survives_a_restart() {
    let d = std::env::temp_dir().join("atlas-thread-test");
    let _ = std::fs::remove_dir_all(&d);
    let s = Store::new(&d);
    let mut t = Thread::default();
    t.append("edit that video", "done", Some("the video edit".into()), 100);
    t.save(&s).unwrap();

    let back = Thread::load(&s);
    assert_eq!(back.current_topic.as_deref(), Some("the video edit"));
    assert_eq!(back.resume_line(&cfg(), 100 + 7200).unwrap(), "We were on the video edit.");
}

// ================= doing it before you ask =================

fn moment() -> Moment {
    Moment { can_afford_work: true, ..Default::default() }
}

#[test]
fn everything_is_off_until_you_turn_it_on() {
    // An assistant that starts doing things unasked on day one is alarming.
    assert!(suggested().iter().all(|r| !r.enabled));
}

#[test]
fn a_daily_rule_fires_in_its_window_and_not_outside_it() {
    let mut a = Anticipator::default();
    a.add(Rule {
        name: "brief".into(),
        trigger: Trigger::Daily { hour: 7, minute: 0, days: vec![0, 1, 2, 3, 4] },
        command: "research news".into(),
        announce: true,
        cooldown_secs: 20 * 3600,
        enabled: true,
        last_fired: 0,
    });
    let m = Moment { minutes_of_day: 7 * 60 + 2, weekday: 2, ..moment() };
    assert_eq!(a.due(&m, 1000).len(), 1);

    let late = Moment { minutes_of_day: 9 * 60, weekday: 2, ..moment() };
    assert!(a.due(&late, 200_000).is_empty(), "outside the window");
}

#[test]
fn a_weekday_rule_does_not_fire_at_the_weekend() {
    let mut a = Anticipator::default();
    a.add(Rule {
        name: "brief".into(),
        trigger: Trigger::Daily { hour: 7, minute: 0, days: vec![0, 1, 2, 3, 4] },
        command: "x".into(), announce: false, cooldown_secs: 60, enabled: true, last_fired: 0,
    });
    let sunday = Moment { minutes_of_day: 7 * 60, weekday: 6, ..moment() };
    assert!(a.due(&sunday, 1000).is_empty());
}

#[test]
fn a_rule_does_not_fire_twice_inside_its_cooldown() {
    let mut a = Anticipator::default();
    a.add(Rule {
        name: "brief".into(),
        trigger: Trigger::OnReturn,
        command: "x".into(), announce: false, cooldown_secs: 900, enabled: true, last_fired: 0,
    });
    let m = Moment { returned: true, ..moment() };
    assert_eq!(a.due(&m, 1000).len(), 1);
    assert!(a.due(&m, 1100).is_empty(), "inside cooldown");
    assert_eq!(a.due(&m, 2000).len(), 1);
}

#[test]
fn nothing_is_prepared_when_the_machine_cannot_afford_it() {
    // Anticipation that makes your laptop stutter is worse than none.
    let mut a = Anticipator::default();
    a.add(Rule {
        name: "heavy".into(),
        trigger: Trigger::OnReturn,
        command: "x".into(), announce: false, cooldown_secs: 0, enabled: true, last_fired: 0,
    });
    let busy = Moment { returned: true, can_afford_work: false, ..Default::default() };
    assert!(a.due(&busy, 1000).is_empty());
}

#[test]
fn a_new_file_can_trigger_preparation() {
    let mut a = Anticipator::default();
    a.add(Rule {
        name: "read it".into(),
        trigger: Trigger::FileAppears { pattern: "*.pdf".into() },
        command: "summarise".into(), announce: false, cooldown_secs: 0, enabled: true, last_fired: 0,
    });
    let m = Moment { new_files: vec!["C:/Users/e/Downloads/invoice.pdf".into()], ..moment() };
    assert_eq!(a.due(&m, 100).len(), 1);

    let other = Moment { new_files: vec!["notes.txt".into()], ..moment() };
    assert!(a.due(&other, 200).is_empty());
}

#[test]
fn a_meeting_coming_up_can_trigger_preparation() {
    let mut a = Anticipator::default();
    a.add(Rule {
        name: "prep".into(),
        trigger: Trigger::BeforeEvent { minutes: 10 },
        command: "x".into(), announce: true, cooldown_secs: 0, enabled: true, last_fired: 0,
    });
    let soon = Moment { minutes_to_event: Some(8), ..moment() };
    assert_eq!(a.due(&soon, 100).len(), 1);
    let far = Moment { minutes_to_event: Some(45), ..moment() };
    assert!(a.due(&far, 200).is_empty());
    let none = Moment { minutes_to_event: None, ..moment() };
    assert!(a.due(&none, 300).is_empty());
}

#[test]
fn glob_patterns_anchor_at_both_ends() {
    assert!(matches("report.pdf", "*.pdf"));
    assert!(!matches("report.pdf.txt", "*.pdf"));
    assert!(matches("invoice-2026.pdf", "invoice*"));
    assert!(!matches("my-invoice.pdf", "invoice*"), "a leading segment anchors");
}

#[test]
fn prepared_work_is_offered_never_announced_as_done() {
    // It prepared something. You decide whether you want it.
    let r = &suggested()[0];
    let line = ready_line(r);
    assert!(line.contains("ready when you want it"), "got: {line}");
    assert!(!line.starts_with("I did"));
}

#[test]
fn rules_can_be_switched_on_by_name() {
    let mut a = Anticipator::default();
    for r in suggested() {
        a.add(r);
    }
    assert!(a.active().is_empty());
    assert!(a.enable("morning brief", true));
    assert_eq!(a.active().len(), 1);
    assert!(!a.enable("no such rule", true));
}

#[test]
fn the_shipped_persona_is_brief_and_does_not_greet() {
    let y = std::fs::read_to_string("config/tools.yaml").unwrap();
    let t: atlas::voice::ToolsConfig = serde_yaml::from_str(&y).unwrap();
    assert!(!t.persona.greet);
    // A CEILING, not a target, and its meaning changed when the register
    // started deciding the actual length.
    //
    // This was `<= 4`, "speech has to stay short", and it was right when
    // `max_spoken_sentences` was the only length control there was. Now
    // `run_command` resolves the cap as
    // `persona.max_spoken_sentences.min(mode_cap).min(register.length())`, so
    // a ceiling below `Register::Chatting.length()` (8) is the binding
    // constraint on every conversation and the register never applies --
    // which is the clipped-reply problem moved into the config file.
    //
    // So: no lower than the chattiest register, and still bounded, because a
    // reply read out loud has to end.
    assert!(
        t.persona.max_spoken_sentences >= atlas::register::Register::Chatting.length(),
        "the ceiling is below the chattiest register, so the register cannot decide"
    );
    assert!(t.persona.max_spoken_sentences <= 10, "speech still has to end");
    // And a task is still short -- that is the register's job now.
    assert_eq!(atlas::register::Register::Working.length(), 2);
    assert!(!t.persona.voice_model.is_empty());
    assert!(t.thread.verbatim > 0 && t.thread.fold_after > t.thread.verbatim);
}

#[test]
fn stripping_list_markers_does_not_eat_a_leading_number() {
    // "1 scheduled, 0 awaiting you" must not become "scheduled, 0 awaiting
    // you" — the count is the whole point of the sentence.
    let p = Persona::default();
    assert_eq!(p.spoken("1 scheduled, 0 awaiting you."), "1 scheduled, 0 awaiting you.");
    assert_eq!(p.spoken("3 things outstanding."), "3 things outstanding.");
    assert_eq!(atlas::persona::strip_list_marker("2026 was a good year"), "2026 was a good year");
}

#[test]
fn a_real_numbered_list_still_loses_its_marker() {
    assert_eq!(atlas::persona::strip_list_marker("1. First item"), "First item");
    assert_eq!(atlas::persona::strip_list_marker("2) Second item"), "Second item");
    assert_eq!(atlas::persona::strip_list_marker("- bullet"), "bullet");
}
