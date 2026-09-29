use atlas::finance::{Source, Transaction};
use atlas::ledger::{
    categorise, keep_for, relevant_rule, spoken, summarise, trading_rules, Category, NOT_ADVICE,
};
use atlas::plainchange::{area_of, ask, explain, sentence_from_test, spoken as change_spoken, written, Diff};

fn txn(desc: &str, amount: f64) -> Transaction {
    Transaction {
        date: "2026-08-01".into(),
        description: desc.into(),
        amount,
        category: None,
        source: Source::LocalFile,
    }
}

// ================= explaining a change without code =================

#[test]
fn a_test_name_is_already_a_sentence_about_behaviour() {
    // Which is why they're the best description of a change there is.
    assert_eq!(
        sentence_from_test("a_dangling_word_means_you_are_not_finished"),
        "A dangling word means you are not finished"
    );
}

#[test]
fn files_are_named_the_way_you_would_name_them() {
    // "How it hears you" means something. src/endpoint.rs doesn't.
    assert_eq!(area_of("src/endpoint.rs"), "how it hears and speaks");
    assert_eq!(area_of("src/persona.rs"), "how it talks to you");
    assert_eq!(area_of("src/panel.rs"), "what you see");
    assert_eq!(area_of("src/policy.rs"), "permissions");
}

#[test]
fn you_are_told_what_will_be_different_not_what_lines_moved() {
    let d = Diff {
        files: vec!["src/endpoint.rs".into()],
        tests_added: vec!["it_stops_listening_when_you_stop_talking".into()],
        lines_added: 40,
        ..Default::default()
    };
    let e = explain(&d, "make it stop listening sooner");
    assert!(e.headline.contains("it stops listening when you stop talking"));
    assert!(!e.headline.contains("src/"), "no filenames in the headline");
    assert_eq!(e.areas, vec!["how it hears and speaks"]);

    let w = written(&e);
    assert!(w.contains("It will now:"));
    assert!(!w.contains("fn "), "no code anywhere in it");
    assert!(!w.contains("+++") && !w.contains("@@"), "and no diff");
}

#[test]
fn something_atlas_used_to_promise_and_no_longer_does_is_the_thing_to_stop_on() {
    let d = Diff {
        files: vec!["src/publish.rs".into()],
        tests_removed: vec!["a_post_edited_after_approval_does_not_fire".into()],
        ..Default::default()
    };
    let e = explain(&d, "tidy up posting");
    assert!(e.no_longer[0].contains("edited after approval"));
    assert!(e.watch_out[0].contains("used to promise it no longer does"));
}

#[test]
fn a_change_with_no_test_difference_says_it_cannot_be_sure() {
    // Rather than inventing a summary of something it can't read.
    let d = Diff { files: vec!["src/look.rs".into()], lines_added: 30, ..Default::default() };
    let e = explain(&d, "tidy the styling");
    assert!(!e.certain);
    assert!(change_spoken(&e).contains("can't tell you exactly what changed"));
}

#[test]
fn a_big_change_and_new_settings_are_both_flagged() {
    let d = Diff {
        files: vec!["src/voice.rs".into()],
        tests_added: vec!["something_new_happens".into()],
        lines_added: 400,
        settings_touched: vec!["endpoint.silence_below_db".into()],
        ..Default::default()
    };
    let e = explain(&d, "rework listening");
    assert!(e.watch_out.iter().any(|w| w.contains("big change")));
    assert!(e.watch_out.iter().any(|w| w.contains("new settings")));
}

#[test]
fn the_question_is_never_apply_the_diff() {
    let e = explain(&Diff::default(), "x");
    assert!(ask(&e).contains("keep that"));
    assert!(!ask(&e).to_lowercase().contains("diff"));
}

// ================= reading statements =================

#[test]
fn ordinary_spending_is_recognised() {
    assert_eq!(categorise(&txn("KROGER #442", -84.20)), Category::Groceries);
    assert_eq!(categorise(&txn("COMCAST XFINITY", -89.99)), Category::Internet);
    assert_eq!(categorise(&txn("NETFLIX.COM", -15.99)), Category::Subscription);
    assert_eq!(categorise(&txn("SHELL OIL 4471", -52.00)), Category::Transport);
}

#[test]
fn the_things_a_trader_actually_spends_on_are_recognised() {
    assert_eq!(categorise(&txn("CME MARKET DATA FEE", -110.00)), Category::Fees);
    assert_eq!(categorise(&txn("TRADESTATION PLATFORM FEE", -99.00)), Category::Fees);
    assert_eq!(categorise(&txn("INTERACTIVE BROKERS TRANSFER", -5000.00)), Category::Investment);
    assert_eq!(categorise(&txn("JETBRAINS SUBSCRIPTION", -289.00)), Category::Software);
}

