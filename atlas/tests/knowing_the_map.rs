//! The notes index, built from a real folder and checked against it.
//!
//! `contents.rs` shipped complete: a line, a rule for what makes a line worth
//! reading, a search over an index, a drift check, a boot report. All of it
//! passed its own tests against hand-built `Contents` values, and **nothing in
//! the program ever built one from a folder**. An index nothing writes cannot
//! drift, so `nudge::drifted` — also complete, also tested — had nothing to be
//! raised about and no caller. Two modules, both green, one dead feature.
//!
//! `tests/contents_trace_voice.rs` covers the reasoning against hand-built
//! values, and still should. This file covers the half that was missing: a
//! real folder on disk, an index written from it, the drift that appears when
//! the folder moves on without it, the nudge that says so, and the rebuild
//! that answers the nudge.
//!
//! The end-to-end tests go through `Daemon` rather than calling the new
//! functions directly, because calling the functions directly is exactly what
//! already existed while the feature was dead.

use atlas::config::Config;
use atlas::contents::{
    drift, from_folder, load, master_path, names_on_disk, parse, rebuild, save, says_for, Contents,
    Line, MASTER_FILE, MAX_LINES, MAX_SAYS_CHARS,
};
use atlas::daemon::Daemon;
use atlas::intent::{Intent, Parser};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-map-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// A notes folder inside its own root, so the master index has somewhere to
/// live beside it — which is where `master_path` puts it.
fn notes(tag: &str) -> PathBuf {
    let root = tmp(tag);
    let dir = root.join("notes");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(dir: &Path, name: &str, body: &str) {
    std::fs::write(dir.join(format!("{name}.md")), body).unwrap();
}

/// A note in the shape `research::save` writes: a heading, prose, sources.
fn a_note(topic: &str, first_line: &str) -> String {
    format!("# {topic}\n\n{first_line}\n\nMore detail nobody needs at boot.\n\n## Sources\n- https://example.invalid/a\n")
}

// ===================== what a line says ==================================

#[test]
fn the_line_says_what_is_inside_not_what_it_is_called() {
    // The module's first rule. A summary lifted from the heading would be the
    // filename again, which is the thing `is_useful` rejects.
    let said = says_for(&a_note("spain-trip", "Flights are booked; the hotel is still undecided."));
    assert_eq!(said, "Flights are booked; the hotel is still undecided.");
    assert!(!said.contains("spain-trip"), "the summary restated the title: {said}");
    assert!(Line::new("spain-trip", &said).is_useful());
}

#[test]
fn a_url_on_its_own_is_a_source_not_a_description() {
    let said = says_for("# links\n\nhttps://example.invalid/a-very-long-url-that-says-nothing\n");
    assert!(said.is_empty(), "a bare URL was taken as the summary: {said}");
}

#[test]
fn a_two_word_line_is_not_a_summary() {
    // Front matter, a date, a one-word tag: all lines, none of them a
    // description of what is in the note.
    let said = says_for("# notes\n\n2026-09-14\n\nThe roof quote came back higher than the last one.\n");
    assert_eq!(said, "The roof quote came back higher than the last one.");
}

#[test]
fn a_long_first_line_is_cut_to_one_sentence() {
    let said = says_for("# x\n\nThe quote came back. Then a great deal of further detail that nobody wants to read at boot time and which is the note itself.\n");
    assert_eq!(said, "The quote came back.");
}

#[test]
fn a_single_long_sentence_is_cut_at_a_word_boundary() {
    let long = "word ".repeat(60);
    let said = says_for(&format!("# x\n\n{long}\n"));
    assert!(said.chars().count() <= MAX_SAYS_CHARS + 1, "not truncated: {}", said.chars().count());
    assert!(said.ends_with('…'), "truncation is not marked: {said}");
    assert!(!said.contains("wor…"), "cut mid-word: {said}");
}

#[test]
fn a_note_with_no_prose_gets_an_empty_line_and_is_reported_as_useless() {
    // Deliberately not a fallback to the title. Inventing a summary here
    // would defeat `useless_lines`, which is the check that finds notes the
    // index cannot actually help you find.
    let dir = notes("blank");
    write(&dir, "empty-one", "# empty-one\n\n## Sources\n");
    let c = from_folder(&dir);
    assert_eq!(c.lines.len(), 1);
    assert_eq!(c.lines[0].says, "");
    assert_eq!(c.useless_lines().len(), 1, "a note with nothing in it was called useful");
}

#[test]
fn a_summary_never_carries_a_newline() {
    // The written format is one line per item. A description with a newline
    // in it produces a file that does not parse back, and an index that does
    // not parse back reads as drift that never happened.
    let said = says_for("# x\n\nThe roof quote came back higher\nthan the last one\n");
    assert!(!said.contains('\n'), "a line break survived into the summary: {said:?}");
}

// ===================== reading a real folder =============================

#[test]
fn the_index_is_built_from_the_folder() {
    let dir = notes("build");
    write(&dir, "spain-trip", &a_note("spain-trip", "Flights booked, hotel undecided."));
    write(&dir, "roof", &a_note("roof", "The quote came back higher than last year."));

    let c = from_folder(&dir);
    let names: Vec<&str> = c.lines.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, vec!["roof", "spain-trip"], "not sorted, or not read: {names:?}");
    assert!(c.lines[1].says.contains("Flights booked"));
}

