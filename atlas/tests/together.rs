//! Three trades that are one bet.
//!
//! Every number here is arithmetic on the positions listed in the test.
//! (28 Sep 2026: they were first run through a Python reference that also held
//! material that left personal Atlas, so it went with it.)

use atlas::together::{if_i_add, legs, net, spoken, Position, TogetherConfig};

fn cfg() -> TogetherConfig {
    TogetherConfig::default()
}

fn at(pair: &str, long: bool, risk: f64) -> Position {
    Position { pair: pair.into(), long, risk }
}

fn of(n: &atlas::together::Netted, ccy: &str) -> f64 {
    n.per_currency.iter().find(|(c, _)| c == ccy).map(|(_, v)| *v).unwrap_or(0.0)
}

#[test]
fn three_separate_longs_are_one_short_dollar_bet() {
    // The whole module. Each trade is one per cent and one per cent is the
    // rule; the log looks diversified; and one dollar print takes all three
    // out together for three per cent.
    let held = vec![at("EURUSD", true, 1.0), at("GBPUSD", true, 1.0), at("AUDUSD", true, 1.0)];
    let n = net(&held);
    assert!((of(&n, "USD") - -3.0).abs() < 1e-9, "{:?}", n.per_currency);
    assert!((of(&n, "EUR") - 1.0).abs() < 1e-9);
    assert!((of(&n, "GBP") - 1.0).abs() < 1e-9);
    assert!((of(&n, "AUD") - 1.0).abs() < 1e-9);
    assert!((n.as_counted - 3.0).abs() < 1e-9, "the ticket says three separate one-R trades");

    let (worst, amount) = n.biggest().unwrap();
    assert_eq!(worst, "USD");
    assert!((amount - -3.0).abs() < 1e-9);
    assert!((n.really_carrying().unwrap() - 1.0).abs() < 1e-9, "all of the risk is one bet");
}

#[test]
fn positions_that_genuinely_offset_are_genuinely_two_trades() {
    // Long EURUSD and short EURGBP cancels the euro out. The arithmetic has to
    // say so, or the warning fires on everything and gets ignored.
    let held = vec![at("EURUSD", true, 1.0), at("EURGBP", false, 1.0)];
    let n = net(&held);
    assert!((of(&n, "EUR")).abs() < 1e-9, "the euro cancels: {:?}", n.per_currency);
    assert!((of(&n, "USD") - -1.0).abs() < 1e-9);
    assert!((of(&n, "GBP") - 1.0).abs() < 1e-9);
    assert!(n.biggest().unwrap().1.abs() <= cfg().say_above);
}

#[test]
fn a_short_is_the_other_way_round() {
    let n = net(&[at("EURUSD", false, 2.0)]);
    assert!((of(&n, "EUR") - -2.0).abs() < 1e-9);
    assert!((of(&n, "USD") - 2.0).abs() < 1e-9);
}

#[test]
fn a_pair_atlas_does_not_know_is_named_and_never_silently_dropped() {
    // A silent zero here is the whole failure this module exists to prevent,
    // arriving through the door marked "unknown instrument" — it would make
    // the exposure look BETTER than it is.
    let held = vec![at("EURUSD", true, 1.0), at("XAUUSD", true, 5.0)];
    let n = net(&held);
    assert_eq!(n.unknown, vec!["XAUUSD".to_string()]);
    assert!((n.as_counted - 6.0).abs() < 1e-9, "it still counts toward what is at risk");
    let said = spoken(&held, &cfg());
    assert!(said.contains("XAUUSD"), "{said}");
    // The module's own sentence is "That makes this look safer than it is".
    // The assertion here read "looks safer than it is" and had never been run.
    assert!(said.contains("look safer than it is"), "{said}");
}

#[test]
fn adding_a_trade_that_doubles_a_bet_is_said_before_it_is_taken() {
    // Before the trade, which is the only time the answer is useful. After it
    // is on, this is a report; before it, it is a decision.
    let held = vec![at("EURUSD", true, 1.0), at("GBPUSD", true, 1.0)];
    let said = if_i_add(&held, &at("AUDUSD", true, 1.0), &cfg()).unwrap();
    assert!(said.contains("USD"), "{said}");
    assert!(said.contains("3.0 R"), "{said}");
    assert!(said.contains("one bet"), "{said}");
}

