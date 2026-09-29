//! The crew's second pass: work admitted by what it costs.
//!
//! Each test is one rule, with the reason it exists. Rebuilt 26 Sep 2026 from
//! the 10 Sep design (`docs/improvements-project/handover-crew-10sep.md`),
//! whose code never reached the tree.

use atlas::crew::{Crew, Ending, Job, Limits, Needs, Room, Taken, Urgency, Work, MAX_WAITING};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Work that runs until released.
fn held() -> (Arc<AtomicBool>, Work) {
    let go = Arc::new(AtomicBool::new(false));
    let g = go.clone();
    let w: Work = Box::new(move |c| {
        while !g.load(Ordering::SeqCst) && !c.stopping() {
            std::thread::sleep(Duration::from_millis(2));
        }
        Ok("done".into())
    });
    (go, w)
}

fn quick() -> Work {
    Box::new(|_| Ok("quick".into()))
}

fn counted(n: &Arc<AtomicUsize>) -> Work {
    let n = n.clone();
    Box::new(move |_| {
        n.fetch_add(1, Ordering::SeqCst);
        Ok("counted".into())
    })
}

fn limits(slots: usize) -> Limits {
    // `Limits::slots` is what `Crew::new` has always meant: a slot count and
    // no other rule.
    let l = Limits::slots(slots);
    assert_eq!((l.cores, l.keep_free_mb, l.battery_floor_percent), (None, 0, 0));
    Limits { waiting_hands: 4, ..l }
}

fn room(r: Room) -> Box<dyn Fn() -> Room + Send> {
    Box::new(move || r)
}

fn job(name: &str, needs: Needs) -> Job {
    Job::new(name).needs(needs)
}

/// Settle until `f` holds or two seconds pass.
fn settle_until(c: &mut Crew, t: u64, mut f: impl FnMut(&Crew) -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        c.settle(t);
        if f(c) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(3));
    }
    f(c)
}

fn started(t: Result<Taken, String>) -> bool {
    matches!(t, Ok(Taken::Started(_)))
}

// ---------------------------------------------------------------------------
// What work costs
// ---------------------------------------------------------------------------

#[test]
fn whole_machine_work_runs_one_at_a_time() {
    // Two renders side by side don't halve the wait: ffmpeg starts more
    // threads than there are cores, and two of them fight over the same
    // cores and cache. Sequential is genuinely faster.
    let mut c = Crew::with_limits(limits(4));
    let (go, w) = held();
    assert!(started(c.hand_job(job("render", Needs::TheWholeMachine), 0, w)));
    let second = c.hand_job(job("render 2", Needs::TheWholeMachine), 0, quick());
    assert!(matches!(second, Ok(Taken::Queued(_))), "{second:?}");
    go.store(true, Ordering::SeqCst);
    assert!(settle_until(&mut c, 1, |c| c.active() == 0 && c.queued() == 0));
}

#[test]
fn nothing_that_thinks_runs_beside_a_whole_machine_job() {
    let mut c = Crew::with_limits(limits(4));
    let (go, w) = held();
    c.hand_job(job("render", Needs::TheWholeMachine), 0, w).unwrap();
    let id = c.hand_job(job("summary", Needs::OneCore), 0, quick()).unwrap().id();
    assert_eq!(c.active(), 1);
    let why = c.why_waiting(id).unwrap();
    assert!(why.contains("render") && why.contains("whole machine"), "{why}");
    go.store(true, Ordering::SeqCst);
    assert!(settle_until(&mut c, 1, |c| c.queued() == 0));
}

#[test]
fn a_whole_machine_job_waits_for_thinking_work_to_finish() {
    let mut c = Crew::with_limits(limits(4));
    let (go, w) = held();
    c.hand_job(job("index", Needs::OneCore), 0, w).unwrap();
    let id = c.hand_job(job("build", Needs::TheWholeMachine), 0, quick()).unwrap().id();
    assert!(c.why_waiting(id).unwrap().contains("once index is done"));
    go.store(true, Ordering::SeqCst);
    assert!(settle_until(&mut c, 1, |c| !c.in_hand(id)));
}

