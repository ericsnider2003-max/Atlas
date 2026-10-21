//! **Free-form speech, measured (CONV1 phase 1, 12 Oct 2026).**
//!
//! Eric's standard (06_AUDIT_AND_GAPS/CONVERSATION_LAYER_C1.md): say any
//! coherent sentence, in any order, offline, and Atlas acts. He has been told
//! more than once that this was fixed when it was not. So this file measures
//! before it claims: a corpus of ordinary spoken sentences, each with the
//! ordered list of things it asks for, run against the router with NO model.
//!
//! Each expected step is written as the plain command it means ("open
//! chrome"), and turned into the real reading by the real router
//! (`splitter::reading_of`: the phrase table, then the helpers the turn asks
//! when the table has nothing). An expected step that does not itself read is
//! a broken case, and fails the test. A case expecting nothing at all is one
//! Atlas cannot do offline: passing it means NOT inventing an action.
//!
//! A phrase-table command is compared by its `Intent`. A helper is compared
//! by what that helper itself takes from the words (which window and which
//! screen, which day's weather), and a reminder or a text by its words with
//! the courtesies off -- so "remind me at 5, no wait, at 6" read as one
//! reminder still at 5 is a fail, not a pass. A fail that DID something (a
//! reading that is not the one wanted) is counted separately as a wrong
//! action: worse than "I didn't catch that".
//!
//! "Before" is the router as it was on origin/main c797d247: the whole
//! sentence read once (`reading_of(said)`), which is exactly what the turn did
//! without a model. "After" is `splitter::spoken_steps`, which the turn now
//! asks first (`turn_unwatched`), falling back to the same whole reading.
//!
//! Measured 12 Oct 2026, no model:
//!
//! ```text
//!                      before                 after
//! CORPUS   (133)       33 (25%), 20 wrong     130 (98%), 1 wrong
//! HELD_OUT  (32)        6 (19%),  6 wrong      22 (69%), 1 wrong
//! ```
//!
//! (13 Oct 2026, review fixes: six negated sentences added -- "don't close
//! chrome", "do not delete the report and open notepad", ... -- which both
//! routers leave undone, hence +6 on both sides; and the negation rule made
//! "if it's not too much trouble open discord" a fail that was a pass, so
//! after went 125/127 -> 130/133, not 131.)
//!
//! Read the two rows differently. CORPUS was written first and the rules were
//! tuned against it, so 98% is a ceiling, not a forecast. HELD_OUT was written
//! after the rules were settled and measured once; 69% is the honest estimate
//! for sentences the rules were not fitted to. (Two of its first-run fails
//! were wrong actions -- an app called "oh", an app called "notepad like" --
//! and were made safe, not passing, by refusing names Atlas doesn't know.)
//!
//! The tests below pin all four after-figures so they cannot silently
//! regress. What still fails is printed by them and listed in the CONV1
//! phase 1 report: it is phase 2's input (model-assisted routing). None of
//! this closes CONV1, which closes only on Eric's live offline acceptance run.

use atlas::config::Config;
use atlas::intent::{KnownNames, Parser};
use atlas::splitter::{plain_request, reading_of, spoken_steps, Reading};

/// Passes at or above this many, out of the whole corpus. Raised when the
/// router gets better; never lowered to make a change pass.
const AFTER_AT_LEAST: usize = 130;
/// What the router read before the splitter, for the record.
const BEFORE_WAS: usize = 33;
/// The held-out set (`HELD_OUT`), measured once after the rules were fixed.
const HELD_OUT_AFTER_AT_LEAST: usize = 22;
/// Sentences read as the WRONG action (not "didn't catch that", but doing
/// something not asked). Never raised.
const WRONG_AT_MOST: usize = 1;
const HELD_OUT_WRONG_AT_MOST: usize = 1;

fn parser() -> Parser {
    let cfg = Config::load(std::path::Path::new("config")).unwrap();
    let mut p = Parser::new(&cfg.commands);
    // As the turn does (`names_for_the_parser`): "close" and "switch to"
    // take an app Atlas knows.
    p.know_names(KnownNames {
        apps: cfg.apps.apps.keys().cloned().collect(),
        modes: Vec::new(),
        peers: Vec::new(),
    });
    p
}

