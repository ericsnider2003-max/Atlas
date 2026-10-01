use atlas::reference::{
    for_trading, is_stale, needs_the_shelf, nothing_found, quoted, stale_warning, trading_mb,
    worth_having, Found, ReferenceConfig, Sort, WHAT_IT_COSTS, WHY_NOT_A_BIGGER_MODEL,
};

#[test]
fn the_specific_knowledge_is_the_small_part() {
    // The thing that surprises people, and the reason this beats a bigger
    // model.
    assert!(trading_mb() < 100, "all the reference is {}MB", trading_mb());
    let encyclopaedia = worth_having()
        .into_iter()
        .find(|s| s.kind == Sort::Encyclopaedia)
        .unwrap();
    assert!(encyclopaedia.mb > trading_mb() * 40, "the general knowledge is what costs");
    assert!(WHAT_IT_COSTS.contains("least useful part"));
}

#[test]
fn a_model_recalls_something_shaped_like_the_answer_which_is_worse_than_nothing() {
    assert!(WHY_NOT_A_BIGGER_MODEL.contains("confidently, which is worse than not knowing"));
    assert!(WHY_NOT_A_BIGGER_MODEL.contains("The model reasons; the shelf remembers"));
}

#[test]
fn anything_with_a_number_in_it_comes_off_the_shelf() {
    for q in [
        "how much margin per contract",
        "what's the tick value",
        "when is the mark-to-market deadline",
        "what is the wash sale rule",
    ] {
        assert!(needs_the_shelf(q), "{q}");
    }
}

#[test]
fn judgement_goes_to_the_model_instead() {
    for q in [
        "should I take this trade",
        "is this a good idea",
        "help me think through the risk",
    ] {
        assert!(!needs_the_shelf(q), "{q}");
    }
}

#[test]
fn numbers_never_come_from_the_model_alone_and_that_is_not_configurable() {
    // The whole point of having a shelf.
    //
    // This asserted `numbers_come_from_the_shelf`, a `#[serde(skip)]` bool
    // pinned true that nothing read. Deleted 19 Sep 2026: what makes it true
    // is where the question goes. `Intent::Ask(q) if needs_the_shelf(q)` is
    // matched *above* the arm that answers from the model, and it returns
    // `nothing_found` rather than falling through -- so a numeric question
    // cannot reach the model by any configuration.
    let d = crate::common::source_of("daemon");
    let shelf_arm = d
        .find("Intent::Ask(q) if crate::reference::needs_the_shelf(q)")
        .expect("the shelf no longer catches numeric questions");
    let model_arm = d[shelf_arm..]
        .find("Intent::Ask(q) =>")
        .expect("the catch-all Ask arm is gone");
    assert!(model_arm > 0, "the shelf arm has to come first or it never matches");
    // The arm names its handler (`execute_inner` as a table, 30 Sep 2026).
    let arm = &d[shelf_arm..shelf_arm + model_arm];
    let handler = match arm.find("self.on_") {
        Some(i) => {
            let name: String = arm[i + 5..].chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            let at = d.find(&format!("fn {name}(")).expect("the shelf's handler is gone");
            let end = d[at..].find("\n    }\n").map(|e| at + e).unwrap_or(d.len());
            d[at..end].to_string()
        }
        None => arm.to_string(),
    };
    assert!(
        handler.contains("nothing_found"),
        "the shelf arm no longer refuses -- it would fall through to the model"
    );

    // And an old config naming the removed key still loads.
    let parsed: ReferenceConfig =
        serde_yaml::from_str("enabled: true\nnumbers_come_from_the_shelf: false\n").unwrap();
    assert!(parsed.enabled, "an unknown key must not stop the section parsing");
}

#[test]
fn some_reference_material_goes_wrong_when_stale_and_some_just_gets_old() {
    // The distinction that decides how loudly to say something.
    assert!(Sort::Rules.goes_wrong_when_stale());
    assert!(Sort::Specs.goes_wrong_when_stale());
    assert!(!Sort::Encyclopaedia.goes_wrong_when_stale());
}

#[test]
fn stale_tax_material_is_flagged_differently_from_a_stale_encyclopaedia() {
    let tax = worth_having().into_iter().find(|s| s.name.contains("tax")).unwrap();
    let enc = worth_having().into_iter().find(|s| s.kind == Sort::Encyclopaedia).unwrap();

    let tax_said = stale_warning(&tax, 400).unwrap();
    assert!(tax_said.contains("goes wrong rather than just old"));
    assert!(tax_said.contains("Worth checking"));

    let enc_said = stale_warning(&enc, 800).unwrap();
    assert!(enc_said.contains("won't have changed much"));
}

#[test]
fn fresh_material_gets_no_warning() {
    let tax = worth_having().into_iter().find(|s| s.name.contains("tax")).unwrap();
    assert!(!is_stale(&tax, 30));
    assert!(stale_warning(&tax, 30).is_none());
}

#[test]
fn crypto_goes_stale_fastest_because_it_moves_fastest() {
    let crypto = worth_having().into_iter().find(|s| s.name.contains("crypto")).unwrap();
    let options = worth_having().into_iter().find(|s| s.name.contains("options")).unwrap();
    assert!(crypto.stale_after_days < options.stale_after_days);
}

#[test]
fn an_answer_off_the_shelf_always_carries_where_it_came_from() {
    // The whole reason for having this rather than trusting the model is that
    // the answer is traceable.
    let f = Found {
        text: "the tick is 0.25 and worth $12.50".into(),
        from: "futures contract specs".into(),
        as_of: "March".into(),
        confidence: 0.9,
    };
    let said = quoted(&f);
    assert!(said.contains("futures contract specs"));
    assert!(said.contains("as of March"));
}

