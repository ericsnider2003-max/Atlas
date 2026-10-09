//! Eric's ruling H3 (25 Sep 2026): read PDFs and scans, unzip when needed,
//! and scan for viruses before opening anything — Windows Defender on the
//! file, and on everything a zip unpacks to.
//!
//! The PDFs in `tests/fixtures/documents` were made by real programs, not by
//! hand: LibreOffice from a Word file (what Word and most offices produce),
//! Chromium's print-to-PDF (what browsers produce), reportlab, pdfLaTeX, a
//! photo-only "scanned" page, and one locked with a password by qpdf.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::files::{what_was_scanned, FilesConfig, Scanned};
use atlas::intent::Intent;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use atlas::unpack::{ScanConfig, Verdict};
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new("tests/fixtures/documents").join(name)
}

fn pdf(name: &str) -> atlas::pdftext::Pdf {
    atlas::pdftext::read(&std::fs::read(fixture(name)).unwrap()).unwrap()
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-docs-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

// ------------------------------------------------------------------ PDFs

#[test]
fn a_pdf_saved_from_word_reads_as_its_words() {
    let p = pdf("letter.pdf");
    assert_eq!(p.pages, 1);
    assert!(p.text.contains("Invoice for March"), "{}", p.text);
    assert!(p.text.contains("Dear Eric, the tripod you ordered shipped on Tuesday."), "{}", p.text);
    assert!(p.text.contains("$149.99"), "{}", p.text);
    assert!(atlas::pdftext::looks_like_words(&p.text));
}

#[test]
fn a_pdf_printed_from_a_browser_reads_as_its_words() {
    let p = pdf("chrome.pdf");
    assert!(p.text.contains("Night markets"), "{}", p.text);
    assert!(p.text.contains("Every Friday from eight until late."), "{}", p.text);
}

#[test]
fn pdfs_from_other_programs_read_too_and_pages_are_kept_apart() {
    let p = pdf("reportlab.pdf");
    assert_eq!(p.pages, 2);
    assert!(p.text.contains("Tide tables for Saturday morning\nHigh water at 6:42 am"), "{}", p.text);
    assert!(p.text.contains("\n\nSecond page here"), "{}", p.text);
    let p = pdf("latex.pdf");
    assert!(p.text.contains("budget meeting"), "{}", p.text);
}

#[test]
fn a_scanned_pdf_is_known_for_a_picture_and_its_page_handed_back() {
    let p = pdf("scan.pdf");
    assert!(atlas::files::pdf_is_really_a_scan(p.text_chars(), p.pages));
    assert_eq!(p.images.len(), 1);
    assert!(p.images[0].starts_with(&[0xFF, 0xD8]), "the page photo is the JPEG itself");
}

#[test]
fn a_locked_pdf_is_said_to_be_locked() {
    let e = atlas::pdftext::read(&std::fs::read(fixture("locked.pdf")).unwrap()).unwrap_err();
    assert!(e.contains("password"), "{e}");
    assert!(atlas::pdftext::read(b"hello").is_err());
}

// ------------------------------------------------------------------ zips

#[test]
fn a_zip_unpacks_beside_itself_whole() {
    let dir = scratch("zip");
    let zip = dir.join("photos.zip");
    std::fs::copy(fixture("photos.zip"), &zip).unwrap();
    let dest = atlas::unpack::folder_beside(&zip);
    assert_eq!(dest, dir.join("photos"));
    let files = atlas::unpack::unzip_stoppable(&zip, &dest, &FilesConfig::default(), &|| false).unwrap();
    assert_eq!(files.len(), 2);
    assert!(std::fs::read_to_string(dest.join("trip").join("notes.txt")).unwrap().starts_with("Packed the tripod"));
    assert_eq!(std::fs::read(dest.join("trip").join("tides.pdf")).unwrap(), std::fs::read(fixture("reportlab.pdf")).unwrap());
    // Never over what's there: the next one gets its own name.
    assert_eq!(atlas::unpack::folder_beside(&zip), dir.join("photos (2)"));
    assert!(atlas::unpack::unzip_stoppable(&zip, &dest, &FilesConfig::default(), &|| false).is_err());
}

#[test]
fn a_zip_that_climbs_out_or_balloons_is_refused_before_a_byte_is_written() {
    let dir = scratch("bad-zip");
    let dest = dir.join("out");
    let e = atlas::unpack::unzip_stoppable(&fixture("climbs-out.zip"), &dest, &FilesConfig::default(), &|| false).unwrap_err();
    assert!(e.contains("climb out"), "{e}");
    assert!(!dest.exists(), "nothing written");

    let small = FilesConfig { max_unpacked_mb: 10, ..FilesConfig::default() };
    let e = atlas::unpack::unzip_stoppable(&fixture("balloon.zip"), &dest, &small, &|| false).unwrap_err();
    assert!(e.contains("30MB"), "{e}");
    assert!(!dest.exists());
}

#[test]
fn a_word_file_reads_as_its_paragraphs() {
    let t = atlas::unpack::docx_text(&fixture("letter.docx")).unwrap();
    assert!(t.contains("Invoice for March\nDear Eric"), "{t}");
    assert!(t.contains("Total due: $149.99"), "{t}");
}

// ------------------------------------------------------------------ the scan

#[test]
fn cancelled_unpack_preserves_archive_and_reports_partial_files() {
    let dir = scratch("zip-stopped");
    let zip = fixture("photos.zip");
    let before = std::fs::read(&zip).unwrap();
    let dest = dir.join("out");
    let stopped = || dest.join("trip/notes.txt").exists();
    let error = atlas::unpack::unzip_stoppable(&zip, &dest, &FilesConfig::default(), &stopped).unwrap_err();
    assert!(error.contains("Stopped"), "{error}");
    assert_eq!(std::fs::read(&zip).unwrap(), before);
    let entries = atlas::unpack::entries_of(&before).unwrap();
    let notes = entries.iter().find(|e| e.name == "trip/notes.txt").unwrap();
    assert_eq!(std::fs::read(dest.join("trip/notes.txt")).unwrap(), atlas::unpack::read_entry(&before, notes).unwrap());
    assert!(!dest.join("trip/tides.pdf").exists());
    let error = atlas::unpack::unzip_stoppable(&zip, &dest, &FilesConfig::default(), &|| false).unwrap_err();
    assert!(error.contains("already there"), "{error}");
}

#[test]
fn stopped_scanner_never_starts_a_command() {
    let verdict = atlas::unpack::scan_stoppable(&fixture("letter.pdf"), &ScanConfig {
        command: "this-command-must-not-start".into(), args: vec![], threat_exit: 2,
    }, &|| true);
    assert!(matches!(verdict, Verdict::NotScanned(reason) if reason.contains("before starting")));
}

#[test]
fn archive_size_limit_does_not_round_small_archives_down_to_zero() {
    let dir = scratch("zip-zero-budget");
    let dest = dir.join("out");
    let config = FilesConfig { max_unpacked_mb: 0, ..FilesConfig::default() };
    assert!(atlas::unpack::unzip_stoppable(&fixture("photos.zip"), &dest, &config, &|| false).is_err());
    assert!(!dest.exists());
}

/// A stand-in scanner: a script that exits as Defender would.
#[cfg(unix)]
fn scanner(dir: &Path, exit: i32, says: &str) -> ScanConfig {
    let s = dir.join(format!("scan-{exit}.sh"));
    std::fs::write(&s, format!("#!/bin/sh\necho '{says}'\nexit {exit}\n")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&s, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    ScanConfig { command: s.display().to_string(), args: vec!["{path}".into()], threat_exit: 2 }
}

#[cfg(unix)]
#[test]
fn the_scanner_s_answer_is_taken_as_it_gives_it() {
    let dir = scratch("scanner");
    let f = fixture("letter.pdf");
    assert_eq!(atlas::unpack::scan_stoppable(&f, &scanner(&dir, 0, "no threats"), &|| false), Verdict::Clean);
    assert_eq!(
        atlas::unpack::scan_stoppable(&f, &scanner(&dir, 2, "Threat                  : Virus:DOS/EICAR_Test_File"), &|| false),
        Verdict::Threat("Virus:DOS/EICAR_Test_File".into())
    );
    assert!(matches!(atlas::unpack::scan_stoppable(&f, &scanner(&dir, 5, ""), &|| false), Verdict::NotScanned(_)));
    assert!(matches!(
        atlas::unpack::scan_stoppable(&f, &ScanConfig { command: "/no/such/scanner".into(), args: vec![], threat_exit: 2 }, &|| false),
        Verdict::NotScanned(_)
    ));
    // The shipped default on Windows is Defender, reporting only.
    let d = ScanConfig::default();
    if cfg!(windows) {
        assert!(d.command.ends_with("MpCmdRun.exe") && d.args.contains(&"-DisableRemediation".to_string()));
    }
}

fn daemon_with(scan: ScanConfig, tag: &str) -> (Daemon<'static>, &'static MockPlatform) {
    let mut c = Config::load(Path::new("config")).unwrap();
    c.tools.as_mut().unwrap().files.virus_scan = scan;
    let c: &'static Config = Box::leak(Box::new(c));
    let p: &'static MockPlatform =
        Box::leak(Box::new(MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])));
    (Daemon::new(c, p, None, Store::new(scratch(tag)), Proactive::new(ProactiveConfig::default())), p)
}

