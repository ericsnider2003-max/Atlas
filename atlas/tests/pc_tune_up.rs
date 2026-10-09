//! "Make it run well", held to what it may and may not touch.
//!
//! Eric, 2 Oct 2026: "can't properly optimize my PC... For optimization I
//! want Atlas to be able to do more as it is a very tedious process. Like
//! closing things in the task manager that aren't needed, moving files, doing
//! deeper dives and actually making it run well."
//!
//! Closing programs is the one place a bad rule does real damage on someone
//! else's machine, so the choosing is tested on process lists made up here --
//! nothing in this file ever closes, ends or switches off anything real.
//! What it proves: Windows' own processes are never offered, nor Atlas (by
//! process, by child, or by whatever its program is called on that machine),
//! nor the program in front of you, nor a helper of another program, nor
//! anything you've just used; the CPU numbers are the share of the machine
//! Task Manager would show; startup entries an administrator owns, or that
//! start Atlas, are never offered; and moved files come back through "undo".

use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use atlas::tune::{
    atlas_family, cpu_share, look_at_space, may_move_into, move_destination, move_files_into_controlled, parse_logon_tasks,
    parse_reg_values, pick_startup_to_stop, pick_to_close, sample_load, startup_switched_off, startup_words, tune_ask, Load,
    Plan, Proc, Spare, StartupEntry, StartupFrom, TuneAsk, TuneUndo,
};
use std::path::{Path, PathBuf};

