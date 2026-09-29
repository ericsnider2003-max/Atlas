use atlas::attention::{hear, Attention, Heard, Mode};
use atlas::delegate::{interpret, Delegation, Reach, State, Step};
use atlas::grants::{grant_in_instruction, span_from_answer, AppFacts, Permissions, Span};

fn known() -> Vec<String> {
    vec!["claude".into(), "chrome".into(), "discord".into(), "notepad".into(), "excel".into()]
}
fn configured() -> AppFacts {
    AppFacts { known: true, confirm_each_time: false }
}
fn discord_facts() -> AppFacts {
    AppFacts { known: true, confirm_each_time: true }
}
fn unknown() -> AppFacts {
    AppFacts { known: false, confirm_each_time: false }
}

// ================= pause and resume =================

#[test]
fn pause_and_resume_are_recognised_in_natural_speech() {
    for s in ["pause", "hold on", "give me a minute", "wait", "hang on"] {
        assert_eq!(hear(s), Some(Heard::Pause), "{s:?}");
    }
    for s in ["resume", "I'm ready", "i am ready to continue", "carry on", "I'm back"] {
        assert_eq!(hear(s), Some(Heard::Resume), "{s:?}");
    }
}

#[test]
fn a_pause_word_inside_a_sentence_does_not_pause_atlas() {
    // "don't pause the video" must not stop Atlas. Whole-utterance match only.
    assert_eq!(hear("dont pause the video"), None);
    assert_eq!(hear("wait for the download to finish"), None);
}

#[test]
fn pausing_stops_atlas_talking_but_not_listening() {
    let mut a = Attention::default();
    a.pause(None, 100);
    assert!(!a.may_speak(), "silence is the point of pausing");
    assert!(a.allows(Some(Heard::Resume)), "must still hear you say resume");
    assert!(!a.allows(Some(Heard::Pause)), "everything else waits");
    assert!(!a.allows(None));
}

#[test]
fn resuming_names_what_it_was_doing() {
    let mut a = Attention::default();
    a.pause(Some("the research job".into()), 100);
    assert_eq!(a.resume(200), "Picking the research job back up.");
    assert_eq!(a.mode, Mode::Active);
}

#[test]
fn suspended_tasks_are_handed_back_on_resume_not_lost() {
    let mut a = Attention::default();
    a.pause(None, 100);
    a.suspend(7);
    a.suspend(9);
    a.suspend(7); // no duplicates
    a.resume(200);
    assert_eq!(a.release(), vec![7, 9]);
    assert!(a.release().is_empty(), "released once");
}

#[test]
fn pausing_twice_and_resuming_when_active_are_both_harmless() {
    let mut a = Attention::default();
    a.pause(None, 100);
    assert_eq!(a.pause(None, 110), "Already paused.");
    a.resume(200);
    assert_eq!(a.resume(210), "Wasn't paused.");
}

#[test]
fn you_can_ask_how_long_it_has_been_paused() {
    let mut a = Attention::default();
    a.pause(Some("the email draft".into()), 0);
    let s = a.status(600);
    assert!(s.contains("the email draft") && s.contains("10 minutes"), "got: {s}");
}

// ================= permission to use an app =================

#[test]
fn a_configured_app_needs_no_permission() {
    let p = Permissions::default();
    assert!(p.check("chrome", "type", &configured()).allowed());
}

#[test]
fn an_app_atlas_does_not_know_is_asked_about_first() {
    let p = Permissions::default();
    let v = p.check("photoshop", "type", &unknown());
    assert!(!v.allowed());
    assert!(v.message().contains("I don't know photoshop"), "got: {}", v.message());
}

#[test]
fn naming_the_app_in_your_instruction_is_the_permission() {
    // "use Excel to build that sheet" — you already said yes by asking.
    let g = grant_in_instruction("use excel to build that sheet", &known());
    assert_eq!(g, Some(("excel".into(), Span::Once)));
}

