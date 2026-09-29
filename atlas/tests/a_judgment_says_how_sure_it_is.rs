//! Graded judgments, and the rule that a judgment says how sure it is.
//!
//! # What this replaced
//!
//! `hollow::spoken` did this:
//!
//! ```ignore
//! let worst: Vec<String> = found.iter().take(3).map(..).collect();
//! ```
//!
//! `worst` was a variable name. They were the first three *questions* that
//! came back hollow, in the order `SELF_QUESTIONS` happens to be written --
//! so on a run where the badly broken thing was asked about fifth, Atlas
//! reported three mild findings and a count. The name said "worst", the code
//! said "first", and nothing was wrong enough to fail.
//!
//! The same defect as the daemon's ordered `contains` arms, in a different
//! module, found the same way: **order posing as judgment.**
//!
//! # The two rules this file defends
//!
//! 1. **Not knowing is not a middle band.** A score under every band comes
//!    back `Nothing`, not the lowest one. Collapsing those is how `readings`
//!    returned all zeros and looked like a healthy machine -- the bug
//!    `hollow.rs` itself was written about.
//! 2. **A judgment on an edge is not acted on.** `settled()` is `None` there,
//!    and a severity Atlas is unsure of is worse than no severity, because it
//!    gets acted on.

use atlas::hollow::{Hollow, Why};
use atlas::judgment::{
    self, how_strongly, which_band, add_up, Band, JudgmentConfig, Signal, HOW_BAD,
};
use atlas::whichone::Clarity;

fn cfg() -> JudgmentConfig {
    JudgmentConfig::default()
}


fn hollow(what: &str, why: Why, answer: &str) -> Hollow {
    Hollow { what: what.into(), why, answer: answer.into() }
}

const BANDS: &[Band] = &[
    Band { id: "low", plain: "hardly anything", from: 1.0 },
    Band { id: "middling", plain: "worth a look", from: 3.0 },
    Band { id: "high", plain: "deal with this", from: 6.0 },
];

// ===================== not knowing is its own answer ==================

#[test]
fn a_score_under_every_band_is_off_the_scale_rather_than_the_bottom_of_it() {
    // The `readings`-returns-zeros bug, in one assertion. Something that
    // scored under every band is not in the lowest band; it is unmeasured,
    // and that is a different sentence.
    let g = which_band(0.0, BANDS, &cfg());
    assert_eq!(g.clarity, Clarity::Nothing);
    assert_eq!(g.settled(), None);
    assert_eq!(g.band, "");
}

#[test]
fn a_condition_with_no_evidence_is_unmeasured_rather_than_unlikely() {
    let h = how_strongly(&[], &[], &cfg());
    assert_eq!(h.clarity, Clarity::Nothing);
    assert_eq!(h.settled(), None);
    // Not 0.0-as-a-probability: `Nothing` is what carries the meaning, and
    // reading `likely` without checking `clarity` is the mistake this pairing
    // exists to make visible.
    assert!(h.because.is_empty());
}

#[test]
fn balanced_evidence_is_said_to_be_balanced_rather_than_half_true() {
    // 0.5 means the evidence is balanced, not that the condition half-holds.
    let h = how_strongly(&[Signal::of("for", 1.0)], &[Signal::of("against", 1.0)], &cfg());
    assert_eq!(h.likely, 0.5);
    assert_eq!(h.clarity, Clarity::Close, "a coin toss was reported as an answer");
    assert_eq!(h.settled(), None);
}

// ===================== edges ==========================================

#[test]
fn a_score_sitting_on_a_bands_floor_is_in_it_and_not_clearly() {
    let just_in = which_band(3.0, BANDS, &cfg());
    assert_eq!(just_in.band, "middling");
    assert_eq!(just_in.clarity, Clarity::Close, "barely in was reported as settled");
    assert_eq!(just_in.settled(), None);

    let well_in = which_band(4.5, BANDS, &cfg());
    assert_eq!(well_in.band, "middling");
    assert_eq!(well_in.clarity, Clarity::Clear);
    assert_eq!(well_in.settled(), Some("middling"));
}