fn proc(pid: u32, parent: u32, name: &str, mem_mb: u64, cpu_ms: u64, windowed: bool) -> Proc {
    Proc { pid, parent, name: name.into(), session: 1, mem_mb, cpu_ms, windowed }
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-pc-tune-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

// ---------- CPU, measured over a stretch ----------

#[test]
fn cpu_is_the_share_of_the_whole_machine_over_the_window() {
    // Two cores' worth for the whole second, on an eight-core machine: 25%.
    assert_eq!(cpu_share(2000, 1000, 8), 25.0);
    // One core flat out on four cores over two seconds.
    assert_eq!(cpu_share(2000, 2000, 4), 25.0);
    assert_eq!(cpu_share(0, 2000, 4), 0.0);
    // Nothing measured is no reading, not a division by zero.
    assert_eq!(cpu_share(500, 0, 4), 0.0);
    assert_eq!(cpu_share(500, 1000, 0), 0.0);
    // A reading can't come out above the whole machine.
    assert_eq!(cpu_share(9000, 1000, 2), 100.0);
}

#[test]
fn a_programs_cpu_is_what_its_processes_used_between_the_two_readings() {
    let before = vec![
        proc(10, 1, "explorer", 100, 50_000, true),
        proc(20, 10, "chrome", 300, 10_000, true),
        proc(21, 20, "chrome", 200, 4_000, false),
        proc(30, 10, "gone", 50, 999, false),
        proc(40, 10, "oldname", 10, 70_000, false),
    ];
    let after = vec![
        proc(10, 1, "explorer", 100, 50_100, true),
        proc(20, 10, "chrome", 320, 11_000, true),
        proc(21, 20, "chrome", 210, 5_000, false),
        // Started in between: all its CPU time was used in the window.
        proc(22, 20, "chrome", 90, 500, false),
        // Windows reused number 40 for something else: its time isn't
        // read as the old process's (which would have come out negative).
        proc(40, 10, "newthing", 10, 300, false),
    ];
    let load = sample_load(&before, &after, 2000, 4);
    let chrome = load.iter().find(|l| l.name == "chrome").unwrap();
    // 1000 + 1000 + 500 ms over 2 s on 4 cores = 31.25%.
    assert!((chrome.cpu_pct - 31.25).abs() < 0.01, "{}", chrome.cpu_pct);
    assert_eq!(chrome.mem_mb, 320 + 210 + 90, "memory is now, added up");
    assert_eq!(chrome.pids.len(), 3);
    assert!(chrome.windowed && chrome.alone);
    let newthing = load.iter().find(|l| l.name == "newthing").unwrap();
    assert!((newthing.cpu_pct - cpu_share(300, 2000, 4)).abs() < 0.01);
    assert!(load.iter().all(|l| l.name != "gone"), "a process that ended is no one's load");
    assert_eq!(load[0].name, "chrome", "busiest first");
}

#[test]
fn services_and_helpers_are_marked_as_such() {
    let after = vec![
        Proc { pid: 4, parent: 0, name: "System".into(), session: 0, mem_mb: 5, cpu_ms: 0, windowed: false },
        Proc { pid: 700, parent: 4, name: "services".into(), session: 0, mem_mb: 10, cpu_ms: 0, windowed: false },
        Proc { pid: 800, parent: 700, name: "svchost".into(), session: 0, mem_mb: 60, cpu_ms: 0, windowed: false },
        // In your session, but started by a service: still Windows'.
        proc(900, 800, "SomeBroker", 400, 0, false),
        proc(1000, 1, "explorer", 100, 0, true),
        // A tray program started by the shell stands on its own.
        proc(1100, 1000, "Steam", 600, 0, false),
        // Its web helper doesn't: closing it would break Steam, not free it.
        proc(1200, 1100, "steamwebhelper", 700, 0, false),
        // Whatever started this one is gone.
        proc(1300, 5555, "Orphan", 300, 0, false),
    ];
    let load = sample_load(&after, &after, 1000, 4);
    let get = |n: &str| load.iter().find(|l| l.name == n).unwrap().clone();
    assert!(get("svchost").system && get("services").system && get("System").system);
    assert!(get("SomeBroker").system, "started by a service");
    assert!(!get("Steam").system && get("Steam").alone);
    assert!(!get("steamwebhelper").alone, "a helper of another program");
    assert!(get("Orphan").alone);
}

// ---------- choosing what to close ----------

fn heavy(name: &str, pid: u32, mem_mb: u64, cpu: f32, windowed: bool) -> Load {
    Load { name: name.into(), pids: vec![pid], cpu_pct: cpu, mem_mb, windowed, system: false, alone: true }
}

fn spare() -> Spare {
    Spare { min_mb: 200, min_cpu: 5.0, keep: vec!["onedrive".into()], ..Default::default() }
}

#[test]
fn windows_own_processes_and_your_keep_list_are_never_offered() {
    let load = vec![
        heavy("explorer", 1, 900, 30.0, true),
        heavy("MsMpEng", 2, 800, 40.0, false),
        heavy("svchost", 3, 700, 20.0, false),
        heavy("dwm", 4, 600, 25.0, false),
        heavy("SearchIndexer", 5, 500, 15.0, false),
        heavy("msiexec", 6, 500, 15.0, false),
        heavy("powershell", 7, 500, 15.0, true),
        heavy("OneDrive", 8, 900, 9.0, false),
        heavy("Discord", 9, 410, 2.0, true),
    ];
    let picked = pick_to_close(&load, &spare());
    let names: Vec<&str> = picked.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, vec!["Discord"], "only the program that is nobody's");
    // The system flag alone is enough, whatever the name.
    let mut svc = heavy("UnheardOf", 10, 2000, 50.0, false);
    svc.system = true;
    assert!(pick_to_close(&[svc], &spare()).is_empty());
}

#[test]
fn atlas_is_never_offered_by_name_by_process_or_by_child() {
    let mut s = spare();
    s.own_pids = vec![500, 501];
    // Renamed on a friend's machine: its own program name is spared too.
    s.own_names = vec!["MyHelper".into()];
    let load = vec![
        heavy("atlas", 500, 900, 30.0, true),
        heavy("Atlas-Hub", 499, 900, 30.0, true),
        heavy("MyHelper", 777, 900, 30.0, true),
        // A child of Atlas, whatever it's called.
        heavy("node", 501, 900, 30.0, false),
        heavy("llama-server", 502, 4000, 80.0, false),
        heavy("whisper", 503, 900, 30.0, false),
        heavy("Spotify", 600, 400, 1.0, true),
    ];
    let picked = pick_to_close(&load, &s);
    assert_eq!(picked.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), vec!["Spotify"]);
}

