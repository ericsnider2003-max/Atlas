//! The voice-and-conversation sweep (30 Sep 2026): ordinary sentences the
//! parser took for commands, yes/no that had to be word-perfect, a name with
//! a pause after it, "bye" thrown away, and the wake word lost for good.

use atlas::intent::{Intent, Parser};

fn parser() -> Parser {
    let c = atlas::config::Config::load(std::path::Path::new("config")).unwrap();
    Parser::new(&c.commands)
}

#[test]
fn ordinary_sentences_are_not_taken_for_commands() {
    let p = parser();
    for s in [
        "what's on my mind", "what's on TV tonight", "back up a second, what did you mean", "drop the kids off at 3",
        "just talk to me normally", "this is a great idea", "that is my favorite song", "invite Sam to dinner",
        "schedule is busy", "block him on Twitter", "can you book me a flight Friday", "explain the yield curve",
        "explain what a Roth IRA is", "improve my sleep", "open to suggestions", "google is down again",
        "research shows coffee is fine", "they said no to the offer", "I'm not doing the dishes tonight",
        "write me a letter to my landlord", "write me a report on solar panels",
    ] {
        assert!(matches!(p.parse(s), Intent::Unknown(_)), "{s:?} became {:?}", p.parse(s));
    }
}

#[test]
fn the_commands_they_resembled_still_work() {
    let p = parser();
    for (s, want) in [
        ("what's on today", "Agenda"), ("what's on tomorrow", "Agenda"), ("back up my files", "BackUp"),
        ("invite a friend", "Pair"), ("schedule dentist tomorrow at 3pm", "Schedule"),
        ("book lunch with Sam friday at noon", "Schedule"), ("block off friday afternoon for writing", "Schedule"),
        ("improve atlas", "Improve"), ("open chrome", "OpenApp"), ("look up the population of France", "Research"),
        ("google best budget laptops", "Research"), ("research solar panel efficiency", "Research"),
        ("drop the task about taxes", "DropTask"), ("call me Eric", "AddressAs"), ("this is my sister Anna", "NameThis"),
    ] {
        let got = format!("{:?}", p.parse(s));
        assert!(got.starts_with(want), "{s:?} became {got}");
    }
}

#[test]
fn yes_and_no_are_heard_the_way_people_say_them() {
    use atlas::session::{is_no, is_yes};
    for s in ["yes", "yes please", "yeah go for it", "sure, do it", "sounds good", "ok go ahead", "absolutely"] {
        assert!(is_yes(s), "{s}");
    }
    for s in ["yes but change the subject first", "yeah wait", "no", "okay actually don't", "yes and also book a table for six at the place near work"] {
        assert!(!is_yes(s), "{s}");
    }
    for s in ["no", "no thanks, I'm good", "nah leave it", "not right now", "nope"] {
        assert!(is_no(s), "{s}");
    }
    for s in ["no wait, do it", "no, yes go ahead"] {
        assert!(!is_no(s), "{s}");
    }
}

#[test]
fn a_conversation_ends_when_you_say_so() {
    use atlas::session::ends_the_conversation;
    for s in ["bye", "Thanks, that's all.", "that's it", "ok thanks", "goodbye", "no that's it"] {
        assert!(ends_the_conversation(s), "{s}");
    }
    for s in ["thanks for the reminder, now move it to 4", "that's it for the draft, send it", "bye the way"] {
        assert!(!ends_the_conversation(s), "{s}");
    }
    assert!(!atlas::voice::not_really_said("bye"), "'bye' is a word, not a ghost");
}

#[test]
fn a_dropped_wake_word_comes_back_by_itself_and_far_apart_failures_dont_add_up() {
    use atlas::input::{Tier, Tiers, FAILURES_COUNT_WITHIN_SECS, TRY_WAKE_AGAIN_AFTER_SECS};
    let mut t = Tiers::default();
    // Three hiccups hours apart: never dropped.
    assert!(t.failed_at(1_000).is_none());
    assert!(t.failed_at(1_000 + FAILURES_COUNT_WITHIN_SECS + 10).is_none());
    assert!(t.failed_at(1_000 + 2 * (FAILURES_COUNT_WITHIN_SECS + 10)).is_none());
    assert_eq!(t.tier, Tier::Voice);
    // Three in a row: dropped to push-to-talk...
    let at = 50_000;
    t.failed_at(at);
    t.failed_at(at + 5);
    assert!(t.failed_at(at + 10).is_some());
    assert_eq!(t.tier, Tier::PushToTalk);
    // ...and tried again by itself, a couple of minutes later.
    assert!(t.try_the_wake_word_again(at + 20).is_none());
    assert!(t.try_the_wake_word_again(at + 10 + TRY_WAKE_AGAIN_AFTER_SECS).is_some());
    assert_eq!(t.tier, Tier::Voice);
}

#[test]
fn how_a_sentence_is_read_is_not_fooled_by_one_word() {
    use atlas::register::{read, Register};
    let d = Default::default();
    assert_ne!(read("seriously, what's the best broker for options?", &d), Register::Rough);
    assert_eq!(read("seriously", &d), Register::Rough);
    assert_ne!(read("what happens to bonds when rates rise", &d), Register::AboutAtlas);
    assert_ne!(read("find me a good book on options", &d), Register::Working);
    assert_eq!(read("open chrome", &d), Register::Working);
}
