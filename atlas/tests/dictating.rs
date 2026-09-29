//! Typing what you say, end to end.
//!
//! Dictation is the one capability in this tree that sends real keystrokes
//! into a window Atlas does not own, so the tests here assert the actual
//! characters that reached the platform, not that a function returned `Ok`.
//! `MockPlatform` records every `type_text` as an `Action::Type`, which is the
//! only honest place to check "did the right thing get typed".
//!
//! ## Why this file exists at all
//!
//! `dictate.rs` was complete and tested from the day it was written, and
//! `tests/wiring.rs` said of it: *"`Intent::Dictate` has no arm in
//! `daemon::execute`."* That was true when written and then stopped being
//! true in the worse direction — the variant was removed from `intent.rs`
//! entirely and the comment stayed, so the note described a missing arm for a
//! variant that no longer existed. Everything below is the wiring that
//! sentence was describing.

use atlas::config::Config;
use atlas::daemon::{Autonomy, Daemon};
use atlas::platform::mock::{Action, MockPlatform};
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-dict-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// The shipped config with dictation switched on.
///
/// Shipped-off is correct for a capability that types into your windows, so
/// the tests turn it on rather than the config being changed to suit them.
fn cfg() -> Config {
    let mut c = Config::load(Path::new("config")).unwrap();
    if let Some(t) = c.tools.as_mut() {
        t.dictate.enabled = true;
    }
    c
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor {
        id: 1,
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
        primary: true,
    }])
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    let mut d = Daemon::new(
        c,
        p,
        None,
        Store::new(tmp(tag)),
        Proactive::new(ProactiveConfig::default()),
    );
    d.autonomy = Autonomy::Unattended;
    d
}

/// Everything that actually reached the keyboard, in order.
fn typed(p: &MockPlatform) -> Vec<String> {
    p.actions()
        .into_iter()
        .filter_map(|a| match a {
            Action::Type(t) => Some(t),
            _ => None,
        })
        .collect()
}

// --- starting and stopping --------------------------------------------------

#[test]
fn starting_dictation_names_the_window_it_is_typing_into() {
    // The window is the fact you need to catch it being the wrong one, which
    // is why `policy::classify` grades this `ProceedAndReport` rather than
    // `AutoProceed`.
    let (c, p) = (cfg(), plat());
    p.focus_on("notepad.exe", "untitled - Notepad");
    let mut d = daemon(&c, &p, "start");

    let said = d.turn("start dictating", 1_000);
    assert!(said.to_lowercase().contains("notepad"), "got {said:?}");
    assert!(said.contains("stop dictating"), "it has to say how to get out: {said:?}");
}

#[test]
fn with_dictation_off_nothing_starts_and_it_says_which_setting() {
    let mut c = Config::load(Path::new("config")).unwrap();
    if let Some(t) = c.tools.as_mut() {
        t.dictate.enabled = false;
    }
    let p = plat();
    p.focus_on("notepad.exe", "untitled - Notepad");
    let mut d = daemon(&c, &p, "off");

    let said = d.turn("start dictating", 1_000);
    assert!(said.contains("dictate.enabled"), "name the setting: {said:?}");
    d.turn("hello there", 1_001);
    assert!(typed(&p).is_empty(), "nothing should have been typed");
}

#[test]
fn with_no_window_it_refuses_rather_than_typing_into_whatever_is_there() {
    let (c, p) = (cfg(), plat());
    // No `focus_on` at all: the platform cannot say what is in front.
    let mut d = daemon(&c, &p, "nowindow");

    let said = d.turn("start dictating", 1_000);
    assert!(said.contains("can't tell which window"), "got {said:?}");
    assert!(typed(&p).is_empty());
}

// --- the words themselves ---------------------------------------------------

