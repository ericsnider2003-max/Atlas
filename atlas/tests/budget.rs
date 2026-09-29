use atlas::budget::{
    allow, estimate, first_run_estimate, overnight_estimate, report, route, Approval, BudgetConfig,
    Difficulty, Job, Ledger, Spend, Tier,
};

fn on() -> BudgetConfig {
    BudgetConfig { enabled: true, ..Default::default() }
}

/// A realistic night's task: the Atlas source as cached context, a short
/// instruction, and a patch back.
fn task(what: &str) -> Job {
    Job {
        what: what.into(),
        cached_input: 120_000,
        fresh_input: 1_500,
        expected_output: 3_000,
        can_wait: true,
    }
}

// ================= nothing costs anything until you say so =================

#[test]
fn everything_runs_locally_until_you_turn_hosting_on() {
    let cfg = BudgetConfig::default();
    assert!(!cfg.enabled);
    assert_eq!(route(Difficulty::Hard, &cfg), Tier::Local);
    assert_eq!(estimate(&task("x"), Tier::Local, &cfg), 0.0);
    assert!(report(&Ledger::default(), &cfg, 0).contains("on your machine"));
}

#[test]
fn simple_work_never_reaches_a_hosted_model_at_all() {
    let cfg = on();
    assert_eq!(route(Difficulty::Trivial, &cfg), Tier::Local, "a threshold change is free");
    match allow(&task("bump a timeout"), Difficulty::Trivial, &Ledger::default(), &cfg, 0) {
        Approval::Local(_) => {}
        o => panic!("{o:?}"),
    }
}

#[test]
fn the_cheapest_model_that_can_do_it_is_the_one_used() {
    let cfg = on();
    assert_eq!(route(Difficulty::Simple, &cfg), Tier::Haiku);
    assert_eq!(route(Difficulty::Real, &cfg), Tier::Sonnet);
}

#[test]
fn your_ceiling_wins_over_what_atlas_thinks_it_needs() {
    let cfg = BudgetConfig { ceiling: Tier::Haiku, ..on() };
    assert_eq!(route(Difficulty::Hard, &cfg), Tier::Haiku, "never above what you allowed");
}

// ================= the discounts stack =================

#[test]
fn caching_the_codebase_is_where_most_of_the_saving_comes_from() {
    // The source barely changes between requests, and a cache read costs a
    // tenth of a fresh one.
    let cached = estimate(&task("x"), Tier::Sonnet, &on());
    let uncached = estimate(&task("x"), Tier::Sonnet, &BudgetConfig { use_caching: false, ..on() });
    assert!(cached < uncached / 2.0, "cached {cached:.4} vs uncached {uncached:.4}");
}

#[test]
fn overnight_work_goes_at_the_batch_rate() {
    let cfg = on();
    let waits = estimate(&task("x"), Tier::Sonnet, &cfg);
    let urgent = estimate(&Job { can_wait: false, ..task("x") }, Tier::Sonnet, &cfg);
    assert!((waits - urgent * 0.5).abs() < 1e-9, "half price for anything that can wait");
}

#[test]
fn the_first_request_of_the_night_costs_more_and_is_counted_separately() {
    // Filling the cache is not free. A plan that ignores that underestimates.
    let cfg = on();
    let first = first_run_estimate(&task("x"), Tier::Sonnet, &cfg);
    let later = estimate(&task("x"), Tier::Sonnet, &cfg);
    assert!(first > later * 3.0, "first {first:.4}, later {later:.4}");
}

#[test]
fn a_nights_work_is_priced_before_it_happens() {
    let jobs: Vec<Job> = (0..20).map(|i| task(&format!("task {i}"))).collect();
    let (total, note) = overnight_estimate(&jobs, Difficulty::Real, &on());
    assert!(note.contains("20 tasks on Sonnet"), "got: {note}");
    assert!(note.contains("codebase cached") && note.contains("overnight rate"));
    // Twenty real code changes overnight should be small money, not alarming.
    assert!(total < 1.0, "twenty tasks came to ${total:.2}");
    assert!(total > 0.05, "and it isn't free either");
}

#[test]
fn a_single_urgent_change_is_still_pennies() {
    let one = estimate(&Job { can_wait: false, ..task("fix the parser") }, Tier::Sonnet, &on());
    assert!(one < 0.10, "${one:.4} for one change");
}

// ================= the cap stops rather than warns =================

#[test]
fn the_daily_cap_is_a_stop_not_a_warning() {
    let cfg = BudgetConfig { daily_cap: 0.05, ..on() };
    let mut l = Ledger::default();
    l.record(Spend { at: 100, tier: Tier::Sonnet, dollars: 0.049, what: "x".into(), batched: true });
    match allow(&task("another"), Difficulty::Real, &l, &cfg, 200) {
        Approval::Refused(why) => {
            assert!(why.contains("past $0.05"), "got: {why}");
            assert!(why.contains("Spent $0.05"), "and what's gone already: {why}");
        }
        o => panic!("expected a refusal, got {o:?}"),
    }
}