/// What reading or unpacking came to (28 Sep 2026: these run on the crew,
/// off the daemon's loop, so `execute` says it has started and the answer
/// comes when the errand ends -- `the_hub_is_always_there.rs` has why). The
/// assertions below are the same as before on the answer itself.
fn when_done(d: &mut Daemon<'static>, started: String) -> String {
    let mut said = d.errands_done_for_test();
    if said.is_empty() {
        // Done on the loop (a full crew): the answer is what `execute` said.
        return started;
    }
    said.remove(0)
}

#[cfg(unix)]
#[test]
fn read_this_pdf_scans_first_then_reads() {
    let dir = scratch("read-clean");
    let (mut d, _) = daemon_with(scanner(&dir, 0, "clean"), "read-clean-d");
    let path = fixture("letter.pdf").display().to_string();
    let said = d.execute(&Intent::ReadDocument(format!("read this pdf \"{path}\"")));
    let said = when_done(&mut d, said);
    assert!(said.contains("1 page") && said.contains("Invoice for March"), "{said}");

    let (mut d, _) = daemon_with(scanner(&dir, 2, "Threat : Trojan:Win32/Test"), "read-threat-d");
    let said = d.execute(&Intent::ReadDocument(format!("read this pdf \"{path}\"")));
    let said = when_done(&mut d, said);
    assert!(said.contains("Trojan:Win32/Test") && said.contains("haven't opened it"), "{said}");
    assert!(!said.contains("Invoice"), "not read: {said}");
}

