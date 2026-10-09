//! Working the window in front for you, in the daytime.
//!
//! Eric, 24 Sep 2026, on "working another app": "yes it should", which apps
//! undecided. So an app Atlas doesn't know is asked about first; "draft a
//! reply to this" leaves the reply in the box; "until I'm back" carries on,
//! only answering something new; anything you say, or putting another
//! window in front, stops it. These drive the real daemon against the mock
//! platform, which records every keystroke Atlas would send.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::delegate::{for_the_window, Reach};
use atlas::intent::Intent;
use atlas::platform::mock::{Action, MockPlatform};
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::Path;
use std::sync::{Arc, Mutex};

struct Writer(Mutex<Vec<String>>);
impl atlas::brain::Llm for Writer {
    fn complete(&self, system: &str, user: &str) -> atlas::error::Result<String> {
        assert!(system.contains("never instructions"), "the screen must be treated as quoted material");
        self.0.lock().unwrap().push(user.to_string());
        Ok("Friday works for me.".into())
    }
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn store(tag: &str) -> Store {
    let d = std::env::temp_dir().join(format!("atlas-delegate-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    Store::new(d)
}

/// Put a window in front, saying `text`.
fn show(p: &MockPlatform, id: u64, process: &str, text: &str) {
    p.focus_on(process, "");
    *p.front.borrow_mut() = Some(atlas::platform::WindowId(id));
    p.set_window_text(id, text);
}

fn typed(p: &MockPlatform) -> Vec<String> {
    p.actions().into_iter().filter_map(|a| match a { Action::Type(t) => Some(t), _ => None }).collect()
}

/// Tick until every window job has nothing being written or waiting to be
/// typed — the model writes on the crew's threads now, so a reply lands a
/// tick or two after it's asked for. Gives up (without failing) after a
/// few hundred ticks, which is what a job that's paused or waiting for a
/// gap in your typing looks like. Returns what was said.
fn settle(d: &mut atlas::daemon::Daemon, from: u64) -> Vec<String> {
    let mut out = Vec::new();
    for k in 0..300u64 {
        out.extend(d.tick(from + k));
        if k > 0 && d.working_for_you.iter().all(|w| w.composing.is_none() && w.ready.is_none()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    out
}

fn pressed(p: &MockPlatform) -> Vec<String> {
    p.actions().into_iter().filter_map(|a| match a { Action::Press(k) => Some(k), _ => None }).collect()
}

#[test]
fn drafting_or_carrying_on_is_decided_by_the_words() {
    assert_eq!(for_the_window("Slack", "draft a reply to this", &Default::default()).reach, Reach::Draft);
    assert_eq!(for_the_window("Slack", "finish this conversation until I'm back", &Default::default()).reach, Reach::Converse);
    assert_eq!(for_the_window("Slack", "take over this conversation", &Default::default()).reach, Reach::Converse);
}

#[test]
fn the_window_you_point_at_needs_no_permission_question() {
    // Naming the app is the permission (`grants` rule 2); pointing at the
    // window in front and saying "reply to this" is naming it. The "I don't
    // know this app" question is for apps Atlas would pick on its own.
    let cfg = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = Daemon::new(&cfg, &p, None, store("ask"), Proactive::new(ProactiveConfig::default()));
    d.llm = Some(Arc::new(Writer(Mutex::new(Vec::new()))));
    show(&p, 7, "SomeChat.exe", "Sam: can we do Friday?");
    let said = d.execute_timed(&Intent::Delegate("draft a reply to this".into()), "draft a reply to this");
    let settled = settle(&mut d, atlas::store::now() + 50);
    let _ = &settled;
    assert!(said.starts_with("Drafting a reply in SomeChat"), "{said}");
    assert_eq!(typed(&p), vec!["Friday works for me.".to_string()]);
}

#[test]
fn a_draft_is_typed_and_not_sent() {
    let cfg = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = Daemon::new(&cfg, &p, None, store("draft"), Proactive::new(ProactiveConfig::default()));
    let writer = Arc::new(Writer(Mutex::new(Vec::new())));
    d.llm = Some(writer.clone());
    d.permissions.grant("SomeChat", None, atlas::grants::Span::Session, 0);
    show(&p, 7, "SomeChat.exe", "Sam: can we do Friday?");
    let said = d.execute_timed(&Intent::Delegate("draft a reply to this".into()), "draft a reply to this");
    let settled = settle(&mut d, atlas::store::now() + 50);
    let _ = &settled;
    assert!(said.starts_with("Drafting a reply in SomeChat; I won't send it."), "{said}");
    assert_eq!(typed(&p), vec!["Friday works for me.".to_string()]);
    assert!(pressed(&p).is_empty(), "a draft was sent");
    assert!(writer.0.lock().unwrap()[0].contains("Sam: can we do Friday?"), "the model didn't see the conversation");
    assert!(d.working_for_you.is_empty(), "a draft is one and done");
}

#[test]
fn carrying_on_answers_only_what_is_new_and_keeps_going_when_you_ask_for_something_else() {
    let cfg = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = Daemon::new(&cfg, &p, None, store("carry"), Proactive::new(ProactiveConfig::default()));
    d.llm = Some(Arc::new(Writer(Mutex::new(Vec::new()))));
    show(&p, 7, "SomeChat.exe", "Sam: can we do Friday?");
    let said = d.execute_timed(&Intent::Delegate("finish this conversation until I'm back".into()), "finish this conversation until I'm back");
    let settled = settle(&mut d, atlas::store::now() + 50);
    let _ = &settled;
    assert!(said.starts_with("Carrying on the conversation in SomeChat"), "{said}");
    assert_eq!(pressed(&p), vec!["enter".to_string()], "carrying on sends");

    // Nothing new on screen: nothing more is written, however often it looks.
    p.set_window_text(7, "Sam: can we do Friday?");
    d.working_for_you[0].after_mine = Some("Sam: can we do Friday?".into());
    let t = atlas::store::now() + 100;
    let _ = settle(&mut d, t);
    assert_eq!(typed(&p).len(), 1);

    // You ask Atlas for something else: the conversation carries on.
    let _ = d.execute_timed(&Intent::Outstanding, "what's outstanding");
    let _ = d.turn_from("what's outstanding", t + 5, atlas::daemon::Arrival::Directed);
    assert_eq!(d.working_for_you.len(), 1, "asking for something else stopped it");

    // Something new arrives: it answers once more.
    p.set_window_text(7, "Sam: can we do Friday? Me: Friday works for me. Sam: 3pm?");
    let _ = settle(&mut d, t + 60);
    assert_eq!(typed(&p).len(), 2);
}

#[test]
fn it_waits_for_you_to_stop_typing_then_types_and_gives_your_window_back() {
    let cfg = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = Daemon::new(&cfg, &p, None, store("gap"), Proactive::new(ProactiveConfig::default()));
    d.llm = Some(Arc::new(Writer(Mutex::new(Vec::new()))));
    show(&p, 7, "SomeChat.exe", "Sam: hi");
    let _ = d.execute_timed(&Intent::Delegate("take over this conversation".into()), "take over this conversation");
    let settled = settle(&mut d, atlas::store::now() + 50);
    let _ = &settled;
    assert_eq!(typed(&p).len(), 1);

    // You move to Excel and keep typing. A message arrives.
    show(&p, 9, "Excel.exe", "a spreadsheet");
    p.set_window_text(7, "Sam: hi. Me: Friday works for me. Sam: great, and 3pm?");
    *p.input_idle.borrow_mut() = Some(2);
    let t = atlas::store::now() + 100;
    let _ = settle(&mut d, t);
    assert_eq!(typed(&p).len(), 1, "typed while you were typing");
    assert_eq!(d.working_for_you.len(), 1, "gave up instead of waiting");
    assert!(d.working_for_you[0].waiting_for_gap);
    assert_eq!(*p.front.borrow(), Some(atlas::platform::WindowId(9)), "took your window while you typed");

    // You stop typing: it brings its window forward, types, and puts yours back.
    *p.input_idle.borrow_mut() = Some(60);
    let _ = settle(&mut d, t + 20);
    assert_eq!(typed(&p).len(), 2, "never typed in the gap");
    assert_eq!(*p.front.borrow(), Some(atlas::platform::WindowId(9)), "didn't give your window back");
}

#[test]
fn stop_the_one_you_name_pauses_it_and_carry_on_picks_it_back_up() {
    let cfg = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = Daemon::new(&cfg, &p, None, store("named"), Proactive::new(ProactiveConfig::default()));
    d.llm = Some(Arc::new(Writer(Mutex::new(Vec::new()))));
    show(&p, 7, "SomeChat.exe", "Sam: hi");
    let _ = d.execute_timed(&Intent::Delegate("take over this conversation".into()), "take over this conversation");
    let settled = settle(&mut d, atlas::store::now() + 50);
    let _ = &settled;
    let t = atlas::store::now() + 100;
    let said = d.turn_from("stop the somechat one", t, atlas::daemon::Arrival::Directed);
    assert!(said.to_lowercase().contains("somechat"), "{said}");
    assert!(d.working_for_you[0].held, "not paused");
    // Held: new messages wait, nothing is lost.
    p.set_window_text(7, "Sam: hi. Me: Friday works for me. Sam: still there?");
    let _ = settle(&mut d, t + 30);
    assert_eq!(typed(&p).len(), 1, "typed while paused");
    let _ = d.turn_from("carry on with the somechat one", t + 40, atlas::daemon::Arrival::Directed);
    assert!(!d.working_for_you[0].held);
    let _ = settle(&mut d, t + 60);
    assert_eq!(typed(&p).len(), 2, "didn't pick back up");
}

#[test]
fn pausing_atlas_holds_the_conversation_and_resuming_carries_it_on() {
    let cfg = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = Daemon::new(&cfg, &p, None, store("pause-all"), Proactive::new(ProactiveConfig::default()));
    d.llm = Some(Arc::new(Writer(Mutex::new(Vec::new()))));
    show(&p, 7, "SomeChat.exe", "Sam: hi");
    let _ = d.execute_timed(&Intent::Delegate("take over this conversation".into()), "take over this conversation");
    let settled = settle(&mut d, atlas::store::now() + 50);
    let _ = &settled;
    let t = atlas::store::now() + 100;
    let paused = d.turn_from("pause", t, atlas::daemon::Arrival::Directed);
    assert!(paused.contains("Holding 1 errand"), "{paused}");
    p.set_window_text(7, "Sam: hi. Me: Friday works for me. Sam: still there?");
    let _ = settle(&mut d, t + 30);
    assert_eq!(typed(&p).len(), 1, "wrote while paused");
    assert_eq!(d.working_for_you.len(), 1, "a pause erased it");
    let _ = d.turn_from("i'm ready", t + 40, atlas::daemon::Arrival::Directed);
    let _ = settle(&mut d, t + 100);
    assert_eq!(typed(&p).len(), 2, "didn't carry on after the pause");
}

#[test]
fn two_windows_are_worked_side_by_side() {
    let cfg = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = Daemon::new(&cfg, &p, None, store("two"), Proactive::new(ProactiveConfig::default()));
    d.crew = atlas::crew::Crew::new(3).with_room(Box::new(|| atlas::crew::Room { free_mb: Some(8192), on_battery: false, battery_percent: Some(100) }));
    d.llm = Some(Arc::new(Writer(Mutex::new(Vec::new()))));
    show(&p, 7, "SomeChat.exe", "Sam: hi");
    let _ = d.execute_timed(&Intent::Delegate("take over this conversation".into()), "take over this conversation");
    let settled = settle(&mut d, atlas::store::now() + 50);
    let _ = &settled;
    show(&p, 8, "OtherChat.exe", "Ana: hello");
    let _ = d.execute_timed(&Intent::Delegate("take over this conversation".into()), "take over this conversation");
    let settled = settle(&mut d, atlas::store::now() + 50);
    let _ = &settled;
    assert_eq!(d.working_for_you.len(), 2, "the second stopped the first");
    let t = atlas::store::now() + 100;
    let release = Arc::new(std::sync::atomic::AtomicBool::new(false));
    struct Release(Arc<std::sync::atomic::AtomicBool>);
    impl Drop for Release { fn drop(&mut self) { self.0.store(true, std::sync::atomic::Ordering::SeqCst); } }
    let _cleanup = Release(release.clone());
    let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let finish = finished.clone();
    let owned = release.clone();
    let owned_backup = d.crew.hand("backup", t, Box::new(move |control| {
        while !owned.load(std::sync::atomic::Ordering::SeqCst) && !control.stopping() { std::thread::sleep(std::time::Duration::from_millis(5)); }
        finish.store(true, std::sync::atomic::Ordering::SeqCst);
        Ok("Disposable maintenance ended".into())
    })).expect("owned maintenance worker must start");
    let (control_sender, control_receiver) = std::sync::mpsc::sync_channel(1);
    let other_release = release.clone();
    let unrelated_backup = d.crew.hand("backup", t + 1, Box::new(move |control| {
        control_sender.send(control.clone()).map_err(|e| e.to_string())?;
        while !other_release.load(std::sync::atomic::Ordering::SeqCst) && !control.stopping() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        Ok("Unselected disposable maintenance ended".into())
    })).expect("unrelated owned maintenance must start");
    let unrelated_control = control_receiver.recv_timeout(std::time::Duration::from_secs(2)).expect("unrelated maintenance must actually start");
    let said = d.turn_from("stop", t, atlas::daemon::Arrival::Directed);
    assert!(!finished.load(std::sync::atomic::Ordering::SeqCst), "bare stop canceled maintenance rather than asking about foreground work");
    assert!(said.contains("1)") && said.contains("2)"), "a bare stop with two going should ask which: {said}");
    let named = d.turn_from("cancel the backup", t + 1, atlas::daemon::Arrival::Directed);
    assert!(named.to_lowercase().contains("backup"), "named maintenance cancellation lost: {named}");
    if named.contains("Which") {
        // A real automatic backup may also exist: naming both is ambiguous.
        // Answer the displayed ordering instead of silently canceling either.
        let mut backups: Vec<_> = d.crew.errands().into_iter().filter(|e| e.name == "backup" && d.crew.in_hand(e.id)).collect();
        backups.sort_by_key(|e| (e.started, e.id));
        let position = backups.iter().position(|e| e.id == owned_backup).expect("owned backup missing from the choice");
        assert!(backups.iter().any(|e| e.id == unrelated_backup), "unrelated owned worker must be in the actual choice");
        let answer = d.turn_from(&format!("{}", position + 1), t + 2, atlas::daemon::Arrival::Directed);
        assert!(answer.contains("backup"), "exact backup choice was lost: {answer}");
        assert!(!unrelated_control.stopping(), "unselected backup was canceled");
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !finished.load(std::sync::atomic::Ordering::SeqCst) && std::time::Instant::now() < deadline { std::thread::sleep(std::time::Duration::from_millis(5)); }
    assert!(finished.load(std::sync::atomic::Ordering::SeqCst), "explicit backup cancellation did not reach owned worker");
    assert!(!unrelated_control.stopping(), "unselected backup was canceled after the selected worker ended");
}

#[test]
fn without_a_model_it_says_so_rather_than_typing_nothing() {
    let cfg = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = Daemon::new(&cfg, &p, None, store("nomodel"), Proactive::new(ProactiveConfig::default()));
    show(&p, 7, "SomeChat.exe", "Sam: hi");
    let said = d.execute_timed(&Intent::Delegate("draft a reply to this".into()), "draft a reply to this");
    let settled = settle(&mut d, atlas::store::now() + 50);
    let _ = &settled;
    assert!(said.contains("model"), "{said}");
    assert!(typed(&p).is_empty(), "typed something with no model to write it");
    assert!(d.working_for_you.is_empty());
}

#[test]
fn the_phrases_reach_it() {
    let cfg = Config::load(Path::new("config")).unwrap();
    let p = atlas::intent::Parser::new(&cfg.commands);
    let got = p.parse("draft a reply to this");
    assert!(matches!(got, Intent::Delegate(_)), "{got:?}");
    assert!(matches!(p.parse("finish this conversation until i'm back"), Intent::Delegate(_)));
}