#[test]
fn naming_an_app_atlas_has_never_heard_of_still_counts_as_permission() {
    let g = grant_in_instruction("use photoshop to fix the colour", &known());
    assert_eq!(g, Some(("photoshop".into(), Span::Once)));
}

#[test]
fn merely_mentioning_an_app_grants_nothing() {
    assert_eq!(grant_in_instruction("chrome keeps crashing on me", &known()), None);
    assert_eq!(grant_in_instruction("what is excel good for", &known()), None);
}

#[test]
fn a_granted_app_is_then_allowed_without_asking_again() {
    let mut p = Permissions::default();
    p.grant("photoshop", None, Span::Once, 100);
    assert!(p.check("photoshop", "type", &unknown()).allowed());
}

#[test]
fn a_one_off_grant_is_spent_after_it_is_used() {
    let mut p = Permissions::default();
    p.grant("photoshop", Some("type"), Span::Once, 100);
    assert!(p.check("photoshop", "type", &unknown()).allowed());
    p.consume("photoshop", "type");
    assert!(!p.check("photoshop", "type", &unknown()).allowed(), "asks again next time");
}

#[test]
fn discord_is_confirmed_every_single_time_however_often_approved() {
    // A wrong message there is public and permanent, so no accumulated
    // history relaxes it.
    let mut p = Permissions::default();
    for _ in 0..50 {
        p.grant("discord", None, Span::Always, 100);
    }
    let v = p.check("discord", "type", &discord_facts());
    assert!(!v.allowed(), "still asks after fifty approvals");
    assert!(v.message().contains("Send that in discord?"));
}

#[test]
fn session_grants_are_dropped_on_restart_but_always_survives() {
    let mut p = Permissions::default();
    p.grant("photoshop", None, Span::Session, 100);
    p.grant("blender", None, Span::Always, 100);
    p.new_session();
    assert_eq!(p.granted_apps(), vec!["blender"]);
}

#[test]
fn how_you_answer_decides_how_long_permission_lasts() {
    assert_eq!(span_from_answer("yes"), Some(Span::Once));
    assert_eq!(span_from_answer("yes, always"), Some(Span::Always));
    assert_eq!(span_from_answer("for this session"), Some(Span::Session));
    assert_eq!(span_from_answer("no"), None);
    assert_eq!(span_from_answer("hmm"), None, "a mumble is not consent");
}

#[test]
fn a_refusal_that_mentions_always_does_not_grant_always() {
    // The function was inverted, and this is what it cost. It read:
    //
    //     if !is_yes(&t) {
    //         if t.contains("always") { return Some(Span::Always) }
    //         ...
    //     }
    //
    // `is_yes` is a strict whitelist over the whole answer, so "yes, always"
    // is not a yes and fell into the NEGATIVE branch -- which is what made
    // the intended case work, and is why the test above passed. Every
    // refusal fell there too.
    //
    // `Span::Always` is the one `new_session` deliberately keeps across
    // restarts, so "no, don't always use Discord" would have granted Discord
    // permanent standing permission.
    for said in [
        "no, don't always do that",
        "never, not this session",
        "do not always use excel",
        "not this session",
        "no thanks, not from now on",
        "don't - not every time",
    ] {
        assert_eq!(
            span_from_answer(said),
            None,
            "{said:?} was read as consent"
        );
    }
}

#[test]
fn a_bare_breadth_word_is_still_an_answer_but_a_bare_mumble_is_not() {
    // The other side, so the refusal check has not made the function deaf.
    assert_eq!(span_from_answer("always"), Some(Span::Always));
    assert_eq!(span_from_answer("from now on"), Some(Span::Always));
    assert_eq!(span_from_answer("just for now"), Some(Span::Session));
    assert_eq!(span_from_answer("sure, every time"), Some(Span::Always));
    assert_eq!(span_from_answer("go ahead"), Some(Span::Once));
    assert_eq!(span_from_answer("maybe later"), None);
    assert_eq!(span_from_answer(""), None);
}

