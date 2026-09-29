use atlas::chain::{what_stands, Chain, Next, Step, StepState};
use atlas::triage::{can_wait, sort_all, spoken, triage, Corrections, Message, Needs};

fn msg(id: &str, from: &str, subject: &str, body: &str) -> Message {
    Message {
        id: id.into(),
        from: from.into(),
        subject: subject.into(),
        body: body.into(),
        at: 0,
        to_many: false,
        is_reply: false,
        answered: false,
    }
}

// ================= sorted by what it asks of you =================

#[test]
fn a_question_with_a_date_on_it_sorts_highest() {
    // The only thing that can go wrong by being ignored.
    let m = msg("1", "marta@vendor.com", "Certification",
        "Can you confirm the entity details by Friday? We can't submit without them.");
    let t = triage(&m);
    assert_eq!(t.needs, Needs::Deadline);
    assert_eq!(t.by_when.as_deref(), Some("by friday"));
}

#[test]
fn something_only_you_can_decide_is_told_apart_from_a_question() {
    let m = msg("2", "dev@team.com", "Two options",
        "Both work. Your call which one we go with.");
    assert_eq!(triage(&m).needs, Needs::Decision);
}

#[test]
fn atlas_will_draft_a_reply_but_never_a_decision() {
    // The decision is the part only you can make.
    let question = msg("3", "marta@vendor.com", "Quick one", "Could you send the account number?");
    assert!(triage(&question).draftable);

    let decision = msg("4", "dev@team.com", "Sign off?", "Need you to approve the new schema.");
    assert!(!triage(&decision).draftable, "it can't decide for you");
}

#[test]
fn being_one_of_eight_recipients_means_it_probably_is_not_yours() {
    let m = Message {
        to_many: true,
        ..msg("5", "boss@co.com", "Thoughts?", "Can you let me know what you think of this?")
    };
    let t = triage(&m);
    assert_eq!(t.needs, Needs::Reading);
    assert!(t.because.contains("one of several"));
}

#[test]
fn something_that_says_it_needs_nothing_is_believed() {
    let m = msg("6", "ops@co.com", "Deploy done", "FYI, the deploy went out. No action needed.");
    assert_eq!(triage(&m).needs, Needs::Reading);
}

#[test]
fn automated_mail_is_recognised_and_dropped() {
    let m = msg("7", "no-reply@shop.com", "50% off everything!", "Shop now. Unsubscribe here.");
    assert_eq!(triage(&m).needs, Needs::Nothing);
}

#[test]
fn something_you_have_already_answered_is_with_them() {
    let m = Message { answered: true, ..msg("8", "marta@vendor.com", "Re: docs", "Thanks — can you also send the other one?") };
    let t = triage(&m);
    assert_eq!(t.needs, Needs::TheirMove);
    assert!(t.because.contains("it's with them"));
}

#[test]
fn the_gist_comes_from_the_body_because_subjects_are_written_to_be_opened() {
    let m = msg("9", "marta@vendor.com", "Update",
        "Hi Eric,\nThe compliance team came back and the certification will take six weeks from submission.");
    assert!(triage(&m).gist.contains("six weeks"), "got: {}", triage(&m).gist);
}

#[test]
fn every_message_says_why_it_was_sorted_that_way() {
    // So you can disagree with it.
    for m in [
        msg("a", "x@y.com", "s", "Could you confirm by Friday?"),
        msg("b", "no-reply@z.com", "Sale", "Unsubscribe"),
    ] {
        assert!(!triage(&m).because.is_empty());
    }
}

// ================= what Atlas says about an inbox =================

fn inbox() -> Vec<Message> {
    vec![
        msg("1", "no-reply@shop.com", "Sale", "50% off. Unsubscribe."),
        msg("2", "ops@co.com", "Deploy", "FYI, deploy went out. No action needed."),
        msg("3", "marta@vendor.com", "Certification",
            "Can you confirm the entity details by Friday?"),
        msg("4", "dev@team.com", "Schema", "Your call which option we go with."),
        msg("5", "sam@co.com", "Quick one", "Could you send the account number?"),
    ]
}

