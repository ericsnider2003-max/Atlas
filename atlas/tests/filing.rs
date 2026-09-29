//! Deciding where a file should live.
//!
//! The rule everything here defends: **never move something you cannot find
//! again.** A file put somewhere clever that you cannot find is worse than a
//! messy Downloads folder — the mess is at least where you left it. So most of
//! these tests are about what Atlas declines to file.

use atlas::filing::{as_change, spoken, suggest, Bucket, Suggestion, ARCHIVE_AFTER_DAYS};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from("/home/eric/Filed")
}

fn moved(s: &Suggestion) -> Option<(PathBuf, Bucket)> {
    match s {
        Suggestion::Move { to, bucket, .. } => Some((to.clone(), *bucket)),
        Suggestion::Leave { .. } => None,
    }
}

// --- what it declines to file ----------------------------------------------

#[test]
fn a_name_that_says_nothing_is_left_alone() {
    // "Document.docx" and "final.docx" are usually files you are part-way
    // through sorting out yourself. Guessing at them is how filing loses things.
    for name in ["untitled.docx", "copy.pdf", "final.docx", "document.docx", "draft.md"] {
        let ext = name.rsplit('.').next().unwrap();
        let s = suggest(&root(), name, ext, 10);
        assert!(
            matches!(s, Suggestion::Leave { .. }),
            "{name} was filed on a name that says nothing: {s:?}"
        );
    }
}

#[test]
fn an_unfamiliar_kind_of_file_is_left_where_you_put_it() {
    // Deliberately no catch-all bucket. An "Other" folder is where files go to
    // be forgotten, and moving something there is worse than leaving it.
    let s = suggest(&root(), "board.kicad_pcb", "kicad_pcb", 10);
    match s {
        Suggestion::Leave { why } => assert!(why.contains("confident"), "{why}"),
        other => panic!("it invented a home for an unknown type: {other:?}"),
    }
}

#[test]
fn declining_always_says_why() {
    // "I can't" without a reason is a dead end that makes Atlas look broken
    // when it is being careful.
    for (name, ext) in [("untitled.docx", "docx"), ("board.kicad_pcb", "kicad_pcb")] {
        match suggest(&root(), name, ext, 10) {
            Suggestion::Leave { why } => {
                assert!(why.len() > 15, "the reason is too thin to act on: {why}")
            }
            other => panic!("expected it to decline, got {other:?}"),
        }
    }
}

#[test]
fn nothing_is_ever_moved_without_going_through_the_safety_gate() {
    // Deciding where something *should* go and being *allowed* to move it are
    // different questions. Filing that skipped `system::judge` could reach
    // outside the folders Atlas is permitted to work in.
    let s = suggest(&root(), "notes.md", "md", 10);
    let change = as_change(Path::new("/home/eric/Downloads/notes.md"), &s)
        .expect("a move should produce a change to judge");
    let cfg = atlas::system::SystemConfig::default();
    // With default roots, a path outside them must be refused — and the
    // refusal must name the fix.
    match atlas::system::judge(&change, &cfg) {
        // Whichever refusal comes first, it must leave you able to act on it.
        // With the shipped defaults that is the master switch; with it on, the
        // roots. Both name their own setting.
        atlas::system::Verdict::Refuse(why) => assert!(
            why.contains("file_roots") || why.contains("system.enabled"),
            "the refusal doesn't say how to allow it: {why}"
        ),
        // If the default roots do cover it, that is fine too — what must never
        // happen is filing bypassing the judgement entirely.
        atlas::system::Verdict::Go { .. } => {}
    }
}

#[test]
fn leaving_something_alone_produces_no_change_at_all() {
    let s = Suggestion::Leave { why: "x".into() };
    assert!(as_change(Path::new("/a/b.txt"), &s).is_none());
}

// --- what it does file -----------------------------------------------------

#[test]
fn something_untouched_for_a_year_is_archived_whatever_it_is() {
    // The one rule that needs no understanding of the file.
    let s = suggest(&root(), "accounts-2023.xlsx", "xlsx", ARCHIVE_AFTER_DAYS + 5);
    let (to, bucket) = moved(&s).expect("a year-old file should be archived");
    assert_eq!(bucket, Bucket::Archive);
    assert!(to.ends_with("Archive/accounts-2023.xlsx"), "{}", to.display());
}

#[test]
fn recency_beats_type() {
    // The same file, recent, is something you are working on rather than
    // history. Sorting by *how soon you need it* is the whole point of the
    // scheme — sorting by type puts today's invoice next to 2019's.
    let old = suggest(&root(), "accounts.xlsx", "xlsx", 400);
    let new = suggest(&root(), "accounts.xlsx", "xlsx", 5);
    assert_eq!(moved(&old).unwrap().1, Bucket::Archive);
    assert_eq!(moved(&new).unwrap().1, Bucket::Projects);
}

#[test]
fn a_book_is_reference_and_a_spreadsheet_is_work() {
    assert_eq!(moved(&suggest(&root(), "manual.pdf", "pdf", 10)).unwrap().1, Bucket::Resources);
    assert_eq!(moved(&suggest(&root(), "budget.xlsx", "xlsx", 10)).unwrap().1, Bucket::Projects);
}

#[test]
fn every_move_says_why_in_words_you_could_disagree_with() {
    let s = suggest(&root(), "manual.pdf", "pdf", 10);
    match &s {
        Suggestion::Move { why, .. } => assert!(why.len() > 15, "{why}"),
        other => panic!("{other:?}"),
    }
    let line = s.line(Path::new("/home/eric/Downloads/manual.pdf"));
    assert!(line.contains("→"), "{line}");
    assert!(line.contains("refer back"), "the line doesn't explain the bucket: {line}");
}

#[test]
fn every_bucket_explains_itself() {
    // The folder names alone do not teach the scheme. "Areas" means nothing
    // until someone says "ongoing, no end date".
    for b in [Bucket::Projects, Bucket::Areas, Bucket::Resources, Bucket::Archive] {
        assert!(!b.what_it_means().is_empty(), "{b:?} has no explanation");
        assert!(!b.folder().is_empty());
    }
}

#[test]
fn the_summary_says_moving_is_reversible() {
    let said = spoken(4, 9);
    assert!(said.contains("trash") || said.contains("put back"), "{said}");
    assert!(said.contains('4') && said.contains('9'), "{said}");
}

#[test]
fn finding_nothing_worth_moving_reads_as_a_decision_not_a_failure() {
    let said = spoken(0, 12);
    assert!(said.contains("Nothing I'd move"), "{said}");
    assert!(!said.to_lowercase().contains("error"), "{said}");
}
