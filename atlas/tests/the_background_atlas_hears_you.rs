//! 29 Sep 2026, Eric's laptop: the background Atlas (what setup starts)
//! recorded from the microphone named in the shipped tools.yaml,
//! "Microphone Array (Realtek(R) Audio)". His laptop's microphone is Intel
//! Smart Sound. ffmpeg: "Could not find audio only device". Every wake-word
//! clip failed, Atlas dropped to push-to-talk, and the held key recorded
//! nothing and said nothing. Only `atlas --voice` picked the real microphone.

fn source(path: &str) -> String {
    // Through `read_source_path` (29 Sep 2026): daemon.rs and main.rs were
    // split into src/daemon/*.rs and src/main/*.rs, read here as one.
    crate::common::read_source_path(path).unwrap_or_else(|| panic!("{path}: not found"))
}

/// A method's body inside an impl: from its signature to the first line
/// closing at its indent.
fn method_body(src: &str, signature: &str) -> String {
    let at = src.find(signature).unwrap_or_else(|| panic!("{signature} not found"));
    let rest = &src[at..];
    let end = rest.find("\n    }\n").map(|i| i + 6).unwrap_or(rest.len());
    rest[..end].to_string()
}

fn body_of(src: &str, signature: &str) -> String {
    let at = src.find(signature).unwrap_or_else(|| panic!("{signature} not found"));
    let rest = &src[at..];
    // The next top-level item ends it.
    let end = rest[1..].find("\nfn ").map(|i| i + 1).unwrap_or(rest.len());
    rest[..end].to_string()
}

#[test]
fn the_background_atlas_picks_the_microphone_this_machine_has() {
    let main = source("src/main.rs");
    let daemon = body_of(&main, "fn run_daemon(");
    let pick = daemon.find("pick_the_microphone(").expect("run_daemon doesn't pick a microphone");
    let voice = daemon.find("Voice::new(").expect("run_daemon builds no Voice");
    assert!(pick < voice, "the microphone is picked after the recorder was built from tools.yaml");
    let daemon_new = daemon.find("Daemon::try_new(").expect("run_daemon builds no checked Daemon");
    assert!(pick < daemon_new, "the daemon was given the unpicked configuration");
    assert!(daemon.contains("cfg_owned.tools = Some(tc_owned.clone())") && daemon.contains("let cfg = &cfg_owned"), "the checked constructor must receive the microphone-adjusted configuration");
}

#[test]
fn both_doors_pick_the_microphone_the_same_way() {
    let main = source("src/main.rs");
    assert_eq!(main.matches("fn pick_the_microphone(").count(), 1);
    let voice_loop = body_of(&main, "fn voice_loop(");
    assert!(voice_loop.contains("pick_the_microphone("), "atlas --voice no longer uses the shared pick");
    assert_eq!(
        main.matches("atlas::audio::probe_devices(\"ffmpeg\")").count(),
        1,
        "a second copy of the microphone pick has grown back"
    );
}

#[test]
fn a_microphone_that_cant_be_opened_says_why() {
    let said = "[dshow @ 0000023967f3f080] Could not find audio only device with name [Microphone Array (Realtek(R) Audio)] among source devices of type audio.\n\
                [in#0 @ 0000023967f42e80] Error opening input: I/O error\n";
    let why = atlas::voice::recorder_could_not_open(said).expect("a reason");
    assert!(why.starts_with("Could not find audio only device"), "{why}");
    // Nothing written is not a broken microphone: the key let go too soon.
    assert_eq!(atlas::voice::recorder_could_not_open(""), None);
    assert_eq!(atlas::voice::recorder_could_not_open("\n  \n"), None);
}

#[test]
fn every_listening_failure_is_written_down() {
    let daemon = source("src/daemon.rs");
    assert!(
        !daemon.contains("Err(_) => self.degrade(mouth)"),
        "a listening failure is dropped without saying why"
    );
    assert!(daemon.contains("fn degrade_because("));
}

#[test]
fn a_closed_laptop_behind_monitors_is_still_at_the_desk() {
    // Lid closed behind two monitors with AirPods connected read as "away",
    // and away with a headset listens through the headset.
    let main = source("src/main.rs");
    let pick = body_of(&main, "fn pick_the_microphone(");
    assert!(pick.contains("at_desk: laptop_active || screens_on"), "at the desk is the laptop's screen alone again");
}

#[test]
fn words_through_the_talk_key_lift_typing_only_at_once() {
    use atlas::input::{Tier, Tiers};
    let mut t = Tiers::default();
    t.audio_unavailable();
    assert_eq!(t.tier, Tier::Typed);
    assert!(t.heard_you().is_some());
    assert_eq!(t.tier, Tier::PushToTalk, "a microphone that just gave words left Atlas at typing-only");
}

#[test]
fn a_room_that_is_quiet_is_not_a_microphone_that_is_deaf() {
    // Eric's laptop: the Intel array measured -51.6 dB of quiet room and was
    // ruled out at -45; the muted webcam gave digital silence at -91.
    use atlas::hearing::{Ear, Hearing, HearingConfig, Where};
    let cfg = HearingConfig::default();
    let mut h = Hearing::default();
    let devs = vec![
        atlas::audio::Device::new("Microphone (HD Pro Webcam C920)", atlas::audio::Kind::Input),
        atlas::audio::Device::new("Microphone Array (Intel\u{ae} Smart Sound Technology for Digital Microphones)", atlas::audio::Kind::Input),
    ];
    h.observe_devices(&devs);
    h.record_level("Microphone (HD Pro Webcam C920)", -91.0, 1);
    h.record_level("Microphone Array (Intel\u{ae} Smart Sound Technology for Digital Microphones)", -51.6, 1);
    let w = Where { at_desk: true, presence_unknown: false, headset_connected: false, phone_active: false, audio_playing: false };
    let c = h.decide(&w, &cfg, 2);
    assert_eq!(c.ear, Ear::Desk("Microphone Array (Intel\u{ae} Smart Sound Technology for Digital Microphones)".into()));
}

