//! `Intent::Files` reaches a real search.
//!
//! It used to answer by treating whatever you said as a filename and reporting
//! its category: "the budget is a Document. It needs an application that opens
//! it." That is a well-formed sentence about nothing. `index.search` and
//! `index.search_content` had both existed the whole time and no caller
//! anywhere in the daemon ever reached either of them.
//!
//! `asking` sits in front of it because a spoken question is a bad shape for
//! an index: mostly scaffolding words that match everything, sometimes a
//! pointer at something the index has never seen, sometimes two questions in
//! one breath.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::index::{Index, IndexConfig};
use atlas::intent::Intent;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-finding-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
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
    Daemon::new(
        c,
        p,
        None,
        Store::new(tmp(tag)),
        Proactive::new(ProactiveConfig::default()),
    )
}

/// Bare pronouns ("that", "it") are caught earlier still, by
/// `resolve_subject`, which asks its own question. This covers what that pass
/// does not: a phrase that points without using a pronoun at all.
#[test]
fn a_question_that_points_instead_of_naming_gets_asked_about() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "points");

    let out = d.execute(&Intent::Files("the one earlier".into()));

    assert!(
        out.contains("which one") || out.contains("what's it about"),
        "a referent the index has never seen must be asked about, not guessed \
         at — a guessed one retrieves confidently and wrongly. Got: {out}"
    );
    assert!(
        !out.contains("is a "),
        "must not fall back to classifying the words as a filename: {out}"
    );
}

#[test]
fn an_empty_result_says_what_it_actually_looked_for() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "empty");

    let out = d.execute(&Intent::Files(
        "can you find me the thing about the quarterly budget".into(),
    ));

    assert!(
        out.to_lowercase().contains("budget"),
        "\"I couldn't find anything\" leaves you unable to tell a bad search \
         from an empty disk. Got: {out}"
    );
    assert!(
        !out.to_lowercase().contains("can you"),
        "scaffolding words should have been stripped before searching: {out}"
    );
}

/// When no filename matches, the daemon reads *inside* the indexed documents
/// and answers with the line the word was found on.
///
/// The file is named opaquely on purpose -- nothing in its name matches the
/// question -- so the only way to reach it is by content. Before the wire this
/// returned "nothing in the indexed files matches"; a real note about the
/// harvest, sitting on the disk with the word in its body, was invisible.
#[test]
fn a_word_only_in_the_body_is_found_by_reading_inside() {
    // A separate tag from the daemon's store below, so the store's own
    // `tmp()` does not wipe the folder we are about to index.
    let dir = tmp("inside-corpus");
    // Opaque name, so filename search cannot reach it. The body carries the
    // word the question is about.
    std::fs::write(
        dir.join("7f3a12.md"),
        "planning for next year -- we agreed to expand the asparagus harvest and \
         double the beds along the south fence.",
    )
    .unwrap();

    let idx_cfg = IndexConfig {
        roots: vec![dir.to_string_lossy().to_string()],
        exclude_dirs: vec![],
        exclude_exts: vec![],
        max_depth: 8,
        max_enrich_mb: 20,
    };

    let mut c = cfg();
    c.indexing = Some(idx_cfg.clone());
    let p = plat();
    let mut d = daemon(&c, &p, "inside");
    // The index the background scan would have built, built here directly.
    d.index = Index::scan(&idx_cfg);

    let out = d.execute(&Intent::Files(
        "can you find me the note about the asparagus harvest".into(),
    ));

    assert!(
        out.to_lowercase().contains("asparagus"),
        "the word lives only in the file's body, so a real search must reach it \
         by reading inside -- not report the disk as empty. Got: {out}"
    );
    assert!(
        out.contains("written inside"),
        "a body match is a different answer from a filename match and should say \
         so, with the line it was found on. Got: {out}"
    );
    assert!(
        !out.starts_with("Nothing in the"),
        "before the content fallback this returned the empty-search sentence; \
         that regression must not come back. Got: {out}"
    );
}

#[test]
fn two_questions_in_one_breath_are_not_silently_merged() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "two");

    let out = d.execute(&Intent::Files(
        "find the budget spreadsheet and also the travel receipts".into(),
    ));

    assert!(
        out.contains("two questions"),
        "searching both at once retrieves the average of two topics and the \
         best match for neither, so the second must at least be said out \
         loud. Got: {out}"
    );
}

/// "Convert this pdf to text" is not a search for files named after the words
/// *pdf*, *to* and *text* -- it is a question about a change, and the answer
/// says what that change costs rather than what the disk holds. Before this,
/// the whole `convert this` / `join these` half of `Intent::Files` came back
/// as the filename search that shares the intent. Driven through the real
/// parser, so the phrase that reaches the conversion is the one a person says.
#[test]
fn a_conversion_is_answered_as_one_not_searched_for() {
    use atlas::intent::Parser;
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "convert");
    let parser = Parser::new(&c.commands);

    let said = parser.parse("convert this pdf to text");
    assert_eq!(
        said,
        Intent::Files("pdf to text".into()),
        "the spoken phrase must route to the files intent with the formats as \
         its argument: {said:?}"
    );
    let out = d.execute(&said);

    assert!(
        out.contains("lose") && out.contains("tables"),
        "pulling text out of a PDF loses its layout and tables, and that is \
         what the conversion answer must say -- not treat the words as a \
         filename to look up. Got: {out}"
    );
    assert!(
        !out.starts_with("Searching") && !out.starts_with("Nothing in the"),
        "a conversion must not fall through to the filename search: {out}"
    );
}

/// A change that does not exist is refused plainly, in the conversion path --
/// so "convert an audio file to a PDF" gets an honest no rather than a hunt
/// for files.
#[test]
fn a_conversion_that_cannot_be_done_says_so() {
    use atlas::intent::Parser;
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "cannot-convert");
    let parser = Parser::new(&c.commands);

    let out = d.execute(&parser.parse("convert this audio to pdf"));

    assert!(
        out.contains("can't") || out.contains("cannot") || out.contains("does not"),
        "an impossible conversion should be refused in plain words: {out}"
    );
}

/// The guard against hijacking real searches: a question that merely contains
/// the word " to " -- and names no format on either side -- is still a search.
#[test]
fn an_ordinary_search_that_contains_to_is_not_mistaken_for_a_conversion() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, "to-search");

    let out = d.execute(&Intent::Files("the note about how to prune roses".into()));

    assert!(
        !out.contains("you'd lose") && !out.starts_with("Yes -- I'd"),
        "neither 'how' nor 'prune roses' is a format, so this is a search and \
         must not be read as a conversion: {out}"
    );
}
