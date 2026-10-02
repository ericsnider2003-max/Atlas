//! Sorting a folder, held to what it may and may not do.
//!
//! Eric, 2 Oct 2026: "can't organize my PC properly." Before this, "organize
//! my PC" filed only the desktop's and Downloads' loose files by extension
//! into Documents\Filed, left pictures, videos, archives and installers
//! where they were, found no copies, took no folder you named, and wrote
//! nothing "undo" could find (`organize`).
//!
//! Everything here happens in folders made for the test under the system's
//! temporary folder -- nothing of anyone's is looked at or moved. What it
//! proves: files are grouped by kind and by age into folders inside the one
//! sorted; copies are found by size, then hash, then bytes, and only the
//! copies move; nothing is ever written over; hidden and system files,
//! shortcuts, unfinished downloads and code projects are left alone; and
//! through the daemon, "yes" moves them and "undo that" puts every file
//! back where it was with nothing deleted.

use atlas::daemon::Daemon;
use atlas::organize::{carry_out_moves, plan_folders, plan_said, read_sort_request, FileKind, Reason, TO_REVIEW};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use atlas::system::SystemConfig;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Under Cargo's own scratch folder for tests, not the system's temporary
/// folder: on Windows that is inside AppData, which is never sorted.
fn tmp(tag: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("atlas-sorting-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A file with these bytes, last changed `days_ago` -- old enough that it
/// isn't mistaken for one still being saved.
fn put(path: &Path, bytes: &[u8], days_ago: u64) {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).unwrap();
    }
    std::fs::write(path, bytes).unwrap();
    let when = SystemTime::now() - Duration::from_secs(days_ago * 86_400 + 600);
    std::fs::File::options().write(true).open(path).unwrap().set_modified(when).unwrap();
}

fn now() -> u64 {
    atlas::store::now()
}

/// Every file under `dir`, with its bytes, by path relative to `dir`.
fn everything(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push((p.strip_prefix(dir).unwrap().to_path_buf(), std::fs::read(&p).unwrap()));
            }
        }
    }
    out.sort();
    out
}

fn may_work_in(dir: &Path) -> SystemConfig {
    SystemConfig { enabled: true, file_roots: vec![dir.display().to_string()], may_prepare_itself: false }
}

#[test]
fn the_plan_groups_by_kind_and_by_age_inside_the_same_folder() {
    let d = tmp("kinds");
    put(&d.join("taxes.pdf"), b"pdf", 3);
    put(&d.join("holiday.jpg"), b"jpg", 3);
    put(&d.join("song.mp3"), b"mp3", 3);
    put(&d.join("backup.zip"), b"zip", 3);
    put(&d.join("budget.xlsx"), b"xlsx", 3);
    put(&d.join("clip.mp4"), b"mp4", 3);
    put(&d.join("script.py"), b"py", 3);
    put(&d.join("old-notes.txt"), b"txt", 800);
    put(&d.join("mystery.xyz"), b"?", 3);
    // Already in a folder of yours: left.
    put(&d.join("Keep").join("inside.pdf"), b"inside", 3);
    let plan = plan_folders(&[d.clone()], None, false, now(), Duration::from_secs(10));
    let into = |name: &str| plan.moves.iter().find(|m| m.from == d.join(name)).map(|m| m.into.clone());
    assert_eq!(into("taxes.pdf"), Some(d.join("Documents")));
    assert_eq!(into("holiday.jpg"), Some(d.join("Pictures")));
    assert_eq!(into("song.mp3"), Some(d.join("Music")));
    assert_eq!(into("backup.zip"), Some(d.join("Archives")));
    assert_eq!(into("budget.xlsx"), Some(d.join("Spreadsheets")));
    assert_eq!(into("clip.mp4"), Some(d.join("Videos")));
    assert_eq!(into("script.py"), Some(d.join("Code")));
    // Untouched for over a year: a year folder inside its kind.
    let old = plan.moves.iter().find(|m| m.from == d.join("old-notes.txt")).unwrap();
    assert!(old.into.starts_with(d.join("Documents")) && old.into != d.join("Documents"), "{:?}", old.into);
    assert!(matches!(old.reason, Reason::Kind { kind: FileKind::Documents, year: Some(_) }));
    assert_eq!(into("mystery.xyz"), None, "no confident home: left");
    assert!(plan.unplaced.contains(&d.join("mystery.xyz")));
    assert!(plan.moves.iter().all(|m| !m.from.starts_with(d.join("Keep"))), "the folders inside are yours already");
    let said = plan_said(&plan);
    assert!(said.contains("8 files") && said.contains("taxes.pdf") && said.contains("year folder"), "{said}");
    assert!(said.contains("mystery.xyz") && said.ends_with("Go ahead?"), "{said}");
    // Nothing moved by planning.
    assert!(d.join("taxes.pdf").exists() && !d.join("Documents").exists());
}