#[test]
fn money_moving_between_your_own_accounts_is_not_income_or_spend() {
    // Counting a brokerage transfer as spending makes every total wrong.
    let txns = vec![
        txn("PAYROLL DEPOSIT", 5000.0),
        txn("INTERACTIVE BROKERS TRANSFER", -4000.0),
        txn("KROGER", -100.0),
    ];
    let s = summarise(&txns);
    assert_eq!(s.in_total, 5000.0);
    assert_eq!(s.out_total, -100.0, "the transfer is neither");
    assert_eq!(s.net(), 4900.0);
}

#[test]
fn spend_a_business_commonly_deducts_is_totalled_separately() {
    let txns = vec![
        txn("JETBRAINS SUBSCRIPTION", -289.0),
        txn("CME MARKET DATA FEE", -110.0),
        txn("KROGER", -84.0),
    ];
    let s = summarise(&txns);
    assert_eq!(s.possibly_deductible, 399.0, "groceries are not in there");
}

#[test]
fn things_it_cannot_place_are_counted_rather_than_guessed() {
    let s = summarise(&[txn("SQ *8H2K1LM", -42.0)]);
    assert_eq!(s.unrecognised, 1);
    assert!(spoken(&s).contains("1 I couldn't place"));
}

#[test]
fn what_atlas_says_leads_with_the_shape_of_the_month() {
    let txns = vec![txn("PAYROLL DEPOSIT", 6000.0), txn("RENT", -2200.0), txn("KROGER", -400.0)];
    let said = spoken(&summarise(&txns));
    assert!(said.contains("In 6000") && said.contains("net 3400"));
    assert!(said.contains("Housing"), "the biggest thing is named");
}

// ================= knowing enough to be useful =================

#[test]
fn it_knows_the_rules_that_actually_catch_traders_out() {
    let names: Vec<&str> = trading_rules().iter().map(|r| r.name).collect();
    for expected in [
        "wash sales",
        "Section 1256 contracts",
        "trader tax status",
        "mark-to-market election",
        "estimated payments",
        "the capital loss limit",
    ] {
        assert!(names.contains(&expected), "missing {expected}");
    }
}

#[test]
fn each_rule_says_why_it_matters_to_you_not_just_what_it_is() {
    for r in trading_rules() {
        assert!(!r.why_you.is_empty(), "{} has no reason to care", r.name);
        assert!(r.what.len() > 60, "{} is too thin to be useful", r.name);
    }
}

#[test]
fn the_mark_to_market_deadline_trap_is_spelled_out() {
    // The one that costs a year if nobody mentions it.
    let r = trading_rules().into_iter().find(|r| r.name == "mark-to-market election").unwrap();
    assert!(r.why_you.contains("previous"), "the deadline is for last year: {}", r.why_you);
}

#[test]
fn it_volunteers_one_relevant_rule_rather_than_a_lecture() {
    let many: Vec<Transaction> = (0..40).map(|_| txn("CME MARKET DATA FEE", -110.0)).collect();
    let s = summarise(&many);
    let r = relevant_rule(&s, &many).expect("frequent trading should raise something");
    assert_eq!(r.name, "wash sales");
}

#[test]
fn a_quiet_month_gets_no_rule_at_all() {
    let quiet = vec![txn("KROGER", -84.0)];
    assert!(relevant_rule(&summarise(&quiet), &quiet).is_none());
}

#[test]
fn categories_carry_the_caveat_that_actually_matters() {
    assert!(Category::Meals.note().unwrap().contains("50%"));
    assert!(Category::Office.note().unwrap().contains("exclusively"));
    assert!(Category::Internet.note().unwrap().contains("business share"));
    assert!(Category::Investment.note().unwrap().contains("aren't income or expense"));
}

#[test]
fn it_knows_how_long_to_keep_things_and_that_basis_is_different() {
    assert!(keep_for("receipts").contains("three years"));
    assert!(keep_for("basis records").contains("after you sell"), "the trap");
}

#[test]
fn it_says_plainly_that_this_is_not_advice() {
    assert!(NOT_ADVICE.contains("not your situation"));
    assert!(NOT_ADVICE.contains("ask someone"));
    assert!(!NOT_ADVICE.contains("consult a qualified"), "not boilerplate");
}

#[test]
fn deductible_is_hedged_because_whether_yours_qualifies_is_about_you() {
    assert!(Category::Software.often_deductible());
    assert!(!Category::Groceries.often_deductible());
    // The word is "often", and the summary says "worth checking" rather than
    // "you can deduct".
    let s = summarise(&[txn("JETBRAINS SUBSCRIPTION", -2000.0)]);
    assert!(spoken(&s).contains("worth checking"));
}