#[test]
fn a_note_is_named_the_way_a_person_names_it() {
    let dir = notes("stem");
    write(&dir, "roof", &a_note("roof", "The quote came back higher."));
    let c = from_folder(&dir);
    assert_eq!(c.lines[0].name, "roof", "the index named the file, not the note");
}

#[test]
fn a_subfolder_is_a_folder_line_and_says_how_much_is_in_it() {
    let dir = notes("sub");
    let inner = dir.join("work");
    std::fs::create_dir_all(&inner).unwrap();
    write(&inner, "a", &a_note("a", "Something about the contract."));
    write(&inner, "b", &a_note("b", "Something else about the contract."));

    let c = from_folder(&dir);
    let f = c.lines.iter().find(|l| l.name == "work").expect("the folder was not listed");
    assert!(f.is_folder);
    assert_eq!(f.says, "2 notes inside");
    assert!(f.is_useful(), "a folder line that says nothing is no better than the listing");
}

#[test]
fn only_notes_and_folders_are_things_the_index_claims_to_describe() {
    let dir = notes("other");
    write(&dir, "real", &a_note("real", "A real note about the roof."));
    std::fs::write(dir.join("scratch.txt"), "not a note").unwrap();
    std::fs::write(dir.join("photo.png"), [0u8; 4]).unwrap();

    let on_disk = names_on_disk(&dir);
    assert_eq!(on_disk, vec!["real"], "something that is not a note was counted: {on_disk:?}");
}

#[test]
fn an_index_left_inside_the_folder_is_never_listed_as_a_note_about_itself() {
    let dir = notes("selfindex");
    write(&dir, "real", &a_note("real", "A real note about the roof."));
    std::fs::write(dir.join(MASTER_FILE), "- real — something\n").unwrap();
    assert_eq!(names_on_disk(&dir), vec!["real"]);
}

#[test]
fn the_master_index_lives_beside_the_folder_not_inside_it() {
    // Inside, it would be the one thing you have to list the folder to find,
    // and the folder is what it exists to save you listing.
    let dir = notes("where");
    let at = master_path(&dir);
    assert_eq!(at.file_name().unwrap(), MASTER_FILE);
    assert_eq!(at.parent().unwrap(), dir.parent().unwrap());
    assert!(!at.starts_with(&dir), "the index was put inside the folder it describes");
}

// ===================== written down and read back ========================

#[test]
fn the_index_survives_the_round_trip_to_disk() {
    // The index is written on one run and read on the next. Anything that
    // does not survive that trip reads as drift that never happened — which
    // is the one failure mode that would make the whole check untrustworthy.
    let dir = notes("round");
    write(&dir, "spain-trip", &a_note("spain-trip", "Flights booked, hotel undecided."));
    write(&dir, "blank", "# blank\n");
    let inner = dir.join("work");
    std::fs::create_dir_all(&inner).unwrap();

    let built = from_folder(&dir);
    let at = master_path(&dir);
    save(&built, &at).unwrap();
    let back = load(&dir.to_string_lossy(), &at).expect("nothing loaded back");
    assert_eq!(back, built, "the index changed on its way to disk and back");
}

