//! Hand tracking on its own thread.
//!
//! Everything in `handtrack` and `handshape` was correct and would still have
//! felt unusable, because it was queued behind a daemon tick that sleeps for
//! up to two seconds. Prediction covers a hundred milliseconds, not two
//! thousand.
//!
//! What is tested here is the scheduling, the stopping and the reporting —
//! the parts most likely to be wrong, and none of which need a camera or a
//! model.

use atlas::error::Result;
use atlas::handloop::{start, Eyes, Pointer, Said, Setup};
use atlas::handshape::{Gesture, Landmarks, Point, Test, Vocabulary, POINTS};
use atlas::handtrack::{PaceConfig, SmoothConfig};
use atlas::platform::{Button, PixelRect, WindowId};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Doubles
// ---------------------------------------------------------------------------

/// A pointer that records what it was asked to do.
#[derive(Default)]
struct Noted {
    moves: AtomicUsize,
    clicks: AtomicUsize,
    overlays: AtomicUsize,
    placed: AtomicUsize,
}

/// Wrapped so the trait can be implemented for a local type.
struct Hand(Arc<Noted>);

impl Pointer for Hand {
    fn move_cursor(&self, _x: i32, _y: i32) -> Result<()> {
        self.0.moves.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    fn click(&self, _x: i32, _y: i32, _b: Button) -> Result<()> {
        self.0.clicks.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    fn window_at(&self, _x: i32, _y: i32) -> Result<Option<WindowId>> {
        Ok(None)
    }
    fn rect_of(&self, _w: WindowId) -> Result<PixelRect> {
        Ok(PixelRect { x: 0, y: 0, width: 100, height: 100 })
    }
    fn place(&self, _w: WindowId, _r: PixelRect) -> Result<()> {
        self.0.placed.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    fn draw_overlay(&self, _e: &[atlas::overlay::Element]) -> Result<()> {
        self.0.overlays.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    fn screen(&self) -> (i32, i32) {
        (1920, 1080)
    }
}

/// Eyes that hand back a scripted sequence, then nothing.
struct Scripted {
    frames: Vec<Option<Landmarks>>,
    at: usize,
    /// Counted so a test can prove the loop actually looked.
    looks: Arc<AtomicUsize>,
    /// How long each look pretends to take.
    costs_ms: u64,
}

impl Eyes for Scripted {
    fn look(&mut self) -> Option<Landmarks> {
        self.looks.fetch_add(1, Ordering::Relaxed);
        if self.costs_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(self.costs_ms));
        }
        let f = self.frames.get(self.at).cloned().flatten();
        self.at += 1;
        f
    }
}

fn hand_at(x: f32, y: f32, pinching: bool) -> Landmarks {
    let mut points = [Point::default(); POINTS];
    points[0] = Point::from(x, y + 0.2);
    points[9] = Point::from(x, y);
    for (i, k) in [2usize, 5, 9, 13, 17].iter().enumerate() {
        points[*k] = Point::from(x - 0.03 + i as f32 * 0.015, y + 0.02);
    }
    for (i, t) in [4usize, 8, 12, 16, 20].iter().enumerate() {
        // Fingers out, spread — unless pinching, where thumb meets index.
        points[*t] = Point::from(x - 0.03 + i as f32 * 0.015, y - 0.18);
    }
    if pinching {
        points[4] = Point::from(x, y - 0.18);
        points[8] = Point::from(x, y - 0.18);
    }
    Landmarks { points, right: Some(true), sure: 0.95 }
}

fn setup(frames: Vec<Option<Landmarks>>, looks: Arc<AtomicUsize>, noted: Arc<Noted>) -> Setup {
    Setup {
        eyes: Box::new(Scripted { frames, at: 0, looks, costs_ms: 0 }),
        pointer: Box::new(Hand(noted)),
        vocabulary: Vocabulary::default(),
        smoothing: SmoothConfig::default(),
        pace: PaceConfig::default(),
        idle_per_second: 4,
        hands: atlas::handweight::HandsConfig::default(),
    }
}

// ---------------------------------------------------------------------------
// It runs on its own, at its own rate
// ---------------------------------------------------------------------------

#[test]
fn it_keeps_looking_without_anything_driving_it() {
    // The whole point. Nothing ticks this — it runs itself.
    let looks = Arc::new(AtomicUsize::new(0));
    let noted = Arc::new(Noted::default());
    let frames = vec![Some(hand_at(0.5, 0.5, false)); 50];
    let mut t = start(setup(frames, looks.clone(), noted.clone()));

    std::thread::sleep(std::time::Duration::from_millis(250));
    t.stop();

    assert!(
        looks.load(Ordering::Relaxed) > 2,
        "looked {} times in 250ms — the daemon tick would have managed one in \
         two seconds",
        looks.load(Ordering::Relaxed)
    );
}

#[test]
fn the_pointer_moves_far_more_often_than_the_detector_runs() {
    // Prediction between readings is what makes a slow detector feel
    // immediate, and it can only happen on a loop that owns its own time.
    let looks = Arc::new(AtomicUsize::new(0));
    let noted = Arc::new(Noted::default());
    let frames: Vec<Option<Landmarks>> = (0..40)
        .map(|i| Some(hand_at(0.3 + i as f32 * 0.01, 0.5, false)))
        .collect();
    let mut s = setup(frames, looks.clone(), noted.clone());
    // A detector that costs real time, so there is a gap to fill.
    s.eyes = Box::new(Scripted {
        frames: (0..40)
            .map(|i| Some(hand_at(0.3 + i as f32 * 0.01, 0.5, false)))
            .collect(),
        at: 0,
        looks: looks.clone(),
        costs_ms: 30,
    });
    let mut t = start(s);
    std::thread::sleep(std::time::Duration::from_millis(400));
    t.stop();

    let seen = looks.load(Ordering::Relaxed);
    let moved = noted.moves.load(Ordering::Relaxed);
    assert!(seen > 0 && moved > seen, "{moved} pointer moves for {seen} looks");
}

// ---------------------------------------------------------------------------
// Stopping
// ---------------------------------------------------------------------------

#[test]
fn stopping_actually_stops_it() {
    // A tracking thread that outlives the request would keep moving the mouse
    // after Eric said stop — the single worst way this could fail.
    let looks = Arc::new(AtomicUsize::new(0));
    let noted = Arc::new(Noted::default());
    let frames = vec![Some(hand_at(0.5, 0.5, false)); 500];
    let mut t = start(setup(frames, looks.clone(), noted.clone()));

    std::thread::sleep(std::time::Duration::from_millis(150));
    t.stop();
    let after_stop = looks.load(Ordering::Relaxed);
    std::thread::sleep(std::time::Duration::from_millis(200));

    assert_eq!(
        looks.load(Ordering::Relaxed),
        after_stop,
        "it looked again after being told to stop"
    );
    assert!(!t.running());
}

#[test]
fn dropping_the_handle_stops_it_too() {
    let looks = Arc::new(AtomicUsize::new(0));
    let noted = Arc::new(Noted::default());
    let frames = vec![Some(hand_at(0.5, 0.5, false)); 500];
    {
        let _t = start(setup(frames, looks.clone(), noted.clone()));
        std::thread::sleep(std::time::Duration::from_millis(120));
    }
    let after_drop = looks.load(Ordering::Relaxed);
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert_eq!(
        looks.load(Ordering::Relaxed),
        after_drop,
        "a dropped handle must not leave a thread driving the mouse"
    );
}

#[test]
fn it_clears_its_marks_off_the_screen_when_it_stops() {
    let looks = Arc::new(AtomicUsize::new(0));
    let noted = Arc::new(Noted::default());
    let mut t = start(setup(vec![None; 20], looks, noted.clone()));
    std::thread::sleep(std::time::Duration::from_millis(80));
    t.stop();
    assert!(
        noted.overlays.load(Ordering::Relaxed) > 0,
        "an outline left on screen after stopping is a desktop that looks stuck"
    );
}

// ---------------------------------------------------------------------------
// What it reports
// ---------------------------------------------------------------------------

#[test]
fn a_bound_gesture_is_reported_by_what_it_does_not_by_its_shape() {
    let looks = Arc::new(AtomicUsize::new(0));
    let noted = Arc::new(Noted::default());
    let mut s = setup(
        vec![Some(hand_at(0.5, 0.5, true)); 30],
        looks,
        noted.clone(),
    );
    s.vocabulary = Vocabulary {
        gestures: vec![Gesture {
            name: "pinch".into(),
            does: "mute everything".into(),
            tests: vec![Test::Pinched],
            hold_ms: 0,
            two_handed: false,
            bands: Vec::new(),
        }],
    };
    let mut t = start(s);
    std::thread::sleep(std::time::Duration::from_millis(200));
    let said = t.heard();
    t.stop();

    assert!(
        said.iter().any(|s| *s == Said::Did("mute everything".into())),
        "{said:?}"
    );
}

#[test]
fn an_empty_vocabulary_reports_no_gestures_at_all() {
    let looks = Arc::new(AtomicUsize::new(0));
    let noted = Arc::new(Noted::default());
    let mut t = start(setup(vec![Some(hand_at(0.5, 0.5, true)); 30], looks, noted));
    std::thread::sleep(std::time::Duration::from_millis(150));
    let said = t.heard();
    t.stop();
    assert!(
        !said.iter().any(|s| matches!(s, Said::Did(_))),
        "nothing bound, nothing evaluated: {said:?}"
    );
}

#[test]
fn a_machine_that_cannot_keep_up_says_so_once_and_not_every_frame() {
    // A loop that complains twenty times a second about being slow is itself
    // the problem.
    let looks = Arc::new(AtomicUsize::new(0));
    let noted = Arc::new(Noted::default());
    let mut s = setup(vec![Some(hand_at(0.5, 0.5, false)); 200], looks.clone(), noted);
    s.eyes = Box::new(Scripted {
        frames: vec![Some(hand_at(0.5, 0.5, false)); 200],
        at: 0,
        looks,
        costs_ms: 120,
    });
    s.pace = PaceConfig { want_per_second: 20, floor_per_second: 6, share_of_a_core: 0.2 };
    let mut t = start(s);
    std::thread::sleep(std::time::Duration::from_millis(2500));
    let said = t.heard();
    t.stop();

    let complaints = said.iter().filter(|s| matches!(s, Said::Trouble(_))).count();
    assert!(complaints <= 1, "complained {complaints} times");
}

#[test]
fn asking_what_it_said_never_blocks() {
    let looks = Arc::new(AtomicUsize::new(0));
    let noted = Arc::new(Noted::default());
    let mut t = start(setup(vec![None; 5], looks, noted));
    // Nothing to report yet — this must return immediately rather than wait.
    let began = std::time::Instant::now();
    let _ = t.heard();
    assert!(began.elapsed().as_millis() < 50);
    t.stop();
}

#[test]
fn losing_the_hand_does_not_kill_the_loop() {
    let looks = Arc::new(AtomicUsize::new(0));
    let noted = Arc::new(Noted::default());
    let frames: Vec<Option<Landmarks>> = (0..60)
        .map(|i| {
            if i % 3 == 0 {
                None
            } else {
                Some(hand_at(0.5, 0.5, false))
            }
        })
        .collect();
    let mut t = start(setup(frames, looks.clone(), noted));
    // Until it has looked a few times past the lost hands, not a fixed
    // 250 ms: on a busy Windows laptop that wasn't enough to be scheduled
    // four times (6 Oct 2026). A loop that died stays at its count and
    // still fails, after the deadline.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while looks.load(Ordering::Relaxed) <= 3 && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    t.stop();
    assert!(looks.load(Ordering::Relaxed) > 3, "it kept going");
}

#[test]
fn two_trackers_do_not_share_anything() {
    // Nothing is shared on purpose: a mutex on the pointer path would put a
    // contended lock in the hottest loop in the program.
    let a = Arc::new(Noted::default());
    let b = Arc::new(Noted::default());
    let looks = Arc::new(AtomicUsize::new(0));
    let mut one = start(setup(vec![Some(hand_at(0.4, 0.4, false)); 40], looks.clone(), a.clone()));
    let mut two = start(setup(vec![Some(hand_at(0.6, 0.6, false)); 40], looks, b.clone()));
    std::thread::sleep(std::time::Duration::from_millis(180));
    one.stop();
    two.stop();
    assert!(a.moves.load(Ordering::Relaxed) > 0);
    assert!(b.moves.load(Ordering::Relaxed) > 0);
}

#[test]
fn the_reporting_channel_survives_a_daemon_that_never_listens() {
    // The daemon might be busy for a long time. The thread must not wedge.
    let looks = Arc::new(AtomicUsize::new(0));
    let noted = Arc::new(Noted::default());
    let mut s = setup(vec![Some(hand_at(0.5, 0.5, true)); 300], looks.clone(), noted);
    s.vocabulary = Vocabulary {
        gestures: vec![Gesture {
            name: "pinch".into(),
            does: "do a thing".into(),
            tests: vec![Test::Pinched],
            hold_ms: 0,
            two_handed: false,
            bands: Vec::new(),
        }],
    };
    let t = start(s);
    std::thread::sleep(std::time::Duration::from_millis(300));
    let kept_going = looks.load(Ordering::Relaxed);
    drop(t);
    assert!(kept_going > 2, "it kept looking with nobody reading: {kept_going}");
}

/// Guards against the loop being quietly moved back onto the daemon tick.
#[test]
fn tracking_does_not_run_on_the_daemon_tick() {
    let daemon = crate::common::source_of("daemon");
    let unused: Vec<&str> = daemon
        .lines()
        .filter(|l| l.contains("handloop::start") && !l.trim_start().starts_with("//"))
        .collect();
    // It may be started from the daemon; it must not be *driven* by it.
    for line in daemon.lines() {
        let l = line.trim();
        if l.starts_with("//") {
            continue;
        }
        assert!(
            !(l.contains("handloop::run") || l.contains("fn run(setup")),
            "the tracking loop is being called from the daemon: {l}"
        );
    }
    let _ = unused;
}

// ---------------------------------------------------------------------------
// Not running when nothing needs it (2 Oct 2026)
// ---------------------------------------------------------------------------

#[test]
fn with_no_hand_for_the_quiet_spell_it_stops_itself_and_says_so() {
    // Nothing used to stop it: once on, the camera and both models ran until
    // Eric said stop, with the room empty and the fans going.
    let looks = Arc::new(AtomicUsize::new(0));
    let noted = Arc::new(Noted::default());
    let mut s = setup(vec![None; 10_000], looks.clone(), noted);
    s.hands.stop_after_quiet_secs = 1;
    let t = start(s);
    std::thread::sleep(std::time::Duration::from_millis(1600));
    let said = t.heard();
    assert!(!t.running(), "a thread that ended is not still watching");
    assert!(
        said.iter().any(|s| matches!(s, Said::Trouble(w) if w.contains("stopped watching your hands"))),
        "{said:?}"
    );
    assert!(said.contains(&Said::HandsGone), "so the daemon leaves steering too: {said:?}");
    let after = looks.load(Ordering::Relaxed);
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert_eq!(looks.load(Ordering::Relaxed), after, "and the camera really is off");
}

#[test]
fn an_empty_room_is_looked_at_far_less_often_than_a_hand() {
    // Counted over two seconds that start after the first second and a bit,
    // once a hand has been gone long enough to count as an empty room.
    let window = |frames: Vec<Option<Landmarks>>| {
        let looks = Arc::new(AtomicUsize::new(0));
        let mut s = setup(frames, looks.clone(), Arc::new(Noted::default()));
        s.idle_per_second = 4;
        let mut t = start(s);
        std::thread::sleep(std::time::Duration::from_millis(1300));
        let from = looks.load(Ordering::Relaxed);
        std::thread::sleep(std::time::Duration::from_millis(2000));
        let to = looks.load(Ordering::Relaxed);
        t.stop();
        to - from
    };
    let with_hand = window(vec![Some(hand_at(0.5, 0.5, false)); 10_000]);
    let empty = window(vec![None; 10_000]);
    // Four a second for two seconds, with a look's slack either side.
    assert!(empty <= 10, "{empty} looks at an empty room in two seconds");
    assert!(empty * 2 < with_hand, "{empty} looks at an empty room against {with_hand} at a hand");
}