#[test]
fn old_installers_go_to_review_and_a_program_on_the_desktop_stays() {
    let d = tmp("installers").join("Downloads");
    put(&d.join("ToolSetup-x64.msi"), b"msi", 90);
    put(&d.join("fresh-installer.exe"), b"exe", 2);
    let plan = plan_folders(&[d.clone()], None, false, now(), Duration::from_secs(10));
    let m = plan.moves.iter().find(|m| m.from == d.join("ToolSetup-x64.msi")).unwrap();
    assert_eq!(m.into, d.join(TO_REVIEW).join("Old installers"));
    let m = plan.moves.iter().find(|m| m.from == d.join("fresh-installer.exe")).unwrap();
    assert_eq!(m.into, d.join("Installers"));
    // A program somewhere other than Downloads that doesn't call itself an
    // installer may be one you run from there.
    let desk = tmp("installers-desk").join("Desktop");
    put(&desk.join("PortableEditor.exe"), b"exe", 90);
    let plan = plan_folders(&[desk.clone()], None, false, now(), Duration::from_secs(10));
    assert!(plan.moves.is_empty() && plan.unplaced.contains(&desk.join("PortableEditor.exe")));
}

#[test]
fn copies_are_found_by_size_then_hash_then_bytes_and_only_copies_move() {
    let d = tmp("copies");
    put(&d.join("photo.jpg"), b"the same picture", 30);
    put(&d.join("photo (1).jpg"), b"the same picture", 10);
    put(&d.join("Trips").join("photo-again.jpg"), b"the same picture", 5);
    // The same size, different bytes: not a copy.
    put(&d.join("other.jpg"), b"the samf picture", 5);
    // Empty files are all "the same" and none of them is a copy.
    put(&d.join("empty-a.txt"), b"", 5);
    put(&d.join("empty-b.txt"), b"", 5);
    std::fs::create_dir_all(d.join("Nothing here")).unwrap();
    let plan = plan_folders(&[d.clone()], None, true, now(), Duration::from_secs(10));
    let mut copies: Vec<PathBuf> = plan.moves.iter().map(|m| m.from.clone()).collect();
    copies.sort();
    assert_eq!(copies, vec![d.join("Trips").join("photo-again.jpg"), d.join("photo (1).jpg")]);
    for m in &plan.moves {
        assert_eq!(m.into, d.join(TO_REVIEW).join("Duplicates"));
        assert_eq!(m.reason, Reason::Copy { of: d.join("photo.jpg") }, "the oldest stays");
    }
    assert_eq!(plan.empty_folders, vec![d.join("Nothing here")]);
    let said = plan_said(&plan);
    assert!(said.contains("2 files are a copy") && said.contains("is the same as photo.jpg"), "{said}");
    assert!(said.contains(TO_REVIEW) && said.contains("never delete") && said.contains("empty"), "{said}");

    // Carried out: the copies are in "To review", the original where it was,
    // every byte still on the disk.
    let before = everything(&d);
    let done = carry_out_moves(&plan, &may_work_in(&d), now());
    assert_eq!(done.moved.len(), 2, "{:?}", done.not);
    assert!(d.join("photo.jpg").exists() && d.join(TO_REVIEW).join("Duplicates").join("photo (1).jpg").exists());
    let after = everything(&d);
    assert_eq!(before.len(), after.len(), "nothing deleted");
    let mut b: Vec<Vec<u8>> = before.into_iter().map(|(_, x)| x).collect();
    let mut a: Vec<Vec<u8>> = after.into_iter().map(|(_, x)| x).collect();
    b.sort();
    a.sort();
    assert_eq!(a, b, "the same contents, only in other places");
}

#[test]
fn nothing_is_ever_written_over() {
    let d = tmp("clash");
    put(&d.join("Documents").join("report.pdf"), b"the one already filed", 50);
    put(&d.join("report.pdf"), b"the loose one", 3);
    let plan = plan_folders(&[d.clone()], None, false, now(), Duration::from_secs(10));
    let done = carry_out_moves(&plan, &may_work_in(&d), now());
    assert_eq!(done.moved.len(), 1, "{:?}", done.not);
    assert_eq!(std::fs::read(d.join("Documents").join("report.pdf")).unwrap(), b"the one already filed");
    assert_eq!(std::fs::read(d.join("Documents").join("report (2).pdf")).unwrap(), b"the loose one");
    assert!(!d.join("report.pdf").exists());
}

