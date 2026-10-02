//! Only one Atlas at a time.
//!
//! Two instances both read `data/state`, change it in memory and write the
//! whole thing back. Neither file is corrupt; the second write simply erases
//! what the first learned, and nothing reports a problem. You notice weeks
//! later that something you told it didn't stick.

use atlas::onlyone::*;
use std::fs;
use std::path::PathBuf;

struct Dir(PathBuf);
impl Dir {
    fn new(tag: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("atlas-lock-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        Dir(d)
    }
    fn lock(&self) -> OnlyOne {
        OnlyOne::at(&self.0)
    }
    /// A later moment, measured from when the lock was actually written.
    ///
    /// The lock stores the time inside the file, so ageing it is just asking
    /// about a later `now` — no need to touch the filesystem clock, which is
    /// the thing that made the first version of this test lie.
    fn age_it(&self, secs: u64) -> u64 {
        // The lock holds `moment pid` since 29 Sep 2026: read as Atlas reads it.
    let started = fs::read_to_string(self.lock().path())
            .ok()
            .and_then(|s| atlas::onlyone::moment_in(&s))
            .unwrap_or(0);
        started + secs
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// --- taking it --------------------------------------------------------------

#[test]
fn an_empty_folder_is_free() {
    let d = Dir::new("free");
    assert_eq!(d.lock().look(1000), Found::Free);
    assert!(d.lock().take(1000).is_ok());
}

#[test]
fn a_second_start_is_refused_while_the_first_is_alive() {
    let d = Dir::new("second");
    d.lock().take(1000).unwrap();
    let err = d.lock().take(1010).unwrap_err();
    assert!(err.contains("already running"), "got: {err}");
    assert!(err.contains("writing over each other"), "it didn't say why: {err}");
}

#[test]
fn the_refusal_says_what_to_do() {
    // A refusal with no route forward gets worked around, usually by deleting
    // the lock, which is the one thing you shouldn't do while it's live.
    let d = Dir::new("what");
    d.lock().take(1000).unwrap();
    assert!(d.lock().take(1005).unwrap_err().contains("Close the other one"));
}

// --- crashes ----------------------------------------------------------------

#[test]
fn a_lock_whose_contents_are_nonsense_is_treated_as_held() {
    // "I can't tell" must never read as "nobody is there". Refusing to start
    // is recoverable by deleting a file; the other way round costs state.
    let d = Dir::new("nonsense");
    d.lock().take(1000).unwrap();
    fs::write(d.lock().path(), "not a number").unwrap();
    assert!(!d.lock().look(9_999_999).can_take());
}

#[test]
fn a_lock_left_by_a_crash_is_taken_over() {
    // A design that needs a clean shutdown is one that breaks on the first
    // power cut.
    let d = Dir::new("crash");
    d.lock().take(1000).unwrap();
    let much_later = d.age_it(GONE_AFTER_SECS + 60);
    match d.lock().look(much_later) {
        Found::Abandoned { silent_for_secs } => assert!(silent_for_secs > GONE_AFTER_SECS),
        other => panic!("a crashed run blocked startup forever: {other:?}"),
    }
    assert!(d.lock().take(much_later).is_ok());
}

#[test]
fn taking_over_says_it_is_doing_so() {
    let d = Dir::new("says");
    d.lock().take(1000).unwrap();
    let later = d.age_it(GONE_AFTER_SECS + 300);
    let found = d.lock().take(later).unwrap();
    assert!(found.plain().contains("crash"), "got: {}", found.plain());
}

#[test]
fn one_missed_beat_is_not_a_death() {
    // A sleeping machine, a stalled disk or a long transcription can all delay
    // a beat. Treating the first missed one as a death would mean Atlas
    // killing itself for being busy.
    let d = Dir::new("busy");
    d.lock().take(1000).unwrap();
    let one_late = d.age_it(BEAT_EVERY_SECS + 5);
    assert!(matches!(d.lock().look(one_late), Found::Running { .. }));
}

#[test]
fn the_window_is_several_beats_wide() {
    assert!(GONE_AFTER_SECS >= 3 * BEAT_EVERY_SECS, "one slow moment would look fatal");
}

// --- staying alive ----------------------------------------------------------

#[test]
fn beating_keeps_it_held() {
    let d = Dir::new("beat");
    d.lock().take(1000).unwrap();
    let nearly_stale = d.age_it(GONE_AFTER_SECS - 1);
    d.lock().beat(nearly_stale);
    // From the refreshed moment it is fresh again.
    assert!(matches!(d.lock().look(nearly_stale + 5), Found::Running { .. }));
}

#[test]
fn it_knows_when_a_beat_is_due() {
    let d = Dir::new("due");
    let l = d.lock();
    assert!(!l.due(1000, 1000 + BEAT_EVERY_SECS - 1));
    assert!(l.due(1000, 1000 + BEAT_EVERY_SECS));
}

// --- letting go -------------------------------------------------------------

#[test]
fn releasing_frees_it_immediately() {
    // Closing Atlas and reopening it shouldn't mean waiting out the window.
    let d = Dir::new("release");
    d.lock().take(1000).unwrap();
    d.lock().release();
    assert_eq!(d.lock().look(1001), Found::Free);
    assert!(d.lock().take(1001).is_ok());
}

#[test]
fn releasing_a_lock_that_is_gone_is_harmless() {
    let d = Dir::new("norelease");
    d.lock().release();
    d.lock().release();
    assert_eq!(d.lock().look(1000), Found::Free);
}

// --- failing safe -----------------------------------------------------------

#[test]
fn only_a_live_holder_blocks_a_start() {
    assert!(Found::Free.can_take());
    assert!(Found::Abandoned { silent_for_secs: 999 }.can_take());
    assert!(!Found::Running { last_beat_secs_ago: 0 }.can_take());
}

#[test]
fn every_outcome_explains_itself_in_words() {
    for f in [
        Found::Free,
        Found::Running { last_beat_secs_ago: 12 },
        Found::Abandoned { silent_for_secs: 600 },
    ] {
        assert!(f.plain().len() > 20, "{f:?} explains nothing");
    }
}

// --- a laptop that slept (28 Sep 2026) ---------------------------------------
//
// Staleness is measured on the wall clock, and a sleeping laptop beats
// nothing while the wall clock runs on. So the moment the lid opens every lock
// reads Abandoned -- the live Atlas's included, until its first tick beats.
// The overlay closed itself on that first look; a second start took over.

/// A holder that wakes and beats `after` from now, as the live Atlas does on
/// its first tick after the lid opens.
fn wakes_and_beats(lock: OnlyOne, at: u64, after: std::time::Duration) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        std::thread::sleep(after);
        lock.beat(at);
    })
}