#[test]
fn atlas_family_is_every_descendant_and_survives_a_loop() {
    let procs = vec![
        proc(100, 1, "atlas", 0, 0, true),
        proc(101, 100, "llama-server", 0, 0, false),
        proc(102, 101, "conhost", 0, 0, false),
        proc(103, 102, "grandchild", 0, 0, false),
        proc(200, 1, "Discord", 0, 0, true),
        // Two processes naming each other as parent (reused numbers).
        proc(300, 301, "a", 0, 0, false),
        proc(301, 300, "b", 0, 0, false),
    ];
    let mut fam = atlas_family(&procs, 100);
    fam.sort();
    assert_eq!(fam, vec![100, 101, 102, 103]);
    let mut looped = atlas_family(&procs, 300);
    looped.sort();
    assert_eq!(looped, vec![300, 301]);
}

#[test]
fn the_program_in_front_of_you_is_never_offered() {
    let load = vec![heavy("chrome", 42, 3000, 20.0, true), heavy("Teams", 43, 900, 3.0, true)];
    let mut s = spare();
    s.foreground = Some("chrome.exe".into());
    assert_eq!(pick_to_close(&load, &s).iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), vec!["Teams"]);
    // By process number too, when the name couldn't be read.
    let mut s = spare();
    s.foreground_pid = Some(43);
    assert_eq!(pick_to_close(&load, &s).iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), vec!["chrome"]);
}

#[test]
fn helpers_light_programs_and_what_you_just_used_are_left() {
    let mut helper = heavy("steamwebhelper", 1, 900, 10.0, false);
    helper.alone = false;
    let load = vec![
        helper,
        heavy("Notepad", 2, 20, 0.0, true),
        heavy("Code", 3, 1500, 4.0, true),
        heavy("Steam", 4, 600, 0.5, false),
        // Light on memory but busy on the processor: worth naming.
        heavy("Updater", 5, 40, 22.0, false),
    ];
    let mut s = spare();
    s.in_use = vec!["Code.exe".into(), "".into(), "x".into()];
    let names: Vec<String> = pick_to_close(&load, &s).into_iter().map(|l| l.name).collect();
    assert_eq!(names, vec!["Updater".to_string(), "Steam".to_string()], "heaviest first: 40 + 22*50 > 600");
}

#[test]
fn at_most_six_are_offered_at_once() {
    let load: Vec<Load> = (0..12).map(|i| heavy(&format!("App{i}"), 100 + i, 300 + i as u64, 0.0, true)).collect();
    let picked = pick_to_close(&load, &spare());
    assert_eq!(picked.len(), 6);
    assert_eq!(picked[0].name, "App11");
}

#[test]
fn the_offer_names_each_with_its_numbers() {
    let plan = Plan {
        close: vec![heavy("Discord", 1, 410, 0.2, true), heavy("Steam", 2, 2048, 12.0, false)],
        ..Default::default()
    };
    let o = plan.offer();
    assert!(o.contains("close Discord (410 MB), Steam (2.0 GB, 12% CPU, in the background)"), "{o}");
    assert!(o.ends_with("Go ahead?"));
    let moves = Plan { moves: Some((vec![PathBuf::from("/nowhere/a.iso")], PathBuf::from("/data/archive"))), ..Default::default() };
    assert!(moves.offer().contains("move 1 file") && moves.offer().contains("\"undo\" moves them back"), "{}", moves.offer());
}

// ---------- startup ----------

#[test]
fn startup_entries_are_read_with_their_on_or_off_mark() {
    let run = "\r\nHKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run\r\n    Discord    REG_SZ    \"C:\\Users\\x\\Discord\\Update.exe\" --processStart Discord.exe\r\n    Steam Client    REG_SZ    C:\\Program Files (x86)\\Steam\\steam.exe -silent\r\n";
    let vals = parse_reg_values(run);
    assert_eq!(vals[1], ("Steam Client".to_string(), "C:\\Program Files (x86)\\Steam\\steam.exe -silent".to_string()));
    let approved = "\r\nHKEY_CURRENT_USER\\...\\StartupApproved\\Run\r\n    Discord    REG_BINARY    030000000A1B2C3D4E5F6071\r\n    Steam Client    REG_BINARY    020000000000000000000000\r\n    Old    REG_BINARY    070000000000000000000000\r\n";
    assert_eq!(startup_switched_off(approved), vec!["Discord".to_string(), "Old".to_string()]);
}

