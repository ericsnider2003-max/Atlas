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
        "write me a letter to my landlord", "write me a report on solar panels", "I really look up to him",
        "look up to your parents", "work on the garden this weekend", "update the kitchen project budget",
        "add to the shopping list project",
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
        ("improve atlas", "Improve"), ("work on the atlas code", "Improve"), ("open chrome", "OpenApp"), ("look up the population of France", "Research"),
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
    assert_ne!(read("find me a book", &d), Register::Working);
    assert_eq!(read("find me a file about the lease", &d), Register::Working);
    assert_eq!(read("open chrome", &d), Register::Working);
}

/// Finished work said without being asked is kept in the conversation, so
/// "what did you find?" has something to go on (30 Sep 2026 sweep).
#[test]
fn what_atlas_said_on_its_own_is_shown_to_the_model() {
    let mut th = atlas::thread::Thread::default();
    th.append("research tide times", "Looking into tide times.", None, 10);
    th.append("", "Tides at Ventura peak just after noon.", None, 20);
    th.append("thanks", "Any time.", None, 30);
    let msgs = th.messages(12, 4000);
    let all = format!("{msgs:?}");
    assert!(all.contains("You told them without being asked: Tides at Ventura"), "{all}");
    // And no empty turn of yours is made up for it.
    assert!(!all.contains("content: \"\""), "{all}");
}

/// The open floor after a reply: a voice that clearly isn't yours isn't
/// answered, and can't answer Atlas's question for you; your name still
/// works whatever the voice check says (30 Sep 2026 ruling).
#[test]
fn another_voice_on_the_open_floor_is_not_taken_for_yours() {
    use atlas::addressing::{assess, respond, Directed, Response, Situation};
    let other = Situation { other_voice: true, just_spoke: true, ..Default::default() };
    let a = assess("what's the weather tomorrow", &other);
    assert_eq!(a.directed, Directed::Overheard);
    assert!(matches!(respond(&a, &other), Response::Ignore));
    // Asked something: a stranger's "yes" is not your approval, and it says
    // how to answer if it was you.
    let asked = Situation { other_voice: true, awaiting_answer: true, ..Default::default() };
    match respond(&assess("yes go ahead", &asked), &asked) {
        Response::Ask(q) => assert!(q.contains("start with my name"), "{q}"),
        r => panic!("acted on another voice: {r:?}"),
    }
    // By name, it's answered whatever the voice check said.
    assert_eq!(assess("atlas, yes go ahead", &asked).directed, Directed::AtAtlas);
    // Your own voice on the open floor is unaffected.
    let you = Situation { just_spoke: true, ..Default::default() };
    assert!(matches!(respond(&assess("what's the weather tomorrow", &you), &you), Response::Act));
}

#[test]
fn the_daemon_leaves_another_voice_unanswered_on_the_open_floor() {
    use atlas::daemon::{Arrival, Daemon};
    let c = atlas::config::Config::load(std::path::Path::new("config")).unwrap();
    let p = atlas::platform::mock::MockPlatform::new(vec![atlas::platform::Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let dir = std::env::temp_dir().join(format!("atlas-other-voice-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut d = Daemon::new(&c, &p, None, atlas::store::Store::new(dir), atlas::proactive::Proactive::new(atlas::proactive::ProactiveConfig::default()));
    d.last_verdict = atlas::voiceid::Verdict::NotYou(0.12);
    assert_eq!(d.turn_from("open chrome", 1_000, Arrival::OpenMic), "");
    // The wake word (a directed turn) is answered, whatever the check said.
    assert_ne!(d.turn_from("what time is it", 1_010, Arrival::Directed), "");
}

/// The evidence for the voice lines comes from your own turns after the wake
/// word, not only from turns that already passed (30 Sep 2026).
#[test]
fn the_voice_lines_are_judged_against_your_own_turns() {
    let mut v = atlas::voiceid::VoiceId::default();
    assert!(v.calibration_report(0.15).is_none(), "not before ten turns");
    for s in [0.42, 0.45, 0.38, 0.51, 0.47, 0.40, 0.44, 0.39, 0.49, 0.36] {
        v.note_after_name(s);
    }
    v.note_turned_away(0.08);
    v.note_turned_away(0.12);
    let r = v.calibration_report(0.15).unwrap();
    assert!(r.contains("as low as 0.36"), "{r}");
    assert!(r.contains("0 of them were at or under"), "{r}");
    assert!(r.contains("turned away scored at most 0.12"), "{r}");
    assert!(r.contains("a line near 0.24"), "{r}");
    // An off day under the line is said, with the way round it.
    v.note_after_name(0.10);
    let r = v.calibration_report(0.15).unwrap();
    assert!(r.contains("saying \"Atlas\" first always works"), "{r}");
}
