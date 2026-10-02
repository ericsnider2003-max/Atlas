//! Phase 0.4 and 0.5 (Eric's go-ahead, 1 Oct 2026; the 30 Sep thread in
//! `claude/atlas-why-stale-2026-10-01.md`):
//!
//! - "you're responding slowly" was answered "the slowest part of a turn is
//!   speak, about 9.6 seconds", and so was the iPhone question;
//! - "is it smart to upgrade from the 15 Pro Max..." got "I'm not giving you
//!   an answer -- I'm asking why you're asking".
//!
//! Atlas's timings answer "why are you slow" and nothing else; a complaint
//! gets a short sorry; an advice question gets an answer.

use atlas::brain::{asks_how_it_could_be_faster, complains_of_speed, decision_from_chat, ChatReply, ToolCall};
use atlas::intent::Intent;
use atlas::register::Register;

fn chose_recommend(said: &str) -> Intent {
    let r = ChatReply {
        text: "Sorry about that.".into(),
        tool_calls: vec![ToolCall { name: "recommend".into(), arguments: serde_json::json!({}) }],
    };
    decision_from_chat(&r, said).intent
}

#[test]
fn a_complaint_about_speed_is_not_a_question_about_speed() {
    for said in [
        "you're responding slowly",
        "Atlas, you're slow today",
        "this is taking forever",
        "Atlas you're taking too long",
        "hurry up",
    ] {
        assert!(complains_of_speed(said), "{said}");
        assert!(!asks_how_it_could_be_faster(said), "{said}");
        assert_ne!(chose_recommend(said), Intent::Recommend, "timings offered for: {said}");
    }
}

#[test]
fn asking_why_it_is_slow_still_gets_the_timings() {
    for said in [
        "why are you so slow?",
        "Atlas, what's slowing you down?",
        "what would make you faster?",
        "how could you be better at your job?",
    ] {
        assert!(asks_how_it_could_be_faster(said), "{said}");
        assert!(!complains_of_speed(said), "{said}");
        assert_eq!(chose_recommend(said), Intent::Recommend, "{said}");
    }
}

#[test]
fn the_phone_question_never_gets_atlas_timings() {
    let said = "is it smart to upgrade from the 15 Pro Max to the iPhone 17 Pro?";
    assert!(!asks_how_it_could_be_faster(said));
    assert!(!complains_of_speed(said));
    assert_ne!(chose_recommend(said), Intent::Recommend);
}

#[test]
fn the_name_alone_does_not_make_a_sentence_about_atlas() {
    // "Atlas" in front is calling it by name (nearly every sentence Eric says).
    assert_ne!(chose_recommend("Atlas, would it be smart to upgrade my phone"), Intent::Recommend);
}

#[test]
fn a_complaint_turns_the_moment_into_a_sorry_without_figures() {
    let p = atlas::persona::Persona::default();
    let line = p.for_this_turn_on(Register::Chatting, 3, "you're responding slowly", false);
    assert!(line.contains("say sorry"), "{line}");
    assert!(line.contains("Don't quote timings"), "{line}");
    let plain = p.for_this_turn_on(Register::Chatting, 3, "what's a good guitar to start on?", false);
    assert!(!plain.contains("say sorry"), "{plain}");
}

#[test]
fn advice_questions_are_answered_with_a_view_and_a_reason() {
    let character = atlas::persona::Persona::default().character();
    assert!(character.contains("Never answer a question with a question"));
    assert!(character.contains("Advice: your view, why, what's unsure"), "{character}");
    // Current facts are offered as a look-up by the research rule
    // (`brain::made_up_action` / `worth_looking_up`), not by this prompt.
}

#[test]
fn the_bench_counts_a_dodge_or_a_timing_as_a_fault() {
    let dodges = |reply: &str| atlas::talkbench::faults_in(reply, 4).iter().filter(|x| x.starts_with("talks about itself or dodges")).count();
    assert_eq!(dodges("I'm not giving you an answer — I'm asking why you're asking."), 2);
    assert_eq!(dodges("Right now the slowest part of a turn is speak, about 9.6 seconds."), 1);
    assert_eq!(dodges("Yes if your battery is fading; the camera is the main gain. Prices change, so I can look them up."), 0);
    // Both 30 Sep sentences are in the conversation every model is given.
    let at = |needle: &str| atlas::talkbench::SCRIPT.iter().position(|s| s.contains(needle));
    assert_ne!(at("15 Pro Max"), None);
    assert_ne!(at("responding slowly"), None);
}
