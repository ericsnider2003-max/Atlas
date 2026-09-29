use atlas::dictate::{
    may_type_into, parse, refusal, render, DictateConfig, Dictation, Piece, State,
};
use atlas::endpoint::{shape_of, EndpointConfig, Endpointer, Listening, Shape, Why};
use atlas::watching::{describe, Outcome, WatchConfig, Watcher};
use atlas::why::{answer, is_asking_why, Decision, Record};

// ================= knowing when you've stopped =================

fn ecfg() -> EndpointConfig {
    EndpointConfig::default()
}
const LOUD: f32 = -20.0;
const QUIET: f32 = -60.0;

#[test]
fn a_short_reply_ends_almost_immediately() {
    // "Yes" is finished the moment you stop. Waiting seven seconds after it
    // is the thing that makes voice software feel slow.
    let mut e = Endpointer::start(0);
    e.feed(LOUD, "", 200, &ecfg());
    e.feed(LOUD, "yes", 400, &ecfg());
    assert_eq!(e.feed(QUIET, "yes", 500, &ecfg()), Listening::Pausing);
    assert_eq!(e.feed(QUIET, "yes", 900, &ecfg()), Listening::Finished(Why::YouFinished));
}

#[test]
fn a_dangling_word_means_you_are_not_finished_however_long_the_pause() {
    // "I want you to" is plainly mid-thought. Cutting it off there is worse
    // than waiting an extra half second.
    assert_eq!(shape_of("i want you to"), Shape::MidPhrase);
    assert_eq!(shape_of("open chrome and"), Shape::MidPhrase);
    assert_eq!(shape_of("because the"), Shape::MidPhrase);

    let mut e = Endpointer::start(0);
    e.feed(LOUD, "", 200, &ecfg());
    assert_eq!(e.feed(QUIET, "i want you to", 900, &ecfg()), Listening::Pausing, "still thinking");
    assert_eq!(
        e.feed(QUIET, "i want you to", 2100, &ecfg()),
        Listening::Finished(Why::YouFinished),
        "and it does finish, just later"
    );
}

#[test]
fn a_finished_sentence_gets_a_middling_wait() {
    assert_eq!(shape_of("open chrome on the left screen"), Shape::Sentence);
    assert!(Shape::Short.silence_needed(&ecfg()) < Shape::Sentence.silence_needed(&ecfg()));
    assert!(Shape::Sentence.silence_needed(&ecfg()) < Shape::MidPhrase.silence_needed(&ecfg()));
}

#[test]
fn saying_nothing_at_all_gives_up_quickly() {
    let mut e = Endpointer::start(0);
    for ms in (200..3000).step_by(100) {
        e.feed(QUIET, "", ms, &ecfg());
    }
    assert_eq!(e.state, Listening::Finished(Why::Nothing));
    assert!(!e.heard_anything);
}

#[test]
fn a_stuck_microphone_cannot_record_forever() {
    let mut e = Endpointer::start(0);
    for ms in (200..25_000).step_by(500) {
        e.feed(LOUD, "still going", ms, &ecfg());
    }
    assert_eq!(e.state, Listening::Finished(Why::RanTooLong));
}

#[test]
fn the_first_moments_are_ignored_because_they_catch_the_room() {
    let mut e = Endpointer::start(0);
    assert_eq!(e.feed(LOUD, "", 50, &ecfg()), Listening::Waiting, "warmup");
    assert_eq!(e.feed(LOUD, "", 200, &ecfg()), Listening::Speaking);
}

#[test]
fn ending_early_is_the_whole_point_on_this_hardware() {
    // A fixed window transcribes eight seconds whether you spoke for one or
    // seven.
    let mut e = Endpointer::start(0);
    e.feed(LOUD, "", 200, &ecfg());
    e.feed(LOUD, "yes", 400, &ecfg());
    e.feed(QUIET, "yes", 500, &ecfg());
    e.feed(QUIET, "yes", 900, &ecfg());
    assert!(e.finished());
    assert!(e.clip_ms(900) < 1000, "seven seconds of audio not transcribed");
}

// (per-stage turn timing moved wholly to `timing::Turn`; tested in
// tests/timing.rs. The dead `endpoint::Turn` duplicate was removed.)

// ================= why did you do that =================

fn record() -> Record {
    let mut r = Record::default();
    r.note_full(Decision {
        at: 1,
        what: "put Chrome on the left screen".into(),
        because: "the left one is the widest, and Chrome is your reading window".into(),
        instead_of: Some("the right screen, where Claude goes".into()),
        set_by: Some("config/layouts.yaml".into()),
    });
    r.note("listened on the webcam mic", "it hears you at -22dB, the laptop mic at -58", 2);
    r
}

#[test]
fn asking_why_is_recognised_however_you_phrase_it() {
    assert!(is_asking_why("why did you put chrome there"));
    assert!(is_asking_why("how come you used that microphone"));
    assert!(is_asking_why("what made you pick that"));
    assert!(!is_asking_why("open chrome"));
}

