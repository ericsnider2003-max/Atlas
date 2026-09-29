use atlas::persona::Persona;
use atlas::register::{announcement_for, formality, read, CallContext, Formality, Moment, Register};

fn quiet() -> Moment {
    Moment::default()
}

// ================= reading the room =================

#[test]
fn a_task_is_recognised_as_a_task() {
    for said in ["open chrome", "boot workspace", "research the CME spec", "close discord"] {
        assert_eq!(read(said, &quiet()), Register::Working, "{said:?}");
    }
}

#[test]
fn a_polite_task_is_still_a_task() {
    // "Can you open Chrome" is a request wearing a question's clothes.
    assert_eq!(read("can you open chrome", &quiet()), Register::Working);
    assert_eq!(read("could you close discord please", &quiet()), Register::Working);
}

#[test]
fn talking_is_not_tasking() {
    for said in [
        "what did you make of that film",
        "i've been thinking about moving the desk",
        "do you reckon that was a good idea",
    ] {
        assert_eq!(read(said, &quiet()), Register::Chatting, "{said:?}");
    }
}

#[test]
fn a_question_about_atlas_is_told_apart_from_a_question_containing_you() {
    // The distinction that matters: one is about Atlas, the other is a
    // request for an opinion on something else.
    assert_eq!(read("what do you do exactly", &quiet()), Register::AboutAtlas);
    assert_eq!(read("are you recording this", &quiet()), Register::AboutAtlas);
    assert_eq!(read("why did you put chrome there", &quiet()), Register::AboutAtlas);
    assert_eq!(read("what do you think of this draft", &quiet()), Register::Chatting);
}

#[test]
fn frustration_is_noticed_and_overrides_everything_else() {
    let after_failure = Moment { after_a_failure: true, ..quiet() };
    assert_eq!(read("open chrome", &after_failure), Register::Rough);
    assert_eq!(read("that didn't work again", &quiet()), Register::Rough);
    assert_eq!(read("seriously, why isn't this working", &quiet()), Register::Rough);
}

#[test]
fn a_short_fragment_mid_task_reads_as_work_not_chat() {
    let busy = Moment { busy: true, ..quiet() };
    assert_eq!(read("the other one", &busy), Register::Working);
    assert_eq!(read("the other one", &quiet()), Register::Chatting);
}

// ================= how much it says, and how =================

#[test]
fn a_conversation_gets_room_to_actually_be_one() {
    // Two sentences is not a conversation.
    assert_eq!(Register::Working.length(), 2);
    assert!(Register::Chatting.length() >= 8);
}

#[test]
fn jokes_belong_in_conversation_and_nowhere_else() {
    assert!(Register::Chatting.humour_welcome());
    assert!(!Register::Working.humour_welcome());
    assert!(!Register::Rough.humour_welcome(), "not when something just broke");
}

#[test]
fn the_character_instructions_change_with_the_moment() {
    let p = Persona::default();
    let working = p.prompt_for(Register::Working);
    let chatting = p.prompt_for(Register::Chatting);
    assert_ne!(working, chatting, "one fixed prompt is what makes it sound like a machine");
    assert!(working.contains("No commentary, no jokes"));
    assert!(chatting.contains("Talk like a person"));
    assert!(chatting.contains("Do not offer to help"), "no steering it back to work");
}

#[test]
fn atlas_is_told_to_disagree_rather_than_go_along_with_things() {
    let p = Persona::default();
    assert!(p.argues);
    let prompt = p.prompt_for(Register::Chatting);
    assert!(prompt.contains("If you disagree, say so"));
    assert!(prompt.contains("Do not soften it into agreement"));
    assert!(p.system_prompt().contains("Have opinions"));
}

#[test]
fn it_is_told_it_is_not_only_for_work() {
    assert!(Persona::default().system_prompt().contains("not only for work"));
}

#[test]
fn nothing_is_witty_when_something_has_just_gone_wrong() {
    let p = Persona::default();
    let rough = p.prompt_for(Register::Rough);
    assert!(rough.contains("No jokes"));
    assert!(!rough.contains("dry aside"), "humour is switched off");
    assert!(!rough.contains("Do not soften it into agreement"),
        "and it isn't pushed to argue");
}