#[test]
fn a_score_just_short_of_the_next_band_is_also_an_edge() {
    // Only checking the floor would call this settled, and it is exactly as
    // uncertain as one just past the floor below it.
    let nearly_high = which_band(5.75, BANDS, &cfg());
    assert_eq!(nearly_high.band, "middling");
    assert_eq!(nearly_high.clarity, Clarity::Close, "{nearly_high:?}");
}

#[test]
fn the_top_band_has_no_upper_edge_to_be_near() {
    let g = which_band(99.0, BANDS, &cfg());
    assert_eq!(g.band, "high");
    assert_eq!(g.clarity, Clarity::Clear);
}

#[test]
fn how_near_the_edge_counts_as_is_a_setting_rather_than_a_number_that_fits() {
    let on_the_line = 3.2;
    assert_eq!(which_band(on_the_line, BANDS, &cfg()).clarity, Clarity::Close);
    let relaxed = JudgmentConfig { past_the_edge: 0.1, ..cfg() };
    assert_eq!(which_band(on_the_line, BANDS, &relaxed).clarity, Clarity::Clear);
    let strict = JudgmentConfig { past_the_edge: 10.0, ..cfg() };
    assert_eq!(which_band(7.0, BANDS, &strict).clarity, Clarity::Close);
}

#[test]
fn the_bands_may_be_given_in_any_order() {
    // Depending on the caller to list them low-to-high is the kind of
    // unwritten ordering this module exists to remove.
    let shuffled: Vec<Band> = vec![BANDS[2], BANDS[0], BANDS[1]];
    assert_eq!(which_band(4.5, &shuffled, &cfg()), which_band(4.5, BANDS, &cfg()));
}

// ===================== evidence stays a list ==========================

#[test]
fn what_counted_is_kept_rather_than_becoming_a_number_early() {
    // A total with no account of what went into it cannot be argued with,
    // and the reason to grade something instead of thresholding it is so a
    // person can see why.
    let for_it = [Signal::of("said so itself", 1.0), Signal::of("nothing to judge", 0.5)];
    assert_eq!(add_up(&for_it), 1.5);
    let h = how_strongly(&for_it, &[], &cfg());
    // Biggest first, so the reason a person reads is the one that decided it.
    assert_eq!(h.because, vec!["said so itself", "nothing to judge"]);
    assert_eq!(h.likely, 1.0);
    assert_eq!(h.settled(), Some(true));
}

#[test]
fn evidence_against_is_counted_rather_than_ignored() {
    let h = how_strongly(
        &[Signal::of("a", 3.0)],
        &[Signal::of("b", 1.0)],
        &cfg(),
    );
    assert_eq!(h.likely, 0.75);
    assert_eq!(h.settled(), Some(true));
    // And enough against turns it round.
    let other = how_strongly(&[Signal::of("a", 1.0)], &[Signal::of("b", 3.0)], &cfg());
    assert_eq!(other.settled(), Some(false));
}

// ===================== the ranking it was built for ===================

#[test]
fn claiming_to_be_fine_while_empty_outranks_admitting_it_is_not_built() {
    // The two ends of `Why`. One is honest about being unfinished; the other
    // is the failure `hollow.rs` was written about, and it is worse because
    // nothing downstream can tell.
    let honest = add_up(&judgment::severity_of(&hollow("q", Why::SaysSoItself, "not wired up")));
    let worst = add_up(&judgment::severity_of(&hollow("q", Why::ZeroDressedAsFine, "all fine")));
    assert!(worst > honest, "{worst} vs {honest}");

    assert_eq!(which_band(worst, HOW_BAD, &cfg()).band, "claims-it-works");
    assert_eq!(which_band(honest, HOW_BAD, &cfg()).band, "noted");
}

#[test]
fn an_answer_with_no_text_is_worse_than_the_same_answer_with_some() {
    // A person can catch a wrong answer they can read. They cannot catch an
    // empty one.
    let silent = add_up(&judgment::severity_of(&hollow("q", Why::Silent, "")));
    let spoke = add_up(&judgment::severity_of(&hollow("q", Why::Silent, "something")));
    assert!(silent > spoke, "{silent} vs {spoke}");
}