#[test]
fn you_get_the_reason_not_a_restatement_of_the_question() {
    let r = record();
    let said = answer(r.find("why is chrome over there"));
    assert!(said.contains("because the left one is the widest"), "got: {said}");
}

#[test]
fn the_alternative_is_named_when_there_was_one() {
    let said = answer(record().find("why is chrome on the left"));
    assert!(said.contains("Otherwise it would have been the right screen"));
}

#[test]
fn it_says_where_the_rule_lives_so_you_can_change_it_rather_than_argue() {
    let said = answer(record().find("chrome screen"));
    assert!(said.contains("config/layouts.yaml"));
}

#[test]
fn a_measurement_is_given_back_as_the_measurement() {
    let said = answer(record().find("why that microphone"));
    assert!(said.contains("-22dB"), "the actual numbers: {said}");
}

#[test]
fn asking_about_something_it_never_decided_says_so() {
    let said = answer(record().find("why is the sky blue"));
    assert!(said.contains("don't have a decision recorded"));
}

#[test]
fn the_record_is_bounded() {
    let mut r = Record::default();
    for i in 0..500 {
        r.note(&format!("thing {i}"), "reasons", i);
    }
    assert!(r.decisions.len() <= 300);
    assert!(r.last().unwrap().what.contains("499"));
}

// ================= watching something long =================

fn wcfg() -> WatchConfig {
    WatchConfig::default()
}

#[test]
fn something_that_finished_while_you_were_sitting_there_is_not_news() {
    let mut w = Watcher::default();
    let id = w.watch("the render", "ffmpeg.exe", 0);
    w.update(id, Outcome::Finished, "done", 20);
    assert!(w.to_report(&wcfg(), 20).is_empty(), "twenty seconds isn't worth announcing");
}

#[test]
fn something_that_took_ten_minutes_is() {
    let mut w = Watcher::default();
    let id = w.watch("the render", "ffmpeg.exe", 0);
    w.update(id, Outcome::Finished, "done", 600);
    let said = w.to_report(&wcfg(), 600);
    assert_eq!(said.len(), 1);
    assert!(said[0].contains("finished") && said[0].contains("10 minutes"), "got: {}", said[0]);
}

#[test]
fn a_failure_is_always_worth_saying_however_quick() {
    let mut w = Watcher::default();
    let id = w.watch("the build", "cargo.exe", 0);
    w.update(id, Outcome::Failed, "error: linker not found", 5);
    let said = w.to_report(&wcfg(), 5);
    assert_eq!(said.len(), 1);
    assert!(said[0].contains("linker not found"), "with the last line, which is the useful one");
}

#[test]
fn you_are_told_once_not_every_tick() {
    let mut w = Watcher::default();
    let id = w.watch("the render", "ffmpeg.exe", 0);
    w.update(id, Outcome::Finished, "", 600);
    assert_eq!(w.to_report(&wcfg(), 600).len(), 1);
    assert!(w.to_report(&wcfg(), 610).is_empty());
}

#[test]
fn a_process_that_vanished_is_reported_as_that_rather_than_as_success() {
    let mut w = Watcher::default();
    let id = w.watch("the export", "app.exe", 0);
    w.update(id, Outcome::Vanished, "", 300);
    assert!(w.to_report(&wcfg(), 300)[0].contains("disappeared without finishing"));
}

#[test]
fn you_can_ask_what_is_still_going() {
    let mut w = Watcher::default();
    w.watch("the render", "ffmpeg.exe", 0);
    assert_eq!(w.running().len(), 1);
    assert!(describe(w.running()[0], 300).contains("still going, 5 minutes in"));
}

// ================= dictation =================

fn dcfg() -> DictateConfig {
    DictateConfig { enabled: true, ..Default::default() }
}

#[test]
fn spoken_punctuation_becomes_punctuation() {
    let pieces = parse("hello there comma how are you question mark", &dcfg());
    assert_eq!(render(&pieces, &dcfg()), "Hello there, how are you?");
}

#[test]
fn a_word_that_happens_to_be_punctuation_is_left_alone() {
    // "Period drama" is not a full stop.
    let out = render(&parse("we watched a period drama", &dcfg()), &dcfg());
    assert_eq!(out, "We watched a period drama");
}

#[test]
fn punctuation_attaches_to_the_word_rather_than_floating() {
    // The bit that makes dictated text look typed rather than assembled.
    let out = render(&parse("done full stop", &dcfg()), &dcfg());
    assert_eq!(out, "Done.");
    assert!(!out.contains(" ."));
}

#[test]
fn sentences_capitalise_themselves() {
    let out = render(&parse("first one full stop second one full stop", &dcfg()), &dcfg());
    assert_eq!(out, "First one. Second one.");
}