#[test]
fn waiting_work_never_queues_behind_a_render() {
    // The bulkhead: a download barely competes with a render, so making it
    // wait for one is pure loss.
    let mut c = Crew::with_limits(limits(1));
    let (go, w) = held();
    c.hand_job(job("render", Needs::TheWholeMachine), 0, w).unwrap();
    assert!(started(c.hand_job(job("download", Needs::MostlyWaiting), 0, quick())));
    go.store(true, Ordering::SeqCst);
    settle_until(&mut c, 1, |c| c.active() == 0);
}

#[test]
fn the_slot_count_bounds_thinking_not_waiting() {
    // The defect the 10 Sep pass introduced and caught: a plain slot count
    // kept around the new rules let a render and one download fill two
    // slots while seven more downloads, competing with nothing, waited.
    let mut c = Crew::with_limits(Limits { waiting_hands: 8, ..limits(2) });
    let (go, w) = held();
    c.hand_job(job("render", Needs::TheWholeMachine), 0, w).unwrap();
    let mut gates = Vec::new();
    for i in 0..8 {
        let (g, w) = held();
        gates.push(g);
        assert!(started(c.hand_job(job(&format!("download {i}"), Needs::MostlyWaiting), 0, w)), "download {i}");
    }
    assert_eq!(c.active(), 9);
    go.store(true, Ordering::SeqCst);
    gates.iter().for_each(|g| g.store(true, Ordering::SeqCst));
    assert!(settle_until(&mut c, 1, |c| c.active() == 0));
}

#[test]
fn waiting_work_has_its_own_ceiling() {
    // Separate is not unlimited: forty downloads at once is its own problem.
    let mut c = Crew::with_limits(Limits { waiting_hands: 2, ..limits(4) });
    let mut gates = Vec::new();
    for i in 0..3 {
        let (g, w) = held();
        gates.push(g);
        c.hand_job(job(&format!("copy {i}"), Needs::MostlyWaiting), 0, w).unwrap();
    }
    assert_eq!((c.active(), c.queued()), (2, 1));
    let third = c.errands().last().unwrap().id;
    assert!(c.why_waiting(third).unwrap().contains("hands for waiting work"));
    gates.iter().for_each(|g| g.store(true, Ordering::SeqCst));
    assert!(settle_until(&mut c, 1, |c| c.active() == 0 && c.queued() == 0));
}

#[test]
fn a_core_is_left_for_the_person_at_the_machine() {
    // A pool sized to every core leaves nothing spare for Eric.
    let mut c = Crew::with_limits(Limits { cores: Some(3), ..limits(8) });
    let mut gates = Vec::new();
    for i in 0..3 {
        let (g, w) = held();
        gates.push(g);
        c.hand_job(job(&format!("parse {i}"), Needs::OneCore), 0, w).unwrap();
    }
    assert_eq!(c.active(), 2, "three cores: two for work, one for you");
    let third = c.errands().last().unwrap().id;
    assert!(c.why_waiting(third).unwrap().contains("kept free for you"));
    gates.iter().for_each(|g| g.store(true, Ordering::SeqCst));
    assert!(settle_until(&mut c, 1, |c| c.active() == 0 && c.queued() == 0));
}

#[test]
fn a_one_core_machine_still_gets_work_done() {
    // cores - 1 is zero on a one-core machine; the floor is one, or nothing
    // would ever run.
    let mut c = Crew::with_limits(Limits { cores: Some(1), ..limits(2) });
    assert!(started(c.hand_job(job("parse", Needs::OneCore), 0, quick())));
}

// ---------------------------------------------------------------------------
// Memory and power
// ---------------------------------------------------------------------------

#[test]
fn nothing_that_thinks_starts_under_the_memory_margin() {
    // Below the margin the machine swaps, and a laptop that swaps is one
    // where typing stops responding.
    let mut c = Crew::with_limits(Limits { keep_free_mb: 1024, ..limits(2) })
        .with_room(room(Room { free_mb: Some(600), ..Room::default() }));
    let id = c.hand_job(job("index", Needs::OneCore), 0, quick()).unwrap().id();
    assert!(c.in_hand(id) && c.active() == 0);
    let why = c.why_waiting(id).unwrap();
    assert!(why.contains("600 MB") && why.contains("1024 MB"), "{why}");
}

