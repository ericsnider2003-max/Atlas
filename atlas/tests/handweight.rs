//! How heavy hand tracking is allowed to be (2 Oct 2026).
//!
//! Eric: "may need a lighter model for gestures" -- the laptop's fans spun
//! while Atlas watched his hands. The models were already MediaPipe's lite
//! ones; what was heavy was the engine they ran in, running both of them on
//! every look, looking at an empty room as hard as at a hand, never
//! stopping, and giving every machine the same settings. Everything that
//! decides those is arithmetic, so it is tested here with numbers: no
//! camera, no model.
//!
//! The last test runs the real hand models through ONNX Runtime and `tract`
//! and checks they agree, when `ATLAS_ORT_TEST_ROOT` holds an install-shaped
//! folder (`tools/kokoro/libonnxruntime.so` or the .dll, and
//! `models/hand_presence.onnx` / `models/hand_landmarks.onnx`).

use atlas::handshape::{Landmarks, Point, POINTS};
use atlas::handtrack::PaceConfig;
use atlas::handweight::{self, HandsConfig, Machine, Step, Weight};
use atlas::vision::Patch;

fn roomy() -> Machine {
    Machine { cores: 8, ram_gb: 16.0, on_battery: false }
}

fn auto() -> HandsConfig {
    HandsConfig::default()
}

// ---------------------------------------------------------------- the plan

#[test]
fn a_roomy_machine_on_mains_gets_the_full_plan() {
    let p = handweight::plan(&auto(), &roomy(), None, &PaceConfig::default());
    assert!(!p.light, "{}", p.why);
    assert_eq!((p.width, p.height), (640, 480));
    assert_eq!(p.pace.want_per_second, PaceConfig::default().want_per_second);
    assert!(p.why.contains("room"), "{}", p.why);
}

#[test]
fn a_small_or_unplugged_machine_gets_the_light_plan_and_says_why() {
    let base = PaceConfig::default();
    for (m, said) in [
        (Machine { cores: 4, ..roomy() }, "4 processors"),
        (Machine { ram_gb: 4.0, ..roomy() }, "4 GB"),
        (Machine { on_battery: true, ..roomy() }, "battery"),
    ] {
        let p = handweight::plan(&auto(), &m, None, &base);
        assert!(p.light, "{m:?}");
        assert!(p.why.contains(said), "{m:?}: {}", p.why);
        assert_eq!((p.width, p.height), handweight::LIGHT_PICTURE);
        assert!(p.pace.want_per_second <= handweight::LIGHT_MOST_PER_SECOND);
        assert!(p.pace.share_of_a_core < base.share_of_a_core, "lighter on the core too");
        assert!(p.pace.floor_per_second <= p.pace.want_per_second);
        assert!(p.idle_per_second < handweight::plan(&auto(), &roomy(), None, &base).idle_per_second);
    }
}

#[test]
fn memory_that_could_not_be_read_is_not_counted_as_small() {
    let p = handweight::plan(&auto(), &Machine { ram_gb: 0.0, ..roomy() }, None, &PaceConfig::default());
    assert!(!p.light, "{}", p.why);
}

#[test]
fn what_a_look_cost_last_time_decides_the_next_start() {
    let base = PaceConfig::default(); // a fifth of a core, floor of 6 a second
    // 40 ms a look at a fifth of a core is 5 a second: under the floor.
    assert!(handweight::too_slow_for_full(40, &base));
    // 10 ms a look is 20 a second: fine.
    assert!(!handweight::too_slow_for_full(10, &base));
    assert!(!handweight::too_slow_for_full(0, &base), "nothing measured is not slow");

    let p = handweight::plan(&auto(), &roomy(), Some(163), &base);
    assert!(p.light && p.why.contains("163 ms"), "{}", p.why);
    assert!(!handweight::plan(&auto(), &roomy(), Some(8), &base).light);
}

#[test]
fn the_setting_overrules_the_machine_both_ways() {
    let base = PaceConfig::default();
    let small = Machine { cores: 2, ram_gb: 4.0, on_battery: true };
    let full = HandsConfig { weight: Weight::Full, ..auto() };
    let light = HandsConfig { weight: Weight::Light, ..auto() };
    assert!(!handweight::plan(&full, &small, Some(500), &base).light);
    assert!(handweight::plan(&light, &roomy(), None, &base).light);
    assert!(handweight::plan(&light, &roomy(), None, &base).why.contains("you set it"));
}

#[test]
fn the_setting_reads_and_writes_as_one_plain_word() {
    for w in Weight::WORDS {
        let parsed: Weight = serde_yaml::from_str(w).unwrap();
        assert_eq!(parsed.word(), w);
    }
    let cfg: HandsConfig = serde_yaml::from_str("weight: light").unwrap();
    assert_eq!(cfg.weight, Weight::Light);
    assert!(cfg.npu, "the NPU stays on unless switched off");
    assert!(serde_yaml::from_str::<Weight>("heavy").is_err());
    // Shipped config and an install with none agree: automatic.
    let shipped: atlas::voice::ToolsConfig =
        serde_yaml::from_str(&std::fs::read_to_string("config/tools.yaml").unwrap()).unwrap();
    assert_eq!(shipped.hands.weight, Weight::Auto);
}