#[test]
fn prose_in_the_index_file_is_not_mistaken_for_a_line() {
    // The file carries a header saying not to hand-edit it. If that header
    // parsed as index lines, every boot would invent notes that do not exist.
    let c = parse("notes", &format!("{}\n- roof — the quote came back\n", atlas::contents::HEADER));
    assert_eq!(c.lines.len(), 1, "the header leaked into the index: {:?}", c.lines);
    assert_eq!(c.lines[0].name, "roof");
}

#[test]
fn rebuild_writes_the_file_where_boot_will_look_for_it() {
    let dir = notes("rebuild");
    write(&dir, "roof", &a_note("roof", "The quote came back higher."));
    let c = rebuild(&dir).unwrap();
    let at = master_path(&dir);
    assert!(at.is_file(), "rebuild did not write anything");
    assert_eq!(load(&dir.to_string_lossy(), &at).unwrap(), c);
}

// ===================== the drift that could not happen ===================

#[test]
fn a_note_added_after_the_index_was_written_is_drift() {
    let dir = notes("added");
    write(&dir, "roof", &a_note("roof", "The quote came back higher."));
    let c = rebuild(&dir).unwrap();

    write(&dir, "spain-trip", &a_note("spain-trip", "Flights booked, hotel undecided."));
    let d = drift(&c, &names_on_disk(&dir));
    assert_eq!(d.unlisted, vec!["spain-trip"]);
    assert!(d.missing.is_empty());
    assert!(!d.is_clean());
}

#[test]
fn a_note_deleted_after_the_index_was_written_is_drift_too() {
    let dir = notes("deleted");
    write(&dir, "roof", &a_note("roof", "The quote came back higher."));
    write(&dir, "spain-trip", &a_note("spain-trip", "Flights booked."));
    let c = rebuild(&dir).unwrap();

    std::fs::remove_file(dir.join("spain-trip.md")).unwrap();
    let d = drift(&c, &names_on_disk(&dir));
    assert_eq!(d.missing, vec!["spain-trip"]);
    assert!(d.unlisted.is_empty());
}

// ===================== reached from the running program ==================

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

/// A config whose notes folder is this test's own, so the tests never read or
/// write the real `data/notes`.
fn cfg_at(dir: &Path) -> Config {
    let mut c = Config::load(Path::new("config")).unwrap();
    let tools = c.tools.as_mut().expect("the shipped config has a tools section");
    tools.research.notes_dir = dir.to_string_lossy().to_string();
    c
}

/// The store gets its own directory. `tmp` clears what it hands back, and
/// sharing a tag with `notes()` deleted the notes folder out from under the
/// daemon that was about to index it.
fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    let store = tmp(&format!("{tag}-store"));
    Daemon::new(c, p, None, Store::new(store), Proactive::new(ProactiveConfig::default()))
}

#[test]
fn a_daemon_writes_the_first_index_by_itself() {
    // The moment the whole module was waiting for: something in the program
    // constructs a `Contents` from a real folder.
    let dir = notes("boot");
    write(&dir, "roof", &a_note("roof", "The quote came back higher."));
    let (c, p) = (cfg_at(&dir), plat());
    let d = daemon(&c, &p, "boot");

    assert_eq!(d.contents.lines.len(), 1, "the daemon booted with an empty index");
    assert!(master_path(&dir).is_file(), "the first index was never written down");
}

#[test]
fn a_first_index_is_not_drift() {
    // A folder that has never been indexed disagrees with nothing. Greeting a
    // fresh install with "your index is wrong" would be Atlas complaining
    // about a file it had not written yet.
    let dir = notes("first");
    write(&dir, "roof", &a_note("roof", "The quote came back higher."));
    let (c, p) = (cfg_at(&dir), plat());
    let d = daemon(&c, &p, "first");
    assert!(d.index_drift().is_clean(), "a folder indexed one second ago reported drift");
}

#[test]
fn an_empty_notes_folder_writes_no_index_either() {
    // An index of nothing describes nothing and cannot drift. Writing one
    // puts a file on disk every time a daemon starts against a folder nobody
    // has used yet — which is how `data/index.md` ended up inside a shipped
    // zip, written by the test suite itself.
    let dir = notes("emptyfolder");
    let (c, p) = (cfg_at(&dir), plat());
    let d = daemon(&c, &p, "emptyfolder");
    assert!(d.contents.lines.is_empty());
    assert!(!master_path(&dir).exists(), "an index was written for an empty folder");
}