#[test]
fn hidden_system_and_unfinished_files_and_project_folders_are_left() {
    let d = tmp("hidden");
    put(&d.join(".secret.pdf"), b"hidden", 3);
    put(&d.join("desktop.ini"), b"[.ShellClassInfo]", 3);
    put(&d.join("Atlas.lnk"), b"link", 3);
    put(&d.join("movie.mp4.crdownload"), b"half", 3);
    // Changed a moment ago: may still be being saved.
    std::fs::write(d.join("being-saved.docx"), b"now").unwrap();
    put(&d.join("plain.pdf"), b"plain", 3);
    let plan = plan_folders(&[d.clone()], None, false, now(), Duration::from_secs(10));
    let moving: Vec<&Path> = plan.moves.iter().map(|m| m.from.as_path()).collect();
    assert_eq!(moving, vec![d.join("plain.pdf").as_path()]);
    assert_eq!(plan.passed_over, 4);
    assert_eq!(plan.in_use, vec![d.join("being-saved.docx")]);
    let said = plan_said(&plan);
    assert!(said.contains("hidden and system files") && said.contains("open in another program"), "{said}");
    carry_out_moves(&plan, &may_work_in(&d), now());
    for f in [".secret.pdf", "desktop.ini", "Atlas.lnk", "movie.mp4.crdownload", "being-saved.docx"] {
        assert!(d.join(f).exists(), "{f} was touched");
    }

    // A code project, a program's own folder and the top of a drive are
    // never sorted.
    let project = tmp("project");
    put(&project.join("Cargo.toml"), b"[package]", 3);
    put(&project.join("notes.pdf"), b"notes", 3);
    let plan = plan_folders(&[project.clone()], None, false, now(), Duration::from_secs(10));
    assert!(plan.moves.is_empty() && plan.refused.len() == 1, "{:?}", plan.refused);
    assert!(plan_said(&plan).contains("project"), "{}", plan_said(&plan));
    for dir in ["C:\\Windows\\Temp", "C:\\Users\\someone\\AppData\\Local", "/usr/share"] {
        let plan = plan_folders(&[PathBuf::from(dir)], None, false, now(), Duration::from_secs(1));
        assert!(plan.moves.is_empty() && plan.refused.len() == 1, "{dir}");
    }
    // Nor into one.
    let plan = plan_folders(&[d.clone()], Some(Path::new("C:\\Program Files\\Sorted")), false, now(), Duration::from_secs(1));
    assert!(plan.moves.is_empty() && !plan.refused.is_empty());
}

#[test]
fn the_switch_and_the_folders_atlas_may_work_in_are_kept_to() {
    let d = tmp("gate");
    put(&d.join("a.pdf"), b"a", 3);
    let plan = plan_folders(&[d.clone()], None, false, now(), Duration::from_secs(10));
    let off = SystemConfig { enabled: false, ..may_work_in(&d) };
    let done = carry_out_moves(&plan, &off, now());
    assert!(done.moved.is_empty() && done.not[0].contains("switched off"), "{:?}", done.not);
    let elsewhere = SystemConfig { file_roots: vec![tmp("gate-other").display().to_string()], ..may_work_in(&d) };
    let done = carry_out_moves(&plan, &elsewhere, now());
    assert!(done.moved.is_empty() && done.not[0].contains("outside the folders"), "{:?}", done.not);
    assert!(d.join("a.pdf").exists());
}

#[test]
fn a_batch_is_capped_and_the_rest_left_for_next_time() {
    let d = tmp("cap");
    for i in 0..(atlas::organize::MOST_AT_ONCE + 7) {
        put(&d.join(format!("n{i:04}.txt")), format!("{i}").as_bytes(), 3);
    }
    let plan = plan_folders(&[d.clone()], None, false, now(), Duration::from_secs(30));
    assert_eq!(plan.moves.len(), atlas::organize::MOST_AT_ONCE);
    assert_eq!(plan.more_next_time, 7);
    assert!(plan_said(&plan).contains("7 more next time"), "{}", plan_said(&plan));
}

#[test]
fn the_sentence_says_which_folder_where_to_and_whether_only_copies() {
    let d = tmp("asking");
    let to = tmp("asking-to");
    let r = read_sort_request(&format!("sort the files in {} into {}", d.display(), to.display()));
    assert_eq!(r.folders, vec![d.clone()]);
    assert_eq!(r.into, Some(to.clone()));
    assert!(!r.copies_only);
    let r = read_sort_request(&format!("find duplicates in {}", d.display()));
    assert_eq!(r.folders, vec![d.clone()]);
    assert!(r.copies_only && r.into.is_none());
    let r = read_sort_request("sort the files in my zz-no-such-folder-qq");
    assert!(r.folders.is_empty() && r.not_found.as_deref() == Some("my zz-no-such-folder-qq"), "{r:?}");
    // Windows paths read the same on any machine.
    let r = read_sort_request("organize the files in D:\\Photos\\2024 into E:\\Sorted.");
    assert_eq!(r.folders, vec![PathBuf::from("D:\\Photos\\2024")]);
    assert_eq!(r.into, Some(PathBuf::from("E:\\Sorted")));
    // The named everyday folders are found the way this machine has them,
    // never by a fixed path.
    let r = read_sort_request("clean up my desktop");
    assert_eq!(r.folders, atlas::organize::user_folder("desktop").into_iter().collect::<Vec<_>>());
    let r = read_sort_request("organize my downloads");
    assert_eq!(r.folders, atlas::organize::user_folder("downloads").into_iter().collect::<Vec<_>>());
}