#[test]
fn the_hub_can_change_it() {
    let shipped: atlas::voice::ToolsConfig =
        serde_yaml::from_str(&std::fs::read_to_string("config/tools.yaml").unwrap()).unwrap();
    let mut s = atlas::settings::registry(&shipped);
    assert!(s.get("hands.weight").is_some() && s.get("hands.npu").is_some());
    assert!(s.set("hands.weight", "light").is_ok());
    assert!(s.set("hands.weight", "heavy").is_err(), "only its own words");
}

// ---------------------------------------------------- did anything move?

fn picture(w: usize, h: usize, shade: impl Fn(usize, usize) -> u8) -> Vec<u8> {
    let mut v = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            let s = shade(x, y);
            v.extend_from_slice(&[s, s, s]);
        }
    }
    v
}

#[test]
fn a_still_room_has_not_moved_and_a_hand_coming_in_has() {
    let (w, h) = (640, 480);
    let room = picture(w, h, |x, y| ((x / 40 + y / 40) % 2 * 120 + 40) as u8);
    // The same room with a flicker of camera noise.
    let noisy = picture(w, h, |x, y| ((x / 40 + y / 40) % 2 * 120 + 40 + (x + y) % 5) as u8);
    // And with a bright hand-sized block in the middle.
    let hand = picture(w, h, |x, y| {
        if (260..380).contains(&x) && (180..330).contains(&y) {
            250
        } else {
            ((x / 40 + y / 40) % 2 * 120 + 40) as u8
        }
    });
    let before = handweight::thumbnail(&room, w, h);
    assert_eq!(before.len(), handweight::GRID.0 * handweight::GRID.1);
    assert!(handweight::moved(&before, &handweight::thumbnail(&noisy, w, h)) < handweight::MOVED_SHARE);
    assert!(handweight::moved(&before, &handweight::thumbnail(&hand, w, h)) >= handweight::MOVED_SHARE);
    // Nothing to compare with yet: counts as moved, so the first look is real.
    assert_eq!(handweight::moved(&[], &before), 1.0);
}

#[test]
fn a_frame_too_small_or_short_does_not_panic() {
    assert!(handweight::thumbnail(&[0u8; 10], 640, 480).iter().all(|v| *v == 0));
    assert!(handweight::thumbnail(&[], 0, 0).iter().all(|v| *v == 0));
}

// ------------------------------------------------- find, follow, or skip

#[test]
fn a_hand_being_followed_is_never_looked_for_again() {
    let at = Patch::new(0.4, 0.4, 0.2, 0.2);
    assert_eq!(handweight::step(Some(at), 0.0, 0), Step::Follow(at));
    assert_eq!(handweight::step(Some(at), 1.0, 99_999), Step::Follow(at));
}

#[test]
fn with_no_hand_the_finder_runs_only_on_movement_or_now_and_then() {
    assert_eq!(handweight::step(None, 0.0, 100), Step::Skip, "nothing moved, looked recently");
    assert_eq!(handweight::step(None, 0.5, 100), Step::Find, "something moved");
    assert_eq!(handweight::step(None, 0.0, handweight::LOOK_ANYWAY_MS), Step::Find, "a still hand is still a hand");
    assert_eq!(handweight::step(None, 0.0, u32::MAX), Step::Find, "never looked");
}

fn hand_box(x0: f32, y0: f32, x1: f32, y1: f32, sure: f32) -> Landmarks {
    let mut points = [Point::default(); POINTS];
    for (i, p) in points.iter_mut().enumerate() {
        let t = i as f32 / (POINTS - 1) as f32;
        *p = Point::from(x0 + (x1 - x0) * t, y0 + (y1 - y0) * (1.0 - t));
    }
    Landmarks { points, right: None, sure }
}

#[test]
fn the_next_look_reads_a_square_a_little_bigger_than_the_hand() {
    let (w, h) = (640, 480);
    // A hand 64 px wide and 96 px tall in the middle.
    let marks = hand_box(0.45, 0.4, 0.55, 0.6, 0.9);
    let b = handweight::follow_box(&marks, w, h).expect("a box");
    let (bw, bh) = (b.width * w as f32, b.height * h as f32);
    assert!((bw - bh).abs() < 1.0, "square in pixels: {bw} by {bh}");
    let want = 96.0 * (1.0 + 2.0 * handweight::FOLLOW_MARGIN);
    assert!((bw - want).abs() < 1.0, "{bw} against {want}");
    assert!(b.holds(0.5, 0.5));
}

