//! 29 Sep 2026, Eric's laptop: the background Atlas (what setup starts)
//! recorded from the microphone named in the shipped tools.yaml,
//! "Microphone Array (Realtek(R) Audio)". His laptop's microphone is Intel
//! Smart Sound. ffmpeg: "Could not find audio only device". Every wake-word
//! clip failed, Atlas dropped to push-to-talk, and the held key recorded
//! nothing and said nothing. Only `atlas --voice` picked the real microphone.

fn source(path: &str) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"))
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
    let daemon_new = daemon.find("Daemon::new(").expect("run_daemon builds no Daemon");
    assert!(pick < daemon_new, "the daemon was given the unpicked configuration");
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
