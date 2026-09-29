//! "That's actually a task" / "file that under the roof job" corrects how a
//! captured note was filed.
//!
//! Capture splits catching a thought from filing it: a note lands with a
//! *guessed* kind (`capture::kind_of`) and a set of handles, and the guess is
//! sometimes wrong. `capture::Notebook::correct` is the one writer that records
//! the correction -- it changes the kind or adds a handle and marks the note
//! confirmed, so the fix outlives the guess and `find` reaches the note by the
//! handle you actually used. It was written the day capture was and reached by
//! nothing a person could say: every other path only ever *added* notes.
//!
//! `Intent::Refile` is the door to it. These tests drive the plain sentence
//! through a real parser and a real `Daemon`, then read the notebook back and
//! assert the note's stored state changed -- the kind flipped, or the handle
//! landed, and `confirmed` went true -- rather than asserting on the reply
//! wording.

use atlas::capture::Kind;
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::intent::{Intent, Parser};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-refile-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor {
        id: 1,
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
        primary: true,
    }])
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

#[test]
fn the_correction_sentences_route_to_refile() {
    let c = Config::load(Path::new("config")).unwrap();
    let parser = Parser::new(&c.commands);

    // A name to file it under -> the handle is the argument.
    assert_eq!(
        parser.parse("file that under the roof job"),
        Intent::Refile("the roof job".into()),
        "\"file that under X\" must reach the refile handler with X as the argument"
    );
    // A kind correction -> the kind word is the argument.
    assert_eq!(
        parser.parse("that's actually a task"),
        Intent::Refile("task".into()),
        "\"that's actually a task\" must reach the refile handler"
    );
    // And it is not swallowed by `files`, which owns the nearby "what is this
    // file" phrase -- that one searches the disk, this one re-files a note.
    assert_eq!(
        parser.parse("what is this file"),
        Intent::Files(String::new()),
        "a files question must still reach the files handler, not refile"
    );
}

#[test]
fn refiling_as_a_task_flips_the_stored_kind_through_the_daemon() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = daemon(&c, &p, "kind");

    // Catch a thought. Nothing task-ish in the words, so capture files it as a
    // Fact -- the guess this test then corrects.
    d.turn("note that the north skylight leaks in heavy rain", 100);
    assert_eq!(d.notebook.notes.len(), 1, "the capture should have filed one note");
    let before = &d.notebook.notes[0];
    assert_eq!(before.kind, Kind::Fact, "capture's guessed kind for this note is Fact");
    assert!(!before.confirmed, "a fresh capture is an unconfirmed guess");

    // Correct it, through the real parser and daemon.
    d.turn("that's actually a task", 200);

    // The stored note itself changed -- not the reply, the state.
    let after = d.notebook.notes.last().expect("the note is still there");
    assert_eq!(after.kind, Kind::Task, "refiling as a task must rewrite the stored kind");
    assert!(after.confirmed, "a correction marks the note confirmed, so it is not re-guessed");
}

#[test]
fn refiling_under_a_name_adds_a_handle_you_can_find_it_by() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = daemon(&c, &p, "handle");

    d.turn("note that the tiles arrive in three separate deliveries", 100);
    assert_eq!(d.notebook.notes.len(), 1);
    let note_id = d.notebook.notes[0].id;
    assert!(
        !d.notebook.notes[0].about.iter().any(|h| h == "roofjob"),
        "the handle should not be there before it is filed under one"
    );
    assert!(!d.notebook.notes[0].confirmed);

    // File it under a name -- the handle is the argument after "file that under".
    d.turn("file that under roofjob", 200);

    let after = d.notebook.notes.iter().find(|n| n.id == note_id).expect("same note");
    assert!(
        after.about.iter().any(|h| h == "roofjob"),
        "refiling under a name must add that handle to the note, got {:?}",
        after.about
    );
    assert!(after.confirmed, "the correction marks the note confirmed");

    // And the promise the correction exists to keep: the note is now reachable
    // by the handle you filed it under, which the raw text never contained.
    let hits = d.notebook.find("roofjob", 300);
    assert!(
        hits.iter().any(|n| n.id == note_id),
        "the refiled note must be findable by its new handle"
    );
}

#[test]
fn refiling_with_no_recent_note_says_so_rather_than_touching_state() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = daemon(&c, &p, "empty");

    let reply = d.turn("that's actually a task", 100);
    assert!(
        reply.contains("no recent note") || reply.to_lowercase().contains("no recent"),
        "with nothing captured, refile should say there is nothing to refile, got: {reply}"
    );
    assert!(d.notebook.notes.is_empty(), "a refusal must not invent a note");
}