/// The corpus: what was said, and the plain commands it asks for, in order.
/// Grouped by the kind of free-form speech it tests.
const CORPUS: &[(&str, &[&str])] = &[
    // ---- single requests with filler, hedging and variants ----
    (
        "hey could you maybe open up chrome for me",
        &["open chrome"],
    ),
    ("could you open notepad please", &["open notepad"]),
    ("please open discord", &["open discord"]),
    ("um, open chrome", &["open chrome"]),
    ("would you mind opening notepad", &["open notepad"]),
    ("can you just open chrome real quick", &["open chrome"]),
    ("i want you to open notepad", &["open notepad"]),
    ("go ahead and open chrome", &["open chrome"]),
    ("hey atlas, what time is it", &["what time is it"]),
    ("so, what time is it", &["what time is it"]),
    ("could you please check my email", &["check my email"]),
    (
        "hey, can you look at my screen for me",
        &["look at my screen"],
    ),
    ("i'd like you to make a backup", &["make a backup"]),
    (
        "um so could you tidy up my desktop",
        &["tidy up my desktop"],
    ),
    (
        "please organise my desktop for me",
        &["organise my desktop"],
    ),
    (
        "would you be able to research electric cars",
        &["research electric cars"],
    ),
    (
        "could you look into flights to denver for me",
        &["look into flights to denver"],
    ),
    (
        "hey atlas, remind me in 20 minutes to stretch",
        &["remind me in 20 minutes to stretch"],
    ),
    (
        "can you remind me at 5 to call mom please",
        &["remind me at 5 to call mom"],
    ),
    (
        "please set a reminder for 6 to take the bins out",
        &["set a reminder for 6 to take the bins out"],
    ),
    (
        "could you put chrome on the left monitor",
        &["put chrome on the left monitor"],
    ),
    (
        "hey can you move notepad to the right screen please",
        &["move notepad to the right screen"],
    ),
    (
        "what's the weather like tomorrow",
        &["what's the weather like tomorrow"],
    ),
    ("um, how's the machine", &["how's the machine"]),
    ("can you check yourself", &["check yourself"]),
    ("okay so what can you do", &["what can you do"]),
    ("hey atlas please launch discord", &["launch discord"]),
    ("would you mind closing chrome", &["close chrome"]),
    ("i need you to switch to notepad", &["switch to notepad"]),
    ("do you think you could open chrome", &["open chrome"]),
    (
        "if you could open notepad that would be great",
        &["open notepad"],
    ),
    ("atlas, could you maybe just undo that", &["undo that"]),
    (
        "uh, what's on my calendar today",
        &["what's on my calendar today"],
    ),
    (
        "could you make a note that the boiler is noisy",
        &["make a note that the boiler is noisy"],
    ),
    ("so um what's outstanding", &["what's outstanding"]),
    // ---- reordered phrasing ----
    ("chrome, open it", &["open chrome"]),
    (
        "on the left monitor, put chrome",
        &["put chrome on the left monitor"],
    ),
    (
        "at 5 remind me to call mom",
        &["remind me at 5 to call mom"],
    ),
    (
        "in 20 minutes remind me to stretch",
        &["remind me in 20 minutes to stretch"],
    ),
    ("notepad, can you open that", &["open notepad"]),
    ("my email, check it", &["check my email"]),
    (
        "to the right screen, move notepad",
        &["move notepad to the right screen"],
    ),
    ("chrome please, open it up", &["open chrome"]),
    ("my desktop is a mess, tidy it", &["tidy my desktop"]),
    ("the time, what is it", &["what time is it"]),
    (
        "tomorrow's weather please",
        &["what's the weather tomorrow"],
    ),
    ("a backup, make one", &["make a backup"]),
    ("electric cars, research them", &["research electric cars"]),
    ("for my screen, look at it", &["look at my screen"]),
    ("discord, close it", &["close discord"]),
    // ---- polite and hedged ----
    ("i was wondering if you could open chrome", &["open chrome"]),
    ("would you be so kind as to open notepad", &["open notepad"]),
    (
        "if it's not too much trouble open discord",
        &["open discord"],
    ),
    (
        "sorry to bother you but could you open chrome",
        &["open chrome"],
    ),
    (
        "when you get a chance could you check my email",
        &["check my email"],
    ),
    ("could you possibly open chrome", &["open chrome"]),
    ("please, please open chrome", &["open chrome"]),
    ("kindly open notepad", &["open notepad"]),
    ("can you open chrome thanks", &["open chrome"]),
    ("open chrome thank you very much", &["open chrome"]),
    ("maybe open notepad?", &["open notepad"]),
    ("perhaps check my email", &["check my email"]),
    ("i think you should open chrome", &["open chrome"]),
    ("could you do me a favour and open chrome", &["open chrome"]),
    ("it'd be great if you could open notepad", &["open notepad"]),
    // ---- self-corrections: the last instruction wins ----
    ("open notepad no wait open chrome", &["open chrome"]),
    ("open notepad, no wait, chrome", &["open chrome"]),
    ("open notepad, actually open chrome", &["open chrome"]),
    ("open notepad I mean chrome", &["open chrome"]),
    ("open discord, sorry, I meant notepad", &["open notepad"]),
    ("close chrome, no, close notepad", &["close notepad"]),
    ("open chrome, scratch that, open notepad", &["open notepad"]),
    (
        "switch to chrome, or rather notepad",
        &["switch to notepad"],
    ),
    ("open notepad. actually, open chrome", &["open chrome"]),
    ("research cats actually research dogs", &["research dogs"]),
    ("open notepad no open chrome", &["open chrome"]),
    ("open chrome wait no open notepad", &["open notepad"]),
    (
        "remind me at 5, no wait, at 6 to call mom",
        &["remind me at 6 to call mom"],
    ),
    (
        "put chrome on the left monitor, no, the right monitor",
        &["put chrome on the right monitor"],
    ),
    (
        "look up flights to rome, no wait, to paris",
        &["look up flights to paris"],
    ),
    // ---- compound requests ----
    (
        "open chrome and then put it on the left monitor and remind me at 5",
        &[
            "open chrome",
            "put it on the left monitor",
            "remind me at 5",
        ],
    ),
    (
        "open chrome and open notepad",
        &["open chrome", "open notepad"],
    ),
    (
        "open chrome then open notepad",
        &["open chrome", "open notepad"],
    ),
    (
        "open chrome, open notepad, and open discord",
        &["open chrome", "open notepad", "open discord"],
    ),
    (
        "check my email and then what time is it",
        &["check my email", "what time is it"],
    ),
    (
        "look at my screen and also check my email",
        &["look at my screen", "check my email"],
    ),
    (
        "open notepad, after that open chrome",
        &["open notepad", "open chrome"],
    ),
    (
        "what time is it and what's the weather",
        &["what time is it", "what's the weather"],
    ),
    (
        "make a backup and then check yourself",
        &["make a backup", "check yourself"],
    ),
    (
        "open chrome and put it on the right screen",
        &["open chrome", "put it on the right screen"],
    ),
    (
        "tidy my desktop and then remind me in an hour to review it",
        &["tidy my desktop", "remind me in an hour to review it"],
    ),
    (
        "research electric cars and then remind me tomorrow at 9 to read it",
        &[
            "research electric cars",
            "remind me tomorrow at 9 to read it",
        ],
    ),
    (
        "open discord, then switch to chrome",
        &["open discord", "switch to chrome"],
    ),
    (
        "close notepad and open chrome",
        &["close notepad", "open chrome"],
    ),
    (
        "hey could you open chrome and then maybe check my email for me",
        &["open chrome", "check my email"],
    ),
    ("open chrome and notepad", &["open chrome", "open notepad"]),
    (
        "open chrome, notepad and discord",
        &["open chrome", "open notepad", "open discord"],
    ),
    (
        "what time is it, also what's on my calendar",
        &["what time is it", "what's on my calendar"],
    ),
    (
        "make a backup and then look at my screen",
        &["make a backup", "look at my screen"],
    ),
    (
        "open notepad and then close it",
        &["open notepad", "close it"],
    ),
    (
        "set a reminder for 6 to eat and then open notepad",
        &["set a reminder for 6 to eat", "open notepad"],
    ),
    (
        "text sam saying I'm running late and then open chrome",
        &["text sam saying I'm running late", "open chrome"],
    ),
    (
        "open chrome. check my email.",
        &["open chrome", "check my email"],
    ),
    (
        "look at my screen; what time is it",
        &["look at my screen", "what time is it"],
    ),
    (
        "open notepad and after that remind me in 10 minutes to save",
        &["open notepad", "remind me in 10 minutes to save"],
    ),
    (
        "check my email then look at my screen then open chrome",
        &["check my email", "look at my screen", "open chrome"],
    ),
    (
        "what's the weather tomorrow and remind me at 8 to take an umbrella",
        &[
            "what's the weather tomorrow",
            "remind me at 8 to take an umbrella",
        ],
    ),
    (
        "could you open chrome and also notepad",
        &["open chrome", "open notepad"],
    ),
    (
        "check yourself and then tell me what's new",
        &["check yourself", "what's new"],
    ),
    (
        "open chrome then put it on the left monitor",
        &["open chrome", "put it on the left monitor"],
    ),
    (
        "open notepad and move it to the right screen and then open chrome",
        &["open notepad", "move it to the right screen", "open chrome"],
    ),
    (
        "how's the machine and what's outstanding",
        &["how's the machine", "what's outstanding"],
    ),
    // ---- must NOT be split: one instruction with an "and" of its own ----
    (
        "remind me to call mom and dad at 5",
        &["remind me to call mom and dad at 5"],
    ),
    (
        "remind me to buy milk, eggs and bread tomorrow at 9",
        &["remind me to buy milk, eggs and bread tomorrow at 9"],
    ),
    ("research cats and dogs", &["research cats and dogs"]),
    (
        "look up salt and pepper shakers",
        &["look up salt and pepper shakers"],
    ),
    (
        "make a note that the fridge and the freezer are broken",
        &["make a note that the fridge and the freezer are broken"],
    ),
    (
        "text sam saying I'll grab milk and bread",
        &["text sam saying I'll grab milk and bread"],
    ),
    (
        "note that I should open chrome and notepad tomorrow",
        &["note that I should open chrome and notepad tomorrow"],
    ),
    (
        "remind me to open chrome and check my email at 5",
        &["remind me to open chrome and check my email at 5"],
    ),
    (
        "look up rock and roll history",
        &["look up rock and roll history"],
    ),
    ("hang on, go back", &["go back"]),
    ("wait, open chrome", &["open chrome"]),
    // ---- things Atlas cannot do offline: nothing invented ----
    ("play some jazz", &[]),
    ("order me a pizza", &[]),
    ("open chrome and play some music", &[]),
    ("what's the meaning of life", &[]),
    // ---- negation: never the thing you said not to do ----
    ("don't close chrome", &[]),
    ("don't close chrome and open notepad", &[]),
    ("do not delete the report and open notepad", &[]),
    ("never send that", &[]),
    ("please don't open discord", &[]),
    ("do not close workspace", &[]),
];

