//! 29 Sep 2026, the sweep after Eric's laptop: the places where Atlas
//! stopped working and either said nothing or said the wrong thing, and
//! stayed that way until someone ended it in Task Manager.
//!
//! - A background Atlas ended in Task Manager left a lock that read
//!   "running" for minutes, so the next start refused, silently.
//! - A failed model call reached him as an empty reply with a caveat.
//! - A model server that died loading was started again on every pass.
//! - "No model" was explained as "it isn't loaded yet" whatever the cause.
//! - A second question while one was thinking froze the loop.
//! - The Talk page's polling piled up until the hub refused every page.

fn source(path: &str) -> String {
    // Through `read_source_path` (29 Sep 2026): daemon.rs and main.rs were
    // split into src/daemon/*.rs and src/main/*.rs, read here as one.
    crate::common::read_source_path(path).unwrap_or_else(|| panic!("{path}: not found"))
}

fn scratch(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-recovers-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

// ---------------------------------------------------------------- the lock

#[test]
fn the_lock_names_its_holder_and_still_reads_the_old_form() {
    // What `take` writes: the moment, then this process.
    let dir = scratch("lock-line");
    let lock = atlas::onlyone::OnlyOne::at(&dir);
    lock.take(1_000).unwrap();
    let s = std::fs::read_to_string(lock.path()).unwrap();
    assert_eq!(atlas::onlyone::moment_in(&s), Some(1_000));
    assert_eq!(atlas::onlyone::holder_in(&s), Some(std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // Written before 29 Sep 2026: a moment alone.
    assert_eq!(atlas::onlyone::moment_in("1000\n"), Some(1_000));
    assert_eq!(atlas::onlyone::holder_in("1000\n"), None);
}

#[cfg(target_os = "linux")]
#[test]
fn a_lock_whose_holder_has_ended_is_free_at_once() {
    let dir = scratch("dead-holder");
    let lock = atlas::onlyone::OnlyOne::at(&dir);
    let mut child = std::process::Command::new("true").spawn().unwrap();
    let gone = child.id();
    child.wait().unwrap();
    let now = 5_000;
    // Beat a second ago: before, this read "running" for GONE_AFTER_SECS.
    std::fs::write(lock.path(), format!("{} {gone}", now - 1)).unwrap();
    assert_eq!(lock.look(now), atlas::onlyone::Found::Free);
    assert!(lock.take(now).is_ok(), "a start after Task Manager is refused");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_lock_whose_holder_is_alive_still_reads_running() {
    let dir = scratch("live-holder");
    let lock = atlas::onlyone::OnlyOne::at(&dir);
    let now = 5_000;
    // Held by a process that is certainly there: this one.
    std::fs::write(lock.path(), format!("{} {}", now - 1, std::process::id())).unwrap();
    assert!(matches!(lock.look(now), atlas::onlyone::Found::Running { .. }));
    // And one with no holder named keeps the old reading.
    std::fs::write(lock.path(), format!("{}", now - 1)).unwrap();
    assert!(matches!(lock.look(now), atlas::onlyone::Found::Running { .. }));
    let _ = std::fs::remove_dir_all(&dir);
}

// ------------------------------------------------ a start that fails says so

#[test]
fn a_background_start_that_failed_says_why() {
    let root = scratch("start-problem");
    let plain = atlas::firstlaunch::start_failed_words(&root, Some(1));
    assert!(plain.contains("stopped straight away") && plain.contains("code 1"), "{plain}");
    atlas::firstlaunch::note_start_problem(&root, "Atlas is already running -- it checked in 3 seconds ago.");
    let told = atlas::firstlaunch::start_failed_words(&root, Some(1));
    assert!(told.contains("already running"), "what the background Atlas wrote is not shown: {told}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn opening_atlas_watches_the_background_start() {
    let main = source("src/main.rs");
    let at = main.find("if opening.start_background {").expect("opening starts the background Atlas");
    let block = &main[at..at + 600];
    let watched = block.find("start_background_watched(").expect("started unwatched again");
    let shown = block.find("show_problem(").expect("a failed start isn't shown");
    assert!(watched < shown);
}

// ------------------------------------------------------------ setup and updates

#[test]
fn setup_with_a_problem_is_tried_again_next_time() {
    let setup = source("src/setupwin.rs");
    let problems = setup.find("p.problems()").expect("setup no longer counts its problems");
    let only_then = setup[problems..].find("if problems == 0").map(|i| i + problems).expect("setup is marked done whatever happened");
    let mark = setup[only_then..].find("mark_set_up(").map(|i| i + only_then).expect("never marked done");
    assert!(problems < only_then && only_then < mark);
}

#[test]
fn the_overlay_and_typing_box_never_swap_in_an_update() {
    let main = source("src/main.rs");
    let helper = main.find("let helper = matches!(").expect("helpers aren't told apart");
    let gate = main.find("if !helper && std::env::var_os(\"ATLAS_UPDATE_PROBE\")").expect("helpers still swap and count trial starts");
    let swap = main.find("upgrade::swap_checked(").unwrap();
    let trial = main.find("upgrade::trial_on_start(").unwrap();
    assert!(helper < gate && gate < swap && swap < trial);
}

// ------------------------------------------------------------- the model

#[test]
fn a_failed_model_call_is_answered_with_why() {
    let w = atlas::daemon::model_failed_words("Model unreachable: connection refused");
    assert!(w.contains("connection refused") && w.contains("next message"), "{w}");
    assert!(!w.contains("Model unreachable"), "{w}");
    let bare = atlas::daemon::model_failed_words("");
    assert!(bare.contains("language model"), "{bare}");
    // And in the turn: the reason replaces the empty reply before the caveat
    // would be added, and the caveat is skipped for it.
    let daemon = source("src/daemon.rs");
    let failed = daemon.find("let failed_silent = decision.model == brain::Reached::No").expect("the empty reply is back");
    let mark = daemon[failed..].find("integrations::mark(").map(|i| i + failed).unwrap();
    assert!(daemon[failed..mark].contains("model_failed_words("));
}

#[test]
fn a_model_server_that_dies_young_waits_longer_each_time() {
    use atlas::daemon::model_server_pause as pause;
    assert_eq!(pause(0).as_secs(), 60);
    assert_eq!(pause(1).as_secs(), 120);
    assert_eq!(pause(2).as_secs(), 240);
    assert_eq!(pause(9).as_secs(), 30 * 60, "the pause has no ceiling");
    let mut last = 0;
    for d in 0..12 {
        let p = pause(d).as_secs();
        assert!(p >= last, "a later death waits less");
        last = p;
    }
}

#[test]
fn why_the_model_stopped_is_its_own_last_error() {
    let log = "load_tensors: loading model\nggml_vulkan: Device memory allocation failed: out of memory\nmain: exiting\n";
    let w = atlas::models::last_words_in(log).unwrap();
    assert!(w.contains("out of memory"), "{w}");
    assert_eq!(atlas::models::last_words_in("all fine\nlistening\n").as_deref(), Some("listening"));
    assert_eq!(atlas::models::last_words_in("\n  \n"), None);
}

#[test]
fn no_model_is_explained_by_what_was_found() {
    let empty = scratch("no-models");
    let cfg = atlas::models::ModelsConfig { dir: empty.display().to_string(), ..Default::default() };
    let why = atlas::models::why_no_model(&cfg);
    assert!(why.contains("no language model in my models folder"), "{why}");
    let missing = empty.join("not-there");
    let cfg = atlas::models::ModelsConfig { dir: missing.display().to_string(), ..Default::default() };
    let why = atlas::models::why_no_model(&cfg);
    assert!(why.contains("doesn't exist"), "{why}");
    let _ = std::fs::remove_dir_all(&empty);
}

#[test]
fn a_second_question_while_one_is_thinking_takes_the_other_slot() {
    // Both turns named the model's conversation slot, so the second waited
    // on the loop for the first to finish. It now takes the other slot, and
    // is still answered at once (`the_second_scan_holds` measures that).
    let daemon = source("src/daemon.rs");
    let aside = daemon.find("if self.pending_turn.is_some() {\n                        turn.aside = true;").expect("a second turn waits behind the first again");
    let asked = daemon[aside..].find(".converse_noting(").map(|i| i + aside).expect("converse_noting");
    assert!(aside < asked);
    let brain = source("src/brain.rs");
    // Three since 30 Sep 2026: the call made first when a request plainly
    // wants a tool (`converse_with_tools`) is a conversation call too.
    assert_eq!(brain.matches("aside: turn.aside").count(), 3, "the conversation call no longer carries the choice");
}

// ------------------------------------------------------------- the Talk page

#[test]
fn the_talk_page_asks_one_question_at_a_time() {
    let s = atlas::hubpages::TALK_WAIT_SCRIPT;
    let guard = s.find("||out)return;out=true;").expect("fetches pile up again");
    let fetch = s.find("fetch(").unwrap();
    assert!(guard < fetch);
    assert!(s.contains("r.ok?r.json():null"), "a busy reply is read as done");
    let live = atlas::hub::LIVE_SCRIPT;
    assert!(live.find("||out)return;").unwrap() < live.find("out=true;fetch('/hub/changed.json").unwrap());
}

// ------------------------------------------------------ the firewall rule

#[test]
fn a_rule_windows_shows_with_a_variable_in_it_is_still_ours() {
    std::env::set_var("ATLAS_TEST_RULE_HOME", r"C:\Users\erics\AppData\Local");
    let exe = std::path::Path::new(r"C:\Users\erics\AppData\Local\Atlas\atlas.exe");
    let shown = "Rule Name:                            Atlas - your own devices\r\n\
                 Program:                              %ATLAS_TEST_RULE_HOME%\\Atlas\\atlas.exe\r\n";
    assert!(atlas::doorrule::describes_rule_for(shown, exe), "setup asks for Windows' permission every time again");
    let elsewhere = "Program:                              C:\\Other\\Atlas\\atlas.exe\r\n";
    assert!(!atlas::doorrule::describes_rule_for(elsewhere, exe));
    assert_eq!(atlas::doorrule::expand_vars("%NOT_A_VAR_ATLAS%\\x"), "%NOT_A_VAR_ATLAS%\\x");
}

// ------------------------------------------------ what it says it can't do

#[test]
fn atlas_doesnt_say_it_cant_see_the_screen_when_the_picture_reader_is_there() {
    let plan_says = vec![
        "I can't look at your screen and understand it — I can read text off it.".to_string(),
        "No language model fits, so I'll follow rules rather than reason. Most of what I do doesn't need one.".to_string(),
        "One thing at a time here.".to_string(),
    ];
    let said = atlas::daemon::what_this_machine_cant_do(plan_says.clone(), Some(Ok(())), true);
    assert_eq!(said, vec!["One thing at a time here.".to_string()], "the start-up notice contradicts what's installed");
    let missing = atlas::daemon::what_this_machine_cant_do(plan_says, Some(Err("I don't have its picture encoder yet".into())), false);
    assert!(missing[0].contains("picture encoder"), "{missing:?}");
    assert!(missing.iter().any(|l| l.starts_with("No language model fits")));
}

// -------------------------------------------------- is the internet there

#[test]
fn the_internet_check_asks_more_than_one_door() {
    // A network that blocks TCP to 1.1.1.1:53 read as offline all day.
    assert_eq!(atlas::connectivity::ConnectivityConfig::default().probe, atlas::connectivity::SHIPPED_PROBE);
    assert!(atlas::connectivity::ALSO_TRIED.iter().all(|a| a.ends_with(":443")));
    assert_eq!(atlas::connectivity::probe_targets("1.1.1.1:53, 8.8.8.8:443"), vec!["1.1.1.1:53", "8.8.8.8:443"]);
    // An address you chose is asked alone: nothing listening is offline.
    let mut c = atlas::connectivity::Connectivity::new(atlas::connectivity::ConnectivityConfig {
        probe: "127.0.0.1:1".into(),
        timeout_ms: 50,
        cache_secs: 9999,
        assume_offline: false,
    });
    assert_eq!(c.status(1), atlas::connectivity::Reach::Offline);
}

// ------------------------------------------- the microphone, picked again

fn input(name: &str, bluetooth: bool, builtin: bool, id: &str) -> atlas::audio::Device {
    atlas::audio::Device {
        name: name.into(),
        kind: atlas::audio::Kind::Input,
        bluetooth,
        builtin,
        id: if id.is_empty() { None } else { Some(id.into()) },
    }
}

#[test]
fn a_microphone_is_always_picked_when_the_machine_has_one() {
    let tc = atlas::voice::ToolsConfig::default();
    let mut h = atlas::hearing::Hearing::default();
    let w = atlas::hearing::Where { at_desk: true, presence_unknown: false, headset_connected: false, phone_active: false, audio_playing: false };
    let devices = vec![input("Microphone Array (Intel® Smart Sound Technology for Digital Microphones)", false, true, "@device_cm_{X}\\wave_{Y}")];
    let p = atlas::hearing::pick_microphone(&devices, &mut h, &tc, &w, true, 1_000).expect("the only microphone wasn't picked");
    assert!(p.name.starts_with("Microphone Array (Intel"), "{p:?}");
    assert_eq!(p.device, "@device_cm_{X}\\wave_{Y}", "opened by name, not by Windows' id");
    assert!(atlas::hearing::pick_microphone(&[], &mut atlas::hearing::Hearing::default(), &tc, &w, true, 1_000).is_none());
}

#[test]
fn a_different_pick_is_said_and_the_same_one_is_not() {
    let p = atlas::hearing::Picked { name: "Headset (AirPods Pro)".into(), device: "@device_cm_{A}".into(), why: "the headset is on".into(), costs_quality: true };
    assert_eq!(atlas::daemon::microphone_change("Headset (AirPods Pro)", "@device_cm_{A}", &p), None);
    assert_eq!(atlas::daemon::microphone_change("Headset (AirPods Pro)", "Headset (AirPods Pro)", &p), None, "the same microphone by another spelling");
    let said = atlas::daemon::microphone_change("Microphone Array (Intel)", "@device_cm_{I}", &p).expect("a new microphone went unsaid");
    assert!(said.contains("AirPods") && said.contains("headset is on"), "{said}");
}

#[test]
fn every_recording_reads_the_microphone_in_use_now() {
    let voice = source("src/voice.rs");
    assert!(!voice.contains(r#"self.cfg.vars.get("mic_device")"#), "a recorder still reads the start-up pick directly");
    let vars = voice.find("    fn vars(&self) -> Result<Vars> {").expect("vars()");
    let clone = voice[vars..].find("self.cfg.vars.clone()").unwrap() + vars;
    let over = voice[clone..].find("microphone_override()").map(|i| i + clone).expect("the command values ignore a new pick");
    assert!(over - clone < 400);
    let daemon = source("src/daemon.rs");
    let run = daemon.find("self.look_again_at_audio(ears, clock());").unwrap();
    assert!(daemon[run..run + 200].contains("self.look_again_at_the_microphone(clock());"), "the loop never looks again");
}

// ------------------------------------------ a stuck model, a loading model

#[test]
fn a_running_model_server_that_stops_answering_is_restarted() {
    let daemon = source("src/daemon.rs");
    // A failed call makes it suspect...
    let failed = daemon.find("let failed_silent = decision.model == brain::Reached::No").unwrap();
    assert!(daemon[failed..failed + 1200].contains("self.model_suspect = self.starts_model_server;"), "a failed call no longer asks after the server");
    // ...and a suspect server that doesn't answer, and isn't loading, is stopped.
    let keep = daemon.find("fn keep_model_server_waiting(").unwrap();
    let body = &daemon[keep..keep + daemon[keep..].find("\n    }\n").unwrap()];
    let asked = body.find("if self.model_suspect && !loading {").expect("a running server is trusted however stuck");
    let stopped = body[asked..].find("self.helpers.finished(\"model-server\");").map(|i| i + asked).expect("never stopped");
    let probe = body[asked..].find("probe_model_server(").map(|i| i + asked).unwrap();
    assert!(probe < stopped);
}

#[test]
fn a_model_just_started_is_said_to_be_loading() {
    use atlas::models::{probably_still_loading, LOADING_SECS};
    assert!(probably_still_loading(Some(5)));
    assert!(!probably_still_loading(Some(LOADING_SECS + 1)));
    assert!(!probably_still_loading(None), "a server Atlas never started isn't loading");
    assert!(atlas::daemon::STILL_LOADING_WORDS.contains("loading"));
    assert!(atlas::daemon::STILL_LOADING_AFTER.as_secs() <= 5, "a minute of silence before saying so");
}

#[test]
fn the_usual_voice_says_only_what_kokoro_didnt() {
    use atlas::voice::{kokoro_stopped_at, Kokoro};
    let s: Vec<String> = ["First.", "Second.", "Third."].iter().map(|x| x.to_string()).collect();
    match kokoro_stopped_at(&s, 1) {
        Kokoro::Rest(r) => assert_eq!(r, "Second. Third."),
        other => panic!("{other:?}"),
    }
    assert!(matches!(kokoro_stopped_at(&s, 0), Kokoro::Unavailable), "nothing said yet is the whole line");
}

#[test]
fn the_camera_is_one_this_machine_has() {
    let listing = "[dshow @ 01] \"Integrated Webcam\" (video)\n\
                   [dshow @ 01]   Alternative name \"@device_pnp_\\\\?\\usb#vid_0c45\"\n\
                   [dshow @ 01] \"OBS Virtual Camera\" (video)\n\
                   [dshow @ 01] \"Microphone Array (Intel® Smart Sound)\" (audio)\n";
    let cams = atlas::audio::parse_cameras(listing);
    assert_eq!(cams, vec!["Integrated Webcam".to_string(), "OBS Virtual Camera".to_string()]);
    assert_eq!(atlas::audio::pick_camera(&cams, "Integrated Camera").as_deref(), Some("Integrated Webcam"), "the shipped guess was kept");
    assert_eq!(atlas::audio::pick_camera(&cams, "OBS Virtual Camera").as_deref(), Some("OBS Virtual Camera"), "your own choice was overridden");
    assert_eq!(atlas::audio::pick_camera(&["OBS Virtual Camera".to_string()], "x"), None);
    assert_eq!(atlas::audio::pick_camera(&[], "Integrated Camera"), None);
}

#[test]
fn a_shut_lid_reads_as_a_shut_lid() {
    use atlas::layout::built_in_screen_from as screen;
    // (built in, active)
    assert_eq!(screen(&[(true, true), (false, true)]), Some(true), "lid open with a monitor");
    assert_eq!(screen(&[(true, false), (false, true), (false, true)]), Some(false), "lid shut behind two monitors");
    assert_eq!(screen(&[(false, true)]), None, "a desktop has no screen of its own");
    assert_eq!(screen(&[]), None);
}

#[test]
fn the_typing_key_is_blamed_on_another_program_only_when_one_has_it() {
    assert!(atlas::hotkeys::typing_key_refused(1409, "Hot key is already registered.").contains("another program"));
    let other = atlas::hotkeys::typing_key_refused(5, "Access is denied.");
    assert!(!other.contains("another program") && other.contains("Access is denied"), "{other}");
}

#[test]
fn turning_off_start_with_windows_is_checked_not_assumed() {
    assert!(atlas::startup::turned_off_says(Ok(false)).is_ok());
    assert!(atlas::startup::turned_off_says(Ok(true)).unwrap_err().contains("Task Scheduler"), "a refused delete is reported as done");
    // Can't ask: not a reason to say it failed.
    assert!(atlas::startup::turned_off_says(Err("couldn't run schtasks".into())).is_ok());
    let src = source("src/startup.rs");
    let run = src.find("pub fn run(plan: &Plan)").unwrap();
    assert!(src[run..run + 900].contains("CREATE_NO_WINDOW"), "the switch flashes a console again");
}

// ------------------------------------------------ setup and reinstalling

#[test]
fn a_tool_folder_is_swapped_whole_and_keeps_what_only_it_had() {
    let root = scratch("swap");
    let live = root.join("tools").join("piper");
    std::fs::create_dir_all(&live).unwrap();
    std::fs::write(live.join("piper.exe"), b"old").unwrap();
    std::fs::write(live.join("yours.txt"), b"kept").unwrap();
    let new = root.join("unpacked");
    std::fs::create_dir_all(new.join("espeak")).unwrap();
    std::fs::write(new.join("piper.exe"), b"new").unwrap();
    atlas::getpieces::swap_folder(&new, &live).unwrap();
    assert_eq!(std::fs::read(live.join("piper.exe")).unwrap(), b"new");
    assert!(live.join("espeak").is_dir());
    assert_eq!(std::fs::read(live.join("yours.txt")).unwrap(), b"kept", "a file only the old folder had was lost");
    let leftovers: Vec<_> = std::fs::read_dir(root.join("tools")).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(leftovers.len(), 1, "the old folder was left beside it: {leftovers:?}");
    // A new folder that isn't there leaves the live one as it was.
    assert!(atlas::getpieces::swap_folder(&root.join("nothing"), &live).is_err());
    assert_eq!(std::fs::read(live.join("piper.exe")).unwrap(), b"new", "a failed swap broke the tool");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn only_an_atlas_is_ended_to_make_way() {
    use atlas::onlyone::is_atlas_program as is_atlas;
    assert!(is_atlas(std::path::Path::new(r"C:\Users\erics\AppData\Local\Atlas\atlas.exe")));
    assert!(is_atlas(std::path::Path::new(r"C:\x\atlas.exe.set-aside-4242")));
    assert!(is_atlas(std::path::Path::new("/opt/atlas/atlas")));
    assert!(!is_atlas(std::path::Path::new(r"C:\Windows\explorer.exe")));
    assert!(!is_atlas(std::path::Path::new(r"C:\x\notatlas.exe")));
    // A lock naming no holder, or this process, ends nothing.
    let dir = scratch("end-holder");
    std::fs::write(atlas::onlyone::OnlyOne::at(&dir).path(), "1000").unwrap();
    assert!(!atlas::onlyone::end_holder(&dir, std::time::Duration::from_millis(10)));
    std::fs::write(atlas::onlyone::OnlyOne::at(&dir).path(), format!("1000 {}", std::process::id())).unwrap();
    assert!(!atlas::onlyone::end_holder(&dir, std::time::Duration::from_millis(10)), "it would end itself");
    let _ = std::fs::remove_dir_all(&dir);
}

// ------------------------------------------------ the speaker it speaks through

fn wav16(rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
    let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    let mut w = Vec::new();
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&channels.to_le_bytes());
    w.extend_from_slice(&rate.to_le_bytes());
    w.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
    w.extend_from_slice(&(channels * 2).to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&(data.len() as u32).to_le_bytes());
    w.extend_from_slice(&data);
    w
}

#[test]
fn a_reply_is_read_and_fitted_to_the_speaker() {
    use atlas::playout::{fit_to_speaker, parse_wav};
    // Piper's voice: 22,050 Hz, one channel, 16-bit.
    let w = parse_wav(&wav16(22_050, 1, &[0, 16_384, -16_384, 32_767])).unwrap();
    assert_eq!((w.rate, w.channels, w.samples.len()), (22_050, 1, 4));
    assert!((w.samples[1] - 0.5).abs() < 1e-3 && (w.samples[2] + 0.5).abs() < 1e-3);
    // A WASAPI speaker: 48,000 Hz, two channels.
    let out = fit_to_speaker(&w, 48_000, 2);
    assert_eq!(out.len() % 2, 0);
    let frames = out.len() / 2;
    assert!((8..=9).contains(&frames), "4 frames at 22,050 Hz are about 8.7 at 48,000: {frames}");
    assert!(out.chunks(2).all(|f| f[0] == f[1]), "one channel wasn't copied to both");
    // Two channels down to one: mixed.
    let st = parse_wav(&wav16(48_000, 2, &[16_384, 0, 16_384, 0])).unwrap();
    let mono = fit_to_speaker(&st, 48_000, 1);
    assert!(mono.iter().all(|v| (v - 0.25).abs() < 1e-3), "{mono:?}");
    assert!(parse_wav(b"not a wav at all").is_err());
    assert!(fit_to_speaker(&atlas::playout::Wav { rate: 16_000, channels: 1, samples: vec![] }, 48_000, 2).is_empty());
}

#[test]
fn it_speaks_into_your_headphones_when_theyre_there() {
    use atlas::audio::{AudioConfig, Device, Kind};
    use atlas::playout::chosen_output;
    let speakers = vec![Device::new("Speakers (Realtek(R) Audio)", Kind::Output), Device::new("Headphones (AirPods Pro Stereo)", Kind::Output)];
    let cfg = AudioConfig::default();
    assert_eq!(chosen_output(&speakers, &cfg).as_deref(), Some("Headphones (AirPods Pro Stereo)"));
    let named = AudioConfig { preferred_output: vec!["Speakers".into()], ..AudioConfig::default() };
    assert_eq!(chosen_output(&speakers, &named).as_deref(), Some("Speakers (Realtek(R) Audio)"), "your named speaker lost to the headphones");
    assert_eq!(chosen_output(&[], &cfg), None, "nothing listed is the system's default");
    // Only the shipped player is played in place of; a player you named is used.
    assert!(atlas::playout::plays_inside("ffplay") && atlas::playout::plays_inside(r"C:\Atlas\tools\ffmpeg\ffplay.exe"));
    assert!(!atlas::playout::plays_inside("mpv"));
    // The fallback is said once per reason, not every sentence.
    assert!(atlas::playout::note_once("the speaker wouldn't play (test)").is_some());
    assert!(atlas::playout::note_once("the speaker wouldn't play (test)").is_none());
}

// ---------------------------------------------- the voice, as Eric heard it

fn tone(ms: u32, rate: u32, amp: f32) -> Vec<i16> {
    let n = (rate * ms / 1000) as usize;
    (0..n).map(|i| ((i as f32 * 2.0 * std::f32::consts::PI * 220.0 / rate as f32).sin() * amp * 32767.0) as i16).collect()
}

fn hiss(ms: u32, rate: u32, amp: f32) -> Vec<i16> {
    let n = (rate * ms / 1000) as usize;
    let mut x: u32 = 12345;
    (0..n)
        .map(|_| {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
            (((x >> 16) as f32 / 32768.0 - 1.0) * amp * 32767.0) as i16
        })
        .collect()
}

#[test]
fn a_quiet_room_is_silence_and_whisper_is_not_asked() {
    use atlas::audio::{check_speech, SpeechCheck};
    // Eric's room: steady hiss around -55 dB, for twelve seconds.
    let room = hiss(12_000, 16_000, 0.002);
    assert_eq!(check_speech(&room, 16_000), SpeechCheck::Silence, "a quiet room would be transcribed (and come back as \"you\")");
    // Digital silence.
    assert_eq!(check_speech(&vec![0i16; 16_000], 16_000), SpeechCheck::Silence);
    // Speaking over the room: a second of voice.
    let mut said = hiss(1_000, 16_000, 0.002);
    said.extend(tone(1_000, 16_000, 0.3));
    said.extend(hiss(1_000, 16_000, 0.002));
    assert_eq!(check_speech(&said, 16_000), SpeechCheck::Speech);
    // The same, quietly (a laptop mic behind a monitor): turned up, not dropped.
    let mut quiet = hiss(1_000, 16_000, 0.0005);
    quiet.extend(tone(1_000, 16_000, 0.01));
    quiet.extend(hiss(1_000, 16_000, 0.0005));
    match check_speech(&quiet, 16_000) {
        SpeechCheck::Quiet(g) => {
            assert!(g > 1.5 && g <= 30.0, "{g}");
            let up = atlas::audio::turned_up(&quiet, g);
            assert!(atlas::audio::level_db(&up) > atlas::audio::level_db(&quiet) + 3.0);
        }
        other => panic!("quiet speech read as {other:?}"),
    }
    // A click is not speech.
    let mut click = hiss(2_000, 16_000, 0.002);
    click.extend(tone(40, 16_000, 0.5));
    click.extend(hiss(2_000, 16_000, 0.002));
    assert_eq!(check_speech(&click, 16_000), SpeechCheck::Silence);
}

#[test]
fn what_whisper_writes_for_silence_is_not_a_turn() {
    use atlas::voice::not_really_said as ghost;
    // Not "Bye." (30 Sep 2026): it's how a conversation is ended, and a
    // silent clip no longer reaches whisper at all (`check_speech`).
    for s in ["you", "You.", " you ", "Thanks for watching!", "Thank you for watching.", "...", ""] {
        assert!(ghost(s), "{s:?} would be answered");
    }
    for s in ["Thank you.", "Why can you not hear me? What is the issue?", "Can you hear me?", "you there?", "Look at me."] {
        assert!(!ghost(s), "{s:?} was dropped");
    }
}

#[test]
fn a_reply_given_again_and_again_is_not_shown_to_the_model_again_and_again() {
    let mut t = atlas::thread::Thread::default();
    let stuck = "I\u{2019}m here \u{2014} and I\u{2019}m listening. What\u{2019}s on your mind?";
    t.append("And you hear me.", stuck, None, 1);
    t.append("What are you capable of?", "I can help with tasks on your computer.", None, 2);
    t.append("Why can you not hear me? What is the issue?", stuck, None, 3);
    t.append("you", stuck, None, 4);
    t.append("Can you hear me?", stuck, None, 5);
    let msgs = t.messages(20, 4000);
    let text: Vec<String> = msgs.iter().map(|m| m.content.clone()).collect();
    // 30 Sep 2026 (merge): a past reply is shown as its first sentence or
    // two without the stock closers (`repeating::for_history`), and later
    // near copies are left out, so the stuck reply is counted by its opening.
    let opening = "I\u{2019}m here \u{2014} and I\u{2019}m listening.";
    assert_eq!(text.iter().filter(|m| m.contains(opening)).count(), 1, "the same reply is shown to the model again and again: {text:?}");
    assert!(!text.iter().any(|m| m == "you"), "a whisper ghost was kept as something said");
    assert!(text.iter().any(|m| m == "Can you hear me?"), "the newest exchange was dropped");
}

#[test]
fn a_short_opening_copied_from_the_last_reply_is_caught() {
    let earlier = ["I\u{2019}m here \u{2014} and I\u{2019}m listening. What\u{2019}s on your mind?"];
    let mut g = atlas::brain::SpeechGate::new(&earlier);
    let out = g.take("I\u{2019}m here \u{2014} and I\u{2019}m listening. What");
    assert!(g.repeated && out.is_empty(), "the copied opening was said again");
    let mut ok = atlas::brain::SpeechGate::new(&earlier);
    let said = ok.take("The microphone is fine now. What next?");
    assert!(!ok.repeated && !said.is_empty());
}

#[test]
fn at_the_desk_the_laptop_mic_beats_the_airpods_mic() {
    use atlas::audio::{Device, Kind};
    let tc = atlas::voice::ToolsConfig::default();
    let mut laptop = Device::new("Microphone Array (Intel Smart Sound Technology for Digital Microphones)", Kind::Input);
    laptop.builtin = true;
    let devices = vec![laptop, Device::new("Headset (AirPods Pro)", Kind::Input)];
    let at_desk = atlas::hearing::Where { at_desk: true, presence_unknown: false, headset_connected: true, phone_active: false, audio_playing: false };
    // What Eric's start-up measured: the laptop mic at -54 dB, under the -45 floor.
    let mut h = atlas::hearing::Hearing::default();
    h.observe_devices(&devices);
    h.record_level(&devices[0].name, -54.1, 1);
    let p = atlas::hearing::pick_microphone(&devices, &mut h, &tc, &at_desk, true, 2).unwrap();
    assert!(p.name.starts_with("Microphone Array"), "the AirPods' mic was picked, and the AirPods went to call quality: {p:?}");
    assert!(!p.costs_quality);
    // With the lid shut, the laptop is often put away: the AirPods it is
    // (Eric, 29 Sep 2026: "using my laptop mic which is closed and stored
    // away from me").
    let mut h2 = atlas::hearing::Hearing::default();
    h2.record_level(&devices[0].name, -54.1, 1);
    let shut = atlas::hearing::pick_microphone(&devices, &mut h2, &tc, &at_desk, false, 2).unwrap();
    assert!(shut.name.contains("AirPods"), "{shut:?}");
    // But a webcam that hears you takes over from the laptop mic under the lid.
    let mut with_cam = devices.clone();
    with_cam.push(Device::new("Microphone (HD Pro Webcam C920)", Kind::Input));
    let mut h4 = atlas::hearing::Hearing::default();
    h4.record_level("Microphone (HD Pro Webcam C920)", -30.0, 1);
    let cam = atlas::hearing::pick_microphone(&with_cam, &mut h4, &tc, &at_desk, false, 2).unwrap();
    assert!(cam.name.contains("C920"), "{cam:?}");
    // A microphone you named wins over all of it.
    let mut named = tc.clone();
    named.audio.preferred_input = vec!["AirPods".into()];
    let mut h3 = atlas::hearing::Hearing::default();
    let yours = atlas::hearing::pick_microphone(&devices, &mut h3, &named, &at_desk, true, 2).unwrap();
    assert!(yours.name.contains("AirPods") && yours.why.contains("you chose"), "{yours:?}");
}

#[test]
fn a_shut_lid_never_leaves_atlas_with_no_microphone() {
    // Eric's desk, 29 Sep 2026: lid shut behind two monitors, the webcam's
    // mic muted in Windows (-90 dB), AirPods not connected. Taking the laptop
    // mic out for the lid left nothing, and Atlas went deaf.
    use atlas::audio::{Device, Kind};
    let tc = atlas::voice::ToolsConfig::default();
    let mut laptop = Device::new("Microphone Array (Intel Smart Sound Technology for Digital Microphones)", Kind::Input);
    laptop.builtin = true;
    let webcam = Device::new("Microphone (HD Pro Webcam C920)", Kind::Input);
    let devices = vec![laptop, webcam.clone()];
    let w = atlas::hearing::Where { at_desk: true, presence_unknown: false, headset_connected: false, phone_active: false, audio_playing: false };
    let mut h = atlas::hearing::Hearing::default();
    h.observe_devices(&devices);
    h.record_level(&webcam.name, -90.3, 1);
    let p = atlas::hearing::pick_microphone(&devices, &mut h, &tc, &w, false, 2).expect("Atlas was left with no microphone");
    assert!(p.name.starts_with("Microphone Array"), "{p:?}");
}

#[test]
fn a_permission_with_an_apostrophe_still_asks_first() {
    let s = atlas::settings::registry(&Default::default());
    let page = atlas::hub::permissions_page(&s, &[]);
    assert!(page.contains("data-confirm="), "no switch asks before it turns on");
    assert!(!page.contains("return confirm('"), "the question is inside the script's quotes again, where \"that's\" breaks it");
}

#[test]
fn a_report_on_itself_is_the_self_check() {
    let cfg = atlas::config::Config::load(std::path::Path::new("config")).expect("the shipped config");
    let p = atlas::intent::Parser::new(&cfg.commands);
    for said in ["Can you do some work and generate a report on yourself?", "what still needs to be set up", "give me a status report"] {
        assert_eq!(p.parse(said), atlas::intent::Intent::SelfCheck, "{said:?} went to the model, which said it can't");
    }
}

#[test]
fn a_sentence_copied_from_earlier_replies_is_not_kept_again() {
    // 30 Sep 2026 (merge): `thread::without_repeated_sentences` took copied
    // sentences out when a reply was stored; this chat's
    // `repeating::SentenceFilter` stops them before they are said (through
    // `brain::SpeechGate`), so the reply stored is already without them. The
    // same text, through the one that was kept.
    let clean = |reply: &str, earlier: &[&str]| {
        let mut f = atlas::repeating::SentenceFilter::new(earlier);
        for s in atlas::repeating::sentences(reply) {
            f.pass(&s);
        }
        f.kept()
    };
    let tail = "What\u{2019}s your next move? A joke? A memory? Or maybe you\u{2019}re testing if I can still hear you when you\u{2019}re not talking? Either way, I\u{2019}m tuned in.";
    let earlier = format!("You\u{2019}re not wrong. {tail}");
    let now = format!("You\u{2019}re right, I\u{2019}m not calm. {tail}");
    let kept = clean(&now, &[&earlier]);
    assert_eq!(kept, "You\u{2019}re right, I\u{2019}m not calm.", "{kept}");
    assert_eq!(clean("Sure. Done.", &["Sure. Done."]), "Sure. Done.", "short answers repeat because they're right");
    let mut t = atlas::thread::Thread::default();
    t.append("a", &earlier, None, 1);
    t.append("b", &now, None, 2);
    let msgs = t.messages(20, 4000);
    let all: String = msgs.iter().map(|m| m.content.clone()).collect::<Vec<_>>().join(" ");
    // Shown to the model at most once (a stock closer: not at all).
    assert!(all.matches("Either way, I\u{2019}m tuned in.").count() <= 1, "{all}");
    assert!(all.matches("Or maybe you\u{2019}re testing").count() <= 1, "{all}");
}

#[test]
fn a_parked_window_is_marked_minimized_and_unmarked_to_show() {
    use atlas::winpark::with_minimized_bit as bit;
    let style: isize = 0x0080_0000; // some other style bit
    let parked = bit(style, true);
    assert_eq!(parked & 0x2000_0000, 0x2000_0000);
    assert_eq!(parked & 0x0080_0000, 0x0080_0000, "another style bit was lost");
    assert_eq!(bit(parked, false), style);
    // Both helper windows park when hidden and unpark before being shown.
    let overlay = source("src/overlaywin.rs");
    let typebox = source("src/typebox.rs");
    // By the handle eframe gives, not a search by title (which found nothing
    // on Eric's laptop).
    assert!(overlay.contains("winpark::handle_of(cc)") && overlay.contains("winpark::park(self.hwnd)") && overlay.contains("winpark::unpark(hwnd)"));
    assert!(typebox.contains("winpark::handle_of(cc)") && typebox.contains("winpark::park(self.hwnd)") && typebox.contains("winpark::unpark(hwnd)"));
    assert!(!overlay.contains("FindWindowW") && !typebox.contains("FindWindowW(windows::core::PCWSTR::null(), windows::core::PCWSTR(title"), "a window is looked up by title again");
}

// ---- 29 Sep 2026, evening: Eric's report -- "it doesn't hear me most of the
// time", a research request not done, and Atlas saying it didn't care and
// couldn't research. Each test below is one of the causes found in his log.

/// "Atlas, can you see me?" in one breath: the words after the name were in
/// the wake word's own clip and were thrown away ("I heard my name but
/// nothing after it", five times in one evening).
#[test]
fn the_words_said_with_the_name_are_kept() {
    // 30 Sep 2026 (merge): `after_wake_phrase` and `words_after_name` were
    // the same matcher; `words_after_name` is the one kept.
    use atlas::voice::words_after_name as after_wake_phrase;
    assert_eq!(after_wake_phrase("Atlas, can you see me?", "atlas").as_deref(), Some("can you see me?"));
    assert_eq!(after_wake_phrase("Hey Atlas. Research local models.", "hey atlas").as_deref(), Some("Research local models."));
    assert_eq!(after_wake_phrase("Atlas.", "atlas").as_deref(), Some(""));
    assert_eq!(after_wake_phrase("thanks for watching", "atlas"), None);
}

/// Those words start what was said; a listen that hears nothing more leaves
/// them as all of it, and a finished sentence isn't waited on.
#[test]
fn the_name_and_the_rest_make_one_request() {
    use atlas::micthread::{sentence_finished, with_wake_word};
    let nothing = Err(format!("{}", atlas::voice::HEARD_NOTHING));
    assert_eq!(with_wake_word(Some("can you see me?".into()), nothing.clone()), Ok("can you see me?".into()));
    assert_eq!(with_wake_word(Some("research".into()), Ok("local models".into())), Ok("research local models".into()));
    assert_eq!(with_wake_word(None, nothing.clone()), nothing);
    assert_eq!(with_wake_word(Some("  ".into()), Ok("hello there".into())), Ok("hello there".into()));
    assert!(sentence_finished("can you see me?"));
    assert!(!sentence_finished("research ways to"));
    assert!(!sentence_finished("Yes."), "one word may be the start of more");
}

/// "Research ways to improve in house language models ... allowing it to do
/// better ... put it into a document" was answered "About what?": the "it"s
/// inside a long request were taken as needing something said earlier.
#[test]
fn a_long_request_with_it_in_it_is_not_asked_about_what() {
    use atlas::references::argument_leans_on_earlier;
    assert!(argument_leans_on_earlier("it"));
    assert!(argument_leans_on_earlier("that one"));
    assert!(argument_leans_on_earlier("it again"));
    assert!(!argument_leans_on_earlier(
        "ways to improve in house language models allowing it to do better put it into a document"
    ));
    let cfg = atlas::config::Config::load(std::path::Path::new("config")).unwrap();
    let p = atlas::intent::Parser::new(&cfg.commands);
    match p.parse("Research ways to improve in house language models, allowing it to do better. Put it into a document.") {
        atlas::intent::Intent::Research(t) => assert!(!argument_leans_on_earlier(&t), "{t}"),
        other => panic!("not research: {other:?}"),
    }
}

/// "Start that research", "do the research I asked for": asking to get on
/// with it, not a topic -- they went to the model, which said "I'm already on
/// it" and started nothing.
#[test]
fn getting_on_with_the_research_is_recognised() {
    use atlas::references::starts_the_research;
    for s in ["And you start that research.", "Ok then lets get started on the research.",
              "I want you to use the internet and do the research i asked for.",
              "When you start that research, tell me what you find."] {
        assert!(starts_the_research(s), "{s}");
    }
    for s in ["what did the research say", "research quantum computing", "start the timer"] {
        assert!(!starts_the_research(s), "{s}");
    }
}

/// Atlas knows whose it is and what its job is, and the model is told what
/// it told Eric isn't true of it.
#[test]
fn atlas_knows_who_it_is_and_what_its_job_is() {
    let p = atlas::persona::Persona::default();
    // 30 Sep 2026 (the prompt diet): the same statements in fewer words, and
    // "assistant and friend" (Eric: "an assistant that is also a friend").
    for prompt in [p.character(), p.system_prompt()] {
        assert!(prompt.contains("personal assistant and friend of the person who owns this computer"), "{prompt}");
        assert!(prompt.contains("Your job: take things off their plate"));
        assert!(prompt.contains("research the web"));
        assert!(prompt.contains("about getting better"));
        assert!(prompt.contains("can't do research"));
    }
    // Still says to call the tools (the second scan's check).
    assert!(p.character().contains("call the tool"));
}

/// "I'm *you*" read aloud: emphasis marks are taken out of what's spoken,
/// and arithmetic is left alone.
#[test]
fn emphasis_marks_are_not_read_aloud() {
    use atlas::persona::without_emphasis;
    assert_eq!(without_emphasis("I'm *you*, **really**."), "I'm you, really.");
    assert_eq!(without_emphasis("3 * 4 is 12"), "3 * 4 is 12");
    let p = atlas::persona::Persona::default();
    assert!(!p.spoken("I'm not *doing* research.").contains('*'));
}

/// "Have a go" on the Improvements page was refused every time as "the cause
/// is a restatement of the symptom": the session already held the symptom,
/// and was handed it again as the cause.
#[test]
fn have_a_go_hands_over_the_cause_as_the_cause() {
    use atlas::selfaudit::{recommend, Kind, Signal};
    let sig = Signal { kind: Kind::NotUnderstood, subject: "what you said".into(), seen: 9, of: 10, example: "flip channel names".into() };
    let recs = recommend(&[sig], 3);
    let r = recs.first().expect("a recommendation");
    let s = atlas::selfwork::Session::from_recommendation(r, 0);
    assert_eq!(s.diagnosing.symptom.as_deref(), Some(r.symptom.as_str()));
    assert_eq!(s.diagnosing.cause.as_deref(), Some(r.cause.as_str()));
    assert_eq!(s.diagnosing.where_.as_deref(), Some(r.where_.as_str()));
    assert_eq!(s.diagnosing.proof.as_deref(), Some(r.proof.as_str()));
}

#[test]
fn a_failed_call_is_said_once_without_the_error_kind() {
    // The real-model run, 30 Sep 2026: "(platform: platform: I couldn't reach
    // the model at 127.0.0.1:8093: Connection refused. I'll try again with
    // your next message), so the answer you asked for is missing. I'll try
    // again with your next message."
    let w = atlas::daemon::model_failed_words(
        "Model unreachable: platform: platform: I couldn't reach the model at 127.0.0.1:8093: Connection refused (os error 111). I'll try again with your next message",
    );
    assert_eq!(w.matches("try again").count(), 1, "{w}");
    assert!(!w.to_lowercase().contains("platform:"), "{w}");
    assert!(w.contains("Connection refused"), "{w}");
}

// ---- 30 Sep 2026: offline first, online second, and what was measured.

/// A free online service's answer is read, and a refusal is told apart
/// from a failure (a refusal rests the service for less time).
#[test]
fn a_free_online_answer_and_a_refusal_are_read() {
    use atlas::freeonline::reply_from;
    assert_eq!(reply_from(r#"{"choices":[{"message":{"content":"Keep the vents clear."}}]}"#), Ok("Keep the vents clear.".into()));
    assert!(matches!(reply_from(r#"{"message":"API rate limit exceeded"}"#), Err((true, _))));
    assert!(matches!(reply_from(r#"{"error":{"message":"model not found"}}"#), Err((false, _))));
    assert!(matches!(reply_from("<html>502</html>"), Err((false, _))));
    // Thinking out loud isn't part of the answer.
    assert_eq!(reply_from(r#"{"choices":[{"message":{"content":"<think>hm</think>Yes."}}]}"#), Ok("Yes.".into()));
}

/// The services are asked in turn; one that refused is rested and the next
/// answers; nothing personal leaves the machine as written.
#[test]
fn the_free_online_models_are_asked_in_turn_and_nothing_personal_leaves() {
    use atlas::freeonline::{FreeOnline, Provider};
    use std::sync::{Arc, Mutex};
    let sent: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let log = sent.clone();
    let providers = vec![
        Provider { name: "A", url: "https://a.example/chat", model: "a" },
        Provider { name: "B", url: "https://b.example/chat", model: "b" },
    ];
    let online = FreeOnline::with_sender(
        providers,
        Box::new(move |url: &str, body: &str| {
            log.lock().unwrap().push((url.to_string(), body.to_string()));
            if url.contains("a.example") {
                Ok(r#"{"message":"API rate limit exceeded"}"#.to_string())
            } else {
                Ok(r#"{"choices":[{"message":{"content":"Sent to ⟦EMAIL_1⟧."}}]}"#.to_string())
            }
        }),
    );
    let got = online.ask("You are Atlas.", "email eric@example.com about lunch").unwrap();
    assert_eq!(got, "Sent to eric@example.com.", "the placeholder is put back");
    let first: Vec<(String, String)> = sent.lock().unwrap().clone();
    assert_eq!(first.len(), 2, "A refused, B answered");
    assert!(first.iter().all(|(_, b)| !b.contains("eric@example.com")), "the address never left: {first:?}");
    assert_eq!(*online.last_answered_by.lock().unwrap(), Some("B"));
    // A is resting now: the next question goes straight to B.
    let _ = online.ask("You are Atlas.", "hello");
    let after: Vec<String> = sent.lock().unwrap().iter().skip(2).map(|(u, _)| u.clone()).collect();
    assert_eq!(after, vec!["https://b.example/chat".to_string()]);
}

/// The setting that keeps everything offline defaults to allowing the free
/// online models only as the second choice, and is in tools.yaml.
#[test]
fn online_second_is_a_setting_and_on_by_default() {
    assert!(atlas::models::ModelsConfig::default().online_second);
    let c = atlas::config::Config::load(std::path::Path::new("config")).unwrap();
    assert!(c.tools.as_ref().unwrap().models.online_second);
    let yaml = std::fs::read_to_string("config/tools.yaml").unwrap();
    assert!(yaml.contains("online_second: true"));
}

/// Talk stops at a spoken length unless something long was asked for:
/// on the laptop every model answered small talk in five to seven sentences.
#[test]
fn talk_is_spoken_length_unless_you_ask_for_more() {
    use atlas::register::asks_for_length;
    assert!(!asks_for_length("hey, how's it going?"));
    assert!(!asks_for_length("what's a good way to get better at guitar?"));
    assert!(asks_for_length("tell me a story about a lighthouse"));
    assert!(asks_for_length("give me three ideas for dinner"));
    assert!(asks_for_length("explain how vaccines work in detail"));
    assert_eq!(atlas::register::CHAT_SENTENCES, 4);
}

/// The prompt forbids what the models did on the laptop: made-up shared
/// history, and remarks on the hour.
#[test]
fn the_prompt_forbids_made_up_history() {
    let c = atlas::persona::Persona::default().character();
    assert!(c.contains("Never invent past events, shared memories"), "{c}");
    assert!(c.contains("Don't mention the time of day"));
}

/// The bench's checks catch what a person heard as wrong.
#[test]
fn the_talk_bench_catches_made_up_history() {
    use atlas::talkbench::faults_in;
    assert!(faults_in("How about ordering from that Thai place you like?", 5).iter().any(|f| f.contains("invents")));
    assert!(faults_in("I can -- and I'm already doing it.", 5).iter().any(|f| f.contains("invents") || f.contains("claims")));
    assert!(faults_in("Sure. Keep the vents clear.", 5).is_empty());
}

// ---- 30 Sep 2026: hearing with Parakeet (a quarter of whisper base.en's
// mistakes up close, a third across the room, measured on LibriSpeech).

/// The request is what sherpa-onnx's offline server reads: rate, byte count,
/// then f32 samples, little-endian.
#[test]
fn a_recording_is_sent_the_way_the_hearing_server_reads_it() {
    let b = atlas::parakeet::request_bytes(&[0.5, -0.25], 16_000);
    assert_eq!(&b[0..4], &16_000i32.to_le_bytes());
    assert_eq!(&b[4..8], &8i32.to_le_bytes());
    assert_eq!(&b[8..12], &0.5f32.to_le_bytes());
    assert_eq!(&b[12..16], &(-0.25f32).to_le_bytes());
    assert_eq!(atlas::parakeet::text_from(r#"{"lang": "", "text": " Can you see me? ", "timestamps": []}"#).as_deref(), Some("Can you see me?"));
    assert_eq!(atlas::parakeet::text_from("not json"), None);
}

/// Only counted as installed when the server and all four model files are
/// there; the server is told where each file is and how many threads to use.
#[test]
fn parakeet_is_installed_only_when_every_file_is_there() {
    let root = std::env::temp_dir().join(format!("atlas-parakeet-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let bin = root.join("tools/sherpa/bin");
    let model = root.join("models/parakeet");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&model).unwrap();
    std::fs::write(bin.join(atlas::parakeet::server_name()), b"").unwrap();
    for f in ["encoder.int8.onnx", "decoder.int8.onnx", "joiner.int8.onnx"] {
        std::fs::write(model.join(f), b"").unwrap();
    }
    assert!(atlas::parakeet::installed(&root).is_none(), "tokens.txt is missing");
    std::fs::write(model.join("tokens.txt"), b"").unwrap();
    let files = atlas::parakeet::installed(&root).expect("all there");
    let args = atlas::parakeet::launch_args(&files, 8094, 3).join(" ");
    assert!(args.contains("--port=8094") && args.contains("--model-type=nemo_transducer") && args.contains("--num-threads=3"), "{args}");
    assert!(args.contains("encoder.int8.onnx") && args.contains("tokens.txt"));
    assert_eq!(atlas::parakeet::threads_for(8), 4);
    assert_eq!(atlas::parakeet::threads_for(2), 2);
    let _ = std::fs::remove_dir_all(&root);
}

/// `atlas get hearing` fetches the server and the model, pinned.
#[test]
fn getting_hearing_fetches_the_server_and_the_model_pinned() {
    let (_, pieces) = atlas::getpieces::set(Some("hearing")).expect("a set called hearing");
    assert_eq!(pieces.len(), 2);
    assert!(pieces.iter().all(|p| p.sha256.len() == 64 && p.url.starts_with("https://github.com/k2-fsa/sherpa-onnx/releases/download/")));
    assert!(pieces.iter().any(|p| p.key_path() == "models/parakeet/encoder.int8.onnx"));
    assert!(pieces.iter().any(|p| p.key_path().starts_with("tools/sherpa/bin/sherpa-onnx-offline-websocket-server")));
    let yaml = std::fs::read_to_string("config/tools.yaml").unwrap();
    assert!(yaml.contains("stt_engine: auto"));
}

/// The real server and model, through Atlas's own client, when the kit is
/// here (`ATLAS_PARAKEET_KIT`: a folder holding `tools/sherpa` and
/// `models/parakeet`, and a `speech.wav` with its words in `speech.txt`).
#[test]
fn parakeet_hears_real_speech_through_atlas_own_client() {
    let Ok(kit) = std::env::var("ATLAS_PARAKEET_KIT") else { return };
    let kit = std::path::PathBuf::from(kit);
    let started = std::time::Instant::now();
    let heard = atlas::parakeet::transcribe_file(&kit, &kit.join("speech.wav")).expect("installed").expect("heard");
    let first = started.elapsed();
    let again = std::time::Instant::now();
    let heard2 = atlas::parakeet::transcribe_file(&kit, &kit.join("speech.wav")).unwrap().unwrap();
    let second = again.elapsed();
    atlas::parakeet::stop();
    let want = std::fs::read_to_string(kit.join("speech.txt")).unwrap().to_lowercase();
    let norm = |s: &str| s.to_lowercase().chars().filter(|c| c.is_alphanumeric() || *c == ' ').collect::<String>();
    let (h, w) = (norm(&heard), norm(&want));
    let hit = w.split_whitespace().filter(|x| h.split_whitespace().any(|y| y == *x)).count();
    println!("first {first:?} (starts the server), then {second:?}: {heard}");
    assert!(hit * 10 >= w.split_whitespace().count() * 8, "heard {heard:?}, said {want:?}");
    assert_eq!(heard, heard2);
    assert!(second < first);
}

/// Eric, 30 Sep 2026: "Atlas can still freely talk I just want a question
/// answered when I ask or task completed when I ask." The answer comes
/// first; talk after it is still welcome.
#[test]
fn a_question_is_answered_first_and_talk_is_still_free() {
    let p = atlas::persona::Persona::default();
    let c = p.character();
    assert!(c.contains("A question gets its answer, in your first sentence"), "{c}");
    assert!(c.contains("Never answer a question with a question"));
    assert!(c.contains("After that you're free to talk"));
    let chat = p.for_this_turn_on(atlas::register::Register::Chatting, 3, "what's your favourite film", false);
    // Merged 30 Sep 2026: answering first is in the turn's own line; the
    // freedom to talk after is in the character ("After that you're free to
    // talk"). "Go with a tangent" isn't said: a 4B model on this laptop
    // answered every sentence with one and three questions (29 Sep 2026).
    assert!(chat.contains("first"), "{chat}");
    use atlas::talkbench::question_dodged;
    assert!(question_dodged("give me three ideas for dinner tonight", "What's your mood? Something simple?").is_some());
    assert!(question_dodged("give me three ideas for dinner tonight", "Pasta, soup, or eggs. Want the recipe?").is_none());
    assert!(question_dodged("I've had a long day", "Rough one? I'm here.").is_none());
}