#[test]
fn what_you_say_is_typed_and_not_answered() {
    // The point of the whole mode: while dictating, "open chrome" is three
    // words you wanted typed, not a command.
    let (c, p) = (cfg(), plat());
    p.focus_on("notepad.exe", "untitled - Notepad");
    let mut d = daemon(&c, &p, "words");

    d.turn("start dictating", 1_000);
    let reply = d.turn("open chrome", 1_001);

    assert_eq!(typed(&p), vec!["Open chrome".to_string()]);
    assert!(reply.is_empty(), "dictation answers with silence, not chatter: {reply:?}");
    assert!(
        !p.actions().iter().any(|a| matches!(a, Action::Launch(_))),
        "it opened a browser instead of typing the words"
    );
}

#[test]
fn spoken_punctuation_becomes_marks_and_the_sentence_is_capitalised() {
    let (c, p) = (cfg(), plat());
    p.focus_on("notepad.exe", "untitled - Notepad");
    let mut d = daemon(&c, &p, "punct");

    d.turn("start dictating", 1_000);
    d.turn("hello comma how are you question mark", 1_001);

    assert_eq!(typed(&p), vec!["Hello, how are you?".to_string()]);
}

#[test]
fn the_words_after_the_trigger_are_typed_without_a_second_breath() {
    // "type this: dear Sarah" should not need you to say the trigger and then
    // wait. The remainder is `raw_argument`, so its case survives.
    let (c, p) = (cfg(), plat());
    p.focus_on("notepad.exe", "untitled - Notepad");
    let mut d = daemon(&c, &p, "first");

    let said = d.turn("type this Dear Sarah", 1_000);
    assert!(said.to_lowercase().contains("notepad"));
    assert_eq!(typed(&p), vec!["Dear Sarah".to_string()]);
}

#[test]
fn stop_dictating_ends_it_and_the_next_thing_is_a_command_again() {
    let (c, p) = (cfg(), plat());
    p.focus_on("notepad.exe", "untitled - Notepad");
    let mut d = daemon(&c, &p, "stop");

    d.turn("start dictating", 1_000);
    d.turn("hello there", 1_001);
    let said = d.turn("stop dictating", 1_002);
    assert!(said.to_lowercase().contains("stop"), "got {said:?}");

    // Back to being a command: this would have been typed a moment ago.
    d.turn("what's outstanding", 1_003);
    assert_eq!(
        typed(&p),
        vec!["Hello there".to_string()],
        "something was typed after dictation ended"
    );
}

#[test]
fn scratch_that_takes_back_the_last_line_and_keeps_going() {
    let (c, p) = (cfg(), plat());
    p.focus_on("notepad.exe", "untitled - Notepad");
    let mut d = daemon(&c, &p, "scratch");

    d.turn("start dictating", 1_000);
    d.turn("the first sentence", 1_001);
    let said = d.turn("scratch that", 1_002);
    assert!(said.contains("Took back"), "got {said:?}");

    // Still dictating — this is the test that a take-back is not a stop.
    d.turn("the second sentence", 1_003);
    assert_eq!(
        typed(&p),
        vec!["The first sentence".to_string(), "The second sentence".to_string()]
    );
}

// --- the windows it will not type into --------------------------------------

#[test]
fn it_refuses_the_apps_dictation_names_and_says_why() {
    // `dictate.never_into` ships with discord, slack and teams — a misheard
    // sentence there is public, where in a document it is a typo.
    let (c, p) = (cfg(), plat());
    p.focus_on("slack.exe", "general - Slack");
    let mut d = daemon(&c, &p, "slack");

    let said = d.turn("start dictating", 1_000);
    assert!(said.contains("don't dictate into"), "got {said:?}");
    d.turn("something private", 1_001);
    assert!(typed(&p).is_empty());
}

#[test]
fn an_electron_app_is_caught_by_its_title_when_its_process_says_nothing() {
    // The reason both fields are checked. A chat app whose process is
    // `electron` would pass a process-only check and is exactly the window
    // this rule exists for.
    let (c, p) = (cfg(), plat());
    p.focus_on("electron", "general - Slack");
    let mut d = daemon(&c, &p, "electron");

    let said = d.turn("start dictating", 1_000);
    assert!(said.contains("don't dictate into"), "got {said:?}");
    assert!(typed(&p).is_empty());
}

