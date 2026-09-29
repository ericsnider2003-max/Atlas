use atlas::revise::{home_for, proposal, slug, Correction, Home, Mending, REPEATS_NEEDED};

const DAY: u64 = 86_400;

fn c(about: &str, session: u64, at: u64) -> Correction {
    Correction::new(about, "gave a long answer", at, session).wanting("keep it under five lines")
}

// ================= wait for the repeat =================

#[test]
fn one_correction_is_a_note_not_a_rule() {
    let mut m = Mending::default();
    assert!(
        m.heard(c("too long", 1, 0)).is_none(),
        "a rule built from one irritable evening is worse than no rule"
    );
    assert_eq!(m.heard.len(), 1, "but it is remembered as evidence for the next one");
}

#[test]
fn the_second_time_earns_an_edit() {
    let mut m = Mending::default();
    m.heard(c("too long", 1, 0));
    let e = m.heard(c("too long", 2, DAY)).expect("said twice is a rule");
    assert_eq!(e.said_times, REPEATS_NEEDED);
    assert_eq!(e.becomes, "keep it under five lines");
}

#[test]
fn saying_it_twice_in_one_sitting_is_still_once() {
    let mut m = Mending::default();
    m.heard(c("too long", 1, 0));
    assert!(
        m.heard(c("too long", 1, 60)).is_none(),
        "restating a complaint in the same breath is emphasis, not a second occasion"
    );
}

#[test]
fn the_same_complaint_worded_differently_still_counts_as_the_same_one() {
    assert_eq!(slug("too long"), slug("that was way too long"));
    let mut m = Mending::default();
    m.heard(c("too long", 1, 0));
    assert!(m.heard(c("that was way too long", 2, DAY)).is_some());
}

#[test]
fn two_different_complaints_do_not_add_up_to_a_rule() {
    let mut m = Mending::default();
    m.heard(c("too long", 1, 0));
    assert!(m.heard(c("wrong tone", 2, DAY)).is_none());
}

// ================= what right looks like =================

#[test]
fn a_complaint_with_no_fix_in_it_is_never_written_down() {
    let mut m = Mending::default();
    let bare = |s| Correction::new("too long", "gave a long answer", 0, s);
    m.heard(bare(1));
    assert!(
        m.heard(bare(2)).is_none(),
        "'that's wrong' is a reason to ask, not a lesson to file"
    );
}

#[test]
fn an_edit_records_what_right_looks_like_rather_than_displeasure() {
    let mut m = Mending::default();
    m.heard(c("too long", 1, 0));
    let e = m.heard(c("too long", 2, DAY)).unwrap();
    let lower = e.becomes.to_lowercase();
    for word in ["didn't like", "annoyed", "unhappy", "bad"] {
        assert!(!lower.contains(word), "the edit records a mood, which teaches nothing");
    }
    assert!(e.becomes.contains("five lines"));
}

// ================= where it gets written =================

#[test]
fn a_lesson_about_how_a_job_is_done_goes_where_it_is_read_every_time() {
    let c = Correction::new("order", "ran the tests last", 0, 1)
        .wanting("always run the tests first");
    assert_eq!(home_for(&c), Home::HowTo);
    assert!(
        Home::HowTo.read_every_time(),
        "the instructions are read every run; the diary may never be read again"
    );
}

#[test]
fn a_lesson_about_taste_goes_where_taste_is_kept() {
    let c = Correction::new("tone", "sounded like a brochure", 0, 1)
        .wanting("I prefer it blunt");
    assert_eq!(home_for(&c), Home::AboutYou);
    assert!(!Home::AboutYou.read_every_time());
}

#[test]
fn anything_else_lands_in_the_record() {
    let c = Correction::new("name", "called it Atlas Two", 0, 1).wanting("it is called Homelab");
    assert_eq!(home_for(&c), Home::Record);
}

// ================= show the work =================

#[test]
fn atlas_names_the_change_before_making_it() {
    let mut m = Mending::default();
    m.heard(c("too long", 1, 0));
    let e = m.heard(c("too long", 2, DAY)).unwrap();
    let p = proposal(&e);
    assert!(p.contains("keep it under five lines"), "{p}");
    assert!(p.contains("2 times"), "{p}");
    assert!(p.ends_with("Alright?"), "an edit you cannot refuse is not a proposal: {p}");
}

#[test]
fn a_replacement_shows_the_old_line_as_well_as_the_new_one() {
    let mut m = Mending::default();
    m.heard(c("too long", 1, 0));
    let mut e = m.heard(c("too long", 2, DAY)).unwrap();
    e.replacing = Some("answer thoroughly".into());
    let p = proposal(&e);
    assert!(p.contains("answer thoroughly"), "surgical edits name what they replace: {p}");
    assert!(p.contains("keep it under five lines"));
}