#[test]
fn new_paragraph_is_never_a_phrase_you_wanted_typed() {
    let pieces = parse("that's the end new paragraph and now this", &dcfg());
    assert!(pieces.contains(&Piece::NewParagraph));
    assert!(render(&pieces, &dcfg()).contains("\n\n"));
}

#[test]
fn scratch_that_takes_back_the_last_thing() {
    let mut d = Dictation::start("notepad", 0);
    d.heard("this is a mistake", "notepad", &dcfg(), 1).unwrap();
    let back = d.heard("scratch that", "notepad", &dcfg(), 2).unwrap_err();
    assert!(back.contains("Took back: This is a mistake"));
}

#[test]
fn moving_to_another_window_stops_it() {
    // Typing into whatever happens to be in front of you now is how dictated
    // text ends up in the wrong place.
    let mut d = Dictation::start("notepad", 0);
    let err = d.heard("hello", "chrome", &dcfg(), 1).unwrap_err();
    assert!(err.contains("you moved to chrome"));
    assert_eq!(d.state, State::Off);
}

#[test]
fn some_windows_are_never_dictated_into() {
    // A misheard sentence in a chat window is public; in a document it's a
    // typo.
    assert!(!may_type_into("Discord", &dcfg()));
    assert!(!may_type_into("Microsoft Teams", &dcfg()));
    assert!(may_type_into("Notepad", &dcfg()));
    assert!(refusal("Discord").contains("public"));
}

#[test]
fn saying_stop_ends_it() {
    let mut d = Dictation::start("notepad", 0);
    assert!(d.heard("stop dictating", "notepad", &dcfg(), 1).is_err());
    assert_eq!(d.state, State::Off);
}

#[test]
fn it_stops_itself_when_you_wander_off() {
    let mut d = Dictation::start("notepad", 0);
    assert!(!d.idle_check(&dcfg(), 10));
    assert!(d.idle_check(&dcfg(), 100));
    assert_eq!(d.state, State::Off);
}

#[test]
fn dictation_is_off_until_you_turn_it_on() {
    assert!(!DictateConfig::default().enabled);
}

// ================= reading ambiguous words in context =================

use atlas::dictate::{ask_which, read_ambiguous, Reading};

#[test]
fn a_determiner_before_it_makes_it_a_word() {
    // You never say "a" before a full stop.
    assert_eq!(read_ambiguous("we watched a", "period", " drama"), Reading::Literal);
    assert_eq!(read_ambiguous("put the", "dash", " there"), Reading::Literal);
    assert_eq!(read_ambiguous("that's my", "quote", ""), Reading::Literal);
}

#[test]
fn a_noun_after_it_makes_it_a_word_too() {
    assert_eq!(read_ambiguous("classic", "period", " drama"), Reading::Literal);
    assert_eq!(read_ambiguous("a", "comma", " splice"), Reading::Literal);
    assert_eq!(read_ambiguous("the em", "dash", " key"), Reading::Literal);
}

#[test]
fn mid_sentence_with_words_either_side_is_the_mark() {
    // "Hello comma how are you" is the normal dictation case.
    assert_eq!(read_ambiguous("hello there", "comma", " how are you"), Reading::Mark);
    assert_eq!(read_ambiguous("that's done", "period", " next thing"), Reading::Mark);
}

#[test]
fn at_the_end_of_what_you_said_it_is_the_mark() {
    assert_eq!(read_ambiguous("that's the end", "period", ""), Reading::Mark);
}

#[test]
fn the_word_on_its_own_is_a_mark_you_are_inserting() {
    assert_eq!(read_ambiguous("", "comma", ""), Reading::Mark);
}

#[test]
fn a_period_drama_survives_dictation_intact() {
    // The case that made me drop the word entirely, which was the wrong fix.
    let out = render(&parse("we watched a period drama last night", &dcfg()), &dcfg());
    assert_eq!(out, "We watched a period drama last night");
}

#[test]
fn but_period_still_works_as_punctuation_where_it_obviously_is() {
    let out = render(&parse("that's the end period", &dcfg()), &dcfg());
    assert_eq!(out, "That's the end.");
}

#[test]
fn a_comma_splice_is_not_a_comma() {
    let out = render(&parse("that's a comma splice", &dcfg()), &dcfg());
    assert_eq!(out, "That's a comma splice");
}

#[test]
fn and_a_real_comma_still_lands() {
    let out = render(&parse("hello there comma how are you", &dcfg()), &dcfg());
    assert_eq!(out, "Hello there, how are you");
}

#[test]
fn the_unambiguous_phrases_never_go_through_any_of_this() {
    // Nobody says "full stop" or "new paragraph" literally, so there is
    // nothing to weigh up.
    assert_eq!(render(&parse("done full stop", &dcfg()), &dcfg()), "Done.");
    assert!(parse("that's it new paragraph and now this", &dcfg()).contains(&Piece::NewParagraph));
}

#[test]
fn when_it_genuinely_cannot_tell_it_asks_rather_than_guessing() {
    assert!(ask_which("comma").contains("mark, or the word"));
}
