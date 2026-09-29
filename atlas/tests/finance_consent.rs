use atlas::consent::{explain, ConsentConfig, Recorder, Rule, Scope, State, Step};
use atlas::finance::{allowed, parse_csv, review, summary, FinanceConfig, Flag, PageAction, Source, Transaction, Verdict};

fn fin() -> FinanceConfig {
    FinanceConfig { enabled: true, ..Default::default() }
}
const BANK: &str = "https://www.chase.com/accounts";
const NOT_BANK: &str = "https://news.example.com";

// ================= read-only by construction =================

#[test]
fn atlas_can_look_around_a_bank_site() {
    let c = fin();
    assert!(allowed(BANK, &PageAction::Navigate(BANK.into()), &c).ok());
    assert!(allowed(BANK, &PageAction::Read, &c).ok());
    assert!(allowed(BANK, &PageAction::Download, &c).ok(), "the export button is the point");
}

#[test]
fn it_will_never_submit_a_form_on_a_bank_site() {
    // A submit could be anything. This is the single most important line in
    // the module.
    let v = allowed(BANK, &PageAction::Submit, &fin());
    assert!(!v.ok());
    match v {
        Verdict::Refused(why) => assert!(why.contains("Do that yourself")),
        _ => unreachable!(),
    }
}

#[test]
fn buttons_that_move_money_are_refused_by_name() {
    let c = fin();
    for label in [
        "Transfer money", "Send", "Pay bill", "Zelle", "Withdraw",
        "Buy", "Sell", "Confirm payment", "Wire transfer", "Schedule payment",
    ] {
        let v = allowed(BANK, &PageAction::Click(label.into()), &c);
        assert!(!v.ok(), "\"{label}\" should have been refused");
    }
}

#[test]
fn harmless_buttons_still_work() {
    let c = fin();
    for label in ["Statements", "Download CSV", "Next page", "Account details"] {
        assert!(allowed(BANK, &PageAction::Click(label.into()), &c).ok(), "{label}");
    }
}

#[test]
fn typing_is_limited_to_logins_and_date_filters() {
    let c = fin();
    assert!(allowed(BANK, &PageAction::Type { field: "username".into(), text: "x".into() }, &c).ok());
    assert!(allowed(BANK, &PageAction::Type { field: "start date".into(), text: "x".into() }, &c).ok());
}

#[test]
fn an_unrecognised_field_on_a_bank_page_fails_closed() {
    // It could be an amount or an account number. Refusing costs you a manual
    // click; allowing costs money.
    let v = allowed(
        BANK,
        &PageAction::Type { field: "recipient account".into(), text: "123".into() },
        &fin(),
    );
    assert!(!v.ok(), "unknown fields must be refused, not allowed");
}

#[test]
fn none_of_this_applies_to_ordinary_websites() {
    let c = fin();
    assert!(allowed(NOT_BANK, &PageAction::Submit, &c).ok());
    assert!(allowed(NOT_BANK, &PageAction::Click("Buy now".into()), &c).ok());
}

#[test]
fn the_domain_list_covers_banks_cards_and_brokerages() {
    let c = fin();
    for d in ["chase.com", "amex.com", "paypal.com", "coinbase.com", "fidelity.com"] {
        assert!(c.is_financial(&format!("https://{d}/x")), "{d} should be protected");
    }
}

#[test]
fn getting_the_data_without_credentials_is_possible_and_marked_as_such() {
    assert!(!Source::LocalFile.needs_credentials());
    assert!(!Source::Aggregator.needs_credentials());
    assert!(Source::SiteExport.needs_credentials(), "only this one holds your login");
}

// ================= reading the numbers =================

const CSV: &str = "Date,Description,Amount
2026-08-01,COFFEE SHOP,-4.50
2026-08-02,RENT PAYMENT,-1800.00
2026-08-03,NETFLIX SUBSCRIPTION,-15.99
2026-08-04,NETFLIX SUBSCRIPTION,-15.99
2026-09-03,NETFLIX SUBSCRIPTION,-15.99
2026-08-05,SALARY,3200.00";

#[test]
fn a_csv_export_is_read_whatever_the_column_names() {
    let t = parse_csv(CSV, Source::LocalFile);
    assert_eq!(t.len(), 6);
    assert_eq!(t[0].description, "COFFEE SHOP");
    assert_eq!(t[1].amount, -1800.00);
}

#[test]
fn separate_debit_and_credit_columns_are_handled() {
    // Plenty of banks export this way instead of one signed amount.
    let csv = "Posted Date,Payee,Debit,Credit\n2026-08-01,SHOP,25.00,\n2026-08-02,REFUND,,10.00";
    let t = parse_csv(csv, Source::LocalFile);
    assert_eq!(t.len(), 2);
    assert_eq!(t[0].amount, -25.00, "a debit is money out");
    assert_eq!(t[1].amount, 10.00);
}