#[test]
fn ordinary_prose_containing_use_inside_a_word_grants_nothing() {
    // `LEADS` were matched as bare substrings, so `"because "` -- which
    // contains `"use "` at index 4 -- turned the sentence after it into a
    // grant. This function's own doc says *"Merely mentioning an app
    // (\"chrome keeps crashing\") grants nothing."*
    let apps = vec!["chrome".to_string(), "excel".to_string()];
    for said in [
        "because chrome keeps crashing i gave up",
        "the cause chrome gave was odd",
        "i misuse chrome constantly",
        "i paused chrome yesterday",
    ] {
        assert_eq!(
            grant_in_instruction(said, &apps),
            None,
            "{said:?} was read as permission to drive the app"
        );
    }

    // And the real thing still grants.
    assert_eq!(
        grant_in_instruction("use excel to build that sheet", &apps),
        Some(("excel".to_string(), Span::Once))
    );
    assert_eq!(
        grant_in_instruction("open up chrome and find it", &apps),
        Some(("chrome".to_string(), Span::Once))
    );
}

#[test]
fn being_told_not_to_use_an_app_is_not_permission_to_use_it() {
    let apps = vec!["excel".to_string()];
    for said in ["don't use excel", "do not use excel for this", "never use excel"] {
        assert_eq!(
            grant_in_instruction(said, &apps),
            None,
            "{said:?} granted the app it was refusing"
        );
    }
}

#[test]
fn permission_can_be_taken_back() {
    let mut p = Permissions::default();
    p.grant("photoshop", None, Span::Always, 100);
    p.revoke("photoshop");
    assert!(!p.check("photoshop", "type", &unknown()).allowed());
}

// ================= working an app for you =================

#[test]
fn draft_a_response_writes_but_does_not_send() {
    let d = interpret("read this email and draft a response", &known()).unwrap();
    assert_eq!(d.reach, Reach::Draft);
    assert_eq!(d.max_turns, 1, "drafting is one and done");
}

#[test]
fn finish_the_conversation_carries_it_on() {
    let d = interpret("finish the conversation with claude until i am back", &known()).unwrap();
    assert_eq!(d.app, "claude");
    assert_eq!(d.reach, Reach::Converse);
    assert!(d.max_turns > 1);
}

#[test]
fn an_ordinary_sentence_does_not_start_a_delegation() {
    assert!(interpret("open chrome", &known()).is_none());
    assert!(interpret("what did claude say", &known()).is_none());
}

#[test]
fn a_draft_is_placed_in_the_box_and_the_job_ends_there() {
    let mut d = Delegation::new("outlook", "draft a reply", Reach::Draft, 1);
    let p = Permissions::default();
    match d.advance("From: Sam\nCan we move Thursday?", &p, &configured()) {
        Step::Compose { context } => assert!(context.contains("Can we move Thursday?")),
        o => panic!("{o:?}"),
    }
    match d.composed("Thursday works, how about 2pm?") {
        Step::Place { send, text } => {
            assert!(!send, "a draft is never sent");
            assert!(text.contains("2pm"));
        }
        o => panic!("{o:?}"),
    }
    assert_eq!(d.state, State::Done);
}

#[test]
fn a_conversation_runs_several_turns_and_sends_them() {
    let mut d = Delegation::new("claude", "finish the conversation", Reach::Converse, 3);
    let p = Permissions::default();
    for _ in 0..3 {
        assert!(matches!(d.advance("...", &p, &configured()), Step::Compose { .. }));
        match d.composed("next reply") {
            Step::Place { send, .. } => assert!(send),
            o => panic!("{o:?}"),
        }
    }
    assert!(matches!(d.advance("...", &p, &configured()), Step::Finished(_)));
    assert_eq!(d.state, State::Done);
    assert!(d.summary().contains("3 replies"));
}

