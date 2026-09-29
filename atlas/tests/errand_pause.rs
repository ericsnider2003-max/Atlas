//! Pausing one errand without losing it, and working out which one "stop"
//! meant when several are going (Eric's B1 ruling, 23 Sep 2026).

use atlas::config::Config;
use atlas::crew::{Crew, Ending, State, Work};
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use atlas::which_errand::{
    answered, correction, describe, pick, question, verb_and_target, Candidate, Pick, Verb, Why,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Counts to `steps`, one safe point per step. Returns how many steps it
/// actually did — so a pause that lost or repeated work would show.
fn counter(steps: usize, done: Arc<AtomicUsize>) -> Work {
    Box::new(move |ctl| {
        for _ in 0..steps {
            if ctl.checkpoint() {
                return Err("stopped".into());
            }
            done.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(format!("{} steps", done.load(Ordering::SeqCst)))
    })
}

fn until(mut f: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

fn state(c: &Crew, id: u64) -> Option<State> {
    c.errands().into_iter().find(|e| e.id == id).map(|e| e.state)
}

// ================= the crew: pause holds, nothing lost =================

#[test]
fn a_paused_errand_holds_and_carries_on_from_where_it_was() {
    let mut c = Crew::new(2);
    let done = Arc::new(AtomicUsize::new(0));
    let id = c.hand("research", 0, counter(60, done.clone())).unwrap();
    assert!(until(|| done.load(Ordering::SeqCst) >= 5), "it never got going");
    assert!(c.pause(id));
    assert!(until(|| state(&c, id) == Some(State::Holding)), "it never reached a safe point");
    let held_at = done.load(Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(250));
    assert_eq!(done.load(Ordering::SeqCst), held_at, "a held errand kept working");
    assert!(held_at < 60, "it finished before the pause, so this proves nothing");
    assert!(c.settle(1).is_empty(), "holding is not ending");

    assert!(c.resume(id));
    let mut news = Vec::new();
    assert!(until(|| {
        news.extend(c.settle(2));
        !news.is_empty()
    }));
    match &news[0].ending {
        // Every step exactly once: the pause lost nothing and repeated nothing.
        Ending::Done(Ok(s)) => assert_eq!(s, "60 steps"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn stopping_a_held_errand_wakes_it_and_ends_it() {
    let mut c = Crew::new(1);
    let done = Arc::new(AtomicUsize::new(0));
    let id = c.hand("research", 0, counter(1000, done)).unwrap();
    c.pause(id);
    assert!(until(|| state(&c, id) == Some(State::Holding)));
    c.ask_to_stop(id);
    let mut news = Vec::new();
    assert!(until(|| {
        news.extend(c.settle(1));
        !news.is_empty()
    }));
    assert!(matches!(news[0].ending, Ending::Stopped), "{:?}", news[0].ending);
}

#[test]
fn a_held_errand_does_not_keep_queued_work_out() {
    let mut c = Crew::new(1);
    let a_done = Arc::new(AtomicUsize::new(0));
    let a = c.hand("research", 0, counter(1000, a_done)).unwrap();
    let b_done = Arc::new(AtomicUsize::new(0));
    let b = c.hand("build", 0, counter(3, b_done.clone())).unwrap();
    assert_eq!(state(&c, b), Some(State::Waiting), "one hand, so the second waits");
    c.pause(a);
    assert!(until(|| state(&c, a) == Some(State::Holding)));
    let mut finished = Vec::new();
    assert!(until(|| {
        finished.extend(c.settle(1).into_iter().map(|n| n.id));
        finished.contains(&b)
    }), "the queued errand never got the hand the held one wasn't using");
    assert_eq!(b_done.load(Ordering::SeqCst), 3);
    assert_eq!(state(&c, a), Some(State::Holding), "and the held one is still held");
    c.ask_to_stop(a);
}

#[test]
fn a_queued_errand_paused_before_it_started_keeps_its_place_until_resumed() {
    let mut c = Crew::new(1);
    let first = c.hand("research", 0, counter(5, Arc::new(AtomicUsize::new(0)))).unwrap();
    let ran = Arc::new(AtomicUsize::new(0));
    let second = c.hand("build", 0, counter(2, ran.clone())).unwrap();
    assert!(c.pause(second));
    assert_eq!(state(&c, second), Some(State::WaitingPaused));
    assert!(until(|| c.settle(1).iter().any(|n| n.id == first)));
    std::thread::sleep(Duration::from_millis(100));
    c.settle(2);
    assert_eq!(ran.load(Ordering::SeqCst), 0, "it started while paused");
    assert!(c.resume(second));
    assert!(until(|| c.settle(3).iter().any(|n| n.id == second)));
    assert_eq!(ran.load(Ordering::SeqCst), 2);
    assert!(!c.resume(second), "nothing left to resume");
}

// ================= which one: the rules =================

fn cand(id: u64, label: &str, topic: Option<&str>, started: u64) -> Candidate {
    Candidate {
        id,
        label: label.into(),
        topic: topic.map(str::to_string),
        started,
        paused: false,
        can_hold: label != "backup",
    }
}

fn three() -> Vec<Candidate> {
    vec![
        cand(1, "research", Some("postgres pricing"), 100),
        cand(2, "backup", None, 200),
        cand(3, "build", Some("a landing page"), 300),
    ]
}

#[test]
fn the_sentence_is_read_for_a_verb_and_a_target() {
    assert_eq!(verb_and_target("stop"), Some((Verb::Pause, String::new())));
    assert_eq!(verb_and_target("Stop the research."), Some((Verb::Pause, "the research".into())));
    assert_eq!(verb_and_target("put the backup on hold"), Some((Verb::Pause, "the backup".into())));
    assert_eq!(verb_and_target("carry on with the build"), Some((Verb::Resume, "the build".into())));
    assert_eq!(verb_and_target("cancel the second one"), Some((Verb::Cancel, "the second one".into())));
    assert_eq!(verb_and_target("what's the weather"), None);
    assert_eq!(correction("no, the backup"), Some("the backup".into()));
    assert_eq!(correction("not that one"), Some(String::new()));
    assert_eq!(correction("the backup"), None);
}

#[test]
fn a_name_picks_it_whatever_else_is_going() {
    let c = three();
    assert_eq!(pick(Verb::Pause, "the research", &c, &[], 1000), Pick::These(vec![1], Why::Named));
    // What it was asked about counts too.
    assert_eq!(pick(Verb::Pause, "the postgres one", &c, &[], 1000), Pick::These(vec![1], Why::Named));
    assert_eq!(pick(Verb::Pause, "the page", &c, &[], 1000), Pick::These(vec![3], Why::Named));
    // Words about nothing that's running: not ours.
    assert_eq!(pick(Verb::Pause, "the music", &c, &[], 1000), Pick::Nothing);
}

#[test]
fn a_number_counts_oldest_first_and_all_means_all() {
    let c = three();
    assert_eq!(pick(Verb::Pause, "the second one", &c, &[], 1000), Pick::These(vec![2], Why::Numbered));
    assert_eq!(pick(Verb::Pause, "3", &c, &[], 1000), Pick::These(vec![3], Why::Numbered));
    assert_eq!(pick(Verb::Pause, "the last one", &c, &[], 1000), Pick::These(vec![3], Why::Numbered));
    assert_eq!(pick(Verb::Pause, "all of them", &c, &[], 1000), Pick::These(vec![1, 2, 3], Why::All));
}

#[test]
fn with_nothing_named_the_conversation_decides() {
    let c = three();
    let recent = vec!["how much does postgres cost to host".to_string()];
    assert_eq!(pick(Verb::Pause, "", &c, &recent, 1000), Pick::These(vec![1], Why::Conversation));
    // A conversation naming two of them settles nothing.
    let both = vec!["is the research or the backup nearly done".to_string()];
    assert_eq!(pick(Verb::Pause, "", &c, &both, 1000), Pick::Ask(vec![1, 2, 3]));
}

#[test]
fn stop_right_after_starting_something_means_that_thing() {
    let c = three();
    assert_eq!(pick(Verb::Pause, "", &c, &[], 330), Pick::These(vec![3], Why::JustStarted));
    // Two started in the last minute and a half: ask.
    assert_eq!(pick(Verb::Pause, "", &c, &[], 290), Pick::Ask(vec![1, 2, 3]));
}

#[test]
fn nothing_is_called_off_for_good_on_a_guess() {
    let c = three();
    let recent = vec!["how much does postgres cost to host".to_string()];
    assert_eq!(pick(Verb::Cancel, "", &c, &recent, 330), Pick::Ask(vec![1, 2, 3]));
    assert_eq!(pick(Verb::Cancel, "the backup", &c, &[], 330), Pick::These(vec![2], Why::Named));
}

#[test]
fn resuming_only_looks_at_what_is_paused() {
    let mut c = three();
    c[1].paused = true;
    assert_eq!(pick(Verb::Resume, "", &c, &[], 1000), Pick::These(vec![2], Why::OnlyOne));
    assert_eq!(pick(Verb::Pause, "the backup", &c, &[], 1000), Pick::Nothing, "already paused");
}

#[test]
fn the_answer_says_what_paused_what_continues_and_how_to_correct_it() {
    let c = three();
    let picked = [&c[0]];
    let others = [&c[1], &c[2]];
    let s = answered(Verb::Pause, &picked, Why::Conversation, &others);
    assert!(s.contains("Paused the research on postgres pricing"), "{s}");
    assert!(s.contains("nothing lost"), "{s}");
    assert!(s.contains("what you were just talking about"), "{s}");
    assert!(s.contains("Still going: the backup and the build on a landing page"), "{s}");
    assert!(s.contains("no, the"), "a guess must say how to correct it: {s}");
    // Named: no guess, so no correction hint.
    assert!(!answered(Verb::Pause, &picked, Why::Named, &others).contains("no, the"));
    // One with no safe point is not claimed paused.
    let b = answered(Verb::Pause, &[&c[1]], Why::Named, &[&c[0]]);
    assert!(b.contains("no safe point"), "{b}");
    assert!(!b.contains("Paused"), "{b}");
    let q = question(Verb::Pause, &[&c[0], &c[1]]);
    assert!(q.starts_with("Two things are going: 1) the research on postgres pricing, 2) the backup"), "{q}");
    assert_eq!(describe(&c[1]), "the backup");
}

// ================= through the daemon =================

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-errand-pause-{tag}"));
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

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    let mut d =
        Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()));
    // Enough hands that both errands actually run at once — the case "which
    // one?" is about. The queued case is covered on the crew above.
    d.crew = Crew::new(4);
    d
}

fn long() -> Work {
    counter(100_000, Arc::new(AtomicUsize::new(0)))
}

#[test]
fn stop_the_research_pauses_it_and_says_the_build_carries_on() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "named");
    let research = d.crew.hand("research", 10, long()).unwrap();
    let build = d.crew.hand("build", 20, long()).unwrap();
    let reply = d.turn("stop the research", 1000);
    assert!(reply.contains("Paused the research"), "{reply}");
    assert!(reply.contains("Still going: the build"), "{reply}");
    assert!(until(|| state(&d.crew, research) == Some(State::Holding)));
    assert_eq!(state(&d.crew, build), Some(State::Running), "the other one was touched");

    let back = d.turn("carry on with the research", 1010);
    assert!(back.contains("Picking the research back up"), "{back}");
    assert!(until(|| state(&d.crew, research) == Some(State::Running)));
    d.crew.ask_everyone_to_stop();
}

#[test]
fn a_bare_stop_it_cannot_place_is_asked_about_and_the_answer_is_heard() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "ask");
    let research = d.crew.hand("research", 10, long()).unwrap();
    let build = d.crew.hand("build", 20, long()).unwrap();
    let q = d.turn("stop", 1000);
    assert!(q.contains("1) the research"), "{q}");
    assert!(q.contains("2) the build"), "{q}");
    assert_eq!(state(&d.crew, research), Some(State::Running), "nothing paused on a question");
    let a = d.turn("2", 1010);
    assert!(a.contains("Paused the build"), "{a}");
    assert!(until(|| state(&d.crew, build) == Some(State::Holding)));
    assert_eq!(state(&d.crew, research), Some(State::Running));
    d.crew.ask_everyone_to_stop();
}