#[test]
fn a_microphone_is_opened_by_windows_id_when_the_listing_gives_one() {
    // The listing printed "Intel®" as "Intelr", and ffmpeg couldn't open the
    // device by the name it had just printed. Windows' id has no such letter.
    let listing = "[dshow @ 01] \"Microphone Array (Intelr Smart Sound Technology for Digital Microphones)\" (audio)\n\
                   [dshow @ 01]   Alternative name \"@device_cm_{33D9A762-90C8-11D0-BD43-00A0C911CE86}\\wave_{937D4776-EE93-4892-AB6E-C41B70367A09}\"\n\
                   [dshow @ 01] \"HD Pro Webcam C920\" (video)\n\
                   [dshow @ 01]   Alternative name \"@device_pnp_\\\\?\\usb#vid_046d\"\n";
    let d = atlas::audio::parse_devices(listing);
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(d[0].ffmpeg_name(), "@device_cm_{33D9A762-90C8-11D0-BD43-00A0C911CE86}\\wave_{937D4776-EE93-4892-AB6E-C41B70367A09}");
    assert_eq!(atlas::audio::ffmpeg_name_for(&d, &d[0].name), d[0].ffmpeg_name());
    // A name with no id is used as it is.
    assert_eq!(atlas::audio::ffmpeg_name_for(&d, "Headset (AirPods Pro)"), "Headset (AirPods Pro)");
}

#[test]
fn nothing_is_never_a_microphone() {
    let main = source("src/main.rs");
    let pick = body_of(&main, "fn pick_the_microphone(");
    assert!(!pick.contains("choice.ear.name()"), "an ear that isn't a device (\"nothing\", \"your phone\") is recorded from again");
}

#[test]
fn the_loop_looks_after_the_model_server_without_waiting() {
    let daemon = source("src/daemon.rs");
    let run = method_body(&daemon, "    pub fn run(");
    let keep = run.find("keep_model_server_waiting(clock(), std::time::Duration::ZERO, false)").expect("the loop no longer looks after the model");
    let observe = run.rfind("self.observe(clock())").expect("the loop's pass");
    assert!(keep < observe, "looked after only outside the loop's pass");
}

#[test]
fn only_llama_servers_own_answer_means_the_model_is_up() {
    assert!(atlas::models::health_says_ok(r#"{"status":"ok"}"#));
    assert!(!atlas::models::health_says_ok(r#"{"status":"loading model"}"#));
    // Some other program on the port: "token", "book", "ok" in a page.
    assert!(!atlas::models::health_says_ok("<html>token booking ok</html>"));
    assert!(!atlas::models::health_says_ok(""));
}

#[test]
fn tool_and_model_paths_do_not_depend_on_the_starting_folder() {
    let mut t = atlas::voice::ToolsConfig::default();
    t.vars.insert("whisper".into(), "tools/whisper/whisper-cli.exe".into());
    t.vars.insert("stt_model".into(), "models/ggml-base.en.bin".into());
    t.vars.insert("mic_device".into(), "Microphone (USB)".into());
    let t = t.anchored();
    for k in ["whisper", "stt_model"] {
        assert!(std::path::Path::new(&t.vars[k]).is_absolute(), "{k} = {}", t.vars[k]);
    }
    assert_eq!(t.vars["mic_device"], "Microphone (USB)", "a device name was treated as a path");
}

#[test]
fn silence_after_the_wake_word_is_not_a_broken_microphone() {
    let daemon = source("src/daemon.rs");
    let body = method_body(&daemon, "    fn degrade_because(");
    let check = body.find("HEARD_NOTHING").expect("silence is counted as a failure again");
    let drop = body.find("self.degrade(mouth)").expect("degrade");
    assert!(check < drop, "silence is counted before it is recognised");
    let voice = source("src/voice.rs");
    assert_eq!(voice.matches("return Err(AtlasError::Platform(HEARD_NOTHING.into()))").count(), 2, "an empty listen isn't named as silence");
}

#[test]
fn a_broken_microphone_does_not_silence_atlas() {
    let daemon = source("src/daemon.rs");
    let say = method_body(&daemon, "fn say(&self, mouth: &dyn Mouth, line: &str)");
    let gate = say.find("can_speak").expect("typing-only mutes every reply again");
    let speak = say.find("mouth.speak(").expect("say speaks");
    assert!(gate < speak);
    assert_eq!(daemon.matches("let _ = mouth.speak(&crate::spoken_form::for_speech(line));").count(), 0, "a failed reply is dropped without a word again");
}

#[test]
fn no_keyboard_is_the_end_of_a_prompt_loop_not_an_empty_line() {
    let main = source("src/main.rs");
    assert_eq!(main.matches("if io::stdin().read_line(&mut l).is_err() {").count(), 0, "an empty read records for ever");
    assert_eq!(main.matches("if io::stdin().read_line(&mut line).is_err() {").count(), 0, "an empty read spins for ever");
}

#[test]
fn the_sign_in_listening_start_is_the_wake_word_loop() {
    assert_eq!(atlas::startup::Mode::Listening.flag(), "--wake");
}
