//! Atlas running on Windows with no terminal and no window open (Eric, 28
//! Sep 2026): *"I don't want a command terminal to be open. When on windows I
//! don't even want to have the hub or the application open for Atlas to
//! run."*
//!
//! What is tested here is everything that can be decided off Windows: the
//! scheduled task's XML (no three-day limit, runs on battery, one copy, the
//! install folder), the switch-on-once rule that lets an untick stick, what
//! opening Atlas does, the tray menu's entries and what each does, and the
//! setting. The Win32 icon itself (`notifyicon::win`) only cross-compiles here.

use atlas::firstlaunch::{what_opening_does, First};
use atlas::startup::{self, AfterSetup, Mode};
use atlas::notifyicon::{self as tray, TrayAction};
use std::path::PathBuf;

fn scratch(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-nothing-open-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

// ================= the scheduled task =================

fn eric_task() -> String {
    let exe = PathBuf::from(r"C:\Users\erics\AppData\Local\Atlas\atlas.exe");
    startup::task_xml(&exe, Mode::Background, Some(r"LAPTOP\erics"))
}

fn between<'a>(xml: &'a str, tag: &str) -> Vec<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    xml.split(&open).skip(1).filter_map(|rest| rest.split_once(&close).map(|(v, _)| v)).collect()
}

#[test]
fn the_task_is_never_ended_after_three_days() {
    // `schtasks /Create /SC ONLOGON` gives a task the default
    // ExecutionTimeLimit of 72 hours, and Task Scheduler ends the program when
    // it's reached: Atlas would have been killed three days after sign-in.
    let xml = eric_task();
    assert_eq!(between(&xml, "ExecutionTimeLimit"), vec!["PT0S"], "{xml}");
}

#[test]
fn the_task_runs_on_battery_and_keeps_running_when_the_lead_comes_out() {
    let xml = eric_task();
    assert_eq!(between(&xml, "DisallowStartIfOnBatteries"), vec!["false"]);
    assert_eq!(between(&xml, "StopIfGoingOnBatteries"), vec!["false"]);
}

#[test]
fn the_task_starts_one_atlas_at_your_sign_in_with_nothing_elevated() {
    let xml = eric_task();
    assert_eq!(between(&xml, "MultipleInstancesPolicy"), vec!["IgnoreNew"]);
    assert!(xml.contains("<LogonTrigger>"), "{xml}");
    // Your sign-in, not anyone's; and only while you're signed in
    // (InteractiveToken: your desktop, your microphone, no stored password).
    assert_eq!(between(&xml, "UserId"), vec![r"LAPTOP\erics", r"LAPTOP\erics"]);
    assert_eq!(between(&xml, "LogonType"), vec!["InteractiveToken"]);
    assert_eq!(between(&xml, "RunLevel"), vec!["LeastPrivilege"]);
    // Normal priority: a task's default (7) starts the program below normal.
    assert_eq!(between(&xml, "Priority"), vec!["5"]);
}

#[test]
fn the_task_runs_atlas_itself_from_its_own_folder_and_never_a_console() {
    let xml = eric_task();
    assert_eq!(between(&xml, "Command"), vec![r"C:\Users\erics\AppData\Local\Atlas\atlas.exe"]);
    assert_eq!(between(&xml, "Arguments"), vec!["--daemon"]);
    assert_eq!(between(&xml, "WorkingDirectory"), vec![r"C:\Users\erics\AppData\Local\Atlas"]);
    let lower = xml.to_lowercase();
    for console in ["cmd", ".bat", "powershell", "conhost"] {
        assert!(!lower.contains(console), "the task goes through {console}: {xml}");
    }
}

#[test]
fn a_folder_with_an_ampersand_still_makes_a_task_windows_can_read() {
    let exe = PathBuf::from(r"C:\Users\Tom & Jo\Atlas\atlas.exe");
    let xml = startup::task_xml(&exe, Mode::Background, Some("PC\\tom&jo"));
    assert!(xml.contains(r"C:\Users\Tom &amp; Jo\Atlas\atlas.exe"), "{xml}");
    assert!(!xml.contains("Tom & Jo"), "a bare & makes the XML unreadable: {xml}");
    assert!(xml.contains("tom&amp;jo"), "{xml}");
    // Every element that opens, closes.
    for tag in ["Task", "Triggers", "LogonTrigger", "Principals", "Principal", "Settings", "Actions", "Exec", "IdleSettings"] {
        let opens = xml.matches(&format!("<{tag}>")).count() + xml.matches(&format!("<{tag} ")).count();
        let closes = xml.matches(&format!("</{tag}>")).count();
        assert_eq!(opens, closes, "<{tag}> opens {opens} times and closes {closes}");
    }
}

