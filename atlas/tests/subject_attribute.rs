//! Structured facts: (subject, attribute, value).
//!
//! The old book merged by word overlap, so "my car is a Honda" and "my car is
//! a Toyota" shared only "car" and both survived, contradicting each other.
//! Now a fact carries the slot it fills, and a new value in the same slot
//! supersedes the old one however different the words are — vast *and*
//! accurate. And a question that names a subject is answered with the one fact
//! that fills it, not everything that mentions the word.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::facts::{triple, Book, Fact, Kind};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn dir(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-subjattr-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}
fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}
fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

// --- the extractor reads the plain shapes ----------------------------------

#[test]
fn a_plain_statement_becomes_a_slot() {
    let (s, a, v) = triple("my car is a Honda").expect("a type statement is a slot");
    assert_eq!(s, "car");
    assert_eq!(a, "type");
    assert_eq!(v, "honda");
}

#[test]
fn an_explicit_attribute_is_read_as_its_own_slot() {
    let (s, a, v) = triple("the colour of my car is red").expect("attr-of-subject is a slot");
    assert_eq!(s, "car");
    assert_eq!(a, "colour");
    assert_eq!(v, "red");
}

#[test]
fn a_possessive_attribute_is_read() {
    let (s, a, v) = triple("the car's mileage is 40000").expect("possessive is a slot");
    assert_eq!(s, "car");
    assert_eq!(a, "mileage");
    assert_eq!(v, "40000");
}

#[test]
fn something_that_is_not_a_statement_about_one_thing_has_no_slot() {
    // A document sentence with no "X is Y" shape falls back to word-overlap.
    assert!(triple("remember to water the plants on Tuesday").is_none());
}

// --- the contradiction the old book could not handle -----------------------

#[test]
fn correcting_a_value_supersedes_it_even_when_the_words_dont_overlap() {
    let mut b = Book::default();
    b.learn(Fact::stated("my car is a Honda", 100), 100);
    b.learn(Fact::stated("my car is a Toyota", 200), 200);
    // One fact about the car, not two contradicting each other.
    let about = b.recall("car", 200);
    assert_eq!(about.len(), 1, "the correction must supersede, not duplicate: {about:?}");
    // And it is the new value that survives.
    let f = about[0];
    assert!(f.summary.to_lowercase().contains("toyota"), "newest value wins: {}", f.summary);
    assert!(!f.summary.to_lowercase().contains("honda"), "old value is gone: {}", f.summary);
    assert!(f.confirmed >= 1, "the slot was restated, so it is strengthened");
}

#[test]
fn two_different_attributes_of_one_subject_both_survive() {
    let mut b = Book::default();
    b.learn(Fact::stated("my car is a Honda", 100), 100);
    b.learn(Fact::stated("the colour of my car is red", 100), 100);
    // Make and colour are different slots; both are kept.
    let about = b.recall("car", 100);
    assert_eq!(about.len(), 2, "make and colour are different facts: {about:?}");
}

#[test]
fn a_type_and_a_quality_do_not_clobber_each_other() {
    // "is a Honda" (a type) and "is fast" (a quality) are different slots, so
    // learning the second must not overwrite the first.
    let mut b = Book::default();
    b.learn(Fact::stated("my car is a Honda", 100), 100);
    b.learn(Fact::stated("my car is fast", 100), 100);
    let about = b.recall("car", 100);
    assert_eq!(about.len(), 2, "a type and a quality are different slots: {about:?}");
}

// --- precise recall ---------------------------------------------------------

#[test]
fn a_question_naming_a_subject_gets_the_one_fact_that_fills_it() {
    let mut b = Book::default();
    b.learn(Fact::stated("my car is a Toyota", 100), 100);
    b.learn(Fact::new("unrelated", "the garage code is 4471", "the garage code is 4471", Kind::Reference, 100), 100);
    let hit = b.slot_answer("what kind of car do I have", 100).expect("the car slot answers");
    assert!(hit.summary.to_lowercase().contains("toyota"), "precise: {}", hit.summary);
}

#[test]
fn precise_recall_carries_a_correction_through_a_whole_daemon() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(dir("correct")), Proactive::new(ProactiveConfig::default()));
    d.turn("note that my car is a Honda", 100);
    d.turn("note that my car is a Toyota", 200);
    let reply = d.turn("what kind of car do I have", 200);
    assert!(reply.to_lowercase().contains("toyota"), "the current value is answered: {reply}");
    assert!(!reply.to_lowercase().contains("honda"), "the old value is not still asserted: {reply}");
    // The book holds one car fact, not two contradicting — the correction
    // merged rather than piled up.
    assert_eq!(d.facts.recall("car", 200).len(), 1, "one fact about the car, superseded not duplicated");
}

// --- learning from natural corrections (no "note that") ---------------------

#[test]
fn a_natural_correction_updates_what_atlas_knows() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(dir("nat")), Proactive::new(ProactiveConfig::default()));
    d.turn("note that my car is a Honda", 100);
    // No "note that" — a plain correction. It should still update the fact,
    // because it names a slot Atlas already holds.
    let ack = d.turn("actually my car is a Toyota", 200);
    assert!(ack.to_lowercase().contains("updated") || ack.to_lowercase().contains("toyota"), "acknowledged: {ack}");
    let reply = d.turn("what kind of car do I have", 200);
    assert!(reply.to_lowercase().contains("toyota"), "the correction took: {reply}");
    assert!(!reply.to_lowercase().contains("honda"), "the old value is gone: {reply}");
    assert_eq!(d.facts.recall("car", 200).len(), 1, "the natural correction merged into one fact");
}

#[test]
fn ordinary_talk_is_not_vacuumed_into_memory() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(dir("talk")), Proactive::new(ProactiveConfig::default()));
    // A declaration about something Atlas has never heard of, with no correction
    // marker, must not be silently stored — that would make memory a firehose.
    let _ = d.turn("the weather is nice today", 100);
    assert!(
        d.facts.recall("weather", 100).is_empty(),
        "an unmarked statement about an unknown subject is not filed"
    );
}

#[test]
fn a_marked_correction_about_a_new_subject_is_learned() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(dir("marked")), Proactive::new(ProactiveConfig::default()));
    // A clear correction marker is enough on its own, even for a new subject.
    let _ = d.turn("correction: my office is on the third floor", 100);
    let reply = d.turn("what do you know about my office", 100);
    assert!(reply.to_lowercase().contains("third floor"), "the marked correction was learned: {reply}");
    assert!(!d.facts.recall("office", 100).is_empty(), "the office fact is in the book");
}