#[test]
fn sign_in_tasks_leave_out_windows_own() {
    let ps = "\\\tOneDriveUpdate\tReady\tC:\\x\\OneDriveSetup.exe\r\n\\Microsoft\\Windows\\UpdateOrchestrator\\\tSchedule Scan\tReady\tusoclient.exe\n\\Vendor\\\tHelperAtLogon\tDisabled\tC:\\v\\helper.exe\n";
    let t = parse_logon_tasks(ps);
    assert_eq!(t.len(), 2);
    assert_eq!(t[0].key, "\\OneDriveUpdate");
    assert!(t[0].on && t[0].from == StartupFrom::LogonTask);
    assert_eq!(t[1].key, "\\Vendor\\HelperAtLogon");
    assert!(!t[1].on);
}

fn entry(name: &str, from: StartupFrom, command: &str, on: bool) -> StartupEntry {
    StartupEntry { name: name.into(), from, command: command.into(), key: name.into(), on }
}

#[test]
fn only_your_own_unused_entries_are_offered_and_never_atlas() {
    let entries = vec![
        entry("Spotify", StartupFrom::YourRunKey, "\"C:\\Users\\x\\Spotify.exe\" /background", true),
        entry("Already off", StartupFrom::YourRunKey, "C:\\x\\off.exe", false),
        entry("Vendor Tool", StartupFrom::MachineRunKey, "C:\\Program Files\\v\\tool.exe", true),
        entry("Shortcut", StartupFrom::MachineStartupFolder, "C:\\ProgramData\\x.lnk", true),
        // Named oddly, but runs the program you used this week.
        entry("com.squirrel.Slack", StartupFrom::YourRunKey, "C:\\Users\\x\\AppData\\Local\\slack\\slack.exe --startup", true),
        entry("Atlas", StartupFrom::LogonTask, "C:\\Atlas\\atlas.exe --background", true),
        // Atlas under another name: it runs Atlas's own program.
        entry("My Helper", StartupFrom::YourStartupFolder, "D:\\Tools\\MyHelper.exe", true),
        entry("SecurityHealth", StartupFrom::YourRunKey, "C:\\Windows\\system32\\SecurityHealthSystray.exe", true),
        entry("OneDrive", StartupFrom::YourRunKey, "C:\\x\\OneDrive.exe /background", true),
        entry("Zoom", StartupFrom::LogonTask, "C:\\x\\Zoom.exe", true),
    ];
    let picked = pick_startup_to_stop(&entries, &["slack".to_string()], &["onedrive".to_string()], "D:\\Tools\\MyHelper.exe");
    let names: Vec<&str> = picked.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["Spotify", "Zoom"]);
}

#[test]
fn the_startup_list_says_where_each_lives_and_who_may_change_it() {
    let entries = vec![
        entry("Spotify", StartupFrom::YourRunKey, "", true),
        entry("Vendor", StartupFrom::MachineRunKey, "", true),
        entry("Old", StartupFrom::YourRunKey, "", false),
    ];
    let w = startup_words(&entries);
    assert!(w.starts_with("2 start with Windows"), "{w}");
    assert!(w.contains("from your startup list: Spotify") && w.contains("machine-wide startup list: Vendor"), "{w}");
    assert!(w.contains("1 more is switched off") && w.contains("administrator"), "{w}");
}

// ---------- space, and moving files only where you say ----------