/// Written AFTER the rules above were tuned on `CORPUS`, and measured once,
/// without changing the splitter for them: how well the rules generalise to
/// sentences they were not fitted to. Kept separate so the corpus figure
/// cannot hide overfitting.
const HELD_OUT: &[(&str, &[&str])] = &[
    ("yo atlas fire up discord", &["fire up discord"]),
    ("could ya open chrome", &["open chrome"]),
    ("atlas I need notepad open", &["open notepad"]),
    (
        "open chrome and then open discord and then open notepad",
        &["open chrome", "open discord", "open notepad"],
    ),
    ("pop open notepad for me would you", &["open notepad"]),
    ("what's the time please", &["what's the time"]),
    (
        "remind me in an hour to check the oven, and open notepad",
        &["remind me in an hour to check the oven", "open notepad"],
    ),
    (
        "close notepad then close chrome",
        &["close notepad", "close chrome"],
    ),
    (
        "could you check my email and also look at my screen please",
        &["check my email", "look at my screen"],
    ),
    (
        "set a reminder for 7 to call dad then open chrome",
        &["set a reminder for 7 to call dad", "open chrome"],
    ),
    (
        "open discord. then remind me at 3 to join the call",
        &["open discord", "remind me at 3 to join the call"],
    ),
    ("um, uh, what time is it", &["what time is it"]),
    (
        "open notepad, oh and also open chrome",
        &["open notepad", "open chrome"],
    ),
    (
        "put notepad on my other screen",
        &["put notepad on my other screen"],
    ),
    (
        "move chrome over to the left monitor please",
        &["move chrome over to the left monitor"],
    ),
    ("open chrome no sorry discord", &["open discord"]),
    ("launch chrome, actually no, notepad", &["launch notepad"]),
    ("check yourself please and let me know", &["check yourself"]),
    (
        "remind me tomorrow at 9 to send the report and to call jen",
        &["remind me tomorrow at 9 to send the report and to call jen"],
    ),
    (
        "research the best laptops under 1000 dollars and then remind me tonight to decide",
        &[
            "research the best laptops under 1000 dollars",
            "remind me tonight to decide",
        ],
    ),
    (
        "text mom saying I'm on my way",
        &["text mom saying I'm on my way"],
    ),
    (
        "hey so could you please, like, open notepad",
        &["open notepad"],
    ),
    (
        "would you open chrome and put it on the right monitor for me",
        &["open chrome", "put it on the right monitor"],
    ),
    (
        "after you open chrome, open notepad",
        &["open chrome", "open notepad"],
    ),
    (
        "first open chrome then open notepad",
        &["open chrome", "open notepad"],
    ),
    (
        "open notepad and chrome and discord",
        &["open notepad", "open chrome", "open discord"],
    ),
    ("can you tell me what time it is", &["what time is it"]),
    ("I'd like to open chrome", &["open chrome"]),
    (
        "close chrome please and then open notepad",
        &["close chrome", "open notepad"],
    ),
    (
        "what's on my calendar tomorrow and remind me at 8 to prep",
        &["what's on my calendar tomorrow", "remind me at 8 to prep"],
    ),
    ("wait no, open chrome", &["open chrome"]),
    (
        "turn on workspace then put chrome on the right screen",
        &["turn on workspace", "put chrome on the right screen"],
    ),
];

