use atlas::audio::{Device, Kind};
use atlas::hearing::{
    calibration_args, mean_volume, short, Ear, Hearing, HearingConfig, Where,
};

fn cfg() -> HearingConfig {
    HearingConfig::default()
}

/// Your desk: laptop closed on a stand behind the monitors, a webcam on top,
/// AirPods in.
fn your_setup() -> Vec<Device> {
    vec![
        Device::new("Microphone Array (Realtek(R) Audio)", Kind::Input),
        Device::new("Microphone (HD Pro Webcam C920)", Kind::Input),
        Device::new("Headset (AirPods Pro Hands-Free)", Kind::Input),
    ]
}

fn measured() -> Hearing {
    let mut h = Hearing::default();
    h.observe_devices(&your_setup());
    // The closed laptop behind a monitor barely hears anything.
    h.record_level("Microphone Array (Realtek(R) Audio)", -58.0, 100);
    // The webcam has a clear line to your face.
    h.record_level("Microphone (HD Pro Webcam C920)", -22.0, 100);
    h.record_level("Headset (AirPods Pro Hands-Free)", -18.0, 100);
    h
}

fn at_desk() -> Where {
    Where { at_desk: true, headset_connected: true, ..Default::default() }
}
fn away() -> Where {
    Where { at_desk: false, headset_connected: true, ..Default::default() }
}

// ================= it measures rather than guessing =================

#[test]
fn a_muffled_microphone_is_detected_by_listening_not_by_its_name() {
    // The closed-laptop case. Nothing in the device name says "blocked by a
    // monitor" — only a level measurement finds it.
    let h = measured();
    let deaf = h.deaf_devices(&cfg());
    assert_eq!(deaf.len(), 1);
    assert!(deaf[0].name.contains("Realtek"));
}

#[test]
fn the_webcam_wins_at_the_desk_and_the_airpods_keep_full_sound() {
    // The whole point: don't pay the Bluetooth quality cost when something
    // else can hear you perfectly well.
    let mut h = measured();
    let c = h.decide(&at_desk(), &cfg(), 200);
    assert_eq!(c.ear, Ear::Desk("Microphone (HD Pro Webcam C920)".into()));
    assert!(!c.costs_quality);
    assert!(c.why.contains("full sound"), "got: {}", c.why);
}

#[test]
fn walking_away_moves_atlas_to_your_headset() {
    let mut h = measured();
    h.decide(&at_desk(), &cfg(), 0);
    // The situation has to hold before it moves.
    h.decide(&away(), &cfg(), 10);
    let c = h.decide(&away(), &cfg(), 100);
    assert!(matches!(c.ear, Ear::Headset(_)), "got {:?}", c.ear);
    assert!(c.costs_quality, "and it admits the cost");
    assert!(c.why.contains("away from the desk"));
}

#[test]
fn coming_back_returns_to_the_webcam_and_full_quality() {
    let mut h = measured();
    h.decide(&away(), &cfg(), 0);
    h.decide(&away(), &cfg(), 100);
    h.decide(&at_desk(), &cfg(), 200);
    let c = h.decide(&at_desk(), &cfg(), 300);
    assert!(matches!(c.ear, Ear::Desk(_)));
    assert!(!c.costs_quality);
}

#[test]
fn a_flicker_in_presence_does_not_switch_ears_mid_sentence() {
    // Presence detection flickers. Switching microphone between two words is
    // worse than a slightly worse microphone.
    let mut h = measured();
    h.decide(&at_desk(), &cfg(), 0);
    let c = h.decide(&away(), &cfg(), 5);
    assert!(matches!(c.ear, Ear::Desk(_)), "must not move on one frame");
    let back = h.decide(&at_desk(), &cfg(), 8);
    assert!(matches!(back.ear, Ear::Desk(_)), "and the flicker resolves to no change");
}