#[test]
fn adding_a_trade_that_offsets_is_not_warned_about() {
    let held = vec![at("EURUSD", true, 1.0), at("GBPUSD", true, 1.0)];
    assert_eq!(if_i_add(&held, &at("USDCHF", true, 1.0), &cfg()), None, "that reduces the bet");
    assert_eq!(if_i_add(&[], &at("EURUSD", true, 1.0), &cfg()), None, "one trade is one trade");
}

#[test]
fn a_warning_does_not_fire_on_a_position_somebody_already_holds() {
    // A warning that repeats every time the user looks at anything else is a
    // warning that gets ignored, and then the real one is ignored too.
    let held = vec![at("EURUSD", true, 3.0)];
    assert_eq!(
        if_i_add(&held, &at("EURGBP", false, 0.5), &cfg()),
        None,
        "this trade reduces the euro bet rather than adding to it"
    );
}

#[test]
fn a_pair_is_taken_apart_whatever_it_is_written_like() {
    assert_eq!(legs("EURUSD"), Some(("EUR", "USD")));
    assert_eq!(legs("eurusd"), Some(("EUR", "USD")));
    assert_eq!(legs("EUR/USD"), Some(("EUR", "USD")));
    assert_eq!(legs("eur_usd"), Some(("EUR", "USD")));
    assert_eq!(legs(" GBPJPY "), Some(("GBP", "JPY")));
    assert_eq!(legs("NOTAPAIR"), None);
}

#[test]
fn nonsense_sizes_are_left_out_rather_than_poisoning_the_totals() {
    let held = vec![at("EURUSD", true, f64::NAN), at("GBPUSD", true, -1.0), at("AUDUSD", true, 1.0)];
    let n = net(&held);
    assert!((n.as_counted - 1.0).abs() < 1e-9);
    assert!((of(&n, "AUD") - 1.0).abs() < 1e-9);
    assert!((of(&n, "EUR")).abs() < 1e-9);
}

#[test]
fn nothing_open_says_nothing_open() {
    assert_eq!(spoken(&[], &cfg()), "Nothing open.");
    let n = net(&[]);
    assert_eq!(n.biggest(), None);
    assert_eq!(n.really_carrying(), None);
}

#[test]
fn the_biggest_bet_is_reported_by_size_and_not_by_sign() {
    // Short three R of dollar is a bigger bet than long one R of euro, and a
    // comparison that sorted by the signed number would pick the euro.
    let held = vec![at("EURUSD", true, 1.0), at("GBPUSD", true, 1.0), at("AUDUSD", true, 1.0)];
    let n = net(&held);
    assert_eq!(n.biggest().unwrap().0, "USD");
    assert_eq!(n.per_currency[0].0, "USD", "sorted biggest-first by size");
}

#[test]
fn the_summary_marks_the_one_that_is_really_one_trade() {
    let held = vec![at("EURUSD", true, 1.5), at("GBPUSD", true, 1.5), at("AUDUSD", true, 1.0)];
    let said = spoken(&held, &cfg());
    assert!(said.contains("one bet, not several"), "{said}");
    assert!(said.contains("the exposure says one trade"), "{said}");
}

#[test]
fn if_i_add_is_actually_asked_before_a_trade_is_proposed() {
    // `spoken` being wired is a report you have to ask for. The proactive
    // check -- would *this* proposed trade make the real exposure worse --
    // is the thing item 2 of the 12 Sep gap analysis was actually asking
    // for, and it went uncalled for a full session despite existing,
    // tested, and correct. Pinned here so it can't quietly go back to that.
    let main: &str = &crate::common::source_of("main");
    assert!(
        main.contains("together::if_i_add("),
        "the correlation-aware check is built but nothing asks it before proposing a trade"
    );
}
