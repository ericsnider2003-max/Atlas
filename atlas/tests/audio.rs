use atlas::audio::{announce, changed, choose, parse_devices, AudioConfig, Device, Kind};

const FFMPEG_OUTPUT: &str = r#"
[dshow @ 000001] "Integrated Camera" (video)
[dshow @ 000001]   Alternative name "@device_pnp_\\?\usb#vid_04f2"
[dshow @ 000001] "Microphone Array (Realtek(R) Audio)" (audio)
[dshow @ 000001]   Alternative name "@device_cm_{33D9A762}"
[dshow @ 000001] "Headset (AirPods Pro Hands-Free)" (audio)
[dshow @ 000001]   Alternative name "@device_cm_{33D9A763}"
"#;

fn devices() -> Vec<Device> {
    vec![
        Device::new("Microphone Array (Realtek(R) Audio)", Kind::Input),
        Device::new("Headset (AirPods Pro Hands-Free)", Kind::Input),
        Device::new("Speakers (Realtek(R) Audio)", Kind::Output),
        Device::new("Headphones (AirPods Pro Stereo)", Kind::Output),
    ]
}

/// The webcam mic: external, not built in, not Bluetooth. Reachable whether
/// or not the laptop's own screen is active.
fn webcam_mic() -> Device {
    Device::new("Microphone (HD Pro Webcam C920)", Kind::Input)
}

#[test]
fn device_names_come_out_of_ffmpeg_exactly_as_written() {
    // ffmpeg matches these literally. One wrong character means silence
    // rather than an error, which is the worst kind of failure to debug.
    let d = parse_devices(FFMPEG_OUTPUT);
    let names: Vec<&str> = d.iter().map(|x| x.name.as_str()).collect();
    assert_eq!(names, vec!["Microphone Array (Realtek(R) Audio)", "Headset (AirPods Pro Hands-Free)"]);
}

#[test]
fn cameras_and_internal_ids_are_not_mistaken_for_microphones() {
    let d = parse_devices(FFMPEG_OUTPUT);
    assert!(!d.iter().any(|x| x.name.contains("Camera")));
    assert!(!d.iter().any(|x| x.name.contains("@device")));
}

#[test]
fn airpods_and_the_built_in_mic_are_told_apart() {
    let d = devices();
    assert!(d[1].bluetooth, "AirPods");
    assert!(!d[0].bluetooth);
    assert!(d[0].builtin, "Realtek array is built in");
}

#[test]
fn a_webcam_mic_is_neither_bluetooth_nor_builtin() {
    let w = webcam_mic();
    assert!(!w.bluetooth);
    assert!(!w.builtin, "a plugged-in webcam is not the machine's own array");
}

#[test]
fn atlas_listens_on_the_laptop_mic_so_your_headphones_keep_full_quality() {
    // The whole reason this module exists. Opening a Bluetooth mic collapses
    // the connection to a headset profile and everything you're listening to
    // turns muddy — for as long as Atlas is listening, which with a wake word
    // is all day.
    let s = choose(&devices(), &AudioConfig::default(), true);
    assert_eq!(s.input.as_deref(), Some("Microphone Array (Realtek(R) Audio)"));
    assert_eq!(s.output.as_deref(), Some("Headphones (AirPods Pro Stereo)"));
    assert!(!s.degrades_audio);
    assert!(s.why.contains("full sound quality"), "got: {}", s.why);
}

#[test]
fn with_only_a_headset_it_uses_it_and_warns_you_what_that_costs() {
    let only_airpods = vec![
        Device::new("Headset (AirPods Pro Hands-Free)", Kind::Input),
        Device::new("Headphones (AirPods Pro Stereo)", Kind::Output),
    ];
    let s = choose(&only_airpods, &AudioConfig::default(), true);
    assert_eq!(s.input.as_deref(), Some("Headset (AirPods Pro Hands-Free)"));
    assert!(s.degrades_audio);
    assert!(s.why.contains("drop your audio quality"), "got: {}", s.why);
}

#[test]
fn you_can_insist_on_the_headset_mic_for_walking_around() {
    // The only reason to turn the guard off is that you want the headset mic
    // — you're away from the desk and the laptop can't hear you. So off means
    // "use it", not "stop worrying about it".
    let cfg = AudioConfig { avoid_bluetooth_mic: false, ..Default::default() };
    let s = choose(&devices(), &cfg, true);
    assert!(s.input.as_deref().unwrap().contains("AirPods"));
    assert!(s.degrades_audio, "and it still says what that costs");

    // Naming a device explicitly always wins.
    let named = AudioConfig {
        preferred_input: vec!["AirPods".into()],
        ..Default::default()
    };
    let s2 = choose(&devices(), &named, true);
    assert!(s2.input.as_deref().unwrap().contains("AirPods"));
    assert!(s2.degrades_audio, "and it still tells you the cost");
    assert_eq!(s2.why, "you named these");
}