#[test]
fn a_marginally_better_desk_mic_does_not_steal_the_ear_but_a_clearly_better_one_does() {
    // switch_margin is the "clearly better" half of the don't-flap rule. Two
    // desk mics that hear you about equally well must not trade the ear back
    // and forth on measurement wobble; one that is clearly better should win.
    fn two_desks(webcam_db: f32, other_db: f32) -> Hearing {
        let mut h = Hearing::default();
        h.observe_devices(&[
            Device::new("Microphone (HD Pro Webcam C920)", Kind::Input),
            Device::new("Microphone (Blue Yeti)", Kind::Input),
        ]);
        h.record_level("Microphone (HD Pro Webcam C920)", webcam_db, 100);
        h.record_level("Microphone (Blue Yeti)", other_db, 100);
        h
    }
    let desk = Where { at_desk: true, ..Default::default() };

    // Settle onto the webcam first (it is the louder of the two here).
    let mut h = two_desks(-20.0, -30.0);
    let first = h.decide(&desk, &cfg(), 0);
    assert_eq!(first.ear, Ear::Desk("Microphone (HD Pro Webcam C920)".into()));

    // The Yeti now measures a hair better -- inside the 0.15 margin. Even
    // after the settle time, Atlas stays on the webcam.
    h.record_level("Microphone (Blue Yeti)", -18.0, 200);
    h.decide(&desk, &cfg(), 200);
    let held = h.decide(&desk, &cfg(), 400);
    assert_eq!(
        held.ear,
        Ear::Desk("Microphone (HD Pro Webcam C920)".into()),
        "a marginally better desk mic must not steal the ear"
    );

    // Now the Yeti is clearly better -- well past the margin. Atlas moves.
    h.record_level("Microphone (Blue Yeti)", -2.0, 500);
    h.decide(&desk, &cfg(), 500);
    let moved = h.decide(&desk, &cfg(), 700);
    assert_eq!(
        moved.ear,
        Ear::Desk("Microphone (Blue Yeti)".into()),
        "a clearly better desk mic should win"
    );
}

#[test]
fn losing_the_current_microphone_switches_immediately() {
    // Waiting out the settle time when the device is gone would leave Atlas
    // deaf for no reason.
    let mut h = measured();
    h.decide(&at_desk(), &cfg(), 0);
    let webcam_unplugged = vec![
        Device::new("Microphone Array (Realtek(R) Audio)", Kind::Input),
        Device::new("Headset (AirPods Pro Hands-Free)", Kind::Input),
    ];
    h.observe_devices(&webcam_unplugged);
    let c = h.decide(&at_desk(), &cfg(), 1);
    // 29 Sep 2026: it switches at once -- to the laptop's own mic, not the
    // headset. A faint start-up level no longer stands in for "the lid is
    // shut": that guess put Eric on his AirPods' microphone (call-quality
    // sound for everything) with the lid open and the laptop mic working.
    // The lid is read from Windows now, and a shut lid takes the laptop mic
    // out before this is asked (`hearing::pick_microphone`; tested in
    // the_system_recovers_by_itself::at_the_desk_the_laptop_mic_beats_the_airpods_mic).
    assert_eq!(c.ear, Ear::Desk("Microphone Array (Realtek(R) Audio)".into()), "got {:?}", c.ear);
    assert!(!c.costs_quality);
}

#[test]
fn a_closed_laptop_with_no_headset_and_no_webcam_says_so() {
    // Changed 29 Sep 2026. A muffled microphone that still gives sound is
    // used, and said to be faint; "can't hear you" is kept for one that
    // gives digital silence. On Eric's laptop a quiet room on the only
    // working microphone read -51.6 dB, the old rule answered "no microphone
    // can hear you", and Atlas recorded from a device called "nothing".
    let mut h = Hearing::default();
    h.observe_devices(&[Device::new("Microphone Array (Realtek(R) Audio)", Kind::Input)]);
    h.record_level("Microphone Array (Realtek(R) Audio)", -58.0, 0);
    let c = h.decide(&at_desk(), &cfg(), 10);
    assert_eq!(c.ear, Ear::Desk("Microphone Array (Realtek(R) Audio)".into()));
    assert!(c.why.contains("faintly"), "got: {}", c.why);

    let mut silent = Hearing::default();
    silent.observe_devices(&[Device::new("Microphone Array (Realtek(R) Audio)", Kind::Input)]);
    silent.record_level("Microphone Array (Realtek(R) Audio)", -91.0, 0);
    let c = silent.decide(&at_desk(), &cfg(), 10);
    assert_eq!(c.ear, Ear::Deaf);
    assert!(c.why.contains("no microphone"), "got: {}", c.why);
}

#[test]
fn away_with_nothing_to_hear_you_points_at_your_phone() {
    let mut h = measured();
    let no_headset = Where { at_desk: false, headset_connected: false, ..Default::default() };
    let c = h.decide(&no_headset, &cfg(), 10);
    assert_eq!(c.ear, Ear::Deaf);
    assert!(c.why.contains("phone"), "got: {}", c.why);
}