#[test]
fn a_wrong_guess_is_swapped_by_no_the_other_one() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "swap");
    let research = d.crew.hand("research", 10, long()).unwrap();
    let build = d.crew.hand("build", 995, long()).unwrap();
    let first = d.turn("stop", 1000);
    assert!(first.contains("Paused the build"), "just started, so the build: {first}");
    assert!(first.contains("the one I'd just started"), "{first}");
    let swapped = d.turn("no, the research", 1005);
    assert!(swapped.contains("the build is back at work"), "{swapped}");
    assert!(swapped.contains("Paused the research"), "{swapped}");
    assert!(until(|| state(&d.crew, research) == Some(State::Holding)));
    assert!(until(|| state(&d.crew, build) == Some(State::Running)));
    d.crew.ask_everyone_to_stop();
}

#[test]
fn pausing_atlas_holds_every_errand_and_carrying_on_releases_them() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "whole");
    let research = d.crew.hand("research", 10, long()).unwrap();
    let build = d.crew.hand("build", 20, long()).unwrap();
    let r = d.turn("pause", 1000);
    assert!(r.contains("Holding 2 errands"), "{r}");
    assert!(until(|| state(&d.crew, research) == Some(State::Holding)
        && state(&d.crew, build) == Some(State::Holding)));
    d.turn("carry on", 1100);
    assert!(until(|| state(&d.crew, research) == Some(State::Running)
        && state(&d.crew, build) == Some(State::Running)));
    d.crew.ask_everyone_to_stop();
}

#[test]
fn calling_one_off_needs_it_named_and_leaves_the_other() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "cancel");
    let research = d.crew.hand("research", 10, long()).unwrap();
    let build = d.crew.hand("build", 20, long()).unwrap();
    let r = d.turn("cancel the build", 1000);
    assert!(r.contains("Called off the build"), "{r}");
    assert!(until(|| d.crew.errands().iter().all(|e| e.id != build) || {
        d.crew.settle(1001);
        false
    }));
    assert!(d.crew.in_hand(research), "the research was called off too");
    d.crew.ask_everyone_to_stop();
}