#[test]
fn the_shelf_coming_up_empty_is_said_rather_than_quietly_falling_through() {
    // A model asked a specific question it doesn't know answers anyway.
    //
    // This asserted "it names the nearest thing it does have", and on
    // 19 Sep 2026 that turned out to be the bug rather than the feature:
    // every shelf in `worth_having` ships with `as_of` empty, because nothing
    // in this tree fetches reference material. So the nearest thing was one
    // Atlas does not have, and offering to reason from it was a grounded
    // answer that was not grounded -- which is worse than an ungrounded one,
    // because you stop checking it.
    let said = nothing_found("the tick size on cocoa", &for_trading(), true, 0);
    assert!(said.contains("tick"), "it names the nearest thing it would have");
    assert!(said.contains("haven't got one"), "{said}");
    assert!(
        !said.contains("I could reason from that"),
        "it offered to reason from a shelf it hasn't got: {said}"
    );
    assert!(said.contains("would be invented"));

    // A shelf that has actually been fetched reads the way it always did.
    let mut held = for_trading()[0].clone();
    held.as_of = "2026-03-01".into();
    let about = held.covers.clone();
    let on_the_shelf = nothing_found(&about, &[held], false, 0);
    assert!(on_the_shelf.contains("I could reason from that"), "{on_the_shelf}");
}

#[test]
fn with_nothing_close_at_all_it_says_the_number_would_be_invented() {
    // Nothing on any shelf touches this.
    let said = nothing_found("zzqx wobbleflange", &for_trading(), true, 0);
    assert!(said.contains("would be invented"), "got: {said}");
}

#[test]
fn every_shelf_says_where_it_came_from_so_a_wrong_answer_can_be_traced() {
    for s in worth_having() {
        assert!(!s.from.is_empty(), "{} doesn't say where it came from", s.name);
        assert!(s.stale_after_days > 0);
    }
}

// ================= keeping what it finds =================

use atlas::reference::{correct_it, gone_off, never_used, worth_keeping, Kept, Worth};

const DAY: u64 = 86_400;

#[test]
fn a_search_result_with_a_figure_in_it_is_kept() {
    // The kind a model misremembers, which is the whole reason for a shelf.
    assert_eq!(
        worth_keeping("the tier two fee is 40 basis points", 1, false),
        Some(Worth::HasAFigure)
    );
}

#[test]
fn most_of_what_a_search_returns_is_thrown_away() {
    // Keeping everything is how a knowledge store becomes a landfill.
    assert!(worth_keeping("brokers generally charge a fee for trades", 1, false).is_none());
    assert!(worth_keeping("this is a common approach", 2, false).is_none());
}

#[test]
fn something_that_took_four_searches_is_kept_because_the_fifth_time_would_too() {
    assert_eq!(
        worth_keeping("the setting is under Advanced", 4, false),
        Some(Worth::WasHardToFind)
    );
    assert!(worth_keeping("the setting is under Advanced", 2, false).is_none());
}

#[test]
fn you_saying_keep_it_beats_every_other_rule() {
    assert_eq!(worth_keeping("anything at all", 1, true), Some(Worth::YouSaidKeep));
}

#[test]
fn a_figure_from_two_years_ago_is_flagged_rather_than_given_as_current() {
    // Exactly the confident wrongness the shelf exists to prevent.
    let old = Kept {
        fact: "tier two is 40 basis points".into(),
        source: "the broker's site".into(),
        at: 0,
        because: Worth::HasAFigure,
        used: 3,
        corrected: false,
    };
    // Two years on, a fee tier isn't "worth checking" — it's old enough not
    // to rely on, and the wording should say which.
    let said = gone_off(&old, 700 * DAY).unwrap();
    assert!(said.contains("wouldn't rely on it"));
    assert!(said.contains("look again"));

    // And in the middle it's a softer note rather than the same sentence.
    let middling = gone_off(&old, 60 * DAY).unwrap();
    assert!(middling.contains("Worth checking"));
    assert_ne!(said, middling);
}

#[test]
fn something_about_your_own_setup_never_goes_stale_on_a_clock() {
    // A preference from two years ago is still your preference until you say
    // otherwise. It expires when you change it, not on a timer.
    let yours = Kept {
        fact: "I keep the panels on the right of the primary screen".into(),
        source: "you".into(),
        at: 0,
        because: Worth::AboutYou,
        used: 1,
        corrected: false,
    };
    assert!(gone_off(&yours, 900 * DAY).is_none());
    assert_eq!(
        atlas::reference::shelf_life(&yours),
        atlas::freshness::Shelf::Yours
    );
}

#[test]
fn a_correction_replaces_rather_than_sitting_beside_what_it_corrects() {
    // Otherwise Atlas holds both and picks one.
    let mut kept = vec![Kept {
        fact: "the fee is 40 basis points".into(),
        source: "the broker's site".into(),
        at: 0,
        because: Worth::HasAFigure,
        used: 1,
        corrected: false,
    }];
    correct_it(&mut kept, "the fee is 40 basis points", "the fee is 12 basis points", DAY);
    assert_eq!(kept.len(), 1);
    assert!(kept[0].fact.contains("12"));
    assert_eq!(kept[0].because, Worth::YouSaidKeep, "what you said outranks what it found");
}

#[test]
fn things_kept_and_never_used_are_offered_back_rather_than_deleted() {
    // The point of a low bar for looking is that some of it turns out not to
    // matter.
    let kept = vec![Kept {
        fact: "something".into(),
        source: "a search".into(),
        at: 0,
        because: Worth::WasHardToFind,
        used: 0,
        corrected: false,
    }];
    assert_eq!(never_used(&kept, 400 * DAY, 365).len(), 1);
    assert_eq!(never_used(&kept, 100 * DAY, 365).len(), 0);
}
