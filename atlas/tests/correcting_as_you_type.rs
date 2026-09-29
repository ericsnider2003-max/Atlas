//! Eric, 25 Sep 2026 (H4): Atlas fixes mistakes in place as you type, with
//! undo; a fix you change back is left alone in that text or email; and it
//! learns — adaptively, so it neither stops working in general nor learns
//! from one miss.

use atlas::astype::{self, Lessons, Polled, Watch};
use atlas::platform::mock::{Action, MockPlatform};
use atlas::platform::{ActiveWindow, Monitor, WindowId};
use atlas::prose::ProseConfig;

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn on(p: &MockPlatform, app: &str, title: &str, win: u64, text: &str) {
    *p.active.borrow_mut() = Some(ActiveWindow { process: app.into(), title: title.into() });
    *p.front.borrow_mut() = Some(WindowId(win));
    *p.focus_editable.borrow_mut() = Some(true);
    *p.typing_box.borrow_mut() = Some(text.into());
}

fn cfg() -> ProseConfig {
    let mut c = ProseConfig::default();
    c.enabled = true;
    c
}

/// Type `text` into the box and let Atlas see it, paused long enough to act.
fn type_and_pause(p: &MockPlatform, w: &mut Watch, l: &mut Lessons, text: &str, at_ms: &mut u64) -> Polled {
    *p.typing_box.borrow_mut() = Some(text.into());
    *at_ms += 1_000;
    let _ = astype::look_at_the_box(p, w, l, &cfg(), &[], *at_ms);
    *at_ms += 300;
    astype::look_at_the_box(p, w, l, &cfg(), &[], *at_ms)
}

#[test]
fn a_finished_word_with_one_possible_fix_is_fixed_in_place_after_a_pause() {
    let p = plat();
    let (mut w, mut l, mut t) = (Watch::default(), Lessons::default(), 1_000_000u64);
    on(&p, "OUTLOOK.EXE", "Re: Friday", 5, "Hi Sam, ");
    assert_eq!(astype::look_at_the_box(&p, &mut w, &mut l, &cfg(), &[], t), Polled::Nothing, "first sight: nothing already there is touched");

    // Still typing: no pause, nothing done.
    *p.typing_box.borrow_mut() = Some("Hi Sam, I dont ".into());
    t += 120;
    assert_eq!(astype::look_at_the_box(&p, &mut w, &mut l, &cfg(), &[], t), Polled::Nothing);
    t += 120;
    assert_eq!(astype::look_at_the_box(&p, &mut w, &mut l, &cfg(), &[], t), Polled::Nothing, "not paused long enough yet");
    t += 200;
    let got = astype::look_at_the_box(&p, &mut w, &mut l, &cfg(), &[], t);
    assert_eq!(got, Polled::Fixed { was: "dont".into(), becomes: "don't".into() });
    assert_eq!(p.typing_box.borrow().as_deref(), Some("Hi Sam, I don't "));
    let backspaces = p.actions().iter().filter(|a| matches!(a, Action::Press(k) if k == "backspace")).count();
    assert_eq!(backspaces, 5, "the word and the space after it");

    // Typing on past the pause: a word that's no longer at the end isn't
    // reached back for.
    *p.typing_box.borrow_mut() = Some("Hi Sam, I don't teh".into());
    t += 1_000;
    let _ = astype::look_at_the_box(&p, &mut w, &mut l, &cfg(), &[], t);
    *p.typing_box.borrow_mut() = Some("Hi Sam, I don't teh plan".into());
    t += 1_000;
    assert_eq!(astype::look_at_the_box(&p, &mut w, &mut l, &cfg(), &[], t), Polled::Nothing);
}

#[test]
fn changed_back_it_is_left_alone_in_that_text_but_not_in_the_next() {
    let p = plat();
    let (mut w, mut l, mut t) = (Watch::default(), Lessons::default(), 1_000_000u64);
    on(&p, "OUTLOOK.EXE", "Re: Friday", 5, "Hi, ");
    let _ = astype::look_at_the_box(&p, &mut w, &mut l, &cfg(), &[], t);
    assert!(matches!(type_and_pause(&p, &mut w, &mut l, "Hi, I dont ", &mut t), Polled::Fixed { .. }));
    // You change it back.
    assert_eq!(type_and_pause(&p, &mut w, &mut l, "Hi, I dont ", &mut t), Polled::Nothing);
    // Later in the same email: left alone.
    assert_eq!(type_and_pause(&p, &mut w, &mut l, "Hi, I dont know. I dont ", &mut t), Polled::Nothing);

    // A new email: one change-back isn't a lesson.
    on(&p, "OUTLOOK.EXE", "Re: Monday", 5, "Hey, ");
    let _ = astype::look_at_the_box(&p, &mut w, &mut l, &cfg(), &[], t);
    assert!(matches!(type_and_pause(&p, &mut w, &mut l, "Hey, I dont ", &mut t), Polled::Fixed { .. }));
}