#[test]
fn each_way_of_asking_reaches_the_organizer() {
    let c = atlas::config::Config::load(Path::new("config")).unwrap();
    let parser = atlas::intent::Parser::new(&c.commands);
    for said in [
        "organize my PC",
        "organize my downloads",
        "clean up my desktop",
        "sort the files in D:\\Stuff",
        "sort the files in my documents",
        "find duplicates in D:\\Photos",
        "find duplicates in my downloads",
        "organize the files in C:\\Users\\me\\Work",
        "tidy up my downloads folder",
    ] {
        assert_eq!(parser.parse(said), atlas::intent::Intent::TidyDesktop, "{said}");
    }
    assert_eq!(parser.parse("undo that"), atlas::intent::Intent::Undo);
    // What "Make it run well" already answered still goes there.
    assert!(matches!(parser.parse("find duplicate files"), atlas::intent::Intent::PcTune(_)));
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

#[test]
fn yes_sorts_it_and_undo_that_puts_every_file_back_through_the_daemon() {
    let d = tmp("daemon-folder");
    put(&d.join("taxes.pdf"), b"pdf", 3);
    put(&d.join("holiday.jpg"), b"jpg", 3);
    put(&d.join("holiday copy.jpg"), b"jpg", 1);
    put(&d.join("old.txt"), b"old", 900);
    put(&d.join("Setup-x64.msi"), b"msi", 60);
    put(&d.join("Mine").join("kept.pdf"), b"kept", 3);
    let before = everything(&d);

    let mut c = atlas::config::Config::load(Path::new("config")).unwrap();
    let tools = c.tools.get_or_insert_with(Default::default);
    tools.system = may_work_in(&d);
    let p = plat();
    let mut d_ = Daemon::new(&c, &p, None, Store::new(tmp("daemon-store")), Proactive::new(ProactiveConfig::default()));
    let t = 1_790_995_000;
    let plan = d_.turn(&format!("sort the files in {}", d.display()), t);
    assert!(plan.contains("Go ahead?") && plan.contains("copy"), "{plan}");
    assert!(d.join("taxes.pdf").exists(), "nothing moves before the yes");

    let done = d_.turn("yes", t + 10);
    assert!(done.starts_with("Moved") && done.contains("undo that"), "{done}");
    assert!(d.join("Documents").join("taxes.pdf").exists());
    assert!(d.join(TO_REVIEW).join("Duplicates").join("holiday copy.jpg").exists());
    assert!(d.join(TO_REVIEW).join("Old installers").join("Setup-x64.msi").exists());
    assert_eq!(everything(&d).len(), before.len(), "nothing deleted");

    let asked = d_.turn("undo that", t + 20);
    assert!(asked.contains("sorted 5 files"), "{asked}");
    let undone = d_.turn("yes", t + 30);
    assert!(undone.starts_with("Undone") && undone.contains("Put 5 of 5 back"), "{undone}");
    assert_eq!(everything(&d), before, "every file back at its own path, with its own bytes");
    // And the folders the sorting made are gone again; yours stays.
    for made in ["Documents", "Pictures", TO_REVIEW] {
        assert!(!d.join(made).exists(), "{made} was left behind");
    }
    assert!(d.join("Mine").is_dir());
    let left: Vec<(u64, atlas::tune::TuneUndo)> = d_.store.load(atlas::tune::TUNE_UNDO_RECORD);
    assert!(left.is_empty(), "taken back once, not twice");
}

#[test]
fn with_system_changes_off_it_says_so_and_moves_nothing() {
    let mut c = atlas::config::Config::load(Path::new("config")).unwrap();
    c.tools.get_or_insert_with(Default::default).system.enabled = false;
    let p = plat();
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("off-store")), Proactive::new(ProactiveConfig::default()));
    let r = d.turn("organize my PC", 1_790_996_000);
    assert!(r.contains("System changes") && r.contains("switched off"), "{r}");
}

#[test]
fn the_status_page_has_sorting_buttons_that_go_through_talk() {
    let s = atlas::hub::sorting_section();
    assert_eq!(s.matches("action=/hub/talk").count(), 4);
    for said in ["organize my PC", "organize my downloads", "clean up my desktop", "find duplicates in my downloads"] {
        assert!(s.contains(said), "{said}");
    }
}
