//! An idle Atlas sleeps until something happens (5 Oct 2026 audit, Q14).
//!
//! It woke about 20 times a second in `nap_awake`, 50 times a second in each
//! push-to-talk timer, and 10 times a second for every paused errand -- all
//! day, with nothing to do. Now everything that has news rings one doorbell
//! (`atlas::doorbell`) and anything idle waits on it.
//!
//! The bell is process-wide and other tests in this binary ring it too, so
//! these assert what a ring guarantees (a waiter wakes) and never that it
//! stays quiet.

use std::time::{Duration, Instant};

#[test]
fn news_sent_before_the_wait_is_not_slept_through() {
    // The race-free rule: read the count, look, then wait on that count.
    let seen = atlas::doorbell::rung();
    let (tx, rx) = atlas::doorbell::channel::<u8>();
    tx.send(1).unwrap();
    let started = Instant::now();
    assert!(atlas::doorbell::wait_after(seen, 10_000), "a send after the count was read must end the wait");
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(rx.try_recv().ok(), Some(1));
}

#[test]
fn a_send_from_another_thread_wakes_the_waiter() {
    let (tx, rx) = atlas::doorbell::channel::<&'static str>();
    let seen = atlas::doorbell::rung();
    let sender = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(30));
        tx.send("typed").unwrap();
    });
    let started = Instant::now();
    let mut got = None;
    // Woken by the ring, then looks: well under the 10 s it would sleep.
    let mut seen = seen;
    while got.is_none() && started.elapsed() < Duration::from_secs(10) {
        atlas::doorbell::wait_after(seen, 10_000);
        seen = atlas::doorbell::rung();
        got = rx.try_recv().ok();
    }
    sender.join().unwrap();
    assert_eq!(got, Some("typed"));
    assert!(started.elapsed() < Duration::from_secs(2), "took {:?}", started.elapsed());
}

#[test]
fn a_wait_with_nothing_to_wake_it_still_ends_on_time() {
    // Spurious rings from other tests can only end it sooner.
    let started = Instant::now();
    atlas::doorbell::wait_after(atlas::doorbell::rung(), 50);
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn the_push_to_talk_timer_has_work_only_while_the_key_is_held_short_of_talking() {
    let mut g = atlas::hotkeys::Gate::new(300);
    assert!(!g.waiting_for_hold(), "key up: the timer sleeps");
    g.key_down(1_000);
    assert!(g.waiting_for_hold(), "held, not yet talking: the timer checks");
    assert!(g.tick(1_350).is_some(), "the hold crosses its threshold on the timer");
    assert!(!g.waiting_for_hold(), "talking: nothing left for the timer to decide");
    g.key_up(2_000);
    assert!(!g.waiting_for_hold());

    let mut g = atlas::hotkey::Gate::new(300);
    assert!(!g.waiting_for_hold());
    g.down(1_000);
    assert!(g.waiting_for_hold());
    assert!(g.tick(1_350).is_some());
    assert!(!g.waiting_for_hold());
    g.up(2_000);
    assert!(!g.waiting_for_hold());
}

#[test]
fn a_paused_errand_goes_on_the_moment_it_is_resumed() {
    use atlas::crew::{Crew, State};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    let mut c = Crew::new(1);
    let done = Arc::new(AtomicUsize::new(0));
    let d = done.clone();
    let id = c
        .hand(
            "research",
            0,
            Box::new(move |ctl| {
                for _ in 0..400 {
                    if ctl.checkpoint() {
                        return Err("stopped".into());
                    }
                    d.fetch_add(1, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(5));
                }
                Ok(String::new())
            }),
        )
        .unwrap();
    let until = |f: &dyn Fn() -> bool| {
        let end = Instant::now() + Duration::from_secs(5);
        while Instant::now() < end && !f() {
            std::thread::sleep(Duration::from_millis(2));
        }
        f()
    };
    assert!(until(&|| done.load(Ordering::SeqCst) >= 2));
    assert!(c.pause(id));
    let holding = |c: &Crew| c.errands().into_iter().any(|e| e.id == id && e.state == State::Holding);
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end && !holding(&c) {
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(holding(&c), "it never reached a safe point");
    let held_at = done.load(Ordering::SeqCst);
    let resumed = Instant::now();
    assert!(c.resume(id));
    assert!(until(&|| done.load(Ordering::SeqCst) > held_at), "it never went on");
    assert!(resumed.elapsed() < Duration::from_secs(1), "took {:?} to notice the resume", resumed.elapsed());
    c.ask_to_stop(id);
}

#[test]
fn nothing_idle_polls_on_a_timer_any_more() {
    let running = crate::common::source_of("daemon");
    let nap = &running[running.find("fn nap_awake(").expect("nap_awake")..];
    let nap = &nap[..nap.find("\n    }\n").unwrap()];
    assert!(nap.contains("doorbell::wait_after"), "nap_awake sleeps on the doorbell");
    assert!(!nap.contains("keyboard.wait("), "no 10 ms keyboard wait in a loop");

    let goodbye = crate::common::source_of("goodbye");
    let nap = &goodbye[goodbye.find("pub fn nap(").unwrap()..];
    let nap = &nap[..nap.find("\n}\n").unwrap()];
    assert!(!nap.contains("thread::sleep"), "goodbye::nap sleeps on the doorbell");

    let crew = crate::common::source_of("crew");
    let cp = &crew[crew.find("pub fn checkpoint(").unwrap()..];
    let cp = &cp[..cp.find("\n    }\n").unwrap()];
    assert!(!cp.contains("thread::sleep"), "a paused errand sleeps on the doorbell");

    for (name, m) in [("hotkeys", "waiting_for_hold"), ("hotkey", "waiting_for_hold")] {
        let src = crate::common::source_of(name);
        assert!(src.matches("park_timeout").count() >= 1 && src.contains(m), "{name}: the hold timer parks while the key is up");
    }
}