#[test]
fn currency_symbols_and_commas_do_not_break_it() {
    let csv = "Date,Description,Amount\n2026-08-01,BIG THING,\"-1,234.56\"";
    let t = parse_csv(csv, Source::LocalFile);
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].amount, -1234.56);
}

#[test]
fn a_file_it_cannot_understand_yields_nothing_rather_than_nonsense() {
    assert!(parse_csv("just some text\nwith no columns", Source::LocalFile).is_empty());
    assert!(parse_csv("", Source::LocalFile).is_empty());
}

#[test]
fn large_transactions_are_flagged() {
    let t = parse_csv(CSV, Source::LocalFile);
    let flags = review(&t, &fin());
    assert!(flags.iter().any(|f| matches!(f, Flag::Large { t } if t.description == "RENT PAYMENT")));
}

#[test]
fn the_same_charge_twice_in_a_few_days_is_flagged() {
    let t = parse_csv(CSV, Source::LocalFile);
    let flags = review(&t, &fin());
    assert!(
        flags.iter().any(|f| matches!(f, Flag::PossibleDuplicate { a, .. } if a.description.contains("NETFLIX"))),
        "two identical charges a day apart is worth a look"
    );
}

#[test]
fn a_charge_repeating_a_month_later_is_not_a_duplicate() {
    let t = parse_csv(CSV, Source::LocalFile);
    let dupes = review(&t, &fin())
        .into_iter()
        .filter(|f| matches!(f, Flag::PossibleDuplicate { .. }))
        .count();
    // Only the two August charges pair; September is a month away.
    assert_eq!(dupes, 1, "a monthly subscription is not a duplicate charge");
}

#[test]
fn something_billing_repeatedly_is_recognised_as_a_subscription() {
    let t = parse_csv(CSV, Source::LocalFile);
    let flags = review(&t, &fin());
    assert!(flags.iter().any(|f| matches!(f, Flag::Recurring { times, .. } if *times == 3)));
}

#[test]
fn a_quiet_month_says_nothing_unusual() {
    let quiet = vec![Transaction {
        date: "2026-08-01".into(),
        description: "COFFEE".into(),
        amount: -4.5,
        category: None,
        source: Source::LocalFile,
    }];
    assert!(review(&quiet, &fin()).is_empty());
    assert_eq!(summary(&[]), "Nothing unusual.");
}

#[test]
fn the_summary_leads_with_how_much_there_is_to_hear() {
    let t = parse_csv(CSV, Source::LocalFile);
    let s = summary(&review(&t, &fin()));
    // Behaviour, not just wording: the summary leads with a count because
    // review actually found things -- an empty review takes the other branch.
    assert!(!review(&t, &fin()).is_empty(), "the summary claims things to hear but review found none");
    assert!(s.contains("things worth a look") || s.contains("One thing"), "got: {s}");
}

// ================= recording a call =================

fn rec() -> Recorder {
    // The announce-and-object mode; asking first is tested in `asking_before_recording.rs`.
    Recorder::new(ConsentConfig { enabled: true, ask_the_others: false, ..Default::default() })
}

#[test]
fn noting_your_own_side_needs_nobody_else_s_permission() {
    // Your microphone captures you and nobody else.
    let mut r = rec();
    assert_eq!(r.call_started(Scope::YouOnly), Step::Start(Scope::YouOnly));
    assert_eq!(r.state, State::Recording);
    assert!(!r.capturing_others());
}

#[test]
fn recording_everyone_asks_you_first_then_tells_them() {
    let mut r = rec();
    match r.call_started(Scope::Everyone) {
        Step::AskYou(q) => assert!(q.contains("tell everyone first"), "got: {q}"),
        o => panic!("{o:?}"),
    }
    match r.you_approved() {
        Step::Announce(msg) => assert!(msg.contains("taking notes")),
        o => panic!("{o:?}"),
    }
    assert_eq!(r.state, State::Announcing);
    assert!(!r.capturing_others(), "nothing is captured until they've been told");
}

#[test]
fn capture_begins_only_after_the_announcement_actually_lands() {
    let mut r = rec();
    r.call_started(Scope::Everyone);
    r.you_approved();
    assert_eq!(r.announcement_delivered(), Step::Start(Scope::Everyone));
    assert!(r.capturing_others());
    assert!(!r.recorded_others_without_announcing(), "the invariant holds");
}

#[test]
fn silence_is_not_consent() {
    // Muted, no chat, call dropped — whatever the reason, nothing is recorded.
    let mut r = rec();
    r.call_started(Scope::Everyone);
    r.you_approved();
    let step = r.announcement_failed("you were muted");
    assert!(matches!(step, Step::AskYou(_)));
    assert!(!r.capturing_others());
    assert_eq!(r.scope, Scope::YouOnly);
    assert!(!r.recorded_others_without_announcing());
}

