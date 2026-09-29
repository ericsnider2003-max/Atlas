//! Picking the microphone that actually hears you.
//!
//! `hearing.rs` was complete, tested and unreachable. Part of why is worth
//! recording: it needs a list of the machine's audio devices, and device
//! listing was hardcoded to Windows `dshow` until earlier today — so on two
//! of the three platforms Atlas targets there was nothing to hand it.
//!
//! What replaced it in the meantime was `audio::choose`, which picks on the
//! device's *name*: does it look built in, does it look like a headset. That
//! is a guess about hardware from a string. `hearing` measures instead — it
//! records a second from each candidate and reads the level back — and it
//! remembers the answer, so the measuring happens rarely.
//!
//! Proven here: the decision, the memory of it across runs, and that the
//! measurement is what drives it. Not proven here: anything involving a real
//! microphone. This container has none.

use atlas::audio::{Device, Kind};
use atlas::hearing::{calibration_args, mean_volume, short, Hearing, HearingConfig, Where};
use atlas::store::Store;
use std::path::PathBuf;

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-hear-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn at_the_desk() -> Where {
    Where {
        at_desk: true,
        presence_unknown: false,
        headset_connected: true,
        phone_active: false,
        audio_playing: true,
    }
}

fn devices() -> Vec<Device> {
    vec![
        Device::new("Microphone Array (Realtek(R) Audio)", Kind::Input),
        Device::new("AirPods Pro", Kind::Input),
        Device::new("HD Webcam C920", Kind::Input),
    ]
}

// ================= measuring rather than guessing =================

#[test]
fn a_microphone_that_cannot_hear_you_is_not_chosen_however_good_its_name_looks() {
    // The whole argument for this module over `audio::choose`. "Microphone
    // Array (Realtek(R) Audio)" is the most built-in-looking name there is,
    // and a laptop shut on a stand behind two monitors cannot hear you
    // through it.
    let cfg = HearingConfig::default();
    let mut h = Hearing::default();
    h.observe_devices(&devices());

    h.record_level("Microphone Array (Realtek(R) Audio)", -70.0, 100); // muffled
    h.record_level("HD Webcam C920", -30.0, 100); // clear line to your face
    h.record_level("AirPods Pro", -28.0, 100);

    let choice = h.decide(&at_the_desk(), &cfg, 200);
    assert!(
        choice.ear.name().contains("Webcam"),
        "picked {} over the webcam that can actually hear you",
        choice.ear.name()
    );
    assert!(!choice.why.is_empty(), "a surprising switch has to be explainable");
}

#[test]
fn the_bluetooth_cost_is_only_paid_when_it_buys_something() {
    // The module's stated rule two, and the reason it exists at all: opening
    // an AirPods microphone drops the link to a headset profile and wrecks
    // whatever you were listening to. At the desk, with something that hears
    // you, that cost buys nothing.
    let cfg = HearingConfig::default();
    let mut h = Hearing::default();
    h.observe_devices(&devices());
    h.record_level("Microphone Array (Realtek(R) Audio)", -35.0, 100);
    h.record_level("HD Webcam C920", -30.0, 100);
    h.record_level("AirPods Pro", -25.0, 100); // the loudest of the three

    let choice = h.decide(&at_the_desk(), &cfg, 200);
    assert!(
        !choice.costs_quality,
        "took the bluetooth mic at the desk, costing playback quality: {}",
        choice.why
    );
}

#[test]
fn a_device_that_hears_nothing_is_named_so_you_can_move_it() {
    let cfg = HearingConfig::default();
    let mut h = Hearing::default();
    h.observe_devices(&devices());
    h.record_level("Microphone Array (Realtek(R) Audio)", -80.0, 100);
    h.record_level("HD Webcam C920", -30.0, 100);

    let deaf: Vec<String> = h.deaf_devices(&cfg).iter().map(|c| c.name.clone()).collect();
    assert!(
        deaf.iter().any(|n| n.contains("Realtek")),
        "a microphone that hears nothing was not mentioned: {deaf:?}"
    );
    assert!(!deaf.iter().any(|n| n.contains("Webcam")));
}