#[test]
fn the_memory_margin_never_holds_waiting_work() {
    let mut c = Crew::with_limits(Limits { keep_free_mb: 1024, ..limits(2) })
        .with_room(room(Room { free_mb: Some(100), ..Room::default() }));
    assert!(started(c.hand_job(job("download", Needs::MostlyWaiting), 0, quick())));
}

#[test]
fn the_memory_margin_holds_even_work_you_asked_for() {
    // Starting work that makes the machine swap doesn't serve the request.
    let mut c = Crew::with_limits(Limits { keep_free_mb: 1024, ..limits(2) })
        .with_room(room(Room { free_mb: Some(300), ..Room::default() }));
    let t = c.hand_job(job("build", Needs::TheWholeMachine).urgency(Urgency::Now), 0, quick());
    assert!(matches!(t, Ok(Taken::Queued(_))));
}

#[test]
fn the_battery_floor_holds_chores_but_not_what_you_asked_for() {
    let low = Room { free_mb: Some(8000), on_battery: true, battery_percent: Some(20) };
    let mut c = Crew::with_limits(Limits { battery_floor_percent: 30, ..limits(2) }).with_room(room(low));
    let chore = c.hand_job(job("improve", Needs::TheWholeMachine).urgency(Urgency::Later), 0, quick()).unwrap();
    assert!(matches!(chore, Taken::Queued(_)));
    let why = c.why_waiting(chore.id()).unwrap();
    assert!(why.contains("battery at 20%") && why.contains("plug in"), "{why}");

    let mut c = Crew::with_limits(Limits { battery_floor_percent: 30, ..limits(2) }).with_room(room(low));
    assert!(started(c.hand_job(job("build", Needs::TheWholeMachine).urgency(Urgency::Now), 0, quick())));
}

#[test]
fn on_mains_or_above_the_floor_the_battery_holds_nothing() {
    for r in [
        Room { free_mb: None, on_battery: false, battery_percent: Some(5) },
        Room { free_mb: None, on_battery: true, battery_percent: Some(80) },
    ] {
        let mut c = Crew::with_limits(Limits { battery_floor_percent: 30, ..limits(2) }).with_room(room(r));
        assert!(started(c.hand_job(job("improve", Needs::TheWholeMachine).urgency(Urgency::Later), 0, quick())), "{r:?}");
    }
}

#[test]
fn a_machine_that_cannot_say_never_has_its_work_held() {
    // Not known must not block: a crew that refuses because it can't read
    // the battery does nothing on a desktop.
    let mut c = Crew::with_limits(Limits { keep_free_mb: 4096, battery_floor_percent: 90, ..limits(2) })
        .with_room(room(Room::default()));
    assert!(started(c.hand_job(job("improve", Needs::TheWholeMachine).urgency(Urgency::Later), 0, quick())));
}

#[test]
fn memory_and_power_are_read_only_when_something_thinks_and_then_trusted() {
    let reads = Arc::new(AtomicUsize::new(0));
    let r = reads.clone();
    let mut c = Crew::with_limits(limits(4)).with_room(Box::new(move || {
        r.fetch_add(1, Ordering::SeqCst);
        Room::default()
    }));
    c.hand_job(job("download", Needs::MostlyWaiting), 0, quick()).unwrap();
    assert_eq!(reads.load(Ordering::SeqCst), 0, "waiting work needs no reading");
    for i in 0..5 {
        c.hand_job(job(&format!("parse {i}"), Needs::OneCore), 0, quick()).unwrap();
    }
    assert_eq!(reads.load(Ordering::SeqCst), 1, "one reading, trusted for five seconds");
}

// ---------------------------------------------------------------------------
// Order
// ---------------------------------------------------------------------------

/// A one-slot crew with a job occupying it, so the rest queue.
fn busy_one() -> (Crew, Arc<AtomicBool>) {
    let mut c = Crew::with_limits(limits(1));
    let (go, w) = held();
    c.hand_job(job("occupying", Needs::OneCore), 0, w).unwrap();
    (c, go)
}

#[test]
fn what_you_asked_for_goes_before_chores() {
    let (mut c, go) = busy_one();
    let order = Arc::new(Mutex::new(Vec::new()));
    for (name, u) in [("chore", Urgency::Later), ("asked", Urgency::Now)] {
        let o = order.clone();
        c.hand_job(Job::new(name).urgency(u), 0, Box::new(move |_| {
            o.lock().unwrap().push(name);
            Ok(String::new())
        }))
        .unwrap();
    }
    go.store(true, Ordering::SeqCst);
    assert!(settle_until(&mut c, 1, |_| order.lock().unwrap().len() == 2));
    assert_eq!(*order.lock().unwrap(), vec!["asked", "chore"]);
}