#[test]
fn a_lock_whose_holder_was_asleep_is_not_taken_over_when_it_wakes() {
    let d = Dir::new("slept");
    d.lock().take(1000).unwrap();
    // Eight hours with the lid shut.
    let morning = d.age_it(8 * 3600);
    assert!(
        matches!(d.lock().look(morning), Found::Abandoned { .. }),
        "the control: on one look, a slept-through lock reads abandoned"
    );
    let waking = wakes_and_beats(d.lock(), morning, std::time::Duration::from_millis(150));
    let r = d.lock().take_patiently(std::time::Duration::from_secs(3), &|| morning);
    waking.join().unwrap();
    let err = r.expect_err("a second Atlas took the lock from one that had only been asleep");
    assert!(err.contains("already running"), "got: {err}");
}

#[test]
fn a_lock_whose_holder_is_really_gone_is_still_taken_after_the_wait() {
    let d = Dir::new("slept-gone");
    d.lock().take(1000).unwrap();
    let later = d.age_it(8 * 3600);
    let found = d
        .lock()
        .take_patiently(std::time::Duration::from_millis(200), &|| later)
        .expect("a crashed Atlas's lock was never taken over");
    assert!(matches!(found, Found::Abandoned { .. }));
}

#[test]
fn a_free_or_live_lock_is_answered_without_waiting() {
    let d = Dir::new("slept-free");
    let started = std::time::Instant::now();
    assert!(d.lock().take_patiently(std::time::Duration::from_secs(30), &|| 1000).is_ok());
    assert!(d.lock().take_patiently(std::time::Duration::from_secs(30), &|| 1001).is_err());
    assert!(started.elapsed() < std::time::Duration::from_secs(5), "waited on a lock that wasn't abandoned");
}

#[test]
fn the_grace_is_long_enough_for_a_tick_and_short_of_the_staleness_window() {
    assert!(WOKE_GRACE_SECS >= 10, "a woken Atlas may need a few seconds for its first tick");
    assert!(WOKE_GRACE_SECS < GONE_AFTER_SECS);
}

#[test]
fn something_watching_the_lock_does_not_give_up_on_the_first_quiet_look() {
    // The overlay's question, asked every three seconds.
    let mut w = Watching::default();
    let quiet = Found::Abandoned { silent_for_secs: 8 * 3600 };
    assert!(w.still_there(&quiet, 50_000), "gave up on the first look after a sleep");
    assert!(w.still_there(&quiet, 50_000 + WOKE_GRACE_SECS - 1));
    // It beat: back to normal, and the next quiet spell starts afresh.
    assert!(w.still_there(&Found::Running { last_beat_secs_ago: 0 }, 50_000 + WOKE_GRACE_SECS));
    assert!(w.still_there(&quiet, 60_000));
    // Quiet for the whole grace: really gone.
    assert!(!w.still_there(&quiet, 60_000 + WOKE_GRACE_SECS));
}

#[test]
fn something_watching_the_lock_goes_at_once_when_atlas_quits() {
    // A released lock is a clean quit, never a sleep.
    let mut w = Watching::default();
    assert!(!w.still_there(&Found::Free, 1));
}

/// 1 Oct 2026: a crash left the lock as 16 zero bytes, and every start for
/// eight hours said "already running, checked in 0 seconds ago". Unreadable
/// contents are judged by when the file was last written: nobody has
/// rewritten it for longer than a live holder ever goes, so nobody holds it.
#[test]
fn a_lock_a_crash_zeroed_is_taken_over_once_nobody_has_touched_it() {
    let d = Dir::new("zeroed");
    let l = d.lock();
    fs::write(l.path(), [0u8; 16]).unwrap();
    let written = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    // Just written: it may be a holder mid-write, so it's occupied.
    assert!(matches!(l.look(written), Found::Running { .. }));
    // Untouched for longer than a holder ever goes between beats: abandoned.
    let later = written + GONE_AFTER_SECS + 60;
    assert!(matches!(l.look(later), Found::Abandoned { .. }), "{:?}", l.look(later));
    assert!(l.take(later).is_ok());
}