#[test]
fn the_count_is_not_the_point_how_many_need_you_is() {
    // "You have 47 emails" is what the client already told you.
    let said = spoken(&sort_all(&inbox()));
    assert!(said.contains("3 of 5 need you"));
    assert!(said.contains("First:"));
    assert!(said.contains("by friday"), "with the date: {said}");
}

#[test]
fn the_one_with_a_deadline_leads() {
    let sorted = sort_all(&inbox());
    assert_eq!(sorted[0].needs, Needs::Deadline);
}

#[test]
fn it_says_how_many_it_could_draft() {
    assert!(spoken(&sort_all(&inbox())).contains("I can draft 1"));
}

#[test]
fn an_inbox_that_needs_nothing_says_exactly_that() {
    let quiet = vec![msg("1", "no-reply@x.com", "Sale", "Unsubscribe")];
    assert!(spoken(&sort_all(&quiet)).contains("none of it needs you"));
}

#[test]
fn everything_that_can_wait_is_listed_so_you_can_not_look_at_it() {
    let sorted = sort_all(&inbox());
    let waiting = can_wait(&sorted);
    assert_eq!(waiting.len(), 2, "the sale and the FYI");
}

#[test]
fn the_spoken_verdict_names_how_many_can_be_left_unread() {
    // The other half of the sort the daemon speaks: after what needs you, the
    // pile you can ignore, so a re-sort shrinks the inbox you face rather than
    // re-ordering the same one. Two of the five -- the sale and the FYI -- can
    // wait.
    let said = spoken(&sort_all(&inbox()));
    assert!(said.contains("The other 2 can wait"), "got: {said}");
    assert!(said.contains("leave them unread"), "plural for two: {said}");
}

#[test]
fn a_single_message_that_can_wait_is_spoken_in_the_singular() {
    // One that needs you, one that doesn't -- so the tail is reached and its
    // count is one.
    let mixed = vec![
        msg("1", "sam@co.com", "Quick one", "Could you send the account number?"),
        msg("2", "no-reply@shop.com", "Sale", "50% off. Unsubscribe."),
    ];
    let said = spoken(&sort_all(&mixed));
    assert!(said.contains("The other 1 can wait"), "got: {said}");
    assert!(said.contains("leave it unread"), "singular for one: {said}");
}

#[test]
fn an_inbox_where_nothing_can_wait_does_not_claim_any_can() {
    // Every message needs you, so there is no "can wait" tail to speak -- the
    // guard against an off-by-one that would announce a pile that isn't there.
    let all_urgent = vec![
        msg("1", "sam@co.com", "One", "Could you send the account number?"),
        msg("2", "dev@team.com", "Two", "Your call which option we go with."),
    ];
    let said = spoken(&sort_all(&all_urgent));
    assert!(!said.contains("can wait"), "nothing can, so it should not say so: {said}");
}

#[test]
fn disagreeing_with_the_sorting_is_learned_per_sender() {
    let mut c = Corrections::default();
    c.note("newsletter@substack.com", Needs::Reading);
    let m = msg("1", "newsletter@substack.com", "Weekly", "Unsubscribe at the bottom.");
    let adjusted = c.adjust(&m, triage(&m));
    assert_eq!(adjusted.needs, Needs::Reading);
    assert!(adjusted.because.contains("you've told me"));
}

// ================= chains across apps =================

fn export() -> Step {
    Step {
        what: "pull the figures out of the spreadsheet".into(),
        app: "excel".into(),
        reversible: true,
        goes_out: false,
        needs: None,
        produces: Some("figures".into()),
    }
}
fn write() -> Step {
    Step {
        what: "put them in the report".into(),
        app: "word".into(),
        reversible: true,
        goes_out: false,
        needs: Some("figures".into()),
        produces: Some("report".into()),
    }
}
fn send() -> Step {
    Step {
        what: "send it to Marta".into(),
        app: "outlook".into(),
        reversible: false,
        goes_out: true,
        needs: Some("report".into()),
        produces: None,
    }
}