#[test]
fn talking_from_your_phone_makes_the_phone_the_microphone() {
    let mut h = measured();
    let on_phone = Where { phone_active: true, ..away() };
    let c = h.decide(&on_phone, &cfg(), 10);
    assert_eq!(c.ear, Ear::Phone);
    assert!(!c.costs_quality);
}

#[test]
fn a_covered_webcam_does_not_quietly_move_atlas_to_the_headset() {
    // Presence unknown must not be read as "he left".
    let mut h = measured();
    let blind = Where { at_desk: false, presence_unknown: true, headset_connected: true, ..Default::default() };
    let c = h.decide(&blind, &cfg(), 10);
    assert!(matches!(c.ear, Ear::Desk(_)), "got {:?}", c.ear);
}

// ================= learning which one actually works =================

#[test]
fn a_microphone_that_keeps_producing_nonsense_loses_ground() {
    let mut h = measured();
    let webcam = "Microphone (HD Pro Webcam C920)";
    let before = h.candidates.iter().find(|c| c.name == webcam).unwrap().score(cfg().floor_db);
    for _ in 0..8 {
        h.record_turn(&Ear::Desk(webcam.into()), false);
    }
    let after = h.candidates.iter().find(|c| c.name == webcam).unwrap().score(cfg().floor_db);
    assert!(after < before, "repeated failures should count against it");
}

#[test]
fn a_proven_microphone_beats_an_untried_one() {
    let mut h = measured();
    let webcam = "Microphone (HD Pro Webcam C920)";
    for _ in 0..10 {
        h.record_turn(&Ear::Desk(webcam.into()), true);
    }
    let proven = h.candidates.iter().find(|c| c.name == webcam).unwrap();
    let untried = atlas::hearing::Candidate {
        name: "New Mic".into(),
        bluetooth: false,
        measured_db: None,
        good_turns: 0,
        bad_turns: 0,
    };
    assert!(proven.score(cfg().floor_db) > untried.score(cfg().floor_db));
}

#[test]
fn calibration_is_needed_for_a_device_never_measured_and_periodically_after() {
    let mut h = Hearing::default();
    h.observe_devices(&your_setup());
    assert!(h.needs_calibration(&cfg(), 0), "nothing measured yet");
    let m = measured();
    assert!(!m.needs_calibration(&cfg(), 200));
    assert!(m.needs_calibration(&cfg(), 100 + 7 * 3600), "rooms and setups change");
}

// ================= the measurement itself =================

#[test]
fn a_level_reading_is_pulled_out_of_ffmpeg_output() {
    let out = "[Parsed_volumedetect_0 @ 0x5] n_samples: 96000\n\
               [Parsed_volumedetect_0 @ 0x5] mean_volume: -21.7 dB\n\
               [Parsed_volumedetect_0 @ 0x5] max_volume: -3.2 dB";
    assert_eq!(mean_volume(out), Some(-21.7));
    assert_eq!(mean_volume("nothing useful here"), None);
}

#[test]
fn the_listening_test_names_the_device_exactly() {
    let a = calibration_args("Microphone (HD Pro Webcam C920)", 2);
    assert!(a.contains(&"audio=Microphone (HD Pro Webcam C920)".to_string()));
    assert!(a.contains(&"volumedetect".to_string()));
    assert!(a.contains(&"null".to_string()), "measures without writing a file");
}

#[test]
fn devices_are_named_the_way_you_would_say_them() {
    assert_eq!(short("Microphone (HD Pro Webcam C920)"), "the webcam");
    assert_eq!(short("Headset (AirPods Pro Hands-Free)"), "your AirPods");
    assert_eq!(short("Microphone Array (Realtek(R) Audio)"), "the laptop mic");
}

#[test]
fn unplugging_a_device_forgets_it_rather_than_keeping_a_ghost() {
    let mut h = measured();
    assert_eq!(h.candidates.len(), 3);
    h.observe_devices(&[Device::new("Microphone (HD Pro Webcam C920)", Kind::Input)]);
    assert_eq!(h.candidates.len(), 1);
}

#[test]
fn what_atlas_learned_about_a_device_survives_it_staying_plugged_in() {
    let mut h = measured();
    h.record_turn(&Ear::Desk("Microphone (HD Pro Webcam C920)".into()), true);
    h.observe_devices(&your_setup());
    let c = h.candidates.iter().find(|c| c.name.contains("Webcam")).unwrap();
    assert_eq!(c.good_turns, 1, "history is not reset by a device scan");
    assert_eq!(c.measured_db, Some(-22.0));
}