#[test]
fn with_nobody_named_the_trigger_is_any_sign_in() {
    let xml = startup::task_xml(&PathBuf::from(r"C:\A\atlas.exe"), Mode::Background, None);
    assert!(between(&xml, "UserId").is_empty(), "{xml}");
    assert!(xml.contains("<LogonTrigger>"));
}

#[test]
fn the_task_file_is_what_its_declaration_says_it_is() {
    // UTF-16 with its byte-order mark: what the declaration says, and what
    // Task Scheduler writes itself.
    let xml = eric_task();
    assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-16\"?>"));
    let bytes = startup::task_file_bytes(&xml);
    assert_eq!(&bytes[..2], &[0xFF, 0xFE]);
    let units: Vec<u16> = bytes[2..].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    assert_eq!(String::from_utf16(&units).unwrap(), xml);
}

#[test]
fn the_task_file_is_written_where_the_plan_reads_it() {
    let root = scratch("taskfile");
    let exe = root.join("atlas.exe");
    let written = startup::write_task_file(&exe, Mode::Background).unwrap();
    assert_eq!(written, startup::task_file_path(&exe));
    assert_eq!(written, root.join("data").join("state").join("atlas-task.xml"));
    let bytes = std::fs::read(&written).unwrap();
    assert_eq!(&bytes[..2], &[0xFF, 0xFE]);
    let plan = startup::register(&exe, Mode::Background);
    if plan.program == "schtasks" {
        let at = plan.args.iter().position(|a| a == "/XML").expect("made from the XML");
        assert_eq!(plan.args[at + 1], written.display().to_string());
        assert!(plan.args.iter().any(|a| a == "/F"));
        assert!(!plan.args.iter().any(|a| a == "/SC" || a == "/TR"), "{:?}", plan.args);
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_fallback_sign_in_entry_runs_atlas_itself_quoted() {
    let exe = PathBuf::from(r"C:\Program Files\Atlas\atlas.exe");
    let add = startup::run_entry_add(&exe, Mode::Background);
    assert_eq!(add.program, "reg");
    assert!(add.args.contains(&r#""C:\Program Files\Atlas\atlas.exe" --daemon"#.to_string()), "{:?}", add.args);
    assert!(add.args.iter().any(|a| a.starts_with(r"HKCU\")), "per-user, never the machine: {:?}", add.args);
    let del = startup::run_entry_remove();
    assert_eq!(add.args[1], del.args[1], "adding and removing name different places");
    assert!(del.args.contains(&startup::TASK_NAME.to_string()));
}

// ================= switched on once; an untick sticks =================

#[test]
fn after_setup_on_windows_it_starts_with_windows_once_and_starts_now() {
    assert_eq!(
        startup::after_setup(true, true, true, None, false),
        AfterSetup { register: true, start: true }
    );
    // Already running: nothing to start.
    assert_eq!(startup::after_setup(true, true, true, None, true), AfterSetup { register: true, start: false });
}

#[test]
fn unticking_it_later_is_never_undone_by_setup() {
    // Eric turned it off: setup walking again doesn't turn it back on.
    assert_eq!(startup::after_setup(true, true, false, Some(false), true), AfterSetup { register: false, start: false });
    assert!(!startup::after_setup(true, true, true, Some(false), false).register);
    // And on stays on without being registered again every open.
    assert!(!startup::after_setup(true, true, false, Some(true), true).register);
}

#[test]
fn nothing_happens_off_windows_or_without_readable_settings() {
    assert_eq!(startup::after_setup(false, true, true, None, false), AfterSetup { register: false, start: false });
    // The background Atlas stops at once without its settings.
    assert_eq!(startup::after_setup(true, false, true, None, false), AfterSetup { register: false, start: false });
}

#[test]
fn a_later_open_of_the_window_leaves_starting_to_the_opening() {
    // Not the walk that finished setup: the window's opening already started
    // it (`what_opening_does`); starting it here too would race that one.
    assert!(!startup::after_setup(true, true, false, Some(true), false).start);
}

#[test]
fn what_was_decided_is_remembered_on_disk() {
    let state = scratch("decided");
    assert_eq!(startup::decided(&state), None, "nothing decided yet");
    startup::remember(&state, true).unwrap();
    assert_eq!(startup::decided(&state), Some(true));
    startup::remember(&state, false).unwrap();
    assert_eq!(startup::decided(&state), Some(false));
    // So the next setup leaves it off.
    assert!(!startup::after_setup(true, true, true, startup::decided(&state), false).register);
    std::fs::write(startup::decision_file(&state), "garbled").unwrap();
    assert_eq!(startup::decided(&state), None);
    let _ = std::fs::remove_dir_all(&state);
}

// ================= opening Atlas =================

#[test]
fn opening_a_set_up_atlas_that_isnt_running_starts_it_then_opens_the_hub() {
    let o = what_opening_does(true, false, First::Home);
    assert!(o.start_background);
    assert_eq!(o.first, First::Hub("/hub".into()));
}

#[test]
fn opening_a_running_atlas_just_opens_the_hub() {
    let o = what_opening_does(true, true, First::Home);
    assert!(!o.start_background);
    assert_eq!(o.first, First::Hub("/hub".into()));
}

#[test]
fn opening_before_setup_is_the_setup_window_as_before() {
    let o = what_opening_does(false, false, First::Home);
    assert!(!o.start_background, "setup starts it itself when it finishes");
    assert_eq!(o.first, First::Home);
}

#[test]
fn a_page_asked_for_is_kept() {
    let o = what_opening_does(true, false, First::Settings);
    assert!(o.start_background);
    assert_eq!(o.first, First::Settings);
    assert_eq!(what_opening_does(true, true, First::Hub("/hub/outstanding".into())).first, First::Hub("/hub/outstanding".into()));
}

// ================= the icon by the clock =================

#[test]
fn every_menu_entry_does_what_it_says() {
    for paused in [false, true] {
        let menu = tray::tray_menu(paused);
        let words: Vec<&str> = menu.iter().map(|(_, w)| *w).collect();
        for (id, w) in &menu {
            let a = tray::tray_choice(*id, paused).unwrap_or_else(|| panic!("{w} does nothing"));
            let expect = match *w {
                "Open Atlas" => TrayAction::OpenAtlas,
                "Open the hub in my browser" => TrayAction::OpenHubInBrowser,
                // 28 Sep 2026: the entry says Pause stops the microphone
                // too, since now it does (`micthread`).
                "Pause Atlas and stop listening" => TrayAction::Pause,
                "Resume Atlas and listen again" => TrayAction::Resume,
                "Quit Atlas" => TrayAction::Quit,
                other => panic!("an entry nobody mapped: {other}"),
            };
            assert_eq!(a, expect, "{w}");
        }
        assert_eq!(words.first(), Some(&"Open Atlas"));
        assert_eq!(words.last(), Some(&"Quit Atlas"));
        assert!(words.contains(&if paused { tray::PAUSED_ENTRY } else { tray::RUNNING_ENTRY }), "{words:?}");
        assert!(!words.contains(&if paused { tray::RUNNING_ENTRY } else { tray::PAUSED_ENTRY }), "{words:?}");
    }
    // Clicking away from the menu picks 0: nothing happens.
    assert_eq!(tray::tray_choice(0, false), None);
}

#[test]
fn the_icon_says_whether_atlas_is_paused() {
    assert_eq!(tray::tray_tooltip(false), "Atlas — running");
    // 28 Sep 2026: paused means not listening, and the tip says so.
    assert_eq!(tray::tray_tooltip(true), "Atlas — paused, not listening");
    // Windows keeps 127 characters of a tip.
    assert!(tray::tray_tooltip(false).encode_utf16().count() < 128);
}

#[test]
fn pause_and_resume_reach_the_run_loop_and_nothing_else_does() {
    // The icon's thread never touches the daemon: it asks, and the run loop
    // empties the asks once a pass.
    let _ = tray::tray_asks();
    tray::tray_ask(TrayAction::Pause);
    tray::tray_ask(TrayAction::Resume);
    assert_eq!(tray::tray_asks(), vec![TrayAction::Pause, TrayAction::Resume]);
    assert!(tray::tray_asks().is_empty(), "asks are taken once");
    let daemon = crate::common::source_of("daemon");
    let body = daemon.split_once("fn answer_tray(").expect("answer_tray").1.split("\n    fn ").next().unwrap().to_string();
    assert!(body.contains("tray_asks()"));
    assert!(body.contains(r#"self.turn("pause""#), "not the hub's Pause: {body}");
    assert!(body.contains(r#"self.turn("carry on""#), "not the hub's Carry on: {body}");
    assert!(body.contains("tray_paused_now(self.attention.is_paused())"));
    assert!(daemon.contains("self.answer_tray(clock());"), "the run loop never asks");
}

#[test]
fn quitting_from_the_icon_is_the_clean_way_out() {
    let src = std::fs::read_to_string("src/notifyicon.rs").unwrap();
    let quit = src.split_once("TrayAction::Quit => {").expect("Quit handled").1.split('}').next().unwrap().to_string();
    assert!(quit.contains("goodbye::please_stop()"), "{quit}");
    // Removed on the way out, and put back when Explorer restarts.
    assert!(src.contains("NIM_DELETE"));
    assert!(src.contains("TaskbarCreated"));
}

#[test]
fn the_background_atlas_puts_up_the_icon_when_the_setting_says_so() {
    let main = crate::common::source_of("main");
    let body = main.split_once("fn run_daemon(").expect("run_daemon").1.split("\nfn ").next().unwrap().to_string();
    assert!(body.contains("tc.desktop.tray_icon"), "the setting isn't read");
    assert!(body.contains("atlas::notifyicon::show_icon("), "no icon");
    // Held across the run loop, not dropped at once.
    let icon = body.find("let _tray =").expect("held in a named binding, not `let _`");
    let run = body.find("d.run(").expect("the run loop");
    assert!(icon < run);
}

#[test]
fn the_setting_is_on_by_default_and_in_the_list() {
    let shipped: atlas::voice::ToolsConfig =
        serde_yaml::from_str(&std::fs::read_to_string("config/tools.yaml").unwrap()).unwrap();
    assert!(shipped.desktop.tray_icon, "off in the shipped file");
    assert!(atlas::voice::ToolsConfig::default().desktop.tray_icon, "off by default");
    let s = atlas::settings::registry(&shipped);
    let it = s.get("desktop.tray_icon").expect("in the settings list");
    assert_eq!(it.value, atlas::settings::Value::Toggle(true));
    // The icon is put up once, at the start.
    assert!(atlas::settings::needs_a_restart("desktop.tray_icon"));
}

// ================= no console anywhere =================

#[test]
fn opening_atlas_starts_the_background_atlas_with_no_window() {
    let main = crate::common::source_of("main");
    let body = main.split_once("fn run_home(").expect("run_home").1.split("\nfn ").next().unwrap().to_string();
    assert!(body.contains("what_opening_does("));
    assert!(body.contains(r#"spawn_quietly(&exe, &["--daemon"])"#), "{body}");
    let setup = std::fs::read_to_string("src/setupwin.rs").unwrap();
    let after = setup.split_once("fn after_setup(").expect("after_setup").1.split("\nfn ").next().unwrap().to_string();
    assert!(after.contains("startup::turn_on("));
    assert!(after.contains(r#"spawn_quietly(&place.exe, &["--daemon"])"#));
    assert!(after.contains("startup::remember(&state, true)"));
}

#[test]
fn every_way_atlas_starts_goes_to_atlas_exe_and_never_a_console() {
    // The shortcuts: atlas.exe itself, no arguments (a double-click).
    let fl = std::fs::read_to_string("src/firstlaunch.rs").unwrap();
    let make = fl.split_once("fn make_shortcut(exe: &Path, lnk: &Path)").expect("make_shortcut").1.split("\n}").next().unwrap().to_string();
    assert!(make.contains("SetPath(&HSTRING::from(exe.as_os_str()))"), "{make}");
    assert!(!make.contains("SetArguments"), "{make}");
    // Background starts go through the no-window door.
    let quiet = fl.split_once("fn hidden_command(").unwrap().1.split("\n}").next().unwrap().to_string();
    assert!(quiet.contains("CREATE_NO_WINDOW"));
    // Nothing starts a console program without CREATE_NO_WINDOW from the
    // windowless Atlas: the three that did (a console flashed up).
    let win = std::fs::read_to_string("src/platform/win.rs").unwrap();
    for p in ["taskkill", "powershell", "clip"] {
        assert!(!win.contains(&format!("Command::new(\"{p}\")")), "{p} opens a console window");
    }
    // And nothing points at a batch file or cmd to start Atlas.
    for f in ["src/startup.rs", "src/setupwin.rs", "src/notifyicon.rs"] {
        let s = std::fs::read_to_string(f).unwrap();
        let code: String = s.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        assert!(!code.contains("ATLAS.bat") && !code.contains("\"cmd\"") && !code.contains("cmd.exe"), "{f}");
    }
}

#[test]
fn what_eric_is_given_is_atlas_exe_not_the_batch_file() {
    let wf = std::fs::read_to_string("../.github/workflows/windows.yml").unwrap_or_default();
    if wf.is_empty() {
        return; // the workflow lives beside the crate in the repo only
    }
    assert!(!wf.contains("ATLAS.bat"), "the package ships ATLAS.bat");
    assert!(wf.contains(r#"cp dist/atlas.exe "friend/Atlas Setup.exe""#));
}