#[test]
fn no_notes_folder_is_not_an_error_and_writes_nothing() {
    let root = tmp("nofolder");
    let dir = root.join("notes-that-do-not-exist");
    let (c, p) = (cfg_at(&dir), plat());
    let d = daemon(&c, &p, "nofolder");
    assert!(d.contents.lines.is_empty());
    assert!(!master_path(&dir).exists(), "an index was written for a folder that isn't there");
}

#[test]
fn the_daemon_sees_the_folder_move_on_without_it() {
    let dir = notes("sees");
    write(&dir, "roof", &a_note("roof", "The quote came back higher."));
    let (c, p) = (cfg_at(&dir), plat());
    let d = daemon(&c, &p, "sees");

    write(&dir, "spain-trip", &a_note("spain-trip", "Flights booked."));
    let found = d.index_drift();
    assert_eq!(found.unlisted, vec!["spain-trip"]);
    assert!(atlas::nudge::drifted(&found).is_some(), "drift that raises nothing is not detected");
}

#[test]
fn a_daemon_that_starts_with_an_index_already_there_reads_it_rather_than_rebuilding() {
    // The ordinary case in real use, and the one that makes the check mean
    // anything: Atlas was off, the folder moved on, and the index on disk is
    // now a claim that has turned out to be wrong. A `load_index` that
    // re-derived from the folder would agree with it by construction and
    // never find this — which is the same as not having the check.
    //
    // Found by breaking `load_index` deliberately and watching every test
    // still pass: every other daemon test here starts with no index at all,
    // so the load branch was never the thing under test.
    let dir = notes("stale");
    write(&dir, "roof", &a_note("roof", "The quote came back higher."));
    rebuild(&dir).unwrap();
    write(&dir, "spain-trip", &a_note("spain-trip", "Flights booked."));

    let (c, p) = (cfg_at(&dir), plat());
    let d = daemon(&c, &p, "stale");

    assert_eq!(d.contents.lines.len(), 1, "the daemon rebuilt the index instead of reading it");
    assert_eq!(d.index_drift().unlisted, vec!["spain-trip"]);
}

#[test]
fn rebuilding_from_the_daemon_settles_the_drift_and_says_what_changed() {
    let dir = notes("settle");
    write(&dir, "roof", &a_note("roof", "The quote came back higher."));
    let (c, p) = (cfg_at(&dir), plat());
    let mut d = daemon(&c, &p, "settle");

    write(&dir, "spain-trip", &a_note("spain-trip", "Flights booked."));
    let said = d.rebuild_index();

    assert!(said.contains('1'), "the rebuild did not say what it changed: {said}");
    assert!(d.index_drift().is_clean(), "the drift survived the rebuild");
    assert_eq!(d.contents.lines.len(), 2, "the daemon kept the old index in memory");
}

#[test]
fn the_rebuild_reports_against_the_drift_it_found_not_the_one_it_left() {
    // Afterwards there is none by construction. "The index matches what's
    // there" is not an answer to "what did you just do".
    let dir = notes("report");
    write(&dir, "roof", &a_note("roof", "The quote came back higher."));
    let (c, p) = (cfg_at(&dir), plat());
    let mut d = daemon(&c, &p, "report");
    write(&dir, "spain-trip", &a_note("spain-trip", "Flights booked."));

    let said = d.rebuild_index();
    assert!(!said.contains("already matched"), "the rebuild reported the state it created: {said}");
}

#[test]
fn a_rebuild_names_the_notes_it_could_not_summarise() {
    let dir = notes("blanks");
    write(&dir, "empty-one", "# empty-one\n");
    let (c, p) = (cfg_at(&dir), plat());
    let mut d = daemon(&c, &p, "blanks");
    write(&dir, "empty-two", "# empty-two\n");

    let said = d.rebuild_index();
    assert!(
        said.contains("couldn't summarise"),
        "a note the index cannot help you find went unmentioned: {said}"
    );
}