#[test]
fn worst_first_actually_puts_the_worst_first() {
    // The bug, named. In question order this list reads mild, mild, awful.
    let found = vec![
        hollow("what's outstanding", Why::SaysSoItself, "not wired to an action yet"),
        hollow("what's queued", Why::SaysSoItself, "not wired to an action yet"),
        hollow("how's the machine", Why::ZeroDressedAsFine, "all fine"),
    ];
    let ranked = judgment::worst_first(&found, &cfg());
    assert_eq!(ranked[0].0.what, "how's the machine", "the worst one was still third");
    assert_eq!(ranked[0].1.band, "claims-it-works");

    let said = atlas::hollow::spoken(&found, &cfg());
    let awful = said.find("machine").expect("the worst one isn't mentioned");
    let mild = said.find("outstanding").expect("the mild one isn't mentioned");
    assert!(awful < mild, "still in question order: {said}");
    assert!(said.contains("worst first"), "{said}");
}

#[test]
fn findings_it_cannot_tell_apart_keep_the_order_they_came_in() {
    // A false ranking is worse than none. Two findings on the same score are
    // not reshuffled to look decided.
    let found = vec![
        hollow("first", Why::SaysSoItself, "not wired up"),
        hollow("second", Why::SaysSoItself, "not wired up"),
        hollow("third", Why::SaysSoItself, "not wired up"),
    ];
    let ranked = judgment::worst_first(&found, &cfg());
    let order: Vec<&str> = ranked.iter().map(|(h, _)| h.what.as_str()).collect();
    assert_eq!(order, vec!["first", "second", "third"]);
}

#[test]
fn a_severity_it_is_unsure_of_is_left_off_rather_than_said_anyway() {
    // On an edge Atlas says what it found and not how bad. A severity it is
    // not sure of is worse than none, because it gets acted on.
    let on_an_edge = hollow("q", Why::Silent, "something");
    let g = which_band(add_up(&judgment::severity_of(&on_an_edge)), HOW_BAD, &cfg());
    assert_eq!(g.clarity, Clarity::Close, "the fixture isn't on an edge any more: {g:?}");

    let said = atlas::hollow::spoken(&[on_an_edge], &cfg());
    assert!(said.contains("answered with nothing at all"), "{said}");
    assert!(!said.contains("worst kind"), "it stated a severity it wasn't sure of: {said}");
}

// ===================== the pass that happens without you =============

#[test]
fn the_nightly_pass_runs_the_bug_detector_too() {
    // It did not. `hollow_answers()` was reached only when you asked -- so
    // the one sweep nobody triggers was the one that skipped the detector,
    // which is `hollow.rs`'s own failure one level up.
    let daemon = crate::common::source_of("daemon");
    let code: String = daemon
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        code.contains("crate::judgment::worth_raising("),
        "the unprompted pass doesn't weigh what it found"
    );
    // The needle is built at runtime rather than written as a literal.
    //
    // Spelling the function's name here makes `called_names` -- which the
    // deadness scans use, and which reads bare names out of test files --
    // count this assertion as a *test* of that function. It did, immediately:
    // `daemon::hollow_answers` moved out of the helper-untested bucket and
    // the ratchet in `dead_capabilities.rs` dropped by one, because a
    // sentence about the function looked like a call to it.
    //
    // The same self-reference trap `dead_capabilities.rs` documents for its
    // own ceiling constant, and the reason `called_names` skips `//` lines.
    let needle = format!("self.{}_answers()", "hollow");
    assert!(code.matches(&needle).count() >= 2, "the detector still runs only when asked");
}

#[test]
fn a_backlog_of_honest_admissions_is_not_worth_waking_you_for() {
    // Six findings that all say "not wired up" are a to-do list. Read out at
    // night, they are how you learn to ignore the voice.
    let backlog: Vec<Hollow> = (0..6)
        .map(|i| hollow(&format!("q{i}"), Why::SaysSoItself, "not wired to an action yet"))
        .collect();
    let raise = judgment::worth_raising(&backlog, &cfg());
    assert_ne!(raise.settled(), Some(true), "{raise:?}");
}