#[test]
fn an_unscannable_file_is_opened_only_on_your_yes() {
    let (mut d, _) = daemon_with(ScanConfig { command: String::new(), args: vec![], threat_exit: 2 }, "read-unscanned");
    let path = fixture("letter.docx").display().to_string();
    let said = d.execute(&Intent::ReadDocument(format!("read this document \"{path}\"")));
    let said = when_done(&mut d, said);
    assert!(said.contains("couldn't scan") && said.ends_with("Open it anyway?"), "{said}");
    let said = d.turn("yes", atlas::store::now());
    let said = when_done(&mut d, said);
    assert!(said.contains("Invoice for March") && said.contains("open it anyway"), "{said}");
}

#[cfg(unix)]
#[test]
fn unzip_scans_the_zip_and_what_came_out() {
    let dir = scratch("unzip-d");
    let zip = dir.join("photos.zip");
    std::fs::copy(fixture("photos.zip"), &zip).unwrap();
    let (mut d, _) = daemon_with(scanner(&dir, 0, "clean"), "unzip-dd");
    let said = d.execute(&Intent::Unzip(format!("unzip \"{}\"", zip.display())));
    let said = when_done(&mut d, said);
    assert!(said.starts_with("Unpacked 2 files into") && said.contains("found nothing"), "{said}");
    assert!(dir.join("photos").join("trip").join("notes.txt").is_file());
}

// ------------------------------------------------------------------ scans and paths