#[test]
fn the_space_look_finds_big_files_and_copies() {
    let d = tmp("space");
    let temp = tmp("space-temp");
    let two_mb: Vec<u8> = (0..2 * 1_048_576).map(|i| (i % 251) as u8).collect();
    std::fs::write(d.join("setup.exe"), &two_mb).unwrap();
    std::fs::create_dir_all(d.join("again")).unwrap();
    std::fs::write(d.join("again").join("setup (1).exe"), &two_mb).unwrap();
    // Same size, different contents: not a copy.
    let mut other = two_mb.clone();
    other[1_000_000] ^= 0xff;
    std::fs::write(d.join("different.bin"), &other).unwrap();
    // A big file, made sparse so the test writes almost nothing.
    std::fs::File::create(d.join("movie.mkv")).unwrap().set_len(60 * 1_048_576).unwrap();
    let look = look_at_space(&d, &temp, std::time::Duration::from_secs(10));
    assert!(look.complete);
    assert_eq!(look.biggest.len(), 1);
    assert_eq!(look.biggest[0].1, 60);
    assert_eq!(look.duplicates.len(), 1, "{:?}", look.duplicates);
    assert_eq!(look.duplicates[0].len(), 2);
    assert_eq!(look.duplicate_mb, 2);
    assert_eq!(look.extra_copies().len(), 1);
    assert!(look.downloads_mb >= 66);
}

#[test]
fn a_move_needs_a_folder_named_in_full() {
    assert_eq!(move_destination("move my big downloads to D:\\Archive"), Some(PathBuf::from("D:\\Archive")));
    assert_eq!(move_destination("move the duplicates into \"E:/Old Stuff\"."), Some(PathBuf::from("E:/Old Stuff")));
    assert_eq!(move_destination("move my big downloads to /mnt/big/archive"), Some(PathBuf::from("/mnt/big/archive")));
    assert_eq!(move_destination("move my big downloads to my archive"), None, "never guessed");
    assert_eq!(move_destination("move my big downloads"), None);
    assert_eq!(tune_ask("move my big downloads to my archive"), TuneAsk::MoveWhere);
    assert_eq!(
        tune_ask("move the duplicates to D:\\Dupes"),
        TuneAsk::MoveInto { to: PathBuf::from("D:\\Dupes"), duplicates: true }
    );
}

#[test]
fn files_never_go_into_the_systems_folders() {
    let dl = Path::new("C:\\Users\\x\\Downloads");
    assert!(may_move_into(Path::new("D:\\Archive"), dl).is_ok());
    assert!(may_move_into(Path::new("C:\\Users\\x\\Documents\\Old"), dl).is_ok());
    for bad in ["C:\\Windows\\Temp", "C:\\Program Files\\x", "c:\\program files (x86)", "C:\\ProgramData\\y", "C:\\Users\\x\\AppData\\Roaming", "/etc/x", "/usr/local"] {
        assert!(may_move_into(Path::new(bad), dl).is_err(), "{bad}");
    }
    assert!(may_move_into(Path::new("C:\\Users\\x\\Downloads\\"), dl).is_err(), "where they already are");
}

#[test]
fn moved_files_go_back_and_nothing_is_overwritten() {
    let from = tmp("move-from");
    let to = tmp("move-to");
    std::fs::write(from.join("a.zip"), b"first").unwrap();
    std::fs::write(from.join("b.iso"), b"second").unwrap();
    // Already a file of that name where they're going.
    std::fs::write(to.join("a.zip"), b"was here").unwrap();
    let (moved, failed) = move_files_into_controlled(&[from.join("a.zip"), from.join("b.iso"), from.join("missing.bin")], &to, &mut |_| Ok(()), &mut |_, _| Ok(()), &|| false);
    assert_eq!(moved.len(), 2);
    assert_eq!(failed.len(), 1, "the missing one is said, not skipped silently");
    assert_eq!(std::fs::read(to.join("a.zip")).unwrap(), b"was here");
    assert_eq!(std::fs::read(to.join("a (2).zip")).unwrap(), b"first");
    let undo = atlas::tune::TuneUndo::Moves(moved.clone());
    let result = atlas::tune::undo_tune_change_with_checkpoint(&undo, &mut |_| Ok(()));
    assert_eq!(result.unwrap(), "Moved 2 of 2 back.");
    assert_eq!(std::fs::read(from.join("a.zip")).unwrap(), b"first");
    assert_eq!(std::fs::read(from.join("b.iso")).unwrap(), b"second");
    assert!(from.join("b.iso").exists() && !to.join("b.iso").exists());
}

// ---------- reaching it ----------