#[test]
fn the_monthly_cap_holds_even_when_today_has_room() {
    let cfg = BudgetConfig { daily_cap: 100.0, monthly_cap: 0.10, ..on() };
    let mut l = Ledger::default();
    l.record(Spend { at: 0, tier: Tier::Sonnet, dollars: 0.099, what: "x".into(), batched: true });
    assert!(matches!(
        allow(&task("x"), Difficulty::Real, &l, &cfg, 1000),
        Approval::Refused(_)
    ));
}

#[test]
fn yesterdays_spending_does_not_count_against_today() {
    let cfg = BudgetConfig { daily_cap: 0.10, ..on() };
    let mut l = Ledger::default();
    l.record(Spend { at: 0, tier: Tier::Sonnet, dollars: 0.09, what: "x".into(), batched: true });
    let tomorrow = 2 * 86_400;
    assert!(matches!(
        allow(&task("x"), Difficulty::Real, &l, &cfg, tomorrow),
        Approval::Go { .. }
    ));
}

#[test]
fn an_allowed_job_says_what_it_will_cost_before_running() {
    match allow(&task("x"), Difficulty::Real, &Ledger::default(), &on(), 0) {
        Approval::Go { tier, dollars, batched } => {
            assert_eq!(tier, Tier::Sonnet);
            assert!(dollars > 0.0 && dollars < 0.05);
            assert!(batched);
        }
        o => panic!("{o:?}"),
    }
}

// ================= you can always ask what it cost =================

#[test]
fn what_it_has_cost_is_always_answerable() {
    let mut l = Ledger::default();
    l.record(Spend { at: 100, tier: Tier::Sonnet, dollars: 0.42, what: "x".into(), batched: true });
    let said = report(&l, &on(), 200);
    assert!(said.contains("$0.42 today"));
    assert!(said.contains("Cap is"), "and what the ceiling is: {said}");
}

#[test]
fn a_month_with_nothing_spent_says_so_plainly() {
    // This test used to hand `report` an EMPTY ledger and assert "Nothing
    // yet". That is the one case where the sentence is not a measurement:
    // nothing in `src/` calls `Ledger::record`, so an empty ledger is what a
    // machine that has spent four hundred dollars also looks like. See
    // `budget::untracked` and `tests/the_budget_knows_what_it_does_not_know.rs`.
    //
    // What the test was actually pinning — that a month with no spending in
    // it is reported plainly rather than as a row of zeroes — is kept, with a
    // ledger that has genuinely recorded something outside this month.
    let mut l = Ledger::default();
    l.record(Spend {
        at: 100,
        tier: Tier::Sonnet,
        dollars: 0.42,
        what: "last month".into(),
        batched: true,
    });
    let said = report(&l, &on(), 100 + 60 * 60 * 24 * 90);
    assert!(said.contains("Nothing yet"), "got: {said}");

    // And the empty ledger says it cannot tell you, naming what is missing.
    let empty = report(&Ledger::default(), &on(), 1000);
    assert!(
        empty.contains("can't tell") && empty.contains("ledger"),
        "an unwritten ledger reported a month with no spending in it: {empty}"
    );
}

#[test]
fn the_ledger_stays_bounded() {
    let mut l = Ledger::default();
    for i in 0..3000 {
        l.record(Spend { at: i, tier: Tier::Haiku, dollars: 0.001, what: "x".into(), batched: true });
    }
    assert!(l.spends.len() <= 2000);
}

#[test]
fn the_shipped_defaults_are_cautious() {
    let cfg = BudgetConfig::default();
    assert!(!cfg.enabled, "hosted models are opt-in");
    assert!(cfg.daily_cap <= 5.0, "a low daily cap by default");
    assert!(cfg.monthly_cap <= 50.0);
    assert_eq!(cfg.ceiling, Tier::Sonnet, "not the most expensive tier by default");
    assert!(cfg.prefer_batch && cfg.use_caching, "both discounts on by default");
}

#[test]
fn an_empty_ledger_reports_zero_rather_than_minus_zero() {
    // Rust's float `Sum` uses -0.0 as its identity, so an empty ledger summed
    // to negative zero and `{:.2}` rendered it as "-0.00" — a ledger with
    // nothing in it appearing to owe you money. Caught the first time a
    // command printed it.
    let ledger = atlas::budget::Ledger::default();
    assert_eq!(format!("{:.2}", ledger.today(1_000_000)), "0.00");
    assert_eq!(format!("{:.2}", ledger.this_month(1_000_000)), "0.00");
    assert!(ledger.today(1_000_000).is_sign_positive());
}