#[test]
fn an_index_past_its_limit_is_said_to_need_splitting() {
    let dir = notes("big");
    for i in 0..=MAX_LINES {
        write(&dir, &format!("note-{i:03}"), &a_note("x", "Something worth a whole line of prose."));
    }
    let (c, p) = (cfg_at(&dir), plat());
    let mut d = daemon(&c, &p, "big");
    write(&dir, "one-more", &a_note("x", "Something worth a whole line of prose."));

    let said = d.rebuild_index();
    assert!(said.contains("splitting"), "a map that has become a list said nothing: {said}");
}

// ===================== the offer has to run something ====================

#[test]
fn the_relief_the_nudge_offers_parses_to_something_that_rebuilds() {
    // The guard this whole wiring turns on. `proactive::from_nudge` puts the
    // nudge's `relief` in front of you as the command that runs if you say
    // yes; if that text parses to `Unknown`, saying yes agrees to nothing
    // happening — the exact failure this codebase names for `confirmed.rs`.
    let d = atlas::contents::Drift { unlisted: vec!["spain-trip".into()], missing: vec![] };
    let n = atlas::nudge::drifted(&d).expect("drift raised no nudge");
    let offer = atlas::proactive::from_nudge(&n);

    let cfg = Config::load(Path::new("config")).unwrap();
    let parser = Parser::new(&cfg.commands);
    assert_eq!(
        parser.parse(&offer.command),
        Intent::RebuildIndex,
        "the offer's own command does not parse: {:?}",
        offer.command
    );
}

#[test]
fn saying_yes_to_the_offer_actually_rebuilds() {
    let dir = notes("yes");
    write(&dir, "roof", &a_note("roof", "The quote came back higher."));
    let (c, p) = (cfg_at(&dir), plat());
    let mut d = daemon(&c, &p, "yes");
    write(&dir, "spain-trip", &a_note("spain-trip", "Flights booked."));
    assert!(!d.index_drift().is_clean());

    let now = atlas::store::now();
    let said = d.turn("rebuild the index", now);
    assert!(!said.is_empty(), "the command said nothing");
    assert!(d.index_drift().is_clean(), "the command ran and the index is still wrong: {said}");
}

// ===================== the index answers a question ======================

#[test]
fn asked_what_it_has_on_a_subject_it_names_the_notes_worth_opening() {
    let dir = notes("ask");
    write(&dir, "spain-trip", &a_note("spain-trip", "Flights booked, the hotel is undecided."));
    write(&dir, "roof", &a_note("roof", "The quote came back higher than last year."));
    let (c, p) = (cfg_at(&dir), plat());
    let mut d = daemon(&c, &p, "ask");

    let said = d.turn("what do you have on the hotel in spain", atlas::store::now());
    assert!(said.contains("spain-trip"), "the index did not point anywhere: {said}");
    assert!(!said.contains("roof"), "the index opened everything: {said}");
}

#[test]
fn asked_about_something_it_has_nothing_on_it_says_so() {
    // A real answer, and a better one than opening every note on the chance
    // something matches.
    let dir = notes("nothing");
    write(&dir, "roof", &a_note("roof", "The quote came back higher."));
    let (c, p) = (cfg_at(&dir), plat());
    let mut d = daemon(&c, &p, "nothing");

    let said = d.turn("what do you have on sourdough", atlas::store::now());
    assert!(said.to_lowercase().contains("nothing"), "got: {said}");
}

#[test]
fn asked_with_no_subject_it_says_how_much_it_knows_of_without_opening_any() {
    let dir = notes("count");
    for i in 0..3 {
        write(&dir, &format!("n{i}"), &a_note("x", "Something worth a whole line of prose."));
    }
    let (c, p) = (cfg_at(&dir), plat());
    let mut d = daemon(&c, &p, "count");

    let said = d.turn("what's in my notes", atlas::store::now());
    assert!(said.contains('3'), "the count is wrong or absent: {said}");
    assert!(said.contains("have 1 open"), "boot loaded more than the index: {said}");
}

// ===================== the shape of an empty index =======================

#[test]
fn an_empty_index_is_not_a_broken_one() {
    let c = Contents::new("notes");
    assert!(drift(&c, &[]).is_clean());
    assert_eq!(atlas::nudge::what_i_know_of(&c), "I know about 0 things and have 1 open.");
}