#[test]
fn mending_reaches_you_as_an_offer_rather_than_a_silent_rewrite() {
    let mut m = Mending::default();
    m.heard(c("too long", 1, 0));
    let e = m.heard(c("too long", 2, DAY)).unwrap();
    let n = atlas::nudge::offer_to_mend(&e);
    assert!(n.message.ends_with("Alright?"));
    assert!(n.confidence < 1.0, "rewriting its own instructions always asks");
}

// ================= the only metric =================

#[test]
fn the_scoreboard_is_whether_it_makes_the_same_mistake_twice() {
    let mut m = Mending::default();
    m.heard(c("too long", 1, 0));
    let e = m.heard(c("too long", 2, DAY)).unwrap();
    m.applied(e);
    assert_eq!(m.repeat_rate(), 0.0, "written down and not repeated is a clean score");

    // Told, written down, did it again.
    m.heard(c("too long", 3, 2 * DAY));
    assert_eq!(m.repeats_after_fix, 1);
    assert_eq!(m.repeat_rate(), 1.0);
}

#[test]
fn a_fixed_thing_does_not_earn_a_second_edit_for_the_same_complaint() {
    let mut m = Mending::default();
    m.heard(c("too long", 1, 0));
    let e = m.heard(c("too long", 2, DAY)).unwrap();
    m.applied(e);
    assert!(
        m.heard(c("too long", 3, 2 * DAY)).is_none(),
        "the rule already exists — writing it twice is not the fix"
    );
}

#[test]
fn correcting_an_applied_rule_with_something_different_offers_to_replace_it() {
    // `Edit::replacing` was always `None`: once a rule was applied, saying
    // the *same* thing again only ever counted as a repeated mistake and
    // produced no new edit (see `a_fixed_thing_does_not_earn_a_second_edit`,
    // above) -- correct, since there's nothing to change. But saying
    // something *different* about the same subject was handled identically,
    // silently discarding the new instruction forever. That's the gap this
    // closes: a genuinely different correction on an already-applied subject
    // now earns a new edit that names the exact line it replaces.
    let mut m = Mending::default();
    m.heard(c("too long", 1, 0));
    let first = m.heard(c("too long", 2, DAY)).unwrap();
    assert_eq!(first.replacing, None, "the first time through, there is nothing to replace yet");
    m.applied(first);

    let different = Correction::new("too long", "gave a long answer", 3 * DAY, 3)
        .wanting("three sentences, not five lines");
    let second = m.heard(different).expect("a different instruction on a fixed subject is an edit");
    assert_eq!(second.replacing, Some("keep it under five lines".into()));
    assert_eq!(second.becomes, "three sentences, not five lines");

    let p = proposal(&second);
    assert!(p.contains("keep it under five lines"), "names the old line: {p}");
    assert!(p.contains("three sentences, not five lines"), "and the new one: {p}");
}

#[test]
fn repeating_the_identical_instruction_after_it_was_applied_offers_nothing_new() {
    // The counterpart to the test above: restating the same fix, word for
    // word, after it's already been written down is the mistake recurring,
    // not a new instruction -- it must not manufacture an edit that just
    // replaces a line with an identical copy of itself.
    let mut m = Mending::default();
    m.heard(c("too long", 1, 0));
    let first = m.heard(c("too long", 2, DAY)).unwrap();
    m.applied(first);

    assert!(
        m.heard(c("too long", 3, 2 * DAY)).is_none(),
        "the exact same wanted text again names nothing to replace"
    );
}

#[test]
fn a_system_that_has_never_written_anything_scores_zero_rather_than_perfect() {
    let m = Mending::default();
    assert_eq!(m.repeat_rate(), 0.0);
    assert!(m.applied.is_empty(), "zero out of zero is not a good score, it is no score");
}

// ================= housekeeping =================

#[test]
fn notes_that_never_became_rules_are_surfaced_not_deleted() {
    let mut m = Mending::default();
    m.heard(c("one off", 1, 0));
    let stale = m.stale_notes(90 * DAY, 30 * DAY);
    assert_eq!(stale.len(), 1);
    assert_eq!(m.heard.len(), 1, "a complaint that keeps not recurring is itself information");
}

#[test]
fn a_note_that_became_a_rule_is_not_also_reported_as_stale() {
    let mut m = Mending::default();
    m.heard(c("too long", 1, 0));
    let e = m.heard(c("too long", 2, DAY)).unwrap();
    m.applied(e);
    assert!(m.stale_notes(90 * DAY, 30 * DAY).is_empty());
}
