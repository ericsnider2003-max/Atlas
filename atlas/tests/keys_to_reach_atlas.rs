//! Eric's ruling H1 (25 Sep 2026): the wake word, push-to-talk and a typing
//! box, all three, with the keys set per person and none locked in.

use atlas::hotkeys::{key_code, Gate, Hook, Keys, Pressed};
use atlas::quickinput::{Action, QuickInput, QuickInputConfig};
use atlas::voice::PttConfig;
use eframe::egui::{Event, Key, Modifiers};

fn ptt(key: &str) -> PttConfig {
    PttConfig { enabled: true, key: key.into(), hold_ms: 350 }
}

fn typing(key: &str) -> QuickInputConfig {
    QuickInputConfig { hotkey: key.into(), ..QuickInputConfig::default() }
}

#[test]
fn any_key_can_be_the_push_to_talk_key() {
    assert_eq!(key_code("tab"), Some(0x09));
    assert_eq!(key_code("Caps Lock"), Some(0x14));
    assert_eq!(key_code("right_ctrl"), Some(0xA3));
    assert_eq!(key_code("F13"), Some(0x7C));
    assert_eq!(key_code("q"), Some(b'Q' as u32));
    assert_eq!(key_code("f25"), None);
    assert_eq!(key_code("the big red one"), None);
}

#[test]
fn the_keys_come_from_settings_and_a_bad_one_is_named() {
    let k = Keys::from_settings(&ptt("capslock"), &typing("ctrl+alt+space"));
    assert_eq!(k.talk, Some((0x14, 350)));
    assert_eq!(k.typing, Some((2 | 1, 0x20)));
    assert!(k.problems.is_empty());
    assert_eq!(
        k.said(&ptt("capslock"), &typing("ctrl+alt+space")).as_deref(),
        Some("You can also hold Caps Lock to talk, or press Ctrl+Alt+Space to type.")
    );

    let k = Keys::from_settings(&ptt("the big red one"), &typing("a"));
    assert_eq!((k.talk, k.typing), (None, None));
    assert_eq!(k.problems.len(), 2, "{:?}", k.problems);
    assert!(k.problems[1].contains("needs Ctrl, Shift or Win"), "a bare letter would fire while you type");

    let off = PttConfig { enabled: false, ..ptt("tab") };
    assert_eq!(Keys::from_settings(&off, &typing("ctrl+alt+a")).talk, None, "off turns only the key off");
}

#[test]
fn a_tap_goes_back_to_your_app_and_a_hold_is_speech() {
    let mut g = Gate::new(350);
    // A tap: held back, then given back.
    assert_eq!(g.key_down(0), (Hook::Swallow, None));
    assert_eq!(g.tick(100), None);
    assert_eq!(g.key_up(120), (None, true), "the app gets its tab");

    // Twenty quick taps, none of them a recording.
    for i in 0..20u64 {
        let t = 1_000 + i * 200;
        g.key_down(t);
        assert_eq!(g.tick(t + 90), None);
        assert_eq!(g.key_up(t + 100).0, None);
    }

    // A hold: talking starts at the threshold, on the timer, not at release.
    g.key_down(10_000);
    assert_eq!(g.tick(10_200), None);
    assert_eq!(g.tick(10_360), Some(Pressed::TalkStart));
    assert!(g.is_talking());
    // The keyboard's own repeats don't start it twice.
    assert_eq!(g.key_down(10_500), (Hook::Swallow, None));
    assert_eq!(g.key_up(12_000), (Some(Pressed::TalkStop), false), "kept from the app");
    assert!(!g.is_talking());
}

fn key(k: Key) -> Event {
    Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }
}

#[test]
fn the_typing_box_sends_what_you_typed_and_closes() {
    let mut q = QuickInput::new(QuickInputConfig::default());
    q.hotkey(None, 0);
    let a = atlas::typebox::apply_keys(&mut q, &[Event::Text("what's nexy".into()), key(Key::Backspace), Event::Text("t".into())], 1);
    assert_eq!(a, Action::Nothing);
    assert_eq!(q.buffer, "what's next");
    assert_eq!(atlas::typebox::apply_keys(&mut q, &[key(Key::Enter)], 2), Action::Submit("what's next".into()));

    let mut q = QuickInput::new(QuickInputConfig::default());
    q.hotkey(None, 0);
    assert_eq!(atlas::typebox::apply_keys(&mut q, &[key(Key::Escape)], 1), Action::Hide);
    // Opened and wandered off from: it closes itself.
    let mut q = QuickInput::new(QuickInputConfig::default());
    q.hotkey(None, 0);
    assert_eq!(atlas::typebox::apply_keys(&mut q, &[], 31), Action::Hide);
}