#[test]
fn one_thing_claiming_to_be_fine_while_empty_is_worth_it() {
    let bad = vec![
        hollow("how's the machine", Why::ZeroDressedAsFine, "all fine"),
        hollow("what's outstanding", Why::SaysSoItself, "not wired up"),
        hollow("what's queued", Why::SaysSoItself, "not wired up"),
    ];
    assert_eq!(judgment::worth_raising(&bad, &cfg()).settled(), Some(true));
}

#[test]
fn nothing_found_is_nothing_to_raise_rather_than_a_quiet_no() {
    // The distinction the whole module turns on, at the place it matters
    // most: an empty audit is unmeasured, not "probably fine".
    let raise = judgment::worth_raising(&[], &cfg());
    assert_eq!(raise.clarity, Clarity::Nothing);
    assert_eq!(raise.settled(), None);
}

#[test]
fn it_stays_quiet_when_it_cannot_tell() {
    // The right way round for something that speaks while you did not ask.
    // `settled()` is None on a close call, and the daemon acts only on
    // `Some(true)` -- so unsure means silent rather than unsure means speak.
    let daemon = crate::common::source_of("daemon");
    assert!(
        daemon.contains("if raise.settled() == Some(true) {"),
        "it speaks on anything other than a clear yes"
    );
}

#[test]
fn nothing_hollow_is_still_said_plainly() {
    assert_eq!(
        atlas::hollow::spoken(&[], &cfg()),
        "Everything I asked myself came back with a real answer."
    );
}

// ===================== one way to say "not sure" ======================

#[test]
fn there_is_one_word_for_being_unsure_rather_than_one_per_module() {
    // `whichone::Clarity` is shared rather than redeclared. Two ways to say
    // "I am not sure" is how one of them goes unused and then wrong -- which
    // is this tree's most repeated lesson, five name collisions in two days.
    let src = std::fs::read_to_string("src/judgment.rs").expect("judgment.rs");
    assert!(src.contains("use crate::whichone::Clarity;"), "it declared its own");
    assert!(!src.contains("pub enum Clarity"), "there are two Claritys now");

    // And the shapes agree, so a caller can treat them the same way.
    let unsure = which_band(3.0, BANDS, &cfg());
    assert_eq!(unsure.clarity, Clarity::Close);
    assert_eq!(how_strongly(&[Signal::of("a", 1.0)], &[Signal::of("b", 1.0)], &cfg()).clarity, Clarity::Close);
}

// ===================== it is reached =================================

#[test]
fn everything_here_has_a_production_caller() {
    // The risk this module is, named. A general mechanism is an excellent
    // place for things to be built and never reached -- this tree has 372
    // functions only tests call, and a substrate with no caller would be the
    // 373rd.
    //
    // Checked against the program rather than against a list, so it cannot
    // be satisfied by editing the list.
    let mut program = String::new();
    for f in [
        "src/daemon.rs",
        "src/hollow.rs",
        "src/main.rs",
        "src/doctor.rs",
        "src/daily.rs",
        "src/reach.rs",
    ] {
        if let Some(t) = crate::common::read_source_path(f) {
            program.push_str(&t);
        }
    }
    let src = std::fs::read_to_string("src/judgment.rs").expect("judgment.rs");
    let called_inside_judgment = src.clone();

    // Read the program for real rather than a list, so this cannot be
    // satisfied by editing the list. `daily.rs` is in it because that is
    // where the numeric half is reached from.
    for name in [
        "which_band",
        "how_strongly",
        "add_up",
        "severity_of",
        "worst_first",
        "how_unusual",
    ] {
        let reached = program.contains(&format!("judgment::{name}("))
            || called_inside_judgment.matches(&format!("{name}(")).count() > 1;
        assert!(reached, "judgment::{name} has no caller");
    }

    // The composite half too, which was held unwired for half a day and is
    // not any more. `reach.rs` is where it landed -- `Post::quality` was a
    // weighted sum with a hand-fitted `* 20.0` in it -- and not the
    // trading verdict, which is deliberately inert.
    for name in ["weighed_together", "only_partly_measured", "ordinary_for"] {
        assert!(
            program.contains(&format!("judgment::{name}(")),
            "judgment::{name} has no caller"
        );
    }

    // And the one that matters most: the ranking reaches the daemon.
    let daemon = crate::common::source_of("daemon");
    assert!(
        daemon.contains("crate::hollow::spoken(&found, &self.tools_cfg().judgment)"),
        "the self-audit still reports in question order"
    );

    let yaml = std::fs::read_to_string("config/tools.yaml").expect("tools.yaml");
    assert!(yaml.contains("past_the_edge:"), "nowhere to set how near an edge counts");
    assert!(yaml.contains("measured_above:"));
}