/// One reading as the corpus compares it: a command by its intent; a helper
/// by what that helper itself takes from the words (which screen and which
/// window, which day's weather), or for a reminder or a text by the words
/// with the courtesies off. So "remind me at 5, no wait, at 6" read as one
/// reminder still at 5 is a fail.
#[derive(Debug, Clone, PartialEq)]
enum Seen {
    Command(atlas::intent::Intent),
    Helper(&'static str, String),
}

fn seen(words: &str, r: Reading) -> Seen {
    let plain = |w: &str| {
        plain_request(w)
            .to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric() || c.is_whitespace())
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };
    match r {
        Reading::Command(i) => Seen::Command(i),
        Reading::Reminder => Seen::Helper("reminder", plain(words)),
        Reading::MoveWindow => Seen::Helper(
            "move",
            format!("{:?}", atlas::workspace::move_to_screen_asked(words)),
        ),
        Reading::Text => Seen::Helper("text", plain(words)),
        Reading::Weather => Seen::Helper(
            "weather",
            format!("{:?}", atlas::weather::about_the_weather(words)),
        ),
    }
}

fn expected(p: &Parser, plain: &[&str]) -> Vec<Seen> {
    plain
        .iter()
        .map(|w| {
            let r = reading_of(w, p).unwrap_or_else(|| {
                panic!("broken case: the expected step \"{w}\" itself reads as nothing")
            });
            seen(w, r)
        })
        .collect()
}

/// The router before the splitter: the whole sentence, read once.
fn before(p: &Parser, said: &str) -> Vec<Seen> {
    reading_of(said, p)
        .map(|r| seen(said, r))
        .into_iter()
        .collect()
}

/// The router now: the splitter first, else the whole sentence as before.
fn after(p: &Parser, said: &str) -> Vec<Seen> {
    // As the turn does: a negated sentence the table reads as a destructive
    // command is refused outright (`turn_unwatched`).
    if atlas::splitter::negated_destructive(said, p).is_some() {
        return Vec::new();
    }
    match spoken_steps(said, p) {
        Some(steps) => steps
            .into_iter()
            .map(|s| seen(&s.words, s.reading))
            .collect(),
        None => before(p, said),
    }
}