#[test]
fn it_stops_only_on_repeated_change_backs_and_only_where_they_happened() {
    let mut l = Lessons::default();
    let now = 1_000_000;
    // Kept five times, in mail.
    for _ in 0..5 {
        l.kept("dont", "don't", now);
    }
    // Changed back in a game chat, in three different chats.
    for _ in 0..2 {
        l.changed_back("dont", "don't", "discord.exe", now);
        assert!(l.may_fix("dont", "don't", "discord.exe", now), "not stopped on two");
    }
    l.changed_back("dont", "don't", "discord.exe", now);
    assert!(!l.may_fix("dont", "don't", "discord.exe", now), "three change-backs there: stopped there");
    assert!(l.may_fix("dont", "don't", "outlook.exe", now), "still made everywhere else");
    assert!(l.what_it_learned(now).contains("(in discord.exe)"), "{}", l.what_it_learned(now));

    // Spread across apps and outnumbering the keeps: stopped everywhere.
    for app in ["slack.exe", "teams.exe", "chrome.exe"] {
        l.changed_back("dont", "don't", app, now);
    }
    assert!(!l.may_fix("dont", "don't", "outlook.exe", now));

    // The evidence fades: half a year on, it's made again.
    let later = now + 200 * 86_400;
    assert!(l.may_fix("dont", "don't", "outlook.exe", later));
    assert!(l.may_fix("dont", "don't", "discord.exe", later));
}

#[test]
fn your_own_fix_seen_in_three_texts_becomes_one_atlas_makes() {
    let p = plat();
    let (mut w, mut l, mut t) = (Watch::default(), Lessons::default(), 1_000_000u64);
    for (n, title) in ["Re: one", "Re: two", "Re: three"].iter().enumerate() {
        on(&p, "OUTLOOK.EXE", title, 10 + n as u64, "Hi, ");
        let _ = astype::look_at_the_box(&p, &mut w, &mut l, &cfg(), &[], t);
        let _ = type_and_pause(&p, &mut w, &mut l, "Hi, thansk for ", &mut t);
        // You fix it yourself.
        let _ = type_and_pause(&p, &mut w, &mut l, "Hi, thanks for ", &mut t);
        if n < 2 {
            assert!(l.learned_fix("thansk").is_none(), "learned from {} of your fixes", n + 1);
        }
    }
    assert_eq!(l.learned_fix("thansk").as_deref(), Some("thanks"), "{l:?}");
    // And now Atlas makes it.
    on(&p, "OUTLOOK.EXE", "Re: four", 20, "Hi, ");
    let _ = astype::look_at_the_box(&p, &mut w, &mut l, &cfg(), &[], t);
    assert_eq!(
        type_and_pause(&p, &mut w, &mut l, "Hi, thansk ", &mut t),
        Polled::Fixed { was: "thansk".into(), becomes: "thanks".into() }
    );
    // A different word that's only a letter off isn't one of yours.
    assert!(!astype::close_spellings("form", "from"));
}

#[test]
fn it_keeps_out_of_code_editors_and_windows_atlas_is_typing_in() {
    let p = plat();
    let (mut w, mut l, mut t) = (Watch::default(), Lessons::default(), 1_000_000u64);
    on(&p, "Code.exe", "main.rs - Visual Studio Code", 5, "let x = ");
    let _ = astype::look_at_the_box(&p, &mut w, &mut l, &cfg(), &[], t);
    assert_eq!(type_and_pause(&p, &mut w, &mut l, "let x = dont ", &mut t), Polled::Nothing);

    on(&p, "Slack.exe", "Sam", 6, "ok ");
    let _ = astype::look_at_the_box(&p, &mut w, &mut l, &cfg(), &[6], t);
    *p.typing_box.borrow_mut() = Some("ok dont ".into());
    t += 1_000;
    let _ = astype::look_at_the_box(&p, &mut w, &mut l, &cfg(), &[6], t);
    t += 300;
    assert_eq!(astype::look_at_the_box(&p, &mut w, &mut l, &cfg(), &[6], t), Polled::Nothing, "Atlas is working that window");
}

#[test]
fn one_fix_of_yours_is_never_a_lesson_and_only_close_spellings_count() {
    let mut l = Lessons::default();
    assert!(!l.you_fixed("recieve", "receive", 100));
    assert!(!l.you_fixed("recieve", "receive", 200));
    assert!(l.you_fixed("recieve", "receive", 300), "the third one teaches it");
    // A rewrite isn't a spelling fix.
    for t in 0..5 {
        assert!(!l.you_fixed("maybe", "definitely", 400 + t));
    }
    assert!(l.learned_fix("maybe").is_none());
}
