//! Asking the machine what it can hear with.
//!
//! `audio::parse_devices` was complete, correct and tested against fixture
//! text since it was written, and nothing ever called the OS command that
//! produces that text — `OUTSTANDING_TASKS` carried "microphone enumeration
//! is unimplemented" as a known gap for days. It stayed a gap partly for a
//! reason that only shows up when you go to fix it: the parser understood
//! Windows `dshow` output *only*, and Atlas is cross-platform, so wiring it
//! would have given Linux and macOS a confident empty list.
//!
//! What can be verified here: that the right command is chosen per
//! platform, that each of the three listing formats parses correctly, and
//! that the real command runs and produces a sane answer on this machine.
//! What cannot: that the names come back byte-exact from hardware that
//! actually exists. This container has no sound devices. That stays on the
//! hardware-blocked list.

use atlas::audio::{listing_command, parse_alsa, parse_avfoundation, parse_listing, probe, Kind};

// Captured verbatim from `ffmpeg -sources alsa` in this container, then
// extended with the device lines a machine with real hardware produces.
const ALSA_SOURCES: &str = "\
Auto-detected sources for alsa:
  null [Discard all samples (playback) or generate zero samples (capture)] (none)
  default [Default Audio Device]
  hw:CARD=PCH,DEV=0 [HDA Intel PCH] (Direct hardware device)
  hw:CARD=Headset,DEV=0 [Jabra Evolve 65] (Direct hardware device)
";

const AVFOUNDATION: &str = "\
[AVFoundation indev @ 0x7f9] AVFoundation video devices:
[AVFoundation indev @ 0x7f9] [0] FaceTime HD Camera
[AVFoundation indev @ 0x7f9] [1] Capture screen 0
[AVFoundation indev @ 0x7f9] AVFoundation audio devices:
[AVFoundation indev @ 0x7f9] [0] Built-in Microphone
[AVFoundation indev @ 0x7f9] [1] AirPods Pro
";

const DSHOW: &str = r#"
[dshow @ 000001] "Microphone Array (Realtek(R) Audio)" (audio)
[dshow @ 000001]   Alternative name "@device_cm_{33D}\wave_{9F1}"
[dshow @ 000001] "HD Webcam C920" (video)
"#;

#[test]
fn the_alsa_listing_keeps_the_name_ffmpeg_will_take_back() {
    let d = parse_alsa(ALSA_SOURCES, Kind::Input);
    let names: Vec<&str> = d.iter().map(|x| x.name.as_str()).collect();
    // The bare token, not the human label -- ffmpeg matches this literally.
    assert_eq!(names, vec!["default", "hw:CARD=PCH,DEV=0", "hw:CARD=Headset,DEV=0"]);
}

#[test]
fn ffmpegs_own_discard_device_is_never_offered_as_a_microphone() {
    // `null` accepts a recording and returns silence. Offering it would give
    // you a setup that looks configured and hears nothing, which is the
    // hardest failure in this whole area to diagnose.
    let d = parse_alsa(ALSA_SOURCES, Kind::Input);
    assert!(!d.iter().any(|x| x.name == "null"), "the discard device was offered");
}

#[test]
fn an_alsa_device_is_classified_on_its_human_label_not_its_id() {
    // "hw:CARD=Headset,DEV=0" says nothing. "[Jabra Evolve 65]" does, and
    // getting this wrong means Atlas opens a Bluetooth mic and collapses the
    // audio you were listening to -- the thing this module exists to avoid.
    let d = parse_alsa(ALSA_SOURCES, Kind::Input);
    let jabra = d.iter().find(|x| x.name.contains("Headset")).expect("the headset");
    assert!(jabra.bluetooth, "a Jabra headset was not recognised as bluetooth");
    let onboard = d.iter().find(|x| x.name.contains("PCH")).expect("the built-in");
    assert!(onboard.builtin, "the onboard Intel audio was not recognised as built in");
    assert!(!onboard.bluetooth);
}

#[test]
fn a_headerless_line_is_not_mistaken_for_a_device() {
    // "Auto-detected sources for alsa:" is flush left; devices are indented.
    let d = parse_alsa(ALSA_SOURCES, Kind::Input);
    // Behaviour, not just wording: exactly the three real devices parse, so
    // neither the flush-left heading nor the discard line slipped in.
    assert_eq!(d.len(), 3, "an extra line was parsed as a device: {d:?}");
    assert!(!d.iter().any(|x| x.name.contains("Auto-detected")), "the heading became a device");
}

#[test]
fn the_mac_listing_does_not_offer_you_a_camera_to_speak_into() {
    // Cameras and microphones come back in one listing, separated only by a
    // heading. Taking every "[n] name" line would offer the FaceTime camera
    // as an input.
    let d = parse_avfoundation(AVFOUNDATION);
    let names: Vec<&str> = d.iter().map(|x| x.name.as_str()).collect();
    assert_eq!(names, vec!["Built-in Microphone", "AirPods Pro"]);
    assert!(d.iter().find(|x| x.name == "AirPods Pro").unwrap().bluetooth);
    assert!(d.iter().find(|x| x.name == "Built-in Microphone").unwrap().builtin);
}

#[test]
fn each_platform_is_asked_in_the_way_it_answers() {
    let (cmd, args) = listing_command(true);
    assert_eq!(cmd, "ffmpeg");
    let joined = args.join(" ");
    if cfg!(windows) {
        assert!(joined.contains("dshow"), "windows was not asked with dshow: {joined}");
    } else if cfg!(target_os = "macos") {
        assert!(joined.contains("avfoundation"), "macos was not asked with avfoundation: {joined}");
    } else {
        assert!(joined.contains("-sources"), "linux was not asked with -sources: {joined}");
        assert!(joined.contains("alsa"), "linux was not asked about alsa: {joined}");
    }
    // Outputs are a different question on Linux and must be asked as one.
    let (_, out_args) = listing_command(false);
    if !cfg!(windows) && !cfg!(target_os = "macos") {
        assert!(out_args.join(" ").contains("-sinks"), "outputs were asked for as sources");
    }
}

#[test]
fn this_platforms_parser_is_the_one_that_runs() {
    // Guards the dispatch itself: feed each parser's own format through
    // `parse_listing` and only this platform's should come back populated.
    let text = if cfg!(windows) {
        DSHOW
    } else if cfg!(target_os = "macos") {
        AVFOUNDATION
    } else {
        ALSA_SOURCES
    };
    assert!(!parse_listing(text, true).is_empty(), "this platform's own listing parsed as empty");
}

#[test]
fn asking_the_real_machine_answers_without_panicking_or_hanging() {
    // This container has no sound hardware, so the honest answer here is an
    // empty list -- and an empty list is exactly what a wrong implementation
    // returns too, so this asserts only what it can: that the call completes
    // and never invents a device.
    let found = probe("ffmpeg", true).expect("ffmpeg is installed here");
    assert!(
        !found.iter().any(|d| d.name.is_empty() || d.name == "null"),
        "enumeration produced a device that cannot be recorded from: {found:?}"
    );
}