#[test]
fn only_the_box_s_own_line_is_taken_as_typed() {
    assert_eq!(atlas::typebox::typed_line("ATLAS-TYPED:what's next").as_deref(), Some("what's next"));
    assert_eq!(atlas::typebox::typed_line("ATLAS-TYPED:   "), None);
    assert_eq!(atlas::typebox::typed_line("some warning from a graphics driver"), None);
}

#[test]
fn the_daemon_hears_a_held_key_and_opens_the_box_on_the_other() {
    // The loop reads `hotkeys` before the wake word, so both work at once.
    let src = crate::common::source_of("daemon");
    let keys = src.find("crate::hotkeys::Pressed::TalkStart").expect("push-to-talk is handled");
    let wake = src.find("Tier::Voice => match ears.wake_once()").unwrap();
    assert!(keys < wake, "keys are checked before waiting on the wake word");
    // 28 Sep 2026: the wake word is heard on the microphone's own thread and
    // taken by `listen_pass`; the keys still come first.
    let heard = src.find("self.listen_pass(ears, mouth, clock)").expect("the wake word is taken from its thread");
    assert!(keys < heard, "keys are checked before the wake word's news");
    assert!(src.contains("ears.listen_while(&|| h.held())"), "it listens for as long as the key is held");
    assert!(src.contains("crate::typebox::Standby::start("), "the other key opens the box");
    let main = crate::common::source_of("main");
    assert!(main.contains("atlas::hotkeys::start(&keys)") && main.contains("d.hotkeys = Some(h)"), "started for the background Atlas");
}

#[test]
fn keys_fed_some_other_way_arrive_in_order() {
    let (tx, rx) = std::sync::mpsc::channel();
    let gate = std::sync::Arc::new(std::sync::Mutex::new(Gate::new(350)));
    let h = atlas::hotkeys::Hotkeys::from_parts(rx, gate.clone());
    tx.send(Pressed::TypingBox).unwrap();
    assert_eq!(h.poll(), Some(Pressed::TypingBox));
    assert_eq!(h.poll(), None);
    assert!(!h.held());
    gate.lock().unwrap().key_down(0);
    gate.lock().unwrap().tick(400);
    assert!(h.held(), "held is read live, for the listen to stop on");
}

// ------------------------------------------------------------------ customizable, no Alt

#[test]
fn any_key_can_be_set_and_a_key_that_cannot_work_is_refused_before_it_is_kept() {
    use atlas::hotkeys::check_setting;
    assert!(check_setting("quick_input.hotkey", "ctrl+shift+space").is_ok());
    assert!(check_setting("quick_input.hotkey", "f9").is_ok(), "no modifier needed for a key nobody types with");
    assert!(check_setting("quick_input.hotkey", "ctrl+alt+a").is_ok(), "Alt still works for whoever has one");
    assert!(check_setting("quick_input.hotkey", "q").unwrap_err().contains("Ctrl, Shift or Win"));
    assert!(check_setting("push_to_talk.key", "capslock").is_ok());
    assert!(check_setting("push_to_talk.key", "ctrl+x").unwrap_err().contains("one key you hold"));
    assert!(check_setting("push_to_talk.key", "the big red one").is_err());

    // The settings list refuses it too, so nothing unusable is ever kept.
    let t = atlas::voice::ToolsConfig::default();
    let mut s = atlas::settings::registry(&t);
    assert!(s.set("quick_input.hotkey", "q").is_err());
    assert!(s.set("quick_input.hotkey", "ctrl+shift+k").unwrap().contains("ctrl+shift+k"));
    assert!(atlas::settings::needs_a_restart("quick_input.hotkey"), "Windows is handed the keys at start-up");
    assert!(s.get("push_to_talk.key").is_some() && s.get("push_to_talk.enabled").is_some());
}

