//! Choosing between two readings of the same sentence, instead of taking
//! whichever `match` arm came first.
//!
//! # The defect
//!
//! Once an `Intent` carried an argument, the daemon decided what to do with it
//! by asking whether your words *contained* a substring:
//!
//! ```ignore
//! Intent::Mail(what) if what.contains("tax") || what.contains("statement") => { .. }
//! Intent::Mail(what) if what.contains("money") || what.contains("spend")   => { .. }
//! Intent::Mail(what) if what.contains("message") || what.contains("chat")  => { .. }
//! ```
//!
//! Six arms like this, across `Intent::Mail`, `Intent::ReviewPost` and
//! `Intent::Capabilities`. Three problems, in order of how badly they bite:
//!
//! 1. **Order was policy, and was written nowhere.** "How much did I spend on
//!    my trading statement last month" is a money question. It contains
//!    `statement`, so it reached the tax arm. Nothing failed and the answer was
//!    about the wrong thing.
//! 2. **`contains` is a substring search.** `grade` matches "upgrade",
//!    `colour` matches "colourless", `pc` matches "specs".
//! 3. **There was no way to be unsure.** A sentence carrying one word for each
//!    of two readings took the first one, silently. Asking costs a sentence.
//!
//! # The shape
//!
//! Every reading says what belongs to it *and what belongs to one of the
//! others*, all of them are scored against the same sentence independently,
//! and the winner reports how far it won by. Too narrow, and Atlas asks.
//!
//! No model: it runs on the machine with nothing downloaded yet, and it is
//! testable, which a model is not.

use atlas::whichone::{
    self, weigh, which_did_you_mean, Clarity, Reading, WhichOneConfig, ABOUT_A_POST, ABOUT_MAIL,
    WHICH_MACHINE,
};

fn cfg() -> WhichOneConfig {
    WhichOneConfig::default()
}

// ===================== the sentence that went to the wrong arm ==========

#[test]
fn a_money_question_that_says_statement_is_still_a_money_question() {
    // The bug, named. Under the old arms this reached the tax reading because
    // the tax arm was written first.
    let w = weigh(
        "how much did I spend on my trading statement last month",
        ABOUT_MAIL,
        &cfg(),
    );
    assert_eq!(w.settled(), Some("statements"), "{w:?}");
}

#[test]
fn a_tax_question_that_says_statement_is_still_a_tax_question() {
    // And the other direction, so the fix is not just "money always wins".
    let w = weigh("what do the wash sale rules say about my statement", ABOUT_MAIL, &cfg());
    assert_eq!(w.settled(), Some("rules"), "{w:?}");

    let w = weigh("what does HMRC want from me this year", ABOUT_MAIL, &cfg());
    assert_eq!(w.settled(), Some("rules"), "{w:?}");
}

#[test]
fn the_order_the_readings_are_given_in_changes_nothing() {
    // The property the `match` arms could not have. Every reading is scored
    // against the same sentence without seeing the others, so shuffling the
    // set cannot move the answer -- which is the whole difference.
    let said = "how much did I spend on groceries this month";
    let forwards = weigh(said, ABOUT_MAIL, &cfg());

    let mut backwards: Vec<Reading> = ABOUT_MAIL.to_vec();
    backwards.reverse();
    let reversed = weigh(said, &backwards, &cfg());

    assert_eq!(forwards.settled(), reversed.settled());
    assert_eq!(forwards.margin, reversed.margin);
    assert_eq!(forwards.settled(), Some("statements"));
}

// ===================== being unsure is an answer ========================

#[test]
fn one_word_each_is_a_question_rather_than_a_guess() {
    // "Tax" for one reading, "spend" for the other, and nothing else to go on.
    // The old arms took the first. This asks.
    let w = weigh("tax on what I spend", ABOUT_MAIL, &cfg());
    assert_eq!(w.clarity, Clarity::Close, "{w:?}");
    assert_eq!(w.settled(), None, "it acted on a tie");

    let asked = which_did_you_mean(&w, ABOUT_MAIL).expect("a close call should produce a question");
    assert!(asked.starts_with("Did you mean "), "{asked}");
    assert!(asked.ends_with('?'), "{asked}");
    // It names the two that were actually close, not all three -- a question
    // offering every option is one you have to read twice.
    assert_eq!(asked.matches(" or ").count(), 1, "{asked}");
    assert!(!asked.contains("chats"), "it offered a reading that was not in contention: {asked}");
}