#[test]
fn honesty_does_not_switch_off_when_you_are_frustrated() {
    // The opposite would be worse: a bad moment is exactly when agreeing with
    // you is most tempting and least useful.
    let rough = Persona::default().prompt_for(Register::Rough);
    assert!(rough.contains("Never flatter"));
    assert!(rough.contains("Never claim something worked when you did not verify it"));
}

#[test]
fn asking_about_atlas_gets_a_plain_specific_answer_not_marketing() {
    let prompt = Persona::default().prompt_for(Register::AboutAtlas);
    assert!(prompt.contains("No marketing"));
    assert!(prompt.contains("what you can't do"));
}

#[test]
fn an_opinion_is_volunteered_exactly_where_the_moment_welcomes_one() {
    // `register::opinions_welcome` decides this, and `prompt_for` is where it
    // reaches the model. The two registers that welcome a view get the
    // instruction; the two that do not, do not.
    assert!(Register::Chatting.opinions_welcome());
    assert!(Register::AboutAtlas.opinions_welcome());
    assert!(!Register::Working.opinions_welcome());
    assert!(!Register::Rough.opinions_welcome());

    let p = Persona::default();
    let volunteer = "offer it rather than waiting to be asked";
    assert!(p.prompt_for(Register::Chatting).contains(volunteer));
    // The gap this wiring closed: an about-Atlas turn was never told to have
    // a view, though the predicate always said it should.
    assert!(p.prompt_for(Register::AboutAtlas).contains(volunteer));
    assert!(!p.prompt_for(Register::Working).contains(volunteer));
    assert!(!p.prompt_for(Register::Rough).contains(volunteer));
}

// ================= business or casual =================

fn call(title: &str, participants: &[&str]) -> CallContext {
    CallContext {
        title: title.into(),
        participants: participants.iter().map(|s| s.to_string()).collect(),
        my_domains: vec!["homelab.com".into()],
        minutes_of_day: 14 * 60,
        weekday: 2,
        from_calendar: true,
    }
}

#[test]
fn a_client_meeting_reads_as_business() {
    let c = call("Q4 review with Acme", &["sam@acme.com", "me@homelab.com"]);
    assert_eq!(formality(&c), Formality::Business);
    assert!(announcement_for(Formality::Business).contains("For transparency"));
}

#[test]
fn a_saturday_evening_catch_up_with_one_person_reads_as_casual() {
    let c = CallContext {
        title: "catch up".into(),
        participants: vec!["jamie".into()],
        my_domains: vec!["homelab.com".into()],
        minutes_of_day: 20 * 60,
        weekday: 6,
        from_calendar: false,
    };
    assert_eq!(formality(&c), Formality::Casual);
    assert!(announcement_for(Formality::Casual).contains("FYI"));
}

#[test]
fn someone_outside_your_own_domain_pushes_it_toward_business() {
    let internal = call("sync", &["a@homelab.com", "b@homelab.com"]);
    let external = call("sync", &["a@homelab.com", "sam@acme.com"]);
    assert!(formality(&external) == Formality::Business || formality(&internal) != Formality::Business);
}

#[test]
fn when_it_cannot_tell_it_leans_formal() {
    // Sounding a little stiff with a friend is mildly awkward. Sounding
    // casual with a client is worse.
    let vague = CallContext {
        title: "call".into(),
        participants: vec!["someone".into(), "another".into()],
        my_domains: vec![],
        minutes_of_day: 12 * 60,
        weekday: 2,
        from_calendar: false,
    };
    assert_eq!(formality(&vague), Formality::Unsure);
    let said = announcement_for(Formality::Unsure);
    assert!(said.contains("Heads up"), "the middle register: {said}");
    assert!(said.len() < 140, "and short");
}

#[test]
fn every_announcement_makes_declining_easy() {
    for f in [Formality::Business, Formality::Casual, Formality::Unsure] {
        let a = announcement_for(f).to_lowercase();
        assert!(
            a.contains("rather i didn't") || a.contains("turn it off") || a.contains("switch it off"),
            "{f:?} must offer a way out"
        );
    }
}
