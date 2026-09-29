//! Functions whose only evidence of life is a word.
//!
//! `dead_capabilities.rs` decides a function is reached by asking whether any
//! other file calls something with its name. That question is one word wide,
//! and **a word is not an address**. This tree defines `spoken` in 43
//! different modules, `assess` in six, `check` in five. A bare `spoken(`
//! somewhere is evidence that *a* `spoken` is called, and nothing more.
//!
//! This file measures how much of the deadness measurement rests on that.
//!
//! # What is counted, and why it is not "collisions"
//!
//! Counting collisions gives 1,862 of 4,510 functions, and means nothing:
//! `new`, `load`, `save`, `default` and `spoken` are deliberate conventions,
//! and a convention is not a defect. Nor is the scan's inability to resolve
//! `book.save()` -- that is a method call, genuinely unambiguous to the
//! compiler and genuinely opaque to a regex, and a list of 614 of those would
//! be a list nobody could act on.
//!
//! So this counts the narrow case where the ambiguity actually decides the
//! answer. A **free function** (not a method -- this codebase reaches those
//! as `crate::module::name(...)`), which:
//!
//! 1. shares its name with a free function in another module,
//! 2. is not called inside its own module, and
//! 3. is "reached" only by bare `name(` calls elsewhere, with no caller
//!    anywhere writing `module::name(`.
//!
//! For those, `dead_capabilities.rs` says *reached* and cannot know it. Each
//! one is either genuinely live or quietly dead, and the scan cannot tell you
//! which.
//!
//! # Why this exists on this tree now
//!
//! `publishing::rules` was held alive by `id.rules()` in the sibling crate --
//! a real call, to a different `rules`, in a crate that never mentions
//! `publishing`. That one was fixed properly (`common::sibling_reaches`
//! checks the module is even imported), but fixing one instance of a class
//! and writing a note about the rest is how a tree ends up with notes instead
//! of measurements. The main tree has `name_collisions.rs`; this side had
//! nothing, so the class was invisible here.
//!
//! # This is a ceiling, not a defect list
//!
//! Most of these 72 are certainly fine. The number is the size of the blind
//! spot, and the reason to keep it is that it must not grow unnoticed: every
//! new free function sharing a name with another module's is one more place
//! the deadness numbers are guessing.

mod common;

/// The 72, named rather than counted, on this suite's usual rule: a list may
/// grow, it may not grow silently. Sorted, so the diff is readable.
const AMBIGUOUS: &[&str] = &[
    "addressing::assess",
    // afterme::gaps came off 19 Sep 2026: it is private now. `Arrangement`
    // holds the inputs `gaps` wants, so `Arrangement::gaps` is the only way
    // in, and two public ways to ask the same question is how one of them
    // goes unused and then wrong.
    "answering::describe",
    "anticipate::suggested",
    "audio::announce",
    "awareness::describe",
    "backlog::now_secs",
    // booking::assess came off 21 Sep 2026: the Booking intent calls it
    // module-qualified, so the scan can see which `assess` it reached.
    "brief::ask",
    "cloudsync::result",
    // confirmed::answer came off 19 Sep 2026: `atlas walkthrough yes|no`
    // calls it module-qualified, so the scan can see which one it reached.
    "consult::classify",
    "credentials::spoken",
    "credentials::written",
    "delegate::interpret",
    "delivery::spoken",
    "dictate::ask_which",
    "draft::spoken",
    // explain::check (added 21 Sep 2026) is the fixed-Normal convenience over
    // `check_at`; the `atlas explain` handler always calls `check_at` with a
    // depth read from the request, so `check` has no production caller and looks
    // alive only because other modules define `check`.
    "explain::check",
    // finance::review and finance::summary came off 19 Sep 2026: `atlas money`
    // calls both module-qualified, so the scan can see which it reached.
    "faithful::check",
    "gaze::spoken",
    "grading::check",
    "grading::spoken",
    "handoff::should_ask",
    "handoff::spoken",
    "health::assess",
    "health::summary",
    "hollow::audit",
    "identity::explain",
    "language::args",
    "language::plan",
    "learned::spoken",
    "ledger::spoken",
    "ledger::summarise",
    // look::choose went with the HTML renderer when `look` became a design
    // spec and `look_paint` took over the drawing. Removed 18 Sep 2026 when the
    // two trees were collapsed and this guard was carried over.
    "mend::refusal",
    "mend::should_ask",
    // mesh::choose lost its only caller on 18 Sep 2026 -- it was in
    // `Intent::Sync`, passed four hardcoded literals, and discarded the answer.
    "modes::suggested",
    "notify::route",
    "otherside::written",
    // pipeline::review came off 20 Sep: closing the self-improvement loop's
    // Build stage gave it a real module-qualified caller in
    // `Daemon::attempt_own_fix`, so it is no longer reachable only by collision.
    // plainchange::{ask,explain,spoken,written} came off 21 Sep 2026: the
    // plain_change intent is wired and its handler calls them module-qualified.
    "presence::interpret",
    "prose::spoken",
    "recovery::suggest",
    "reference::worth_keeping",
    "references::resolve",
    // `retention::classify` and `retention::classify_within` both exist, and
    // only the second has a caller. The first is kept deliberately -- its own
    // doc says it "remains for a caller that genuinely has only a name" -- so
    // it is a name-collision case rather than something to delete.
    "retention::classify",
    // signin::spoken came off 19 Sep 2026: `atlas access` calls it
    // module-qualified, so the scan can see which `spoken` it reached.
    "stale::spoken",
    "stance::kind_of",
    "stance::spoken",
    "subject::confirm",
    // sync::can_open, sync::merge and sync::spoken came off 18 Sep 2026 --
    // `carry_to_your_other_devices` calls all three module-qualified.
    "system::describe",
    "thread::now_secs",
    "tts::interpret",
    "tune::summary",
    "undo::understand",
    "wanted::decide",
];

#[test]
fn the_list_is_sorted_and_free_of_duplicates() {
    let mut sorted = AMBIGUOUS.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        sorted.as_slice(),
        AMBIGUOUS,
        "AMBIGUOUS must be sorted and unique, so that a diff of it is readable"
    );
}

#[test]
fn no_function_becomes_ambiguous_without_being_listed() {
    let found = common::ambiguous_free_functions();
    let listed: std::collections::BTreeSet<String> =
        AMBIGUOUS.iter().map(|s| s.to_string()).collect();

    let new: Vec<&String> = found.difference(&listed).collect();
    assert!(
        new.is_empty(),
        "these free functions now share a name with another module's and are \
         reached only by an unqualified call, so the deadness scan cannot tell \
         whether they are alive:\n  {}\n\nEither qualify the call site \
         (`module::name(...)`), rename one of them, or add it here.",
        new.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n  ")
    );

    let gone: Vec<&String> = listed.difference(&found).collect();
    assert!(
        gone.is_empty(),
        "these are listed as ambiguous and no longer are:\n  {}\n\nGood -- \
         delete those lines so the list keeps meaning something.",
        gone.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n  ")
    );
}

#[test]
fn the_scan_still_works() {
    // A scan that silently starts returning nothing turns this whole file
    // into a test that passes for any tree at all -- the failure mode every
    // other measurement here has had at least once.
    let found = common::ambiguous_free_functions();
    assert!(
        found.len() > 20,
        "the ambiguity scan found only {}, so it has stopped working",
        found.len()
    );
}