fn score(
    p: &Parser,
    set: &[(&str, &[&str])],
    route: fn(&Parser, &str) -> Vec<Seen>,
) -> (usize, Vec<String>) {
    let mut pass = 0;
    let mut fails = Vec::new();
    for (said, want) in set {
        let want = expected(p, want);
        let got = route(p, said);
        if got == want {
            pass += 1;
        } else {
            let wrong = if got.is_empty() { "" } else { " WRONG ACTION" };
            fails.push(format!(
                "  \"{said}\"{wrong}\n      want {want:?}\n      got  {got:?}"
            ));
        }
    }
    (pass, fails)
}

/// Fails that did something: a reading that is not the one wanted. Worse
/// than "I didn't catch that", so counted on their own.
fn wrong_actions(fails: &[String]) -> usize {
    fails.iter().filter(|f| f.contains("WRONG ACTION")).count()
}

#[test]
fn free_form_the_corpus_is_big_enough_to_mean_something() {
    assert!(CORPUS.len() >= 120, "only {} sentences", CORPUS.len());
    let compounds = CORPUS.iter().filter(|(_, w)| w.len() >= 2).count();
    assert!(compounds >= 25, "only {compounds} compound requests");
}

#[test]
fn free_form_every_expected_step_reads() {
    let p = parser();
    for (_, want) in CORPUS.iter().chain(HELD_OUT) {
        assert_eq!(
            expected(&p, want).len(),
            want.len(),
            "every expected step must read as something"
        );
    }
}

/// Both routes over a set, printed; returns (before, after, wrong actions
/// after, the fails after).
fn measure(p: &Parser, name: &str, set: &[(&str, &[&str])]) -> (usize, usize, usize, String) {
    let (was, was_fails) = score(p, set, before);
    let (now, fails) = score(p, set, after);
    let wrong = wrong_actions(&fails);
    println!(
        "{name}: before {was}/{n} ({} wrong actions), after {now}/{n} ({wrong} wrong actions)\nstill failing ({}):\n{}",
        wrong_actions(&was_fails),
        fails.len(),
        fails.join("\n"),
        n = set.len()
    );
    (was, now, wrong, fails.join("\n"))
}

#[test]
fn free_form_the_corpus_pass_rate_does_not_regress() {
    let p = parser();
    let (was, now, wrong, fails) = measure(&p, "free-form corpus", CORPUS);
    assert_eq!(
        was, BEFORE_WAS,
        "the before figure moved: the phrase table itself changed, so re-measure and update the header"
    );
    assert!(
        now >= AFTER_AT_LEAST,
        "the free-form corpus regressed: {now}/{} (was at least {AFTER_AT_LEAST})\n{fails}",
        CORPUS.len()
    );
    assert!(
        wrong <= WRONG_AT_MOST,
        "{wrong} wrong actions (at most {WRONG_AT_MOST})\n{fails}"
    );
}

#[test]
fn free_form_the_held_out_sentences_do_not_regress() {
    let p = parser();
    let (_, now, wrong, fails) = measure(&p, "held-out", HELD_OUT);
    assert!(
        now >= HELD_OUT_AFTER_AT_LEAST,
        "the held-out sentences regressed: {now}/{} (was at least {HELD_OUT_AFTER_AT_LEAST})\n{fails}",
        HELD_OUT.len()
    );
    assert!(
        wrong <= HELD_OUT_WRONG_AT_MOST,
        "{wrong} wrong actions on the held-out set (at most {HELD_OUT_WRONG_AT_MOST})\n{fails}"
    );
}

#[test]
fn free_form_the_jarvis_sentence_is_three_steps_in_order() {
    let p = parser();
    let got = after(
        &p,
        "open chrome and then put it on the left monitor and remind me at 5",
    );
    assert_eq!(
        got,
        expected(
            &p,
            &[
                "open chrome",
                "put it on the left monitor",
                "remind me at 5"
            ]
        )
    );
}

#[test]
fn free_form_one_reminder_with_an_and_in_it_stays_one() {
    let p = parser();
    assert_eq!(
        after(&p, "remind me to call mom and dad at 5"),
        expected(&p, &["remind me to call mom and dad at 5"])
    );
}

// ================= in the daemon, with every gate in place =================

use atlas::daemon::Daemon;
use atlas::platform::mock::{Action, MockPlatform};
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::session::Pending;
use atlas::store::Store;

const NOW: u64 = 1_790_776_800;

fn two_screens() -> MockPlatform {
    MockPlatform::new(vec![
        Monitor {
            id: 1,
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
            primary: true,
        },
        Monitor {
            id: 2,
            x: 1920,
            y: 0,
            width: 1920,
            height: 1040,
            primary: false,
        },
    ])
}

