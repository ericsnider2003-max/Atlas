//! A spending cap that has never counted a spend must say so.
//!
//! ## What was wrong
//!
//! `atlas budget` loads a `Ledger` from the `budget_ledger` record. **Nothing
//! in the whole tree saves that record, and `Ledger::record` has no caller
//! anywhere in `src/`.** So the ledger is always empty, and:
//!
//! * `budget::report` answered *"Nothing yet this month."* — identical on a
//!   machine that has spent nothing and a machine that has spent four hundred
//!   dollars.
//! * `budget::allow` compared a prospective job against `ledger.today(now)`,
//!   which is always `0.0`, so the daily and monthly caps bound **one job at
//!   a time** and never a total. A cap that does not accumulate is not a cap.
//! * `Approval::Refused` said *"Spent $0.00 so far"*, stating a total nobody
//!   measured.
//!
//! Everything else in the module is built and right — `estimate` prices a job
//! from the configured per-token rates, `route` picks a tier, `a_night` costs
//! a night's work in advance. The missing piece is one line at the end of a
//! hosted model call.
//!
//! ## How it hid from every existing guard
//!
//! `wiring.rs` and `dead_capabilities.rs` ask "can the program reach this?"
//! and the answer was yes — `atlas budget` is a real subcommand that really
//! runs. `no_confident_nothings.rs` looks for sentences that claim certainty
//! about absence, and *"Nothing yet this month"* is exactly that shape but
//! was not on its list.
//!
//! The one that found it is new: `tests/one_name_one_record.rs`, which noticed
//! that `budget_ledger` is read and never written. A record with a reader and
//! no writer is a feature reporting on a file that will never exist, and
//! nothing had been watching the store's key space at all.
//!
//! ## What this file pins
//!
//! That the note and the wiring cannot disagree. The moment someone calls
//! `Ledger::record` from production code, `untracked()` must return `None`
//! and these tests say so — so the caveat is deleted by the change that makes
//! it false, rather than surviving it.

use atlas::budget::{allow, report, untracked, BudgetConfig, Difficulty, Job, Ledger, Tier};

fn hosted_config() -> BudgetConfig {
    BudgetConfig { enabled: true, ..BudgetConfig::default() }
}

/// Every production reference to `Ledger::record`, ignoring comments and the
/// module's own definition and tests.
fn production_callers_of_record() -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![std::path::PathBuf::from("src")];
    while let Some(p) = stack.pop() {
        if p.is_dir() {
            for e in std::fs::read_dir(&p).expect("readable").flatten() {
                stack.push(e.path());
            }
            continue;
        }
        if p.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let text = std::fs::read_to_string(&p).unwrap_or_default();
        // Everything below `#[cfg(test)]` in a file is its own unit tests.
        let live = match text.find("#[cfg(test)]") {
            Some(at) => &text[..at],
            None => &text[..],
        };
        for (i, line) in live.lines().enumerate() {
            let t = line.trim_start();
            if t.starts_with("//") {
                continue;
            }
            // `.record(` on something that is a ledger. Narrow on purpose:
            // `timing` and others have their own `record`.
            if line.contains("ledger.record(") || line.contains("Ledger::record(") {
                out.push(format!("{}:{}", p.display(), i + 1));
            }
        }
    }
    out
}

#[test]
fn the_caveat_exists_exactly_while_nothing_records() {
    let callers = production_callers_of_record();
    match untracked() {
        Some(_) => assert!(
            callers.is_empty(),
            "`budget::untracked()` still says the ledger is not being written, and \
             these places now write it:\n  {}\n\nDelete `untracked`'s body (return \
             `None`) — the caveat is the thing that is now false.",
            callers.join("\n  ")
        ),
        None => assert!(
            !callers.is_empty(),
            "`budget::untracked()` says the spend ledger is being kept, and nothing \
             in src/ calls `Ledger::record`. The caps are then compared against a \
             total that is always zero, which means they bound one job at a time \
             and never a total."
        ),
    }
}

#[test]
fn an_empty_month_is_not_reported_as_having_spent_nothing() {
    let said = report(&Ledger::default(), &hosted_config(), 1_700_000_000);
    if untracked().is_some() {
        assert!(
            !said.contains("Nothing yet"),
            "an unwritten ledger is reported as a month with no spending in it: {said:?}"
        );
        assert!(
            said.contains("can't tell") || said.contains("nothing writes"),
            "the report does not say the total is unknown: {said:?}"
        );
        assert!(
            said.contains("ledger"),
            "the report does not say what is missing, so nobody can fix it: {said:?}"
        );
    }
}

#[test]
fn a_refusal_does_not_claim_a_total_it_never_measured() {
    // A job priced well over the cap, so the refusal arm is reached.
    let mut cfg = hosted_config();
    cfg.daily_cap = 0.01;
    let job = Job {
        what: "a long piece of research".into(),
        cached_input: 200_000,
        fresh_input: 200_000,
        expected_output: 50_000,
        can_wait: false,
    };
    let said = format!("{:?}", allow(&job, Difficulty::Hard, &Ledger::default(), &cfg, 1_700_000_000));
    if untracked().is_some() && said.contains("Refused") {
        assert!(
            !said.contains("$0.00 so far"),
            "the refusal states a spend-so-far that nothing has measured: {said}"
        );
    }
}

#[test]
fn the_local_tier_is_unaffected_by_any_of_this() {
    // So the caveat cannot be read as "the budget module is broken". Work
    // that never leaves the machine costs nothing and needs no ledger, and
    // that path is untouched.
    let cfg = BudgetConfig { enabled: true, ..BudgetConfig::default() };
    let job = Job {
        what: "a small question".into(),
        cached_input: 100,
        fresh_input: 100,
        expected_output: 100,
        can_wait: false,
    };
    let verdict = allow(&job, Difficulty::Trivial, &Ledger::default(), &cfg, 1_700_000_000);
    let said = format!("{verdict:?}");
    assert!(
        said.contains("Local") || said.contains("Go"),
        "a trivial job was refused: {said}"
    );
    // And the disabled case still says the plain true thing.
    let off = BudgetConfig { enabled: false, ..BudgetConfig::default() };
    assert!(report(&Ledger::default(), &off, 1_700_000_000).contains("your machine"));
}

#[test]
fn the_hosted_tiers_are_the_ones_that_need_a_ledger() {
    // Recorded here because it is the reason this matters at all: the caps
    // exist for the tiers that cost money, and those are the ones whose
    // running total is not being kept.
    assert!(!Tier::Local.is_hosted(), "the local tier is not hosted");
    assert!(
        [Tier::Haiku, Tier::Sonnet, Tier::Opus].iter().any(|t| t.is_hosted()),
        "no tier is hosted, so a spend ledger would have nothing to record"
    );
}