#[test]
fn every_band_says_what_being_in_it_means() {
    // A band that can only be printed as a number tells a person something
    // scored 2.4, which is not information.
    for b in HOW_BAD {
        assert!(b.plain.len() > 15, "{}: {:?} is not a sentence", b.id, b.plain);
        assert!(!b.plain.contains(char::is_numeric), "{}: it just says the number", b.id);
    }
    // And they are distinct, which is how a table like this rots.
    let mut said: Vec<&str> = HOW_BAD.iter().map(|b| b.plain).collect();
    said.sort();
    let before = said.len();
    said.dedup();
    assert_eq!(said.len(), before, "two bands mean the same thing");
}

// ===================== numbers ========================================
//
// What `which_band` and `how_strongly` could not do. Both take a score the
// caller has already worked out, which is fine when the evidence is words --
// a matched phrase is worth one, a two-word phrase two, and those are
// comparable because they are the same kind of thing. Numbers are not. A £200
// rise and a 0.3-ATR move cannot be added, and the useful question about
// either is not "how big" but "how unusual for this".

use atlas::judgment::{how_unusual, only_partly_measured, weighed_together, Measured};

fn measured(name: &'static str, value: f64, usually: f64, varies_by: f64, seen: usize) -> Measured {
    Measured { name, value, usually, varies_by, seen }
}

#[test]
fn nothing_speaks_below_the_sample_floor() {
    // `judgment::MIN_SAMPLE` is 30 and its reason is written down there: a
    // finding with twelve observations behind it is a story, and a story that
    // came out of a computer is believed harder than one that did not.
    assert_eq!(cfg().min_seen, atlas::judgment::MIN_SAMPLE);

    let thin = measured("spend", 300.0, 150.0, 50.0, 12);
    assert_eq!(how_unusual(&thin, &cfg()), None, "it spoke on twelve observations");

    let enough = measured("spend", 300.0, 150.0, 50.0, 40);
    assert_eq!(how_unusual(&enough, &cfg()), Some(3.0));
}

#[test]
fn not_measured_is_not_the_same_as_perfectly_ordinary() {
    // Zero would read as "bang on average", which is a claim. The honest
    // answer is that nothing was measured -- the distinction the whole module
    // turns on, and the `readings`-returns-zeros bug in numeric form.
    let no_variation = measured("flat", 5.0, 5.0, 0.0, 100);
    assert_eq!(how_unusual(&no_variation, &cfg()), None);

    let not_a_number = measured("broken", f64::NAN, 5.0, 1.0, 100);
    assert_eq!(how_unusual(&not_a_number, &cfg()), None);

    // And something genuinely ordinary does say zero, which is a different
    // fact and is allowed to be one.
    assert_eq!(how_unusual(&measured("ok", 5.0, 5.0, 2.0, 100), &cfg()), Some(0.0));
}