#[test]
fn a_box_at_the_edge_is_kept_inside_the_picture() {
    let marks = hand_box(0.0, 0.0, 0.1, 0.15, 0.9);
    let b = handweight::follow_box(&marks, 640, 480).unwrap();
    assert!(b.x >= 0.0 && b.y >= 0.0 && b.x + b.width <= 1.0 + 1e-6 && b.y + b.height <= 1.0 + 1e-6, "{b:?}");
}

#[test]
fn an_unsure_tiny_or_impossible_hand_is_not_followed() {
    assert!(handweight::follow_box(&hand_box(0.4, 0.4, 0.6, 0.6, 0.2), 640, 480).is_none(), "unsure");
    assert!(handweight::follow_box(&hand_box(0.5, 0.5, 0.501, 0.501, 0.9), 640, 480).is_none(), "a few pixels");
    assert!(handweight::follow_box(&hand_box(1.5, 1.5, 1.7, 1.7, 0.9), 640, 480).is_none(), "off the picture");
    assert!(handweight::follow_box(&hand_box(0.4, 0.4, 0.6, 0.6, 0.9), 0, 0).is_none());
}

// ------------------------------------------------ how long, and when to stop

#[test]
fn an_empty_room_is_looked_at_a_few_times_a_second_not_twenty() {
    // A hand in view, or only just gone: the pace's own wait.
    assert_eq!(handweight::wait_ms(50, 0, 4), 50);
    assert_eq!(handweight::wait_ms(50, handweight::QUIET_AFTER_MS - 1, 4), 50);
    // Gone a while: the idle rate.
    assert_eq!(handweight::wait_ms(50, 5000, 4), 250);
    assert_eq!(handweight::wait_ms(50, 5000, 2), 500);
    // A pace already slower than idle is left alone.
    assert_eq!(handweight::wait_ms(800, 5000, 4), 800);
    assert_eq!(handweight::wait_ms(50, 5000, 0), 1000, "no divide by nought");
}

#[test]
fn it_stops_itself_after_the_quiet_spell_and_never_when_told_not_to() {
    let cfg = HandsConfig { stop_after_quiet_secs: 120, ..auto() };
    assert!(!handweight::time_to_stop(119_999, &cfg));
    assert!(handweight::time_to_stop(120_000, &cfg));
    let never = HandsConfig { stop_after_quiet_secs: 0, ..auto() };
    assert!(!handweight::time_to_stop(u32::MAX, &never));
    assert!(auto().stop_after_quiet_secs > 0, "on by default: a camera left on is a light left on");
}

// ------------------------------------------- the real models, both engines

/// ONNX Runtime gives `tract`'s answers for both hand models, on the
/// processor, one quiet thread -- the route the hands take when the runtime
/// is here and there's no NPU. Skipped without `ATLAS_ORT_TEST_ROOT`.
#[test]
fn onnx_runtime_reads_hands_the_way_tract_does() {
    let Ok(root) = std::env::var("ATLAS_ORT_TEST_ROOT") else { return };
    let root = std::path::PathBuf::from(root);
    let models = root.join("models");
    for kind in atlas::infer::Kind::for_hands() {
        let file = models.join(kind.file());
        let r = kind.recipe();
        let shape: Vec<i64> = r.shape().iter().map(|d| *d as i64).collect();
        let n: usize = r.values();
        // A smooth made-up picture in the model's own 0-to-1 range.
        let pixels: Vec<f32> = (0..n).map(|i| ((i * 37 % 251) as f32) / 251.0).collect();

        let names = atlas::npu::Session::input_names(&root, &file).expect("the runtime opens it");
        let s = atlas::npu::Session::open_with(&root, &file, &[(names[0].clone(), shape.clone())], atlas::npu::Where::Cpu, true)
            .expect("opens on the processor, one thread");
        let began = std::time::Instant::now();
        let ort = s.run(vec![atlas::npu::In::F32(names[0].clone(), shape, pixels.clone())]).expect("runs");
        let ort_ms = began.elapsed().as_millis();

        let mut t = atlas::infer::Model::load(kind, &models).expect("tract opens it");
        let began = std::time::Instant::now();
        let tract = t.run(&pixels).expect("tract runs");
        let tract_ms = began.elapsed().as_millis();

        assert_eq!(ort.len(), tract.count(), "{}: the same number of results", kind.file());
        for (i, a) in ort.iter().enumerate() {
            let b = tract.at(i).unwrap();
            assert_eq!(a.len(), b.len(), "{} result {i}", kind.file());
            let worst = a.iter().zip(b).map(|(x, y)| (x - y).abs() / (1.0 + y.abs())).fold(0.0f32, f32::max);
            assert!(worst < 1e-3, "{} result {i}: the engines differ by {worst}", kind.file());
        }
        eprintln!("{}: ONNX Runtime {ort_ms} ms, tract {tract_ms} ms (first run each)", kind.file());
    }
}