#[test]
fn the_turn_budget_stops_it_looping_forever() {
    let mut d = Delegation::new("claude", "carry on", Reach::Converse, 2);
    let p = Permissions::default();
    d.advance("x", &p, &configured());
    d.composed("a");
    d.advance("x", &p, &configured());
    d.composed("b");
    assert!(matches!(d.advance("x", &p, &configured()), Step::Finished(_)));
    assert!(d.reason.as_ref().unwrap().contains("2-turn limit"));
}

#[test]
fn calling_it_off_ends_it() {
    // It used to be "you speaking ends it immediately". That was written
    // before errands ran side by side, and it made every new request end
    // the job; now only calling it off does (25 Sep 2026).
    let mut d = Delegation::new("claude", "carry on", Reach::Converse, 20);
    let p = Permissions::default();
    d.advance("x", &p, &configured());
    d.composed("a reply");
    let said = d.called_off();
    assert_eq!(d.state, State::Stopped);
    assert!(said.contains("Stopped working claude after one reply"), "got: {said}");
    assert!(matches!(d.advance("x", &p, &configured()), Step::Finished(_)));
}

#[test]
fn a_stop_phrase_on_screen_ends_it_early() {
    let mut d = Delegation::new("claude", "carry on", Reach::Converse, 20)
        .stopping_on(&["anything else?"]);
    let p = Permissions::default();
    let step = d.advance("Great, that's sorted. Anything else?", &p, &configured());
    assert!(matches!(step, Step::Finished(_)));
    assert_eq!(d.state, State::Done);
}

#[test]
fn nothing_is_composed_for_an_app_atlas_may_not_touch() {
    // Permission is checked before writing, not after — there is no point
    // drafting a reply it isn't allowed to place.
    let mut d = Delegation::new("discord", "reply for me", Reach::Converse, 5);
    let p = Permissions::default();
    match d.advance("someone said hello", &p, &discord_facts()) {
        Step::Confirm(q) => assert!(q.contains("discord")),
        o => panic!("expected a confirmation, got {o:?}"),
    }
    assert_eq!(d.state, State::NeedsConfirm);
    assert!(d.transcript.is_empty(), "nothing written yet");
}

#[test]
fn confirming_lets_it_proceed_and_refusing_stops_it() {
    let mut d = Delegation::new("discord", "reply", Reach::Converse, 5);
    let p = Permissions::default();
    d.advance("hi", &p, &discord_facts());
    d.confirmed();
    assert_eq!(d.state, State::Running);

    let mut d2 = Delegation::new("discord", "reply", Reach::Converse, 5);
    d2.advance("hi", &p, &discord_facts());
    d2.refused();
    assert_eq!(d2.state, State::Stopped);
    assert!(d2.reason.as_ref().unwrap().contains("said no"));
}

#[test]
fn a_delegation_can_be_paused_and_resumed_with_everything_else() {
    let mut d = Delegation::new("claude", "carry on", Reach::Converse, 5);
    let p = Permissions::default();
    d.pause();
    assert!(matches!(d.advance("x", &p, &configured()), Step::Finished(_)));
    d.resume();
    assert!(matches!(d.advance("x", &p, &configured()), Step::Compose { .. }));
}

#[test]
fn what_it_already_wrote_is_part_of_the_context_for_the_next_turn() {
    let mut d = Delegation::new("claude", "carry on", Reach::Converse, 5);
    let p = Permissions::default();
    d.advance("first message", &p, &configured());
    d.composed("my earlier reply");
    match d.advance("their answer", &p, &configured()) {
        Step::Compose { context } => {
            // What Atlas wrote goes in its instructions (the system prompt),
            // not in the quoted screen text, where it would read as someone
            // else's words (audit, 24 Sep 2026).
            let system = d.system_prompt();
            assert!(system.contains("my earlier reply"), "must not repeat itself: {system}");
            assert!(context.contains("their answer"));
            assert!(!context.contains("my earlier reply"));
        }
        o => panic!("{o:?}"),
    }
}