#[test]
fn output_goes_to_headphones_by_default_but_can_be_kept_on_speakers() {
    let s = choose(&devices(), &AudioConfig::default(), true);
    assert!(s.output.as_deref().unwrap().contains("AirPods"));

    let cfg = AudioConfig { prefer_headphones_for_output: false, ..Default::default() };
    let s2 = choose(&devices(), &cfg, true);
    assert!(s2.output.as_deref().unwrap().contains("Realtek"), "replies stay on the speakers");
}

#[test]
fn no_microphone_at_all_is_a_clear_answer_not_a_crash() {
    let s = choose(&[Device::new("Speakers", Kind::Output)], &AudioConfig::default(), true);
    assert!(s.input.is_none());
    assert!(s.why.contains("no microphone"));
}

#[test]
fn headphones_connecting_mid_session_is_noticed() {
    let before = vec![Device::new("Microphone Array (Realtek(R) Audio)", Kind::Input)];
    let after = devices();
    assert!(changed(&before, &after));
    assert!(!changed(&after, &after));
}

#[test]
fn atlas_says_something_useful_when_the_audio_changes_under_it() {
    let with_mic = choose(&devices(), &AudioConfig::default(), true);
    let none = choose(&[], &AudioConfig::default(), true);
    assert_eq!(announce(&with_mic, &none).as_deref(), Some("I've lost the microphone."));
    assert_eq!(announce(&none, &with_mic).as_deref(), Some("Microphone's back."));
    assert!(announce(&with_mic, &with_mic).is_none(), "nothing changed, nothing said");

    let airpods_only = choose(
        &[Device::new("Headset (AirPods Pro Hands-Free)", Kind::Input)],
        &AudioConfig::default(),
        true,
    );
    let said = announce(&with_mic, &airpods_only).unwrap();
    assert!(said.contains("quality will drop"), "got: {said}");
}

// ========================= the closed-lid case =========================
//
// This is the case the module did not handle until tonight: a laptop
// docked to external monitors, lid shut, the built-in array still showing
// up in Windows' device list because dshow does not know the lid state —
// only Atlas's own monitor layout does.

#[test]
fn with_the_lid_down_the_built_in_mic_is_not_offered_even_though_windows_still_lists_it() {
    let d = devices(); // includes the Realtek array, as Windows would still report it
    let s = choose(&d, &AudioConfig::default(), false);
    assert_ne!(
        s.input.as_deref(),
        Some("Microphone Array (Realtek(R) Audio)"),
        "a mic sealed inside a shut lid is not a fallback worth offering"
    );
}

#[test]
fn with_the_lid_down_a_reachable_wired_mic_beats_the_bluetooth_one() {
    let mut d = devices();
    d.push(webcam_mic());
    let s = choose(&d, &AudioConfig::default(), false);
    assert_eq!(s.input.as_deref(), Some("Microphone (HD Pro Webcam C920)"));
    assert!(!s.degrades_audio, "a reachable wired mic should not cost audio quality");
    assert!(s.why.contains("lid down"), "got: {}", s.why);
}

#[test]
fn with_the_lid_down_and_nothing_else_the_headset_is_used_and_the_reason_is_the_lid_not_a_guess() {
    let d = devices(); // only the (excluded) built-in and the AirPods
    let s = choose(&d, &AudioConfig::default(), false);
    assert_eq!(s.input.as_deref(), Some("Headset (AirPods Pro Hands-Free)"));
    assert!(s.degrades_audio);
    assert!(s.why.contains("lid down"), "the reason should name the actual cause: {}", s.why);
}

#[test]
fn with_the_lid_up_the_built_in_mic_is_offered_exactly_as_before() {
    // Confirms the new signal changes nothing for the ordinary case — the
    // whole point of this module before tonight was to prefer the built-in
    // mic, and that has to keep working for anyone whose lid is open.
    let s = choose(&devices(), &AudioConfig::default(), true);
    assert_eq!(s.input.as_deref(), Some("Microphone Array (Realtek(R) Audio)"));
}

#[test]
fn a_lid_state_that_cannot_be_read_defaults_to_trusting_the_built_in_mic() {
    // Guessing the lid is shut on no evidence would silently take away a
    // working microphone. Guessing it is open, at worst, offers a mic that
    // turns out to be unreachable — a louder, more honest failure.
    let s_open_guess = choose(&devices(), &AudioConfig::default(), true);
    assert_eq!(s_open_guess.input.as_deref(), Some("Microphone Array (Realtek(R) Audio)"));
}

#[test]
fn only_input_devices_are_excluded_by_lid_state_not_the_reasoning_itself() {
    // The exclusion is a pre-filter, not a special case bolted onto every
    // branch — with nothing built-in in the list at all, lid state should
    // not change the outcome.
    let no_builtin = vec![
        webcam_mic(),
        Device::new("Headset (AirPods Pro Hands-Free)", Kind::Input),
    ];
    let open = choose(&no_builtin, &AudioConfig::default(), true);
    let closed = choose(&no_builtin, &AudioConfig::default(), false);
    assert_eq!(open.input, closed.input);
}