fn store(tag: &str) -> Store {
    let p = std::env::temp_dir().join(format!("atlas-free-form-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    Store::new(p)
}

fn launched(p: &MockPlatform, app: &str) -> bool {
    p.actions()
        .iter()
        .any(|a| matches!(a, Action::Launch(s) if s.to_lowercase().contains(app)))
}

#[test]
fn free_form_three_steps_run_in_order_with_no_model() {
    let c = Config::load(std::path::Path::new("config")).unwrap();
    let p = two_screens();
    let mut d = Daemon::new(
        &c,
        &p,
        None,
        store("three"),
        Proactive::new(ProactiveConfig::default()),
    );
    let reply = d.turn(
        "hey could you open chrome and then put it on the left monitor and remind me in 20 minutes to stretch",
        NOW,
    );
    assert!(launched(&p, "chrome"), "chrome was not opened: {reply}");
    assert!(
        p.actions().iter().any(|a| matches!(a, Action::Place(s, r) if s.to_lowercase().contains("chrome") && r.x < 1920)),
        "chrome was not put on the left screen: {:?}\n{reply}",
        p.actions()
    );
    // And the reminder is really booked, at the turn's own time.
    assert!(d.scheduler.due(NOW + 19 * 60).is_empty(), "{reply}");
    assert_eq!(d.scheduler.due(NOW + 21 * 60).len(), 1, "{reply}");
    // One reply, naming each step.
    let l = reply.to_lowercase();
    assert!(l.contains("chrome"), "{reply}");
    assert!(l.contains("remind you to stretch"), "{reply}");
    assert!(matches!(d.session.pending, Pending::Nothing), "{reply}");
}

#[test]
fn free_form_a_compound_stops_at_the_step_that_needs_approval() {
    let c = Config::load(std::path::Path::new("config")).unwrap();
    let p = two_screens();
    let mut d = Daemon::new(
        &c,
        &p,
        None,
        store("approval"),
        Proactive::new(ProactiveConfig::default()),
    );
    // Closing an app is always asked about (`policy.yaml` always_ask).
    let reply = d.turn(
        "open notepad, then close chrome, and after that open discord",
        NOW,
    );
    assert!(
        launched(&p, "notepad"),
        "the first step did not run: {reply}"
    );
    assert!(
        matches!(&d.session.pending, Pending::Approval(atlas::intent::Intent::CloseApp(a), _) if a == "chrome"),
        "the second step was not held for approval: {:?}\n{reply}",
        d.session.pending
    );
    assert!(
        !p.actions().iter().any(|a| matches!(a, Action::Close(_))),
        "chrome was closed without a yes: {reply}"
    );
    assert!(
        !launched(&p, "discord"),
        "a step after the approval ran anyway: {reply}"
    );
    // The reply says what was not done, and ends with the question.
    assert!(reply.contains("open discord"), "{reply}");
    assert!(reply.trim_end().ends_with('?'), "{reply}");
    // A no leaves chrome alone and does not run the rest either.
    let no = d.turn("no", NOW + 5);
    assert!(
        !p.actions().iter().any(|a| matches!(a, Action::Close(_))),
        "{no}"
    );
    assert!(!launched(&p, "discord"), "{no}");
}

#[test]
fn free_form_a_step_after_outside_text_is_asked_about() {
    let c = Config::load(std::path::Path::new("config")).unwrap();
    let p = two_screens();
    let mut d = Daemon::new(
        &c,
        &p,
        None,
        store("outside"),
        Proactive::new(ProactiveConfig::default()),
    );
    // Reading your messages is reading what someone else wrote; a message
    // sent as you straight after it is asked about, not run.
    let reply = d.turn(
        "read my messages and then message sam saying I'm on my way",
        NOW,
    );
    assert!(
        matches!(
            &d.session.pending,
            Pending::Approval(atlas::intent::Intent::Message(_), _)
        ),
        "the message was not held: {:?}\n{reply}",
        d.session.pending
    );
    assert!(reply.contains("You asked for this right after I read") && !reply.contains("not from you"), "{reply}");
}

#[test]
fn free_form_a_correction_runs_only_the_last_instruction() {
    let c = Config::load(std::path::Path::new("config")).unwrap();
    let p = two_screens();
    let mut d = Daemon::new(
        &c,
        &p,
        None,
        store("correction"),
        Proactive::new(ProactiveConfig::default()),
    );
    let reply = d.turn("open notepad, no wait, chrome", NOW);
    assert!(launched(&p, "chrome"), "{reply}");
    assert!(!launched(&p, "notepad"), "the corrected step ran: {reply}");
}

fn closed(p: &MockPlatform) -> bool {
    p.actions().iter().any(|a| matches!(a, Action::Close(_)))
}

#[test]
fn free_form_a_negation_never_does_what_it_forbids() {
    let c = Config::load(std::path::Path::new("config")).unwrap();
    for (n, said) in [
        "don't close chrome",
        "don't close chrome and open notepad",
        "do not delete the report and open notepad",
        "never send that",
        "please don't open discord",
        "do not close workspace",
    ]
    .iter()
    .enumerate()
    {
        let p = two_screens();
        let mut d = Daemon::new(
            &c,
            &p,
            None,
            store(&format!("negation-{n}")),
            Proactive::new(ProactiveConfig::default()),
        );
        let _ = d.turn("open chrome", NOW);
        let reply = d.turn(said, NOW + 10);
        assert!(!closed(&p), "{said:?} closed something: {reply}");
        assert!(!launched(&p, "discord"), "{said:?} opened discord: {reply}");
        assert!(
            !launched(&p, "notepad"),
            "{said:?} split around the negation: {reply}"
        );
        assert!(
            !matches!(d.session.pending, Pending::Approval(..)),
            "{said:?} offered to do it: {:?}\n{reply}",
            d.session.pending
        );
    }
    // The splitter itself leaves every one of them whole.
    let parser = parser();
    for said in [
        "don't close chrome and open notepad",
        "open notepad and don't close chrome",
        "do not delete the report and open notepad",
        "never send that and then open chrome",
        "please don't open discord",
        "open chrome without closing notepad",
    ] {
        assert!(spoken_steps(said, &parser).is_none(), "{said}");
    }
    // A negation that IS the command still works.
    assert!(atlas::splitter::negated_destructive("stop taking notes", &parser).is_none());
    assert!(atlas::splitter::negated_destructive("close what I don't need", &parser).is_none());
}

#[test]
fn free_form_it_is_resolved_before_anything_is_held_or_asked() {
    let c = Config::load(std::path::Path::new("config")).unwrap();
    let p = two_screens();
    let mut d = Daemon::new(
        &c,
        &p,
        None,
        store("it-held"),
        Proactive::new(ProactiveConfig::default()),
    );
    let reply = d.turn("open notepad then close it", NOW);
    assert!(launched(&p, "notepad"), "{reply}");
    assert!(
        matches!(&d.session.pending, Pending::Approval(atlas::intent::Intent::CloseApp(a), _) if a == "notepad"),
        "held as {:?}, not as notepad\n{reply}",
        d.session.pending
    );
    let yes = d.turn("yes", NOW + 5);
    assert!(
        p.actions()
            .iter()
            .any(|a| matches!(a, Action::Close(s) if s.to_lowercase().contains("notepad"))),
        "a yes did not close notepad: {yes}\n{:?}",
        p.actions()
    );
}

#[test]
fn free_form_after_outside_text_a_close_is_held_with_its_real_target() {
    let c = Config::load(std::path::Path::new("config")).unwrap();
    let p = two_screens();
    let mut d = Daemon::new(
        &c,
        &p,
        None,
        store("outside-close"),
        Proactive::new(ProactiveConfig::default()),
    );
    let _ = d.turn("open notepad", NOW);
    let reply = d.turn("read my messages and then close notepad", NOW + 5);
    assert!(
        reply.contains("You asked for this right after I read") && !reply.contains("not from you"),
        "not held by the outside-text gate: {reply}"
    );
    assert!(
        matches!(&d.session.pending, Pending::Approval(atlas::intent::Intent::CloseApp(a), _) if a == "notepad"),
        "{:?}\n{reply}",
        d.session.pending
    );
    assert!(!closed(&p), "closed before the yes: {reply}");
    let yes = d.turn("yes", NOW + 10);
    assert!(
        p.actions()
            .iter()
            .any(|a| matches!(a, Action::Close(s) if s.to_lowercase().contains("notepad"))),
        "{yes}"
    );
}

#[test]
fn free_form_after_outside_text_a_delete_is_held() {
    let c = Config::load(std::path::Path::new("config")).unwrap();
    let p = two_screens();
    let mut d = Daemon::new(
        &c,
        &p,
        None,
        store("outside-delete"),
        Proactive::new(ProactiveConfig::default()),
    );
    let reply = d.turn("read my messages and then delete the noise", NOW);
    assert!(
        matches!(
            &d.session.pending,
            Pending::Approval(atlas::intent::Intent::SortMail(_), _)
        ),
        "the delete was not held: {:?}\n{reply}",
        d.session.pending
    );
    assert!(reply.contains("You asked for this right after I read") && !reply.contains("not from you"), "{reply}");
    // With mail as the outside text: whatever happens, nothing is deleted
    // without a yes -- held, or stopped because the mail step didn't happen.
    let p2 = two_screens();
    let mut d2 = Daemon::new(
        &c,
        &p2,
        None,
        store("outside-delete-2"),
        Proactive::new(ProactiveConfig::default()),
    );
    let reply = d2.turn("check my email and then delete the noise", NOW);
    assert!(
        matches!(
            &d2.session.pending,
            Pending::Nothing | Pending::Approval(atlas::intent::Intent::SortMail(_), _)
        ),
        "{:?}\n{reply}",
        d2.session.pending
    );
    assert!(
        reply.contains("delete the noise") || reply.contains("You asked for this right after I read"),
        "the delete is not named as not done: {reply}"
    );
}

#[test]
fn free_form_a_step_that_did_not_happen_stops_the_rest() {
    let c = Config::load(std::path::Path::new("config")).unwrap();
    let p = two_screens();
    let mut d = Daemon::new(
        &c,
        &p,
        None,
        store("stops"),
        Proactive::new(ProactiveConfig::default()),
    );
    let _ = d.turn("open chrome", NOW);
    // No mailbox is set up here, so the first part cannot happen; the close
    // after it is neither run nor even asked about.
    let reply = d.turn("check my email and then close chrome", NOW + 5);
    assert!(!closed(&p), "{reply}");
    assert!(
        matches!(d.session.pending, Pending::Nothing),
        "went on to the close after a step that didn't happen: {:?}\n{reply}",
        d.session.pending
    );
    assert!(reply.contains("close chrome"), "{reply}");
}

#[test]
fn free_form_a_compound_is_kept_once_as_said() {
    let c = Config::load(std::path::Path::new("config")).unwrap();
    let p = two_screens();
    let mut d = Daemon::new(
        &c,
        &p,
        None,
        store("record"),
        Proactive::new(ProactiveConfig::default()),
    );
    let thread_before = d.thread.recent.len();
    let turns_before = d.session.turns.len();
    let said = "hey could you open chrome and then open notepad for me";
    let reply = d.turn(said, NOW);
    assert!(launched(&p, "chrome") && launched(&p, "notepad"), "{reply}");
    assert_eq!(
        d.thread.recent.len(),
        thread_before + 1,
        "{:?}",
        d.thread.recent
    );
    assert_eq!(d.thread.recent.last().map(|e| e.said.as_str()), Some(said));
    assert_eq!(d.session.turns.len(), turns_before + 1);
    assert_eq!(d.session.turns.last().map(|x| x.said.as_str()), Some(said));
    assert!(
        !d.thread
            .recent
            .iter()
            .any(|e| e.said == "open notepad" || e.said == "open chrome"),
        "a rewritten part was kept as if said"
    );
}

#[test]
fn free_form_a_harmless_part_in_between_does_not_wash_the_taint() {
    let c = Config::load(std::path::Path::new("config")).unwrap();
    let p = two_screens();
    let mut d = Daemon::new(
        &c,
        &p,
        None,
        store("taint-between"),
        Proactive::new(ProactiveConfig::default()),
    );
    let _ = d.turn("open notepad", NOW);
    // Messages are someone else's words; opening chrome is harmless and is
    // your own, and its `run_command` clears the session flag -- the close
    // after it must still be held.
    let reply = d.turn(
        "read my messages, then open chrome, then close notepad",
        NOW + 5,
    );
    assert!(
        launched(&p, "chrome"),
        "the harmless part did not run: {reply}"
    );
    assert!(
        matches!(&d.session.pending, Pending::Approval(atlas::intent::Intent::CloseApp(a), _) if a == "notepad"),
        "{:?}\n{reply}",
        d.session.pending
    );
    assert!(
        reply.contains("You asked for this right after I read") && !reply.contains("not from you"),
        "held by the policy, not the outside-text gate: {reply}"
    );
    assert!(!closed(&p), "{reply}");
    assert!(d.session.tainted_by_outside, "the run lost the taint");
}

/// Picks "open notepad" for "deal with notepad", as a model would.
struct PicksNotepad;
impl atlas::brain::Llm for PicksNotepad {
    fn complete(&self, _: &str, user: &str) -> atlas::error::Result<String> {
        let said = user.rsplit("User said:").next().unwrap_or(user);
        Ok(if said.contains("deal with notepad") {
            r#"{"action":"open_app","arg":"notepad","say":"Opening notepad."}"#
        } else {
            r#"{"action":"say","arg":null,"say":"Hello to you."}"#
        }
        .into())
    }
}

#[test]
fn free_form_outside_text_read_inside_a_compound_still_counts_after_it() {
    let c = Config::load(std::path::Path::new("config")).unwrap();
    let p = two_screens();
    let mut d = Daemon::new(
        &c,
        &p,
        Some(std::sync::Arc::new(PicksNotepad) as std::sync::Arc<dyn atlas::brain::Llm>),
        store("taint-after"),
        Proactive::new(ProactiveConfig::default()),
    );
    let reply = d.turn("read my messages and then open chrome", NOW);
    assert!(launched(&p, "chrome"), "{reply}");
    assert!(
        d.session.tainted_by_outside,
        "the compound's last part washed the taint: {reply}"
    );
    // The next turn is the model's pick, and is asked about.
    let reply = d.turn("deal with notepad", NOW + 10);
    assert!(reply.ends_with("Do you want me to do it?"), "{reply}");
    assert!(
        matches!(
            &d.session.pending,
            Pending::Approval(atlas::intent::Intent::OpenApp(_), _)
        ),
        "{:?}",
        d.session.pending
    );
    assert!(!launched(&p, "notepad"), "{reply}");
}

#[test]
fn free_form_stop_commands_still_work() {
    let parser = parser();
    for said in ["stop the music", "stop listening", "stop taking notes"] {
        assert!(
            atlas::splitter::negated_destructive(said, &parser).is_none(),
            "{said} is refused as a negation"
        );
        // Left exactly as the phrase table had it.
        assert!(spoken_steps(said, &parser).is_none(), "{said}");
    }
}

/// One spoken sentence is one turn of yours for aging the taint, however many
/// parts it had; and the held part is said to be yours, not the message's.
#[test]
fn free_form_a_compound_ages_the_taint_as_one_turn() {
    let c = Config::load(std::path::Path::new("config")).unwrap();
    let p = two_screens();
    let mut d = Daemon::new(
        &c,
        &p,
        None,
        store("ages-once"),
        Proactive::new(ProactiveConfig::default()),
    );
    let _ = d.turn("open notepad", NOW);
    let before = d.session.owner_turns();
    let reply = d.turn(
        "read my messages, then open chrome, then close notepad",
        NOW + 5,
    );
    assert_eq!(d.session.owner_turns(), before + 1, "{reply}");
    assert!(d.session.tainted_by_outside);
    assert!(reply.contains("You asked for this right after I read"), "{reply}");
}