#[test]
fn the_whole_chain_is_checked_before_any_of_it_runs() {
    // A chain missing something at step four shouldn't run steps one to three.
    let broken = Chain::new("send the report", vec![write(), send()]);
    let e = broken.check().unwrap_err();
    assert!(e.contains("needs figures"));
    assert!(e.contains("nothing before it makes one"));

    assert!(Chain::new("send the report", vec![export(), write(), send()]).check().is_ok());
}

#[test]
fn nothing_irreversible_is_allowed_after_the_send() {
    // A valid chain in every other respect, so the only complaint is the
    // ordering.
    let send_figures = Step { needs: Some("figures".into()), ..send() };
    let bad = Chain::new(
        "send then delete",
        vec![
            export(),
            send_figures,
            Step { what: "delete the working copy".into(), reversible: false, goes_out: false,
                   needs: None, produces: None, app: "explorer".into() },
        ],
    );
    assert!(bad.check().unwrap_err().contains("can't be undone"), "got: {:?}", bad.check());
}

#[test]
fn the_step_that_leaves_the_machine_asks_first_with_what_led_there() {
    // You approve the result, not the instruction.
    let mut c = Chain::new("send the report", vec![export(), write(), send()]);
    c.done(Some(("figures".into(), "Q3: 412k".into())));
    c.done(Some(("report".into(), "Q3 summary.docx".into())));
    match c.next() {
        Next::Confirm { what, context, .. } => {
            assert!(what.contains("send it to Marta"));
            assert!(context.contains("Q3: 412k"));
            assert!(context.contains("Q3 summary.docx"), "everything that led here: {context}");
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn the_steps_before_it_just_run() {
    let c = Chain::new("send the report", vec![export(), write(), send()]);
    assert!(matches!(c.next(), Next::Run { index: 0, .. }));
}

#[test]
fn a_missing_input_stops_and_asks_rather_than_guessing() {
    // A wrong guess halfway through a chain is much worse than one on its own.
    let mut c = Chain::new("send the report", vec![export(), write(), send()]);
    c.states[0] = StepState::Done;
    c.at = 1;
    match c.next() {
        Next::Ask { question, .. } => {
            assert!(question.contains("I need figures"));
            assert!(question.contains("Where from?"));
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn a_failure_undoes_what_can_be_undone_in_reverse() {
    let mut c = Chain::new("send the report", vec![export(), write(), send()]);
    c.done(Some(("figures".into(), "Q3".into())));
    c.done(Some(("report".into(), "doc".into())));
    match c.failed("Marta's address bounced") {
        Next::Stopped { why, undone } => {
            assert!(why.contains("bounced"));
            assert_eq!(undone.len(), 2);
            assert!(undone[0].contains("report"), "reverse order: {undone:?}");
        }
        o => panic!("{o:?}"),
    }
    assert_eq!(c.states[0], StepState::RolledBack);
}

#[test]
fn anything_that_could_not_be_undone_is_named_plainly() {
    let mut c = Chain::new("post and file", vec![
        Step { reversible: false, ..export() },
        write(),
    ]);
    c.done(Some(("figures".into(), "Q3".into())));
    c.failed("the document was locked");
    assert_eq!(what_stands(&c), vec!["pull the figures out of the spreadsheet"]);
}

#[test]
fn later_steps_are_skipped_rather_than_left_looking_pending() {
    let mut c = Chain::new("x", vec![export(), write(), send()]);
    c.done(Some(("figures".into(), "Q3".into())));
    c.failed("nope");
    assert_eq!(c.states[2], StepState::Skipped);
}

#[test]
fn progress_is_named_steps_not_a_percentage() {
    let mut c = Chain::new("send the report", vec![export(), write(), send()]);
    c.done(Some(("figures".into(), "Q3".into())));
    let said = c.spoken();
    assert!(said.contains("put them in the report"));
    assert!(said.contains("2 of 3"));
}

#[test]
fn finishing_says_so() {
    let mut c = Chain::new("send the report", vec![export()]);
    c.done(None);
    assert!(matches!(c.next(), Next::Finished(_)));
    assert!(c.spoken().contains("done"));
}
