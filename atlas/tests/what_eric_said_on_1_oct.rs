//! Eric's own sentences from 1 Oct 2026, where Atlas misunderstood him.
//! Each one is held to what it should have done.

use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::Path;

fn daemon_at<'a>(c: &'a atlas::config::Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    let dir = std::env::temp_dir().join(format!("atlas-eric-1oct-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    Daemon::new(c, p, None, Store::new(dir), Proactive::new(ProactiveConfig::default()))
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

/// "that" and "it" inside a research request are ordinary English, not
/// "research *this*" pointing at the clipboard.
#[test]
fn a_research_request_with_that_or_it_in_it_is_researched() {
    let c = atlas::config::Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = daemon_at(&c, &p, "research-that");
    let t = 1_790_899_251;
    for said in [
        "research things that would allow you to advance your own capabilities",
        "research how an AI system can gain the ability to add its own features when it gets approval from the user",
    ] {
        let reply = d.turn(said, t);
        assert!(!reply.contains("can't tell what you mean"), "{said} -> {reply}");
    }
    // "research this" with nothing to point at still asks, in a fresh Atlas.
    let mut fresh = daemon_at(&c, &p, "research-this");
    let reply = fresh.turn("research this", t + 10);
    assert!(reply.contains("can't tell what you mean") || reply.contains("haven't been given a topic"), "{reply}");
}

/// A long sentence keeps its own "it": the last topic isn't swapped in.
#[test]
fn a_long_sentence_keeps_its_own_it() {
    let c = atlas::config::Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = daemon_at(&c, &p, "keeps-it");
    d.referents.last_topic = Some("Practical ways to extend this Atlas assistant's capabilities".into());
    let said = "I want you to research how an AI system can gain the ability to add its own features when it gets approval from the user.";
    let _ = d.turn(said, 1_790_899_302);
    let kept = d.thread.said_earlier(said).map(|e| e.said.clone()).unwrap_or_default();
    assert_eq!(kept, said, "the words reached Atlas changed");
    assert_eq!(kept.matches("Practical ways").count(), 0);
}

/// Atlas knows it can grow with your yes, and never says it can't.
#[test]
fn atlas_knows_it_can_add_abilities_with_your_yes() {
    let persona = atlas::persona::Persona::default();
    let who = persona.character();
    assert!(who.contains("work on yourself"), "{who}");
    assert!(who.contains("can't research or add abilities"), "the false limit isn't named as one: {who}");
}

/// "Speak." was answered with a two-sentence research brief read out as
/// "We were on ...". The topic is said as its first clause.
#[test]
fn the_topic_we_were_on_is_said_briefly() {
    let brief = "Practical ways to extend this Atlas assistant's capabilities, especially continuous camera viewing with explicit consent and a clear stop control, persistent local memory. Prioritize currently available tools.";
    assert_eq!(atlas::thread::spoken_topic(brief), "Practical ways to extend this Atlas assistant's capabilities");
    assert_eq!(atlas::thread::spoken_topic("tide times at ventura"), "tide times at ventura");
}

/// "Remove the item on my outstanding list" did nothing: there was no way to
/// take anything off it at all.
#[test]
fn things_come_off_the_outstanding_list_when_asked() {
    use atlas::backlog::{removal_asked, Blocker, Removal};
    assert_eq!(removal_asked("Atlas, Remove the Item on my Outstanding List."), Some(Removal::Unsaid));
    assert_eq!(removal_asked("chrome Should be a removed item off my list"), Some(Removal::Words(vec!["chrome".into()])));
    assert_eq!(removal_asked("remove the second one from my outstanding list"), Some(Removal::Number(2)));
    assert_eq!(removal_asked("clear my outstanding list"), Some(Removal::All));
    assert_eq!(removal_asked("remove the background from this photo"), None, "not about the list");
    assert_eq!(removal_asked("what's outstanding"), None);

    let c = atlas::config::Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = daemon_at(&c, &p, "outstanding");
    let t = 1_790_900_000;
    d.backlog.record("research atlas is there two versions of you on my computer", Blocker::Offline, t);
    d.backlog.record("open chrome and sort my tabs", Blocker::NoScreenGap, t + 1);
    d.backlog.record("research the quarterly budget", Blocker::Offline, t + 2);

    let which = d.turn("Atlas, Remove the Item on my Outstanding List.", t + 10);
    assert!(which.starts_with("Which one?") && which.contains("2. open chrome"), "{which}");
    let gone = d.turn("chrome Should be a removed item off my list", t + 20);
    assert!(gone.contains("open chrome and sort my tabs") && gone.contains("off your outstanding list"), "{gone}");
    let gone = d.turn("remove number 1 from my outstanding list", t + 30);
    assert!(gone.contains("two versions of you"), "{gone}");
    assert_eq!(d.backlog.outstanding().len(), 1);
    let _ = d.turn("clear my outstanding list", t + 40);
    assert_eq!(d.backlog.outstanding().len(), 0);
    // Saved: a fresh Atlas on the same store sees it gone.
    let reloaded = atlas::backlog::Backlog::load(&atlas::store::Store::new(
        std::env::temp_dir().join(format!("atlas-eric-1oct-outstanding-{}", std::process::id())),
    ));
    assert_eq!(reloaded.outstanding().len(), 0);
}

/// "Do an optimization run on my computer and tell me what can be cleared or
/// fixed" got two numbers: nothing was measured, and finding was switched off.
#[test]
fn an_optimization_run_measures_and_offers_what_it_found() {
    let csv = "\"chrome.exe\",\"1200\",\"Console\",\"1\",\"350,000 K\"\n\"chrome.exe\",\"1300\",\"Console\",\"1\",\"250,000 K\"\n\"Discord.exe\",\"88\",\"Console\",\"1\",\"410,500 K\"\n";
    let by = atlas::tune::parse_tasklist(csv);
    assert_eq!(by[0], ("chrome".to_string(), 341 + 244));
    assert_eq!(by[1].0, "Discord");
    assert_eq!(atlas::tune::parse_ps("bash 4096\nfirefox web 512000\n")[0], ("firefox web".to_string(), 500));

    // Clearing: only files untouched for the window; recent ones are left.
    let dir = std::env::temp_dir().join(format!("atlas-eric-1oct-temp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("old-install")).unwrap();
    std::fs::write(dir.join("old-install").join("a.tmp"), vec![0u8; 2_000_000]).unwrap();
    std::fs::write(dir.join("fresh.tmp"), b"in use").unwrap();
    let none = atlas::tune::clear_old_files(&dir, 3600);
    assert_eq!((none.files, none.skipped), (0, 2), "nothing is old enough");
    let all = atlas::tune::clear_old_files(&dir, 0);
    assert_eq!(all.files, 2);
    assert!(!dir.join("old-install").exists(), "the emptied folder is gone too");

    // Through Atlas: the sentence reaches the health check, which now names
    // what it measured.
    let c = atlas::config::Config::load(Path::new("config")).unwrap();
    let p = plat();
    // A machine with room, reported through the platform (2 Oct): the real
    // one's memory was read here, and under a build it was "nearly full".
    p.set_readings(atlas::health::Readings { disk_free_gb: 120.0, disk_total_gb: 500.0, ram_used_gb: 6.0, ram_total_gb: 16.0, ..Default::default() });
    let mut d = daemon_at(&c, &p, "optimize");
    // (The long way Eric said it goes through the language model, which
    // picks this tool by its description; the short ways need no model.)
    for (i, said) in ["optimize my pc", "can you speed up my computer", "what's eating the memory"].iter().enumerate() {
        let reply = d.turn(said, 1_790_900_000 + i as u64 * 60);
        assert!(reply.contains("free") && reply.contains("memory"), "{said} -> {reply}");
    }
}

/// "Organize my PC": Eric's desktop is in OneDrive, as Windows puts it when
/// OneDrive backs it up, and every file there was refused as outside the
/// folders Atlas may work in.
#[test]
fn a_desktop_in_onedrive_is_one_atlas_may_organize() {
    use atlas::system::{judge, Change, SystemConfig, Verdict};
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap();
    let cfg = SystemConfig { enabled: true, ..Default::default() };
    let mv = |from: String| Change::MoveFile { from, to: format!("{home}/Documents/Filed/Resources/notes.txt") };
    assert!(!matches!(judge(&mv(format!("{home}/OneDrive/Desktop/notes.txt")), &cfg), Verdict::Refuse(_)));
    assert!(!matches!(judge(&mv(format!("{home}/Desktop/notes.txt")), &cfg), Verdict::Refuse(_)));
    assert!(matches!(judge(&mv(format!("{home}/OneDrive/Secrets/notes.txt")), &cfg), Verdict::Refuse(_)), "only the known folders");
    assert!(matches!(judge(&mv("/etc/passwd".into()), &cfg), Verdict::Refuse(_)));

    let c = atlas::config::Config::load(Path::new("config")).unwrap();
    let parser = atlas::intent::Parser::new(&c.commands);
    for said in ["organize my pc", "organise my computer", "tidy my downloads"] {
        assert_eq!(parser.parse(said), atlas::intent::Intent::TidyDesktop, "{said}");
    }
}

/// Eric: optimizing should *do* things -- close what isn't needed, stop what
/// starts for nothing, clear junk -- not just report.
#[test]
fn an_optimization_run_offers_to_close_stop_and_clear() {
    use atlas::tune::{may_close, parse_reg_run, parse_windowed, Plan};
    let v = "\"Discord.exe\",\"88\",\"Console\",\"1\",\"410,500 K\",\"Running\",\"LE3O\\\\erics\",\"0:01:02\",\"#general - Discord\"\n\
             \"svchost.exe\",\"900\",\"Services\",\"0\",\"30,000 K\",\"Unknown\",\"N/A\",\"0:00:01\",\"N/A\"\n";
    assert_eq!(parse_windowed(v), vec!["Discord".to_string()], "only programs with a window");
    let reg = "\r\nHKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run\r\n    OneDrive    REG_SZ    \"C:\\x\\OneDrive.exe\" /background\r\n    Spotify    REG_SZ    C:\\y\\Spotify.exe\r\n";
    assert_eq!(parse_reg_run(reg), vec!["OneDrive".to_string(), "Spotify".to_string()]);
    let keep = vec!["onedrive".to_string()];
    assert!(may_close("Discord", &keep) && may_close("Spotify", &keep));
    assert!(!may_close("OneDrive", &keep) && !may_close("explorer", &keep) && !may_close("atlas", &keep) && !may_close("claude", &keep));
    let plan = Plan {
        close: vec![("Discord".into(), 410)],
        stop_starting: vec!["Spotify".into()],
        temp: Some((std::env::temp_dir(), 2300)),
    };
    let o = plan.offer();
    assert!(o.contains("close Discord (410 MB)") && o.contains("stop Spotify starting with Windows") && o.contains("2300 MB") && o.ends_with("Go ahead?"), "{o}");
    assert!(Plan::default().is_empty());
}

/// Coding on the laptop worked, but the result was a ruff report read out
/// whole, and the file was always "build.verified.py", wherever that was.
#[test]
fn what_was_built_is_said_plainly_and_kept_by_name() {
    use atlas::build_it::{file_name_for, plain_note};
    let ruff = "F401 [*] `os` imported but unused\n --> main.py:1:8\n  |\n1 | import os\n  |        ^^\n";
    assert_eq!(plain_note(ruff), "os imported but unused");
    assert_eq!(plain_note("clippy::needless_return: unneeded `return` statement"), "unneeded return statement");
    let dir = std::env::temp_dir().join(format!("atlas-eric-1oct-builds-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let first = file_name_for(&dir, "a python script that lists the five biggest files in my Downloads folder", "py");
    assert_eq!(first.file_name().unwrap().to_string_lossy(), "lists-five-biggest-files-downloads.py");
    std::fs::write(&first, "x").unwrap();
    let second = file_name_for(&dir, "a python script that lists the five biggest files in my Downloads folder", "py");
    assert_eq!(second.file_name().unwrap().to_string_lossy(), "lists-five-biggest-files-downloads-2.py", "never over the top");
}