#[test]
fn each_way_of_asking_reaches_the_right_part() {
    assert_eq!(tune_ask("close what I don't need"), TuneAsk::Close);
    assert_eq!(tune_ask("what's slowing my computer down"), TuneAsk::Slowing);
    assert_eq!(tune_ask("what's using my cpu"), TuneAsk::Slowing);
    assert_eq!(tune_ask("what starts with Windows"), TuneAsk::Startup);
    assert_eq!(tune_ask("clean up my startup"), TuneAsk::Startup);
    assert_eq!(tune_ask("clear my temp files"), TuneAsk::ClearTemp);
    assert_eq!(tune_ask("what's taking up my space"), TuneAsk::Space);
    assert_eq!(tune_ask("find duplicate files"), TuneAsk::Space);

    let c = atlas::config::Config::load(Path::new("config")).unwrap();
    let parser = atlas::intent::Parser::new(&c.commands);
    for said in [
        "close what I don't need",
        "what's slowing my computer down",
        "what starts with Windows",
        "clear my temp files",
        "move my big downloads to D:\\Archive",
        "find duplicate files",
    ] {
        assert!(matches!(parser.parse(said), atlas::intent::Intent::PcTune(_)), "{said} -> {:?}", parser.parse(said));
    }
    // The whole run is still the health check, which now uses the same
    // measuring and choosing.
    assert_eq!(parser.parse("optimize my pc"), atlas::intent::Intent::MachineHealth);
}

fn daemon_at<'a>(c: &'a atlas::config::Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    let dir = tmp(&format!("daemon-{tag}"));
    Daemon::new(c, p, None, Store::new(dir), Proactive::new(ProactiveConfig::default()))
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

#[test]
fn off_windows_it_says_so_rather_than_pretending() {
    if cfg!(windows) {
        return;
    }
    let c = atlas::config::Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = daemon_at(&c, &p, "notwin");
    let r = d.turn("close what I don't need", 1_790_990_000);
    assert!(r.contains("only done on Windows"), "{r}");
    let r = d.turn("what starts with Windows", 1_790_990_060);
    assert!(r.contains("only read and changed on Windows"), "{r}");
}

#[test]
fn undo_moves_the_files_back_through_the_ordinary_undo() {
    let c = atlas::config::Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = daemon_at(&c, &p, "undo");
    let from = tmp("undo-from");
    let to = tmp("undo-to");
    std::fs::write(from.join("big.iso"), b"disc").unwrap();
    let (moved, _) = move_files_into_controlled(&[from.join("big.iso")], &to, &mut |_| Ok(()), &mut |_, _| Ok(()), &|| false);
    let t = 1_790_991_000;
    // What `carry_out_optimize` writes when it moves files.
    let id = d.history.note(
        &format!("moved 1 file from Downloads into {}", to.display()),
        "files",
        atlas::undo::Undo::Atlas("move them back".into()),
        true,
        t,
    );
    d.store.save(atlas::tune::TUNE_UNDO_RECORD, &vec![(id, TuneUndo::Moves(moved))]).unwrap();
    let asked = d.turn("undo", t + 5);
    assert!(asked.contains("moved 1 file from Downloads"), "{asked}");
    let mut done = d.turn("yes", t + 10);
    assert!(!done.starts_with("Undone"), "worker acceptance is not completion: {done}");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if let Some(result) = d.tick(t + 11).into_iter().find(|line| line.starts_with("Undone")) { done = result; break; }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(done.starts_with("Undone") && done.contains("Moved 1 of 1 back"), "{done}");
    assert!(from.join("big.iso").exists() && !to.join("big.iso").exists());
    assert_eq!(std::fs::read(from.join("big.iso")).unwrap(), b"disc");
    let left: Vec<(u64, TuneUndo)> = d.store.load(atlas::tune::TUNE_UNDO_RECORD);
    assert!(left.is_empty(), "taken back once, not twice");
}

#[test]
fn the_status_page_has_the_buttons_and_they_go_through_talk() {
    let s = atlas::hub::speed_section();
    assert_eq!(s.matches("action=/hub/talk").count(), 4);
    for said in ["close what I don", "what starts with Windows", "slowing my computer down", "taking up my space"] {
        assert!(s.contains(said), "{said}");
    }
}
