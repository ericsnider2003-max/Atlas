//! A restart used to lose everything in hand, silently: a window being worked
//! for you, research half done, a council mid-debate. Now work you asked for
//! is written down until it ends (`resume`), and what a restart cut off is
//! picked back up, redone once, or named.

use atlas::brain::Llm;
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::error::Result;
use atlas::intent::Intent;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::resume::{Unfinished, SavedWindow};
use atlas::store::Store;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-restart-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn cfg() -> &'static Config {
    Box::leak(Box::new(Config::load(Path::new("config")).unwrap()))
}

/// Slow enough that the redone council is still in hand after the tick.
struct VerySlow;
impl Llm for VerySlow {
    fn complete(&self, _s: &str, _u: &str) -> Result<String> {
        std::thread::sleep(Duration::from_millis(400));
        Ok("For. It helps.".into())
    }
}

/// Answers every seat, slowly enough that the work is still in hand when
/// the "restart" happens.
struct Slow(Mutex<u32>);
impl Llm for Slow {
    fn complete(&self, _s: &str, _u: &str) -> Result<String> {
        *self.0.lock().unwrap() += 1;
        std::thread::sleep(Duration::from_millis(50));
        Ok("For. It helps. It would be OK if it stayed local.".into())
    }
}

#[test]
fn work_you_asked_for_is_written_down_until_it_ends() {
    let root = scratch("record");
    let p = plat();
    let mut d = Daemon::new(cfg(), &p, Some(Arc::new(Slow(Mutex::new(0)))), Store::new(root.clone()), Proactive::new(ProactiveConfig::default()));
    let _ = d.turn("ask the room: should I build the hollow CLI?", 100);
    let written: Vec<Unfinished> = Store::new(root.clone()).load(atlas::resume::RECORD);
    assert_eq!(written.len(), 1, "{written:?}");
    assert_eq!(written[0].label, "council");
    assert_eq!(written[0].asked, "ask the room: should I build the hollow CLI?");
    // Once it ends, it's crossed off.
    let mut t = 101;
    for _ in 0..600 {
        let _ = d.tick(t);
        t += 1;
        if Store::new(root.clone()).load::<Vec<Unfinished>>(atlas::resume::RECORD).is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(Store::new(root).load::<Vec<Unfinished>>(atlas::resume::RECORD).is_empty(), "a finished errand stayed on the list");
}

#[test]
fn what_a_restart_cut_off_is_redone_once_or_named() {
    let root = scratch("cut-off");
    let store = Store::new(root.clone());
    let left = vec![
        Unfinished { label: "council".into(), topic: Some("the hollow CLI".into()), asked: "ask the room: should I build the hollow CLI?".into(), started: 1, redone: 0 },
        Unfinished { label: "mail".into(), topic: None, asked: "check my mail".into(), started: 1, redone: 0 },
        // Already redone once after an earlier restart: named, not tried again.
        Unfinished { label: "research".into(), topic: Some("tides".into()), asked: "research tides".into(), started: 1, redone: 1 },
    ];
    store.save(atlas::resume::RECORD, &left).unwrap();
    let p = plat();
    let seats = Arc::new(VerySlow);
    let mut d = Daemon::new(cfg(), &p, Some(seats.clone()), Store::new(root.clone()), Proactive::new(ProactiveConfig::default()));
    let out = d.tick(200).join(" ");
    assert!(out.contains("I stopped in the middle of the council"), "{out}");
    assert!(out.contains("started it again"), "{out}");
    assert!(out.contains("the mail check") && out.contains("research"), "{out}");
    assert!(out.contains("haven't redone"), "{out}");
    assert!(d.crew.errands().iter().any(|e| e.name == "council"), "the council wasn't redone: {out}");
    assert!(!d.crew.errands().iter().any(|e| e.name == "mail" || e.name == "research"));
    let now: Vec<Unfinished> = Store::new(root).load(atlas::resume::RECORD);
    assert_eq!(now.len(), 1);
    assert_eq!(now[0].redone, 1, "the redo isn't marked as one, so a crash loop would go on");
    // Said once, not every tick.
    let again = d.tick(201).join(" ");
    assert!(!again.contains("I stopped in the middle"), "{again}");
}

#[test]
fn a_chore_atlas_starts_itself_isnt_recorded() {
    use atlas::resume::{what_to_do_with as after, After};
    assert_eq!(after("backup"), After::Skip);
    assert_eq!(after("housekeeping"), After::Skip);
    assert_eq!(after("search-check"), After::Skip);
    assert_eq!(after("conversation-reply"), After::Skip);
    assert_eq!(after("research"), After::Again);
    assert_eq!(after("outreach"), After::Ask);
}

/// Writes a reply.
struct Replies;
impl Llm for Replies {
    fn complete(&self, _s: &str, _u: &str) -> Result<String> {
        Ok("On my way.".into())
    }
}

fn show(p: &MockPlatform, id: u64, text: &str) {
    p.focus_on("SomeChat.exe", "");
    *p.front.borrow_mut() = Some(atlas::platform::WindowId(id));
    p.set_window_text(id, text);
}

#[test]
fn a_window_being_worked_is_picked_back_up_after_a_restart() {
    let root = scratch("window");
    let p = plat();
    show(&p, 7, "Sam: where are you?");
    {
        let mut d = Daemon::new(cfg(), &p, Some(Arc::new(Replies)), Store::new(root.clone()), Proactive::new(ProactiveConfig::default()));
        let _ = d.execute_timed(&Intent::Delegate("take over this conversation".into()), "take over this conversation");
        let _ = d.tick(atlas::store::now() + 50);
        assert_eq!(d.working_for_you.len(), 1);
    }
    let saved: Vec<SavedWindow> = Store::new(root.clone()).load(atlas::resume::WINDOWS);
    assert_eq!(saved.len(), 1, "the window job wasn't written down");
    assert_eq!(saved[0].job.app, "SomeChat");
    // Atlas starts again; the window is still open.
    let mut d = Daemon::new(cfg(), &p, Some(Arc::new(Replies)), Store::new(root.clone()), Proactive::new(ProactiveConfig::default()));
    let out = d.tick(atlas::store::now() + 60).join(" ");
    assert!(out.contains("picked the conversation in SomeChat back up"), "{out}");
    assert_eq!(d.working_for_you.len(), 1);
    assert_eq!(d.working_for_you[0].win, atlas::platform::WindowId(7));
}

#[test]
fn a_window_that_closed_while_atlas_was_down_is_left_and_said_so() {
    let root = scratch("window-gone");
    let store = Store::new(root.clone());
    let job = atlas::delegate::Delegation::new("SomeChat", "reply", atlas::delegate::Reach::Converse, 12);
    store
        .save(atlas::resume::WINDOWS, &vec![SavedWindow { id: 1 << 62, job, win: 99, after_mine: None, started: 1, held: false }])
        .unwrap();
    let p = plat();
    let mut d = Daemon::new(cfg(), &p, Some(Arc::new(Replies)), Store::new(root), Proactive::new(ProactiveConfig::default()));
    let out = d.tick(atlas::store::now() + 60).join(" ");
    assert!(out.contains("that window's gone now"), "{out}");
    assert!(d.working_for_you.is_empty());
}

// ---------------------------------------------------------------- Atlas's own typing

/// After Atlas typed, Windows' "last input" was Atlas's own keys, so its next
/// reply waited for a gap in typing that was its own.
#[test]
fn atlas_typing_doesnt_count_as_you_typing() {
    use atlas::platform::idle::{idle_of_yours, own_input_starts};
    // You last typed at 10 s; Atlas typed from 40 s to 42 s; it's now 45 s.
    let own = own_input_starts(10_000, None, 40_000);
    let own = atlas::platform::OwnInput { to: 42_000, ..own };
    assert_eq!(own.yours_before, 10_000);
    assert_eq!(idle_of_yours(42_000, Some(own), 45_000), 35, "Atlas's own keys counted as yours");
    // You typed after it: that's you.
    assert_eq!(idle_of_yours(44_000, Some(own), 45_000), 1);
    // Atlas typed, then pressed Enter a moment later: still measured from you.
    let enter = own_input_starts(42_000, Some(own), 42_500);
    assert_eq!(enter.yours_before, 10_000);
    // Nothing of Atlas's: as Windows says.
    assert_eq!(idle_of_yours(30_000, None, 45_000), 15);
    // The clock wraps every 49 days.
    let wrap = atlas::platform::OwnInput { yours_before: u32::MAX - 5_000, from: u32::MAX - 1_000, to: 500 };
    assert_eq!(idle_of_yours(400, Some(wrap), 2_000), 7);
}

// ---------------------------------------------------------------- work in phases

#[test]
fn a_phase_written_down_is_found_again_and_a_half_written_one_isnt() {
    let root = scratch("phases");
    let ph = atlas::phases::Phases::for_work(&root, "improve", "diary\nadd a parser");
    assert_eq!(ph.done::<String>("1-draft"), None);
    assert!(ph.finished("1-draft", &"fn parse() {}".to_string()));
    let again = atlas::phases::Phases::for_work(&root, "improve", "diary\nadd a parser");
    assert_eq!(again.done::<String>("1-draft").as_deref(), Some("fn parse() {}"));
    assert_eq!(again.finished_phases(), vec!["1-draft".to_string()]);
    // Different work, different folder.
    assert_eq!(atlas::phases::Phases::for_work(&root, "improve", "diary\nsomething else").done::<String>("1-draft"), None);
    again.close();
    assert_eq!(ph.done::<String>("1-draft"), None);
}

/// Counts calls; the phases should mean there are none.
struct Counting(Mutex<u32>);
impl Llm for Counting {
    fn complete(&self, _s: &str, _u: &str) -> Result<String> {
        *self.0.lock().unwrap() += 1;
        Ok("fn parse() {}".into())
    }
}

#[test]
fn project_work_carries_on_from_its_last_finished_phase() {
    let root = scratch("improve-phases");
    let folder = scratch("improve-project");
    let what = "on the diary project, add a date parser";
    // A previous run got through all three phases before Atlas stopped.
    let ph = atlas::phases::Phases::for_work(&root, "improve", &format!("diary\n{what}"));
    ph.finished(
        "1-draft",
        &atlas::build_it::Outcome::Built { code: "fn parse() {}".into(), rounds: 0, notes: vec![] },
    );
    ph.finished("2-checked", &(false, "Queued \"date parser\" on diary.".to_string(), vec![atlas::workshop::FileEdit { path: "dates.rs".into(), content: "fn parse() {}".into() }]));
    ph.finished("3-explained", &Some("It reads dates.".to_string()));
    let p = plat();
    let model = Arc::new(Counting(Mutex::new(0)));
    let mut d = Daemon::new(cfg(), &p, Some(model.clone()), Store::new(root.clone()), Proactive::new(ProactiveConfig::default()));
    d.workshop.register("diary", &folder.display().to_string(), 1);
    let said = d.execute_timed(&Intent::Improve(what.into()), what);
    assert!(said.starts_with("On it"), "{said}");
    assert!(said.contains("Carrying on from where it stopped — the draft, checking it, explaining it already done."), "{said}");
    let mut t = atlas::store::now() + 50;
    for _ in 0..500 {
        let _ = d.tick(t);
        t += 1;
        if !d.workshop.projects.iter().flat_map(|p| p.ready()).collect::<Vec<_>>().is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let ready: Vec<_> = d.workshop.projects.iter().flat_map(|p| p.ready()).collect();
    assert_eq!(ready.len(), 1, "the change wasn't filed");
    assert!(ready[0].note.contains("It reads dates."), "{}", ready[0].note);
    assert_eq!(*model.0.lock().unwrap(), 0, "finished phases were redone");
    assert!(ph.finished_phases().is_empty(), "the phases outlived the filed change");
}