#[test]
fn an_app_the_workspace_marks_no_input_is_refused_too() {
    // Two separate lists, and this is the other one: `apps.*.no_input` is
    // about Atlas typing anywhere for any reason, and predates dictation.
    // Discord carries it in the shipped `apps.yaml`.
    let blocked = atlas::workspace::input_blocked_apps(&cfg());
    assert!(
        blocked.iter().any(|b| b.to_lowercase().contains("discord")),
        "the shipped apps.yaml is supposed to mark Discord no_input; got {blocked:?}"
    );
}

#[test]
fn moving_to_another_window_stops_it_rather_than_following_you() {
    // Typing into whatever happens to be in front of you *now* is how
    // dictated text ends up in the wrong window.
    let (c, p) = (cfg(), plat());
    p.focus_on("notepad.exe", "untitled - Notepad");
    let mut d = daemon(&c, &p, "moved");

    d.turn("start dictating", 1_000);
    p.focus_on("chrome.exe", "some page - Chrome");
    let said = d.turn("this should not land", 1_001);

    assert!(said.contains("chrome.exe"), "it should say where you went: {said:?}");
    assert!(typed(&p).is_empty(), "text landed in a window you had left");

    // And it is genuinely off, not merely quiet for one turn.
    p.focus_on("notepad.exe", "untitled - Notepad");
    d.turn("nor this", 1_002);
    assert!(typed(&p).is_empty());
}

// --- it does not stay on by itself ------------------------------------------

#[test]
fn dictation_nobody_is_feeding_stops_itself() {
    // Same rule as the camera: a mode you can leave on by accident is a mode
    // that will be left on. `idle_stop_secs` ships at 45.
    let (c, p) = (cfg(), plat());
    p.focus_on("notepad.exe", "untitled - Notepad");
    let mut d = daemon(&c, &p, "idle");

    d.turn("start dictating", 1_000);
    // Only that IT has not stopped — not that the whole tick is silent. The
    // tick has a dozen other reasons to speak (offers, sweeps, the brief) and
    // asserting silence made this test about all of them. It passed for a
    // while and then caught one of the others, which is a test failing for a
    // reason that has nothing to do with what it is testing.
    let early = d.tick(1_010);
    assert!(
        !early.iter().any(|s| s.contains("Stopped dictating")),
        "it gave up while you were mid-thought: {early:?}"
    );

    let out = d.tick(1_000 + 46);
    assert!(
        out.iter().any(|s| s.contains("Stopped dictating")),
        "it should have stopped itself: {out:?}"
    );

    // Off for real — the next thing said is a command again, not text.
    d.turn("hello there", 1_100);
    assert!(typed(&p).is_empty());
}

// --- the rulings adding the intent forced ----------------------------------

#[test]
fn dictation_is_local_operational_and_reported_rather_than_silent() {
    use atlas::intent::Intent;
    let i = Intent::Dictate(String::new());

    assert_eq!(atlas::connectivity::need_of(&i), atlas::connectivity::Need::Local);
    assert_eq!(
        atlas::categories::category_of(&i),
        atlas::categories::Category::LocalOperational
    );
    // Not `AutoProceed`: this is the one capability that sends keystrokes into
    // a window Atlas does not own, and what it reports is which window.
    assert_eq!(atlas::policy::classify(&i), atlas::policy::Decision::ProceedAndReport);
    assert_eq!(atlas::session::kind_of(&i), "dictate");
}

#[test]
fn the_phrases_parse_to_the_intent_with_the_rest_kept_raw() {
    use atlas::intent::{Intent, Parser};
    let c = Config::load(Path::new("config")).unwrap();
    let p = Parser::new(&c.commands);

    assert_eq!(p.parse("start dictating"), Intent::Dictate(String::new()));
    // Case and punctuation survive — the whole reason this command is marked
    // `raw_argument` in commands.yaml.
    assert_eq!(
        p.parse("type this Dear Sarah, thanks"),
        Intent::Dictate("Dear Sarah, thanks".into())
    );
}