#[test]
fn a_chore_that_has_waited_long_enough_moves_up() {
    // Priority without ageing starves the bottom of the queue, and the
    // symptom is a job that never runs with nothing saying why.
    let (mut c, go) = busy_one();
    let order = Arc::new(Mutex::new(Vec::new()));
    let o = order.clone();
    c.hand_job(Job::new("old chore").urgency(Urgency::Later), 0, Box::new(move |_| {
        o.lock().unwrap().push("old chore");
        Ok(String::new())
    }))
    .unwrap();
    let o = order.clone();
    // Twenty-one minutes later: the chore has aged two bands, to level with
    // Now, and it is older.
    c.hand_job(Job::new("new ask").urgency(Urgency::Now), 1_260, Box::new(move |_| {
        o.lock().unwrap().push("new ask");
        Ok(String::new())
    }))
    .unwrap();
    go.store(true, Ordering::SeqCst);
    assert!(settle_until(&mut c, 1_260, |_| order.lock().unwrap().len() == 2));
    assert_eq!(*order.lock().unwrap(), vec!["old chore", "new ask"]);
}

#[test]
fn a_whole_machine_job_at_the_front_is_not_kept_out_by_small_ones() {
    // Without holding the thinking hands for it, a stream of one-core jobs
    // would always find a free hand first and the render would never run.
    let mut c = Crew::with_limits(limits(2));
    let (go, w) = held();
    c.hand_job(job("parse", Needs::OneCore), 0, w).unwrap();
    let render = c.hand_job(job("render", Needs::TheWholeMachine).urgency(Urgency::Now), 0, quick()).unwrap();
    let small = c.hand_job(job("small", Needs::OneCore).urgency(Urgency::Later), 0, quick()).unwrap();
    assert!(matches!(small, Taken::Queued(_)), "a free hand, but held for the render");
    assert!(c.why_waiting(small.id()).unwrap().contains("held so render"));
    // Waiting work is never held: it doesn't compete.
    assert!(started(c.hand_job(job("download", Needs::MostlyWaiting), 0, quick())));
    go.store(true, Ordering::SeqCst);
    assert!(settle_until(&mut c, 1, |c| !c.in_hand(render.id()) && !c.in_hand(small.id())));
}

// ---------------------------------------------------------------------------
// Once, bounded, measured
// ---------------------------------------------------------------------------

#[test]
fn the_same_work_asked_twice_is_done_once() {
    let mut c = Crew::with_limits(limits(2));
    let runs = Arc::new(AtomicUsize::new(0));
    let (go, w) = held();
    let first = c.hand_job(Job::new("mail").keyed("mail"), 0, w).unwrap();
    let second = c.hand_job(Job::new("mail").keyed("mail"), 0, counted(&runs)).unwrap();
    assert_eq!(second, Taken::Joined(first.id()));
    go.store(true, Ordering::SeqCst);
    assert!(settle_until(&mut c, 1, |c| c.active() == 0));
    assert_eq!(runs.load(Ordering::SeqCst), 0, "the second ask never ran on its own");
}

#[test]
fn joining_a_queued_chore_with_an_ask_makes_it_urgent() {
    // "Back up now" joining a queued nightly backup: somebody is now waiting.
    let (mut c, go) = busy_one();
    let order = Arc::new(Mutex::new(Vec::new()));
    let o = order.clone();
    c.hand_job(Job::new("backup").keyed("backup").urgency(Urgency::Later), 0, Box::new(move |_| {
        o.lock().unwrap().push("backup");
        Ok(String::new())
    }))
    .unwrap();
    let o = order.clone();
    c.hand_job(Job::new("other").urgency(Urgency::Soon), 0, Box::new(move |_| {
        o.lock().unwrap().push("other");
        Ok(String::new())
    }))
    .unwrap();
    c.hand_job(Job::new("backup").keyed("backup").urgency(Urgency::Now), 0, quick()).unwrap();
    go.store(true, Ordering::SeqCst);
    assert!(settle_until(&mut c, 1, |_| order.lock().unwrap().len() == 2));
    assert_eq!(order.lock().unwrap()[0], "backup");
}