#[test]
fn two_things_on_different_scales_become_comparable() {
    // The whole point. A grocery bill £150 above a £50 spread and an entry
    // 3 ATR beyond its mean are both "about 3 unusual", and only in that form
    // can they be weighed together at all.
    let money = measured("groceries", 300.0, 150.0, 50.0, 60);
    let atr = measured("entry", 1.0415, 1.0400, 0.0005, 60);
    // Compared with a tolerance rather than for equality: 0.0015/0.0005 does
    // not come out as exactly 3 in binary floating point, and a test that
    // demands it would be testing the representation rather than the idea.
    let near_three = |got: Option<f64>| {
        let v = got.expect("measured");
        assert!((v - 3.0).abs() < 1e-9, "{v}");
    };
    near_three(how_unusual(&money, &cfg()));
    near_three(how_unusual(&atr, &cfg()));
}

#[test]
fn a_weight_can_be_changed_without_measuring_anything_again() {
    // The thing you actually want while tuning: the measurements are the
    // expensive part and the weights are the opinion, so they are separate.
    let parts = [
        (measured("a", 4.0, 0.0, 1.0, 60), 1.0),
        (measured("b", 0.0, 0.0, 1.0, 60), 1.0),
    ];
    let (even, _) = weighed_together(&parts, &cfg());
    assert_eq!(even, Some(2.0));

    let leaning = [
        (measured("a", 4.0, 0.0, 1.0, 60), 3.0),
        (measured("b", 0.0, 0.0, 1.0, 60), 1.0),
    ];
    let (tilted, _) = weighed_together(&leaning, &cfg());
    assert_eq!(tilted, Some(3.0));
}

#[test]
fn a_dimension_with_nothing_behind_it_is_named_rather_than_counted_as_ordinary() {
    // Dividing by the weight asked for rather than the weight that counted
    // pulls every partly-measured composite towards zero -- which reads as
    // "ordinary" and is the same bug in a third costume.
    let parts = [
        (measured("solid", 4.0, 0.0, 1.0, 60), 1.0),
        (measured("thin", 4.0, 0.0, 1.0, 3), 1.0),
    ];
    let (score, missing) = weighed_together(&parts, &cfg());
    assert_eq!(score, Some(4.0), "the unmeasured half dragged the score down");
    assert_eq!(missing, vec!["thin"]);

    let said = only_partly_measured(&missing, 2).expect("it should say so");
    assert!(said.contains("thin"), "{said}");
    assert!(said.contains("1 of 2"), "{said}");
    // And nothing to say when everything counted.
    assert_eq!(only_partly_measured(&[], 2), None);
}

#[test]
fn a_composite_with_nothing_measured_at_all_is_not_a_score_of_zero() {
    let parts = [
        (measured("a", 4.0, 0.0, 1.0, 2), 1.0),
        (measured("b", 9.0, 0.0, 1.0, 1), 1.0),
    ];
    let (score, missing) = weighed_together(&parts, &cfg());
    assert_eq!(score, None, "a setup nobody could measure was scored neutral");
    assert_eq!(missing.len(), 2);
}

#[test]
fn your_quiet_hours_are_measured_rather_than_guessed_at() {
    // The caller this was wired to, and it replaced an invented threshold
    // written earlier the same day: `count < (total / 24) / 2`, which had no
    // baseline behind it and no account of how much your hours actually vary.
    use atlas::daily::Rhythm;
    let mut r = Rhythm::default();
    for _ in 0..20 {
        r.note_day();
        for h in 9..=23 {
            for _ in 0..6 {
                r.saw(h, true);
            }
        }
    }
    // Asleep, and awake, in the same person's own units.
    assert!(r.quiet_at(3, &cfg()), "three in the morning didn't read as quiet");
    assert!(!r.quiet_at(14, &cfg()), "two in the afternoon read as quiet");
}

#[test]
fn a_fortnight_is_still_needed_before_it_will_say() {
    // `min_seen` is 30 and `days_watched` is what it counts, so a new install
    // says nothing about your hours -- which lands on "not quiet", the
    // cautious direction, because not-quiet is what stops Atlas starting
    // work.
    use atlas::daily::Rhythm;
    let mut r = Rhythm::default();
    for _ in 0..5 {
        r.note_day();
        for h in 9..=23 {
            for _ in 0..6 {
                r.saw(h, true);
            }
        }
    }
    assert!(!r.quiet_at(3, &cfg()), "it decided you were asleep on five days of watching");
}

