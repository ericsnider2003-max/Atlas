//! Atlas hedging things it read out of its own records.
//!
//! `certainty.rs` scores an answer for the shapes a model makes when it is
//! filling a gap: stacked hedging, invented figures, and claims about your
//! machine made without looking. To tell an invented claim from a read one it
//! needs to be told which it is — that is what `Grounding` is for, and the
//! module's own tests set all three flags and prove each one works.
//!
//! Production had exactly one caller, and it passed `Grounding::default()`:
//! no source, no context, nothing looked at. Every answer, every time.
//!
//! So an answer Atlas read straight out of your notes was scored as if it had
//! made it up — and then penalised a second time by the rule about claiming
//! things about your machine, because a grounded answer about your machine
//! naturally contains the word "your". The worst case was `Intent::Why`,
//! which reads back decisions Atlas recorded itself: the most grounded thing
//! it can possibly say, scored as invention and delivered as "I don't know".

use atlas::certainty::{assess, phrase, CertaintyConfig, Confidence, Grounding};

/// The sentence at the heart of it: true, read from Atlas's own record, and
/// about your machine — so it trips the invention rule unless something says
/// it was read rather than guessed.
const FROM_THE_RECORD: &str = "Your workspace is set to two monitors because you asked for it.";

#[test]
fn an_answer_read_from_its_own_record_was_being_withheld() {
    // What the single production call site did, spelled out.
    let (level, _, _) = assess(FROM_THE_RECORD, &Grounding::default(), &CertaintyConfig::default());
    assert_eq!(
        level,
        Confidence::Withhold,
        "this test exists because this was the old behaviour; if it no longer \
         reproduces, the scoring changed and the rest of this file needs rereading"
    );

    // And what it turned into out loud.
    let spoken = phrase(FROM_THE_RECORD, level, "it made a claim about your machine without checking");
    assert!(spoken.starts_with("I don't know"), "{spoken}");
}

#[test]
fn told_the_truth_it_simply_answers() {
    let (level, _, why) =
        assess(FROM_THE_RECORD, &Grounding::from_what_it_holds(), &CertaintyConfig::default());
    assert_eq!(level, Confidence::Fine, "still doubting its own record: {why}");
    assert_eq!(
        phrase(FROM_THE_RECORD, level, &why),
        FROM_THE_RECORD,
        "a grounded answer should come back untouched"
    );
}

#[test]
fn a_write_up_with_nothing_behind_it_is_still_caught() {
    // The fix must not become a way to wave anything through. `from_sources`
    // grades a research note by what it actually read, so a note built on
    // nothing keeps the old treatment.
    let thin = "I think the figure is probably around 1234.56, though it could be \
                higher — as far as I know it might be lower.";

    let (with_nothing, _, _) =
        assess(thin, &Grounding::from_sources(0), &CertaintyConfig::default());
    let (with_sources, _, _) =
        assess(thin, &Grounding::from_sources(4), &CertaintyConfig::default());

    assert!(
        with_nothing.severity_is_at_least_qualify(),
        "a note with no sources sailed through: {with_nothing:?}"
    );
    assert!(
        (with_sources as u8) <= (with_nothing as u8),
        "reading sources should never make Atlas less sure"
    );
}

/// Small helper so the assertion above reads as the claim it makes.
trait AtLeastQualify {
    fn severity_is_at_least_qualify(&self) -> bool;
}
impl AtLeastQualify for Confidence {
    fn severity_is_at_least_qualify(&self) -> bool {
        matches!(self, Confidence::Qualify | Confidence::Withhold)
    }
}

#[test]
fn the_three_groundings_are_actually_different() {
    // Guards against the constructors quietly collapsing into one another in
    // a later edit, which would make the call sites decorative.
    let held = Grounding::from_what_it_holds();
    // `default()` is the ungrounded case and stays spelled that way -- a
    // named constructor for it would have been a third `pub fn` with no
    // caller, which is what the deadness guard caught when I added one.
    let alone = Grounding::default();
    assert!(held.from_a_source && held.had_the_context);
    assert!(!alone.from_a_source && !alone.had_the_context);
    assert!(Grounding::from_sources(1).from_a_source);
    assert!(!Grounding::from_sources(0).from_a_source);
}
