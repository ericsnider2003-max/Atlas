//! Meaning search, actually reachable.
//!
//! `recall.rs` carried the semantic half from the start — `Piece.embedding`,
//! the cosine merge behind `RecallConfig::semantic`, `set_embedding` and
//! `unembedded` — and nothing anywhere ever produced a vector, so none of it
//! could run. `meaning.rs` is the encoder seam; these tests prove the wiring
//! the whole way through the daemon, because unit tests on `search` with
//! hand-made vectors are exactly what already existed while the feature was
//! dead (`tests/recall.rs` has them; they pass with or without this wiring).
//!
//! The fake encoder is a shell one-liner that maps related words onto the
//! same vector — which is the entire job description of a real embedding
//! model, minus ninety megabytes. The note and the question deliberately
//! share NO words, so word search alone cannot find it and a hit is proof
//! the meaning path ran.
// The helpers below serve tests that drive a Unix shell encoder.
#![cfg_attr(not(unix), allow(dead_code, unused_imports))]

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use atlas::tools::ExternalTool;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-meaning-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn daemon_at<'a>(c: &'a Config, p: &'a MockPlatform, root: PathBuf) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(root), Proactive::new(ProactiveConfig::default()))
}

fn write_note_in(root: &Path, name: &str, body: &str) -> PathBuf {
    let dir = root.join("data/notes");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, body).unwrap();
    p
}

/// An encoder that puts "apples" and "orchard" at the same point in space and
/// everything else somewhere orthogonal. Sixteen dimensions because
/// `parse_embedding` refuses anything shorter as too small to mean anything.
#[cfg(unix)]
fn fake_encoder() -> ExternalTool {
    ExternalTool {
        command: "sh".into(),
        args: vec![
            "-c".into(),
            r#"input=$(cat); case "$input" in *apple*|*orchard*) echo "1 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0";; *) echo "0 1 0 0 0 0 0 0 0 0 0 0 0 0 0 0";; esac"#.into(),
        ],
        stdin_text: true,
        result_file: None,
        timeout_secs: 30,
    }
}

#[cfg(unix)]
fn semantic_cfg() -> Config {
    let mut c = cfg();
    let t = c.tools.as_mut().expect("the shipped config has a tools section");
    t.recall.semantic = true;
    t.meaning.encoder = Some(fake_encoder());
    c
}

/// The whole point: a note that shares no words with the question is found by
/// what it is about. Word search cannot produce this hit — the same question
/// against the same note with `semantic: false` falls through below — so a
/// hit here is the meaning path running end to end: query embedded at ask
/// time, note embedded off the tick, cosine merged into the ranking.
#[cfg(unix)]
#[test]
fn a_note_sharing_no_words_with_the_question_is_found_by_meaning() {
    let root = tmp("bymeaning");
    write_note_in(
        &root,
        "test-meaning-orchard.md",
        "# Granny Smith orchard\n\nRows of trees behind the barn, pruned in February.\n",
    );
    let (c, p) = (semantic_cfg(), plat());
    let mut d = daemon_at(&c, &p, root);
    // The note gets its vector off the tick, the way it would in real use.
    d.tick(100);
    assert!(
        d.library.unembedded().is_empty(),
        "the tick did not embed the one note in the library"
    );
    let reply = d.turn("what do I know about apples", 200);
    assert!(
        reply.contains("Granny Smith") || reply.contains("orchard"),
        "the note and the question share no words, so only meaning could find it, \
         and it was not found: {reply}"
    );
}

/// The control for the test above: identical note, identical question,
/// meaning switched off. Word search must miss, because nothing overlaps —
/// which is what proves the hit above came from the meaning path and not from
/// some word the two texts quietly share.
#[cfg(unix)]
#[test]
fn the_same_question_with_meaning_off_falls_through() {
    let root = tmp("wordsonly");
    write_note_in(
        &root,
        "test-meaning-control.md",
        "# Granny Smith orchard\n\nRows of trees behind the barn, pruned in February.\n",
    );
    let (c, p) = (cfg(), plat());
    let mut d = daemon_at(&c, &p, root);
    d.tick(100);
    let reply = d.turn("what do I know about apples", 200);
    assert!(
        !reply.contains("Granny Smith"),
        "with semantic off and no shared words this note must not be found: {reply}"
    );
}

/// Vectors survive a restart. The library is rebuilt from disk on every load
/// with `embedding: None`, so without the remembered store each session would
/// re-run the encoder over every note. The second daemon here has NO encoder
/// at all — its pieces can only be embedded if the first session's vectors
/// were persisted and rehydrated by content.
#[cfg(unix)]
#[test]
fn vectors_made_in_one_session_survive_into_the_next() {
    let root = tmp("survives");
    write_note_in(&root, "test-meaning-keep.md", "# Kept note\n\nSomething worth remembering.\n");
    {
        let (c, p) = (semantic_cfg(), plat());
        let mut d = daemon_at(&c, &p, root.clone());
        d.tick(100);
        assert!(d.library.unembedded().is_empty(), "first session never embedded the note");
    }
    // Same root, no encoder: rehydration is the only way this can be embedded.
    let mut c2 = cfg();
    c2.tools.as_mut().unwrap().recall.semantic = true;
    let p2 = plat();
    let d2 = daemon_at(&c2, &p2, root);
    assert!(
        d2.library.unembedded().is_empty(),
        "the vector was not remembered across sessions; the encoder would run again on every start"
    );
}

/// The toggle is the toggle. An installed encoder must not start embedding
/// notes while meaning search is switched off — `semantic: false` is the
/// shipped default precisely so nothing runs until it is asked for.
#[cfg(unix)]
#[test]
fn an_installed_encoder_does_nothing_while_meaning_search_is_off() {
    let root = tmp("respectsoff");
    write_note_in(&root, "test-meaning-off.md", "# Quiet note\n\nNothing should touch this.\n");
    let mut c = cfg();
    c.tools.as_mut().unwrap().meaning.encoder = Some(fake_encoder());
    // recall.semantic stays at the shipped default: false.
    let p = plat();
    let mut d = daemon_at(&c, &p, root);
    d.tick(100);
    d.tick(200);
    assert!(
        !d.library.unembedded().is_empty(),
        "notes were embedded with meaning search switched off"
    );
}

/// An edited note is re-embedded; its old vector is not trusted and is
/// pruned. Content-keyed remembering, proven on state rather than asserted.
#[cfg(unix)]
#[test]
fn an_edited_note_loses_its_stale_vector_and_gets_a_fresh_one() {
    let root = tmp("edited");
    let note = write_note_in(&root, "test-meaning-edit.md", "# Editable\n\nFirst version.\n");
    let (c, p) = (semantic_cfg(), plat());
    let mut d = daemon_at(&c, &p, root);
    d.tick(100);
    assert!(d.library.unembedded().is_empty());
    // The note changes on disk; the library reload sees new content, which
    // misses in the remembered store, so the piece comes back unembedded.
    std::fs::write(&note, "# Editable\n\nSecond version, quite different.\n").unwrap();
    d.reload_library();
    assert!(
        !d.library.unembedded().is_empty(),
        "an edited note kept a vector describing text that no longer exists"
    );
    d.tick(200);
    assert!(d.library.unembedded().is_empty(), "the edited note was never re-embedded");
}