#[test]
fn enough_observations_from_too_few_days_is_still_too_few_days() {
    // Two gates, because they are two different failures. A thousand
    // observations from three days is a large sample *of three days*, and it
    // says nothing about a Saturday -- which is what the question is about.
    use atlas::daily::Rhythm;
    let mut r = Rhythm::default();
    for _ in 0..3 {
        r.note_day();
        for h in 9..=23 {
            // Plenty of observations, nowhere near enough of the week.
            for _ in 0..60 {
                r.saw(h, true);
            }
        }
    }
    assert!(
        !r.quiet_at(3, &cfg()),
        "a confident answer about a week nobody watched"
    );
}

// ===================== counts carry noise of their own ================

#[test]
fn a_count_varies_by_about_its_own_square_root_even_when_nothing_changed() {
    use atlas::judgment::ordinary_for_counts;

    // Nine hours with exactly five in each. The observed spread is zero, and
    // without a floor an hour with four in it is *infinitely* unusual --
    // which is arithmetic, not a fact about the person.
    let identical = vec![5.0; 9];
    let (mid, spread) = ordinary_for_counts(&identical).expect("measured");
    assert_eq!(mid, 5.0);
    assert!((spread - 5.0f64.sqrt()).abs() < 1e-9, "no floor: {spread}");

    // Against the unfloored form, which is right for measured quantities and
    // wrong for counts.
    let (_, raw) = atlas::judgment::ordinary_for(&identical).expect("measured");
    assert_eq!(raw, 0.0);
}

#[test]
fn a_real_spread_wider_than_the_noise_is_kept() {
    use atlas::judgment::ordinary_for_counts;
    // The floor is a floor, not a replacement. Genuinely scattered counts
    // keep their own spread.
    let scattered = vec![0.0, 0.0, 20.0, 40.0, 60.0];
    let (_, floored) = ordinary_for_counts(&scattered).expect("measured");
    let (_, raw) = atlas::judgment::ordinary_for(&scattered).expect("measured");
    assert_eq!(floored, raw, "the floor overrode a real spread");
}

#[test]
fn the_two_forms_are_used_for_the_two_kinds_of_thing() {
    // Counts of events -- hours worked, posts made -- take the floored form.
    // Measured quantities -- R multiples, rates, durations -- take the plain
    // one, because there the spread you observe is the spread there is.
    for (file, want) in [
        ("src/person.rs", "ordinary_for_counts("),
        ("src/daily.rs", "ordinary_for_counts("),
    ] {
        let src = std::fs::read_to_string(file).unwrap_or_default();
        assert!(src.contains(want), "{file} counts events and does not floor the spread");
    }
    // `reach` measures rates and fractions across posts, so it does not.
    let reach = std::fs::read_to_string("src/reach.rs").expect("reach.rs");
    assert!(reach.contains("judgment::ordinary_for("), "reach lost its baseline");
}

#[test]
fn calling_someone_late_stops_once_they_have_kept_the_hour() {
    // The behaviour the floor was found by. Four nights running at 2am and
    // Atlas should stop remarking on it -- at four nights you work nights,
    // and a laptop that went on saying so every evening would be wrong as
    // well as annoying.
    //
    // Without the count floor, a settled nine-to-five has no spread at all
    // and every hour outside it is permanently extraordinary, so the drift
    // never happened.
    use atlas::judgment::{how_unusual, ordinary_for_counts, Measured};
    let settled: Vec<f64> = std::iter::repeat(5.0).take(9).collect();
    let (usually, varies_by) = ordinary_for_counts(&settled).expect("measured");

    let after = |nights: f64| {
        how_unusual(
            &Measured { name: "2am", value: nights, usually, varies_by, seen: 45 },
            &cfg(),
        )
        .expect("measured")
    };
    // Third night: still unusual for this person.
    assert!(after(2.0) <= -1.0, "{}", after(2.0));
    // Fourth: no longer.
    assert!(after(3.0) > -1.0, "{}", after(3.0));
}