#[test]
fn the_question_reads_as_a_sentence_for_every_pair_it_could_offer() {
    // Better than a rule about capitals: build the thing that gets said and
    // read it. Every pair in every set, since any two can end up close.
    for set in [ABOUT_MAIL, ABOUT_A_POST, WHICH_MACHINE] {
        for a in set {
            for b in set {
                if a.id == b.id {
                    continue;
                }
                let w = whichone::Weighed {
                    clarity: Clarity::Close,
                    best: Some(a.id),
                    behind: Some(b.id),
                    margin: 0.0,
                };
                let q = which_did_you_mean(&w, set).expect("a close call asks");
                assert_eq!(q, format!("Did you mean {} or {}?", a.plain, b.plain));
                assert!(!q.contains("  "), "doubled space: {q}");
                assert!(!q.contains(".?") && !q.contains("??"), "doubled punctuation: {q}");
            }
        }
    }
}

#[test]
fn a_clear_winner_is_not_turned_into_a_question() {
    // The failure mode of the fix: asking about everything is its own kind of
    // useless.
    for said in [
        "how much did I spend last month",
        "any messages on telegram",
        "what do the tax rules say",
    ] {
        let w = weigh(said, ABOUT_MAIL, &cfg());
        assert_eq!(w.clarity, Clarity::Clear, "{said} -> {w:?}");
        assert!(which_did_you_mean(&w, ABOUT_MAIL).is_none(), "{said}");
    }
}

#[test]
fn a_sentence_about_none_of_them_is_not_forced_into_one() {
    let w = weigh("open the workspace please", ABOUT_MAIL, &cfg());
    assert_eq!(w.clarity, Clarity::Nothing, "{w:?}");
    assert_eq!(w.settled(), None);
    assert_eq!(w.margin, 0.0);
    // And it is not phrased as a choice, because there was nothing to choose
    // between.
    assert!(which_did_you_mean(&w, ABOUT_MAIL).is_none());
}

#[test]
fn the_threshold_is_read_rather_than_a_number_that_happens_to_match() {
    let said = "tax on what I spend";
    // Shipped: a tie is a question.
    assert_eq!(weigh(said, ABOUT_MAIL, &cfg()).clarity, Clarity::Close);
    // Told to act on anything at all, it acts.
    let eager = WhichOneConfig { act_above_margin: 0.0, ..cfg() };
    assert_eq!(weigh(said, ABOUT_MAIL, &eager).clarity, Clarity::Clear);
    // Told to demand a lot, a normally-clear sentence becomes a question.
    let fussy = WhichOneConfig { act_above_margin: 99.0, ..cfg() };
    assert_eq!(weigh("how much did I spend last month", ABOUT_MAIL, &fussy).clarity, Clarity::Close);
}

// ===================== substring matching, which was the other half =====

#[test]
fn a_word_inside_a_longer_word_is_not_that_word() {
    // `contains("grade")` matched "upgrade". These are whole words.
    let w = weigh("should I upgrade the post", ABOUT_A_POST, &cfg());
    assert_eq!(w.clarity, Clarity::Nothing, "upgrade matched grade: {w:?}");

    let w = weigh("the writing is colourless", ABOUT_A_POST, &cfg());
    assert_eq!(w.clarity, Clarity::Nothing, "colourless matched colour: {w:?}");

    // And the real words still match.
    assert_eq!(weigh("grade this footage", ABOUT_A_POST, &cfg()).settled(), Some("grading"));
}

#[test]
fn punctuation_and_capitals_do_not_change_the_answer() {
    let plain = weigh("how much did I spend last month", ABOUT_MAIL, &cfg());
    let messy = weigh("How much did I SPEND -- last month?!", ABOUT_MAIL, &cfg());
    assert_eq!(plain.settled(), messy.settled());
    assert_eq!(plain.margin, messy.margin);
}

#[test]
fn a_phrase_counts_for_more_than_a_word() {
    // "Strong enough" is evidence about the argument; "strong" on its own is
    // a word people use about colour too. The phrase is worth two because it
    // is two words of agreement, not one.
    let phrase = weigh("is the post strong enough", ABOUT_A_POST, &cfg());
    assert_eq!(phrase.settled(), Some("stance"));
    assert!(phrase.margin >= 2.0, "a two-word phrase scored as one: {phrase:?}");
}

// ===================== the contrast half, which is the point ============

#[test]
fn asking_about_the_colour_of_a_post_that_makes_a_point_gets_the_colour() {
    // Both readings have a word here. The stance reading claims "point" and
    // the grading reading disclaims it, so the colour words win on their own
    // ground rather than on where the arm was written.
    let w = weigh("grade the footage, the point is fine", ABOUT_A_POST, &cfg());
    assert_eq!(w.settled(), Some("grading"), "{w:?}");
}