#[test]
fn a_key_is_set_by_pressing_it() {
    use atlas::settingswin::key_pressed;
    let press = |k: Key, m: Modifiers| Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: m };
    assert_eq!(key_pressed("quick_input.hotkey", &[]), None, "waiting");
    assert_eq!(
        key_pressed("quick_input.hotkey", &[press(Key::Space, Modifiers::CTRL | Modifiers::SHIFT)]),
        Some(Some("ctrl+shift+space".to_string()))
    );
    assert_eq!(key_pressed("quick_input.hotkey", &[press(Key::F9, Modifiers::NONE)]), Some(Some("f9".to_string())));
    assert_eq!(key_pressed("push_to_talk.key", &[press(Key::Tab, Modifiers::SHIFT)]), Some(Some("tab".to_string())), "one key, held");
    assert_eq!(key_pressed("quick_input.hotkey", &[press(Key::Escape, Modifiers::NONE)]), Some(None), "Escape leaves it");
    assert_eq!(atlas::settingswin::pretty_key("ctrl+shift+space"), "Ctrl + Shift + Space");
}

#[test]
fn a_key_is_changed_by_saying_it() {
    use atlas::daemon::key_spoken;
    assert_eq!(key_spoken("control shift space"), "ctrl+shift+space");
    assert_eq!(key_spoken("caps lock"), "capslock");
    assert_eq!(key_spoken("control plus J"), "ctrl+j");
    assert_eq!(key_spoken("F9."), "f9");

    let c: &'static atlas::config::Config =
        Box::leak(Box::new(atlas::config::Config::load(std::path::Path::new("config")).unwrap()));
    let p = atlas::platform::mock::MockPlatform::new(vec![atlas::platform::Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let dir = std::env::temp_dir().join(format!("atlas-setkey-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let mut d = atlas::daemon::Daemon::new(c, &p, None, atlas::store::Store::new(dir),
        atlas::proactive::Proactive::new(atlas::proactive::ProactiveConfig::default()));
    let said = d.execute(&atlas::intent::Intent::SetKey("what are my keys".into()));
    assert!(said.starts_with("Hold Tab to talk, and press Ctrl + Shift + Space to type."), "{said}");
    // Refused before anything is written.
    let said = d.execute(&atlas::intent::Intent::SetKey("set my typing key to q".into()));
    assert!(said.starts_with("I've left it as it was"), "{said}");
}

#[test]
fn keys_are_named_the_way_you_would_write_them_and_each_press_is_said() {
    use atlas::hotkeys::{heard_as, key_word};
    assert_eq!(key_word("tab"), "Tab");
    assert_eq!(key_word("capslock"), "Caps Lock");
    assert_eq!(key_word("rightctrl"), "Right Ctrl");
    assert_eq!(key_word("f9"), "F9");
    assert_eq!(key_word("q"), "Q");
    let (p, t) = (ptt("tab"), typing("ctrl+shift+space"));
    assert_eq!(heard_as(Pressed::TalkStart, &p, &t), "Tab held — Atlas would start listening now.");
    assert_eq!(heard_as(Pressed::TypingBox, &p, &t), "Ctrl+Shift+Space pressed — the typing box would open.");
}

#[test]
fn what_time_is_it_is_answered_from_the_clock_not_asked_which_one() {
    // 26 Sep 2026 07:56 UTC is 00:56 in California (UTC-7): a Saturday.
    let t = 1_790_409_360;
    assert_eq!(atlas::localclock::spoken_now(t, -7 * 3600), "It's 12:56 AM on Saturday 26 September.");
    assert_eq!(atlas::localclock::spoken_now(t, 0), "It's 7:56 AM on Saturday 26 September.");
    let c: &'static atlas::config::Config =
        Box::leak(Box::new(atlas::config::Config::load(std::path::Path::new("config")).unwrap()));
    let p = atlas::platform::mock::MockPlatform::new(vec![atlas::platform::Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let dir = std::env::temp_dir().join(format!("atlas-clock-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let mut d = atlas::daemon::Daemon::new(c, &p, None, atlas::store::Store::new(dir),
        atlas::proactive::Proactive::new(atlas::proactive::ProactiveConfig::default()));
    let said = d.turn("what time is it", 1_000);
    assert!(said.starts_with("It's "), "{said}");
}