#[test]
fn a_photographed_thing_is_named_for_what_it_looks_like() {
    assert_eq!(what_was_scanned("COFFEE HOUSE\nLatte 4.50\nSubtotal 4.50\nTax 0.36\nTOTAL $4.86"), Scanned::Receipt);
    assert_eq!(
        what_was_scanned("The meeting agreed to move the launch to October and revisit the budget next week."),
        Scanned::Page
    );
    assert_eq!(what_was_scanned("hi"), Scanned::Unclear);
    let line = atlas::files::after_scan(Scanned::Receipt, 12);
    assert!(line.starts_with("Looks like a receipt."), "{line}");
}

#[test]
fn the_file_you_meant_is_found_in_what_you_said() {
    use atlas::files::path_in;
    assert_eq!(path_in("read \"C:\\Users\\erics\\Downloads\\my letter.pdf\" please", &["pdf"]).as_deref(), Some("C:\\Users\\erics\\Downloads\\my letter.pdf"));
    assert_eq!(path_in("read this pdf C:\\docs\\tides.pdf.", &["pdf"]).as_deref(), Some("C:\\docs\\tides.pdf"));
    assert_eq!(path_in("read this pdf", &["pdf"]), None);
}

#[test]
fn the_zip_reader_s_parts_each_do_their_one_job() {
    use atlas::unpack::{entries_of, name_inside, nesting, read_entry};
    let bytes = std::fs::read(fixture("photos.zip")).unwrap();
    let entries = entries_of(&bytes).unwrap();
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["trip/notes.txt", "trip/", "trip/tides.pdf"]);
    assert_eq!(nesting(&entries), 1, "no zips inside");
    let notes = read_entry(&bytes, &entries[0]).unwrap();
    assert_eq!(notes.len() as u64, entries[0].size, "deflated back to its full size");
    assert_eq!(name_inside("trip\\notes.txt").unwrap(), Path::new("trip").join("notes.txt"));
    assert!(name_inside("C:\\Windows\\evil.dll").is_err());
    // 5 Oct 2026 audit, Q8: a drive or stream further down, and device names.
    assert!(name_inside("a/C:evil.txt").is_err());
    assert!(name_inside("notes.txt:hidden").is_err());
    for dev in ["CON", "nul.txt", "a/com1.log", "LPT9", "aux "] {
        assert!(name_inside(dev).is_err(), "{dev}");
    }
    for fine in ["console.txt", "com10.txt", "lpt0", "nullish/a.txt"] {
        assert!(name_inside(fine).is_ok(), "{fine}");
    }
    assert!(name_inside("../x").is_err());
    assert!(entries_of(b"not a zip").is_err());
}

#[test]
fn an_absurd_repeat_interval_is_refused_not_overflowed() {
    // 5 Oct 2026 audit, Q20: INTERVAL is a u32 from any calendar you import.
    assert!(atlas::recur::Rule::parse("FREQ=DAILY;INTERVAL=4294967295").is_err());
    assert!(atlas::recur::Rule::parse("FREQ=WEEKLY;INTERVAL=2").is_ok());
}

#[test]
fn windows_own_tools_come_from_windows_own_folder() {
    // 5 Oct 2026 audit, Q9: never a curl.cmd beside atlas.exe or on PATH.
    let root = std::env::temp_dir().join(format!("atlas-sysroot-{}", std::process::id()));
    let sys = root.join("System32");
    std::fs::create_dir_all(sys.join("WindowsPowerShell").join("v1.0")).unwrap();
    std::fs::write(sys.join("curl.exe"), b"").unwrap();
    std::fs::write(sys.join("WindowsPowerShell").join("v1.0").join("powershell.exe"), b"").unwrap();
    assert_eq!(atlas::tools::system_tool("curl", &root), Some(sys.join("curl.exe")));
    assert_eq!(atlas::tools::system_tool("PowerShell", &root), Some(sys.join("WindowsPowerShell").join("v1.0").join("powershell.exe")));
    assert_eq!(atlas::tools::system_tool("reg", &root), None, "not there: left to the old lookup");
    assert_eq!(atlas::tools::system_tool("npm", &root), None, "not a Windows tool");
    let _ = std::fs::remove_dir_all(&root);
}