#[test]
fn no_two_readings_in_a_set_claim_the_same_word_without_disclaiming_it() {
    // The guard that stops this degrading back into what it replaced. A word
    // that means two readings and is disclaimed by neither is a coin toss
    // dressed as a judgment.
    for (name, set) in [
        ("ABOUT_MAIL", ABOUT_MAIL),
        ("ABOUT_A_POST", ABOUT_A_POST),
        ("WHICH_MACHINE", WHICH_MACHINE),
    ] {
        for a in set {
            for b in set {
                if a.id >= b.id {
                    continue;
                }
                for word in a.means {
                    if !b.means.contains(word) {
                        continue;
                    }
                    let disclaimed =
                        a.not.iter().any(|n| b.means.contains(n)) || b.not.contains(word);
                    assert!(
                        disclaimed,
                        "{name}: {} and {} both claim {word:?} and neither says what \
                         tells them apart",
                        a.id, b.id
                    );
                }
            }
        }
    }
}

#[test]
fn every_reading_has_both_halves_and_a_way_to_say_it_out_loud() {
    for (name, set) in [
        ("ABOUT_MAIL", ABOUT_MAIL),
        ("ABOUT_A_POST", ABOUT_A_POST),
        ("WHICH_MACHINE", WHICH_MACHINE),
    ] {
        for r in set {
            assert!(!r.means.is_empty(), "{name}/{}: nothing means it", r.id);
            assert!(!r.not.is_empty(), "{name}/{}: nothing tells it from the others", r.id);
            // `plain` is read out mid-sentence, in "Did you mean X or Y?", so
            // it has to be a phrase rather than an id and it must not carry
            // its own punctuation. Capitals are fine and were wrongly banned
            // by the first version of this test -- "Linux" and "Windows" are
            // proper nouns, and lowercasing them to satisfy a rule about
            // sentence position would have been the test making the product
            // worse.
            assert!(r.plain.len() > 3, "{name}/{}: {:?} is not something to say", r.id, r.plain);
            assert!(
                !r.plain.ends_with('.') && !r.plain.ends_with('?'),
                "{name}/{}: {:?} ends a sentence, and it is used mid-one",
                r.id,
                r.plain
            );
        }
    }
}

// ===================== the machine ladder ===============================

#[test]
fn naming_a_machine_gets_that_machine() {
    for (said, want) in [
        ("can you do this on my macbook", "mac"),
        ("does this work on macos", "mac"),
        ("what about linux", "linux"),
        ("on ubuntu", "linux"),
        ("is there an android version", "android"),
        ("what about my pixel", "android"),
        ("does it run on an ipad", "ios"),
        ("on my iphone", "ios"),
        ("on windows", "windows"),
    ] {
        assert_eq!(weigh(said, WHICH_MACHINE, &cfg()).settled(), Some(want), "{said}");
    }
}

#[test]
fn every_machine_reading_maps_to_a_platform() {
    // The daemon matches these ids by hand. A sixth added here without an arm
    // there would fall into the `_` and be answered about iOS.
    let raw = crate::common::source_of("daemon");
    let arm = raw
        .split("let p = match chose {")
        .nth(1)
        .and_then(|r| r.split_once("};"))
        .map(|(inside, _)| inside)
        .expect("the platform arm");
    for r in WHICH_MACHINE {
        // `ios` is the fallthrough and is named in the comment rather than as
        // an arm, so it is checked by being the documented default.
        if r.id == "ios" {
            assert!(arm.contains("Platform::Ios"), "nothing falls through to iOS");
            continue;
        }
        assert!(
            arm.contains(&format!("Some(\"{}\")", r.id)),
            "{} is a machine you can ask about and the daemon has no arm for it",
            r.id
        );
    }
}

// ===================== reached from the daemon ==========================

#[test]
fn the_daemon_is_what_reaches_it_rather_than_this_test() {
    let raw = crate::common::source_of("daemon");
    let code: String = raw
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");

    assert!(code.contains("crate::whichone::ABOUT_MAIL"), "the mail arms still order themselves");
    assert!(code.contains("crate::whichone::ABOUT_A_POST"), "the post arms still order themselves");
    assert!(code.contains("crate::whichone::WHICH_MACHINE"), "the machine ladder is still an if/else");
    assert!(
        code.contains("crate::whichone::which_did_you_mean"),
        "nothing asks when it cannot tell"
    );

    // The substring guards these replaced, gone rather than left beside them.
    for stale in [
        "what.contains(\"statement\")",
        "what.contains(\"money\")",
        "what.contains(\"chat\")",
        "what.contains(\"colour\")",
        "what.contains(\"strong enough\")",
    ] {
        assert!(!code.contains(stale), "{stale} is still deciding something");
    }

    let yaml = std::fs::read_to_string("config/tools.yaml").expect("tools.yaml");
    assert!(yaml.contains("act_above_margin:"), "there is nowhere to set the threshold");
    assert!(yaml.contains("matched_at_all:"));
}

#[test]
fn the_settings_are_the_ones_read() {
    // Shipped values stated here so a change to either has to come through a
    // test that says what it means.
    let c = WhichOneConfig::default();
    assert_eq!(c.act_above_margin, 1.0);
    assert_eq!(c.matched_at_all, 1.0);
    assert!(!whichone::ABOUT_MAIL.is_empty());
}
