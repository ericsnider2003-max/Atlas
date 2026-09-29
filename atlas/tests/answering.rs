use atlas::answering::{describe, Answer, AnsweringConfig, Channel, Question, Step};
use atlas::consent::{who_gets_told, Scope};
use atlas::presence::Gesture;

fn cfg() -> AnsweringConfig {
    AnsweringConfig::default()
}
fn ordinary() -> Question {
    Question::new("Open Chrome?", false, 0)
}
fn serious() -> Question {
    Question::new("Close everything?", true, 0)
}

// ================= answering without speaking =================

#[test]
fn a_thumbs_up_answers_an_ordinary_question() {
    // The fallback: you're on a call, or it's misheard you twice.
    let mut q = ordinary();
    assert_eq!(q.saw(Gesture::ThumbUp, &cfg()), Step::Settled(Answer::Yes(Channel::Gesture)));
    assert!(q.answer.approved());
}

#[test]
fn a_thumbs_down_declines_anything_at_all() {
    // Saying no is always safe, whatever the stakes.
    let mut q = serious();
    assert_eq!(q.saw(Gesture::ThumbDown, &cfg()), Step::Settled(Answer::No(Channel::Gesture)));
}

#[test]
fn a_nod_is_not_a_signature() {
    // A thumbs-up is two fingers of confidence from a camera that has been
    // wrong before.
    let mut q = serious();
    match q.saw(Gesture::ThumbUp, &cfg()) {
        Step::NeedsAWord(msg) => assert!(msg.contains("need to hear it"), "got: {msg}"),
        o => panic!("{o:?}"),
    }
    assert!(!q.answer.settled(), "the question stays open");
}

#[test]
fn you_can_allow_gestures_to_approve_anything_if_you_want_to() {
    let mut q = serious();
    let loose = AnsweringConfig { gestures_may_approve_anything: true, ..cfg() };
    assert!(matches!(q.saw(Gesture::ThumbUp, &loose), Step::Settled(Answer::Yes(_))));
}

#[test]
fn gestures_can_be_switched_off_entirely() {
    let mut q = ordinary();
    let off = AnsweringConfig { accept_gestures: false, ..cfg() };
    assert_eq!(q.saw(Gesture::ThumbUp, &off), Step::Wait);
}

#[test]
fn an_unrelated_gesture_is_ignored() {
    let mut q = ordinary();
    assert_eq!(q.saw(Gesture::None, &cfg()), Step::Wait);
}

// ================= all three channels stay open =================

#[test]
fn speaking_typing_or_gesturing_all_answer_the_same_question() {
    let mut a = ordinary();
    assert!(matches!(a.heard("yes", &cfg()), Step::Settled(Answer::Yes(Channel::Voice))));

    let mut b = ordinary();
    assert!(matches!(b.typed("yes", &cfg()), Step::Settled(Answer::Yes(Channel::Typed))));

    let mut c = ordinary();
    assert!(matches!(c.saw(Gesture::ThumbUp, &cfg()), Step::Settled(Answer::Yes(Channel::Gesture))));
}

#[test]
fn the_first_answer_wins_and_later_ones_are_ignored() {
    let mut q = ordinary();
    q.heard("no", &cfg());
    q.saw(Gesture::ThumbUp, &cfg());
    assert!(matches!(q.answer, Answer::No(Channel::Voice)), "got {:?}", q.answer);
}

#[test]
fn an_unrelated_sentence_does_not_resolve_the_question() {
    let mut q = ordinary();
    assert_eq!(q.heard("what's the weather", &cfg()), Step::Wait);
    assert!(!q.answer.settled(), "the question is still open");
}

// ================= waiting =================

#[test]
fn the_question_is_repeated_once_before_giving_up() {
    let mut q = ordinary();
    assert_eq!(q.tick(&cfg(), 5), Step::Wait);
    assert_eq!(q.tick(&cfg(), 20), Step::Repeat("Open Chrome?".into()));
    assert_eq!(q.tick(&cfg(), 25), Step::Wait, "asked twice is enough");
}

#[test]
fn silence_is_never_a_yes_however_long_it_lasts() {
    let mut q = serious();
    assert_eq!(q.tick(&cfg(), 100), Step::Settled(Answer::TimedOut));
    assert!(!q.answer.approved());
    assert!(q.answer.settled());
}

#[test]
fn how_you_answered_is_recorded() {
    let mut q = ordinary();
    q.saw(Gesture::ThumbUp, &cfg());
    assert_eq!(q.how(), Some(Channel::Gesture));
    assert_eq!(describe(&q), "you gave a thumbs up");

    let mut t = ordinary();
    t.tick(&cfg(), 100);
    assert!(describe(&t).contains("didn't answer"));
}

// ================= who gets told what =================

#[test]
fn there_are_two_different_announcements_and_they_are_not_the_same_thing() {
    // Telling you is confirmation. Telling the call is consent.
    assert!(who_gets_told(Scope::YouOnly).contains("only you"));
    assert!(who_gets_told(Scope::Everyone).contains("everyone on the call"));
    assert!(who_gets_told(Scope::Off).contains("nothing is being recorded"));
}

#[test]
fn recording_only_yourself_involves_telling_nobody_else() {
    let told = who_gets_told(Scope::YouOnly);
    assert!(told.contains("captures you and nobody else"));
}