#[test]
fn one_objection_stops_everything_and_throws_it_away() {
    let mut r = rec();
    r.call_started(Scope::Everyone);
    r.you_approved();
    r.announcement_delivered();
    match r.someone_objected("Sam") {
        Step::StopAndDiscard(why) => {
            assert!(why.contains("Sam") && why.contains("deleted"), "got: {why}")
        }
        o => panic!("{o:?}"),
    }
    assert_eq!(r.state, State::Stopped);
    assert!(!r.capturing_others());
}

#[test]
fn declining_falls_back_to_your_side_rather_than_nothing() {
    let mut r = rec();
    r.call_started(Scope::Everyone);
    assert_eq!(r.you_declined(), Step::Start(Scope::YouOnly));
}

#[test]
fn a_call_that_ends_mid_announcement_keeps_nothing() {
    let mut r = rec();
    r.call_started(Scope::Everyone);
    r.you_approved();
    assert!(matches!(r.call_ended(), Step::StopAndDiscard(_)));
}

#[test]
fn recording_is_always_visible_while_it_happens() {
    let mut r = rec();
    assert!(r.indicator().is_none());
    r.call_started(Scope::YouOnly);
    assert_eq!(r.indicator().as_deref(), Some("noting your side"));
    let mut r2 = rec();
    r2.call_started(Scope::Everyone);
    r2.you_approved();
    r2.announcement_delivered();
    assert_eq!(r2.indicator().as_deref(), Some("recording the call"));
}

#[test]
fn the_strict_rule_is_assumed_because_atlas_cannot_know_where_anyone_is() {
    let cfg = ConsentConfig::default();
    assert_eq!(cfg.assume, Rule::AllParties);
    assert_eq!(cfg.default_scope, Scope::YouOnly, "the safe scope by default");
    assert!(cfg.ask_every_call);
    assert!(!cfg.enabled, "and off entirely until you turn it on");
}

#[test]
fn with_the_feature_off_nothing_records_at_all() {
    let mut r = Recorder::default();
    assert_eq!(r.call_started(Scope::Everyone), Step::Nothing);
    assert!(!r.capturing_others());
}

#[test]
fn atlas_can_explain_exactly_what_it_does_on_calls() {
    let said = explain(&ConsentConfig { enabled: true, ask_the_others: false, ..Default::default() });
    assert!(said.contains("announce"));
    assert!(said.contains("objects"));
    assert!(said.contains("7 days"), "and how long audio is kept: {said}");
    assert!(explain(&ConsentConfig::default()).contains("don't record"));
}

#[test]
fn a_quoted_field_containing_a_comma_stays_one_field() {
    use atlas::finance::split_row;
    assert_eq!(
        split_row("2026-08-01,\"SHOP, INC\",\"-1,234.56\""),
        vec!["2026-08-01", "SHOP, INC", "-1,234.56"]
    );
    assert_eq!(split_row("a,b,c"), vec!["a", "b", "c"]);
    assert_eq!(split_row("\"say \"\"hi\"\"\",x"), vec!["say \"hi\"", "x"]);
}

#[test]
fn you_can_see_exactly_what_would_appear_in_the_chat() {
    use atlas::consent::{announcement_named, ANNOUNCEMENTS};
    // Four wordings, all short enough that nobody resents the interruption.
    assert_eq!(ANNOUNCEMENTS.len(), 4);
    for (name, text) in ANNOUNCEMENTS {
        assert!(text.len() < 220, "{name} is too long to drop into a call");
        assert!(
            text.to_lowercase().contains("rather i didn't")
                || text.to_lowercase().contains("problem")
                || text.to_lowercase().contains("prefer i turn it off")
                || text.to_lowercase().contains("switch it off"),
            "{name} must make declining easy, or it isn't consent"
        );
    }
    assert!(announcement_named("brief").is_some());
    assert!(announcement_named("nonsense").is_none());
}

#[test]
fn the_whole_script_is_visible_up_front() {
    use atlas::consent::script;
    let s = script(&ConsentConfig { enabled: true, ..Default::default() });
    let stages: Vec<&str> = s.iter().map(|(k, _)| *k).collect();
    assert!(stages.contains(&"on joining"));
    assert!(stages.contains(&"if someone objects"));
    assert!(stages.iter().any(|k| k.contains("asks what it does")));
    assert!(!stages.iter().any(|k| k.contains("call ends")), "no sign-off in the chat");

    let explains = s.iter().find(|(k, _)| k.contains("asks what it does")).unwrap();
    assert!(explains.1.contains("locally"), "people will ask where it goes");
    assert!(explains.1.contains("Nothing is uploaded"));
}
