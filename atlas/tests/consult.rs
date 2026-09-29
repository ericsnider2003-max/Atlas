use atlas::consult::{classify, settled, ConsultConfig, Consultation, Move, Reply};

fn cfg() -> ConsultConfig {
    ConsultConfig::default()
}

// ================= waiting for the whole answer =================

#[test]
fn a_reply_still_arriving_is_not_acted_on() {
    // A streaming reply looks like a short reply. Acting on half of one is
    // how you apply the first paragraph of a two-paragraph fix.
    let c = cfg();
    assert!(!settled("The problem is", "The problem is the cached", 5000, &c), "still growing");
    assert!(!settled("same text", "same text", 500, &c), "stopped, but not for long enough");
    assert!(settled("same text", "same text", 3000, &c));
}

#[test]
fn an_empty_window_never_counts_as_a_finished_reply() {
    assert!(!settled("", "", 999_999, &cfg()));
}

#[test]
fn a_partial_reply_classifies_as_still_coming() {
    assert_eq!(classify(""), Reply::StillComing);
    assert_eq!(Reply::StillComing.is_ready(), false);
}

// ================= a turn is spent on a solution, not a sentence =================

#[test]
fn a_question_back_does_not_use_an_attempt() {
    let r = classify("That's odd. Which version of the audio driver are you on?");
    assert!(matches!(r, Reply::Question(_)), "got {r:?}");
    assert!(!r.costs_an_attempt(), "asking is not attempting");
}

#[test]
fn an_explanation_with_no_code_does_not_use_an_attempt() {
    let r = classify("The handle is cached in the config struct, which is why re-enumerating \
                      doesn't help. You'll want to move it out.");
    assert!(matches!(r, Reply::Guidance(_)));
    assert!(!r.costs_an_attempt());
}

#[test]
fn asking_to_see_more_does_not_use_an_attempt() {
    let r = classify("Can you show me what's in src/voice.rs around line 200?");
    assert!(matches!(r, Reply::WantsMore(_)), "got {r:?}");
    assert!(!r.costs_an_attempt());
}

#[test]
fn only_something_testable_uses_an_attempt() {
    let r = classify("Try this:\n\n```rust\nfn listen() { fresh_device() }\n```");
    assert!(matches!(r, Reply::Solution(_)));
    assert!(r.costs_an_attempt());
}

#[test]
fn a_reply_that_explains_and_gives_the_change_counts_as_a_solution() {
    // Both descriptions fit; the actionable one wins.
    let r = classify("The handle is cached. Replace it:\n```rust\nfn listen() {}\n```");
    assert!(matches!(r, Reply::Solution(_)), "got {r:?}");
}

#[test]
fn the_attempt_count_only_moves_on_solutions() {
    let mut c = Consultation::new("wake word");
    c.record("here's the problem", "which driver version?", false);
    c.record("32.0.101", "can you show me the config?", false);
    c.record("here it is", "try this: ```rust\nfn x(){}\n```", true);
    assert_eq!(c.attempts_used, 1, "three messages, one attempt");
    assert_eq!(c.unproductive(), 2);
    assert!(c.summary().contains("3 exchanges, 1 of them"));
}

// ================= carrying the conversation =================

#[test]
fn it_opens_by_saying_what_it_needs_and_attaching_the_write_up() {
    let c = Consultation::new("wake word");
    match c.open("THE BRIEF", &cfg()) {
        Move::Open { message, attach } => {
            assert!(message.starts_with("Pause all other tasks"));
            assert!(message.contains("already tried"), "so nothing is re-suggested");
            assert!(message.contains("the change itself"), "and it says what it wants back");
            assert_eq!(attach, "THE BRIEF");
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn a_question_is_answered_rather_than_ending_the_conversation() {
    let c = Consultation::new("x");
    let reply = classify("Which version of the driver are you on?");
    match c.next(&reply, 12, &cfg()) {
        Move::Reply(text) => assert!(text.contains("come back"), "got: {text}"),
        o => panic!("{o:?}"),
    }
}

#[test]
fn guidance_is_pushed_toward_something_testable() {
    let c = Consultation::new("x");
    let reply = classify("You'll want to move the handle out of the config struct.");
    match c.next(&reply, 12, &cfg()) {
        Move::Reply(text) => {
            assert!(text.contains("What should I change, specifically"), "got: {text}")
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn a_solution_is_tested_when_there_are_attempts_left() {
    let c = Consultation::new("x");
    let reply = classify("```rust\nfn x() {}\n```");
    assert!(matches!(c.next(&reply, 3, &cfg()), Move::Test(_)));
}

#[test]
fn with_no_attempts_left_it_stops_instead_of_testing() {
    let c = Consultation::new("x");
    let reply = classify("```rust\nfn x() {}\n```");
    match c.next(&reply, 0, &cfg()) {
        Move::Stop(why) => assert!(why.contains("out of attempts")),
        o => panic!("{o:?}"),
    }
}

#[test]
fn a_conversation_going_nowhere_politely_still_stops() {
    let mut c = Consultation::new("x");
    for _ in 0..20 {
        c.record("...", "hmm, tell me more", false);
    }
    match c.next(&classify("what else have you tried?"), 12, &cfg()) {
        Move::Stop(why) => assert!(why.contains("without getting there"), "got: {why}"),
        o => panic!("{o:?}"),
    }
}

#[test]
fn the_result_of_each_attempt_is_reported_back() {
    // Reporting what happened is what makes it a conversation rather than a
    // series of guesses.
    let c = Consultation::new("x");
    assert!(c.report_result(true, "test result: ok. 945 passed").contains("That worked"));

    let failed = c.report_result(false, "thread 'main' panicked at src/voice.rs:212");
    assert!(failed.contains("still failing"));
    assert!(failed.contains("panicked at src/voice.rs:212"), "with the actual error");
    assert!(failed.trim().ends_with("What next?"));
}

#[test]
fn a_wall_of_failure_output_is_trimmed_before_being_pasted_back() {
    let c = Consultation::new("x");
    let huge = format!("error: the real one\n{}\nsummary", "noise\n".repeat(3000));
    let said = c.report_result(false, &huge);
    assert!(said.len() < 2000);
    assert!(said.contains("error: the real one"));
}

#[test]
fn nothing_has_been_asked_yet_is_said_plainly() {
    assert!(Consultation::new("x").summary().contains("Haven't asked yet"));
}