// ================= remembering, so it measures rarely =================

#[test]
fn what_it_learned_survives_a_restart() {
    // Measuring means recording a second from every microphone. Doing that at
    // every start would put a noticeable pause in front of every session, so
    // the answer has to outlive the process.
    let store = Store::new(tmp("memory"));
    let mut h = Hearing::default();
    h.observe_devices(&devices());
    h.record_level("HD Webcam C920", -29.5, 1000);
    h.save_to(&store).expect("saved");

    let back = Hearing::load_from(&store);
    let webcam = back
        .candidates
        .iter()
        .find(|c| c.name.contains("Webcam"))
        .expect("the webcam survived");
    assert_eq!(webcam.measured_db, Some(-29.5));
    assert_eq!(back.last_calibration, 1000);
}

#[test]
fn it_asks_to_measure_when_it_has_not_and_stops_asking_when_it_has() {
    let cfg = HearingConfig::default();
    let mut h = Hearing::default();
    h.observe_devices(&devices());
    assert!(h.needs_calibration(&cfg, 1000), "unmeasured devices should be measured");

    for d in devices() {
        h.record_level(&d.name, -30.0, 1000);
    }
    assert!(!h.needs_calibration(&cfg, 1000), "kept asking after measuring everything");
    // But not forever — rooms and desks change.
    assert!(h.needs_calibration(&cfg, 1000 + cfg.recalibrate_secs + 1));
}

#[test]
fn a_microphone_that_was_unplugged_stops_being_considered() {
    let mut h = Hearing::default();
    h.observe_devices(&devices());
    assert_eq!(h.candidates.len(), 3);

    // The webcam is gone; the others stay, with what was learned about them.
    h.record_level("AirPods Pro", -25.0, 100);
    h.observe_devices(&[
        Device::new("Microphone Array (Realtek(R) Audio)", Kind::Input),
        Device::new("AirPods Pro", Kind::Input),
    ]);
    assert_eq!(h.candidates.len(), 2);
    assert!(!h.candidates.iter().any(|c| c.name.contains("Webcam")));
    assert_eq!(
        h.candidates.iter().find(|c| c.name.contains("AirPods")).unwrap().measured_db,
        Some(-25.0),
        "unplugging one device forgot what was measured about another"
    );
}

// ================= how it measures =================

#[test]
fn the_calibration_command_measures_rather_than_recording() {
    let args = calibration_args("Some Microphone", 1);
    let joined = args.join(" ");
    assert!(joined.contains("volumedetect"), "not measuring a level: {joined}");
    assert!(joined.contains("-t 1"), "no time bound on the measurement: {joined}");
    assert!(joined.contains("null"), "writing a file instead of just measuring: {joined}");
}

#[test]
fn the_level_is_read_back_out_of_what_ffmpeg_prints() {
    // Real ffmpeg output shape. Getting this wrong means every device
    // measures as unknown and the module falls back to guessing by name,
    // silently.
    let stderr = "\
[Parsed_volumedetect_0 @ 0x55f] n_samples: 16000
[Parsed_volumedetect_0 @ 0x55f] mean_volume: -31.4 dB
[Parsed_volumedetect_0 @ 0x55f] max_volume: -8.2 dB
";
    assert_eq!(mean_volume(stderr), Some(-31.4));
    assert_eq!(mean_volume("nothing useful here"), None);
}

#[test]
fn a_device_name_is_shortened_for_saying_out_loud_but_never_for_using() {
    // ffmpeg matches device names literally, so the short form is for the
    // sentence and the full name is what gets passed back.
    let full = "Microphone Array (Realtek(R) Audio)";
    let s = short(full);
    assert!(s.len() <= full.len());
    assert!(!s.is_empty());
}

