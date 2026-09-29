//! The crew's first pass, from outside the module: slow work off the tick.
//!
//! One of the four test files the 10 Sep package named and the tree never
//! got. Rebuilt 26 Sep 2026 against today's `crew`, rule by rule from the
//! design (`docs/improvements-project/handover-crew-10sep.md`).

use atlas::crew::{Crew, Ending, News, State, Work, SHUTDOWN_DEADLINE_SECS};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn next_news(c: &mut Crew) -> News {
    let start = Instant::now();
    loop {
        if let Some(n) = c.settle(1).pop() {
            return n;
        }
        assert!(start.elapsed() < Duration::from_secs(3), "no news");
        std::thread::sleep(Duration::from_millis(3));
    }
}

fn forever() -> Work {
    Box::new(|_| {
        std::thread::sleep(Duration::from_secs(3600));
        Ok(String::new())
    })
}

#[test]
fn the_tick_never_waits_not_even_to_stop() {
    let mut c = Crew::new(1);
    let id = c.hand("render", 0, forever()).unwrap();
    let t = Instant::now();
    c.settle(1);
    c.ask_to_stop(id);
    c.settle(1);
    assert!(t.elapsed() < Duration::from_millis(50), "settle or stop waited: {:?}", t.elapsed());
}

#[test]
fn an_errand_that_dies_without_a_word_is_vanished_not_finished() {
    let mut c = Crew::new(1);
    c.hand("boom", 0, Box::new(|_| panic!("the errand died"))).unwrap();
    assert!(matches!(next_news(&mut c).ending, Ending::Vanished));
}

#[test]
fn stopped_is_not_failed() {
    // Getting this wrong means being told off for changing your mind.
    let mut c = Crew::new(1);
    let id = c
        .hand(
            "download",
            0,
            Box::new(|ctl| {
                while !ctl.stopping() {
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err("interrupted".into())
            }),
        )
        .unwrap();
    c.ask_to_stop(id);
    assert!(matches!(next_news(&mut c).ending, Ending::Stopped));
}

#[test]
fn a_result_is_reported_with_its_errand() {
    let mut c = Crew::new(1);
    let id = c.hand("backup", 7, Box::new(|_| Ok("backed up 12 files".into()))).unwrap();
    let n = next_news(&mut c);
    assert_eq!((n.id, n.name.as_str(), n.started), (id, "backup", 7));
    assert!(matches!(n.ending, Ending::Done(Ok(ref s)) if s == "backed up 12 files"));
}

#[test]
fn more_work_than_hands_waits_rather_than_failing_or_all_starting() {
    let mut c = Crew::new(1);
    let go = Arc::new(AtomicBool::new(false));
    let g = go.clone();
    c.hand(
        "first",
        0,
        Box::new(move |_| {
            while !g.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(2));
            }
            Ok(String::new())
        }),
    )
    .unwrap();
    let second = c.hand("second", 0, Box::new(|_| Ok(String::new()))).unwrap();
    assert_eq!((c.active(), c.queued()), (1, 1));
    assert!(c.errands().iter().any(|e| e.id == second && e.state == State::Waiting));
    go.store(true, Ordering::SeqCst);
    let start = Instant::now();
    while c.in_hand(second) {
        c.settle(1);
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::sleep(Duration::from_millis(3));
    }
}

#[test]
fn a_pause_holds_without_losing_anything() {
    let mut c = Crew::new(1);
    let id = c
        .hand(
            "research",
            0,
            Box::new(|ctl| {
                let mut found = Vec::new();
                for i in 0..20 {
                    if ctl.checkpoint() {
                        return Err("stopped".into());
                    }
                    found.push(i);
                    std::thread::sleep(Duration::from_millis(5));
                }
                Ok(format!("{} sources", found.len()))
            }),
        )
        .unwrap();
    c.pause(id);
    let start = Instant::now();
    while !c.errands().iter().any(|e| e.id == id && e.state == State::Holding) {
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(c.resume(id));
    // Everything it did before the pause is still counted.
    assert!(matches!(next_news(&mut c).ending, Ending::Done(Ok(ref s)) if s == "20 sources"));
}

#[test]
fn shutdown_waits_but_not_forever() {
    let t = Instant::now();
    {
        let mut c = Crew::new(1);
        c.hand("stubborn", 0, forever()).unwrap();
    }
    assert!(t.elapsed() < Duration::from_secs(SHUTDOWN_DEADLINE_SECS + 5));
}

#[test]
fn the_daemon_settles_the_crew_every_tick_and_hands_off_through_one_door() {
    let d = crate::common::source_of("daemon");
    let tick = d.find("pub fn tick(&mut self, t: u64)").expect("tick is gone");
    let end = d[tick..].find("\n    pub fn observe(").map(|e| tick + e).unwrap_or(d.len());
    assert!(d[tick..end].contains("self.take_crew_news(t)"), "the tick no longer collects crew news");
    assert!(d.contains("fn hand_off("), "the one door into the crew is gone");
}