#[test]
fn work_being_stopped_is_not_joined() {
    // A run you called off is not an answer to a new ask.
    let mut c = Crew::with_limits(limits(2));
    let (_go, w) = held();
    let first = c.hand_job(Job::new("research").keyed("research:tides"), 0, w).unwrap();
    c.ask_to_stop(first.id());
    let again = c.hand_job(Job::new("research").keyed("research:tides"), 0, quick()).unwrap();
    assert_ne!(again.id(), first.id());
}

#[test]
fn a_full_waiting_list_refuses_in_a_sentence() {
    let (mut c, go) = busy_one();
    for i in 0..MAX_WAITING {
        c.hand_job(Job::new(&format!("filler {i}")), 0, quick()).unwrap();
    }
    let refused = c.hand_job(Job::new("one too many"), 0, quick()).unwrap_err();
    assert!(refused.contains("one too many") && refused.contains("haven't queued"), "{refused}");
    go.store(true, Ordering::SeqCst);
}

#[test]
fn what_each_errand_waited_and_ran_is_kept() {
    let (mut c, go) = busy_one();
    c.hand_job(Job::new("second"), 0, quick()).unwrap();
    std::thread::sleep(Duration::from_millis(60));
    let (name, ms) = c.longest_wait().unwrap();
    assert_eq!(name, "second", "a job still waiting counts: it's the case that matters");
    assert!(ms >= 50);
    go.store(true, Ordering::SeqCst);
    assert!(settle_until(&mut c, 1, |c| c.recently_finished().len() == 2));
    let r = c.recently_finished();
    let second = r.iter().find(|t| t.name == "second").unwrap();
    assert!(second.waited_ms >= 50, "{second:?}");
    let first = r.iter().find(|t| t.name == "occupying").unwrap();
    assert!(first.ran_ms >= 50 && first.waited_ms < 50, "{first:?}");
}

#[test]
fn the_tick_is_hurried_only_when_something_could_start() {
    // Work blocked behind a render is not a reason to spin.
    let mut c = Crew::with_limits(limits(2));
    let (go, w) = held();
    c.hand_job(job("render", Needs::TheWholeMachine), 0, w).unwrap();
    c.hand_job(job("parse", Needs::OneCore), 0, quick()).unwrap();
    assert!(!c.wants_attention(), "nothing could start and nothing has finished");
    go.store(true, Ordering::SeqCst);
    let start = Instant::now();
    while !c.wants_attention() && start.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(c.wants_attention(), "the render finished: its news is waiting");
    assert!(settle_until(&mut c, 1, |c| c.active() == 0 && c.queued() == 0));
}

#[test]
fn stopped_work_is_still_reported_as_stopped() {
    // The first pass's rule survives the second.
    let mut c = Crew::with_limits(limits(1));
    let (_go, w) = held();
    let id = c.hand_job(job("render", Needs::TheWholeMachine), 0, w).unwrap().id();
    c.ask_to_stop(id);
    let start = Instant::now();
    loop {
        if let Some(n) = c.settle(1).pop() {
            assert!(matches!(n.ending, Ending::Stopped));
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(2));
        std::thread::sleep(Duration::from_millis(3));
    }
}

#[test]
fn every_kind_of_crew_errand_says_what_it_costs() {
    // `crew_job` in the daemon is the one place this is decided. A new crew
    // citizen that isn't in it defaults to one core — which is safe — but the
    // heavy ones must be named or two builds would run side by side.
    let d = crate::common::source_of("daemon");
    let at = d.find("fn crew_job(").expect("crew_job is gone");
    let body = &d[at..at + 2500];
    for heavy in ["\"build\"", "\"improve\"", "\"council\""] {
        assert!(body.contains(heavy), "{heavy} no longer marked as using the whole machine");
    }
    assert!(body.contains("\"backup\"") && body.contains("MostlyWaiting"));
    let hand_off = d.find("fn hand_off(").unwrap();
    assert!(d[hand_off..hand_off + 1200].contains("crew_job("), "hand_off no longer says what work costs");
    assert!(!d.contains("self.crew.hand(name"), "a crew errand is being handed in without its cost");
}
