//! Spending as little as possible on a hosted model.
//!
//! Atlas works locally and free. A hosted model is only for the one thing a
//! 3B can't do: writing real code against a large codebase. This module makes
//! that as cheap as it can be, and makes it impossible to be surprised by a
//! bill.
//!
//! Four mechanisms, and they stack:
//!
//! 1. **Try local first.** Most tasks never reach a hosted model at all.
//! 2. **Cheapest model that can do it.** Reading a diff is not the same job as
//!    designing a change.
//! 3. **Cache the codebase.** The source barely changes between requests, and
//!    a cache read costs a tenth of a fresh one.
//! 4. **Batch overnight work.** Half price for anything that can wait, which
//!    is exactly what "while I sleep" means.
//!
//! On top of that, a hard spending cap that stops rather than warns.
//!
//! Rates are per million tokens and are checked against
//! <https://platform.claude.com/docs/en/about-claude/pricing>; they change, so
//! they live in config rather than in the code.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// Runs on your machine. Free.
    Local,
    /// Cheap and quick — reading, classifying, summarising.
    Haiku,
    /// The workhorse for real code.
    Sonnet,
    /// Only when something genuinely defeats Sonnet.
    Opus,
}

impl Tier {
    pub fn is_hosted(&self) -> bool {
        *self != Tier::Local
    }
    pub fn name(&self) -> &'static str {
        match self {
            Tier::Local => "the local model",
            Tier::Haiku => "Haiku",
            Tier::Sonnet => "Sonnet",
            Tier::Opus => "Opus",
        }
    }
}

/// Dollars per million tokens.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
pub struct Rate {
    pub input: f64,
    pub output: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct BudgetConfig {
    /// Hosted models are off until you turn them on.
    pub enabled: bool,
    /// Hard stop, in dollars. Not a warning — a stop.
    pub daily_cap: f64,
    pub monthly_cap: f64,
    /// The most expensive tier Atlas may reach for on its own.
    pub ceiling: Tier,
    /// Anything that can wait goes through the batch API at half price.
    pub prefer_batch: bool,
    /// Reuse the cached codebase rather than resending it.
    pub use_caching: bool,
    pub haiku: Rate,
    pub sonnet: Rate,
    pub opus: Rate,
    /// A cache read costs this fraction of a fresh input token.
    pub cache_read_fraction: f64,
    /// Writing to the cache costs a little more than a plain input token.
    pub cache_write_multiplier: f64,
    pub batch_multiplier: f64,
}

impl Default for BudgetConfig {
    fn default() -> Self {
        BudgetConfig {
            enabled: false,
            daily_cap: 1.0,
            monthly_cap: 15.0,
            ceiling: Tier::Sonnet,
            prefer_batch: true,
            use_caching: true,
            haiku: Rate { input: 1.0, output: 5.0 },
            sonnet: Rate { input: 2.0, output: 10.0 },
            opus: Rate { input: 5.0, output: 25.0 },
            cache_read_fraction: 0.10,
            cache_write_multiplier: 1.25,
            batch_multiplier: 0.50,
        }
    }
}

impl BudgetConfig {
    pub fn rate(&self, t: Tier) -> Option<Rate> {
        match t {
            Tier::Local => None,
            Tier::Haiku => Some(self.haiku),
            Tier::Sonnet => Some(self.sonnet),
            Tier::Opus => Some(self.opus),
        }
    }
}

/// One piece of work, sized.
#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub what: String,
    /// Tokens of context that stay the same between requests — the codebase.
    pub cached_input: u64,
    /// Tokens that differ each time — the instruction, the failing test.
    pub fresh_input: u64,
    pub expected_output: u64,
    /// Can this wait until morning?
    pub can_wait: bool,
}

/// What a job would cost, in dollars.
pub fn estimate(job: &Job, tier: Tier, cfg: &BudgetConfig) -> f64 {
    let Some(rate) = cfg.rate(tier) else { return 0.0 };
    let m = 1_000_000.0;

    let (cached_cost, fresh) = if cfg.use_caching && job.cached_input > 0 {
        (job.cached_input as f64 / m * rate.input * cfg.cache_read_fraction, job.fresh_input)
    } else {
        (0.0, job.cached_input + job.fresh_input)
    };

    let cost = cached_cost
        + fresh as f64 / m * rate.input
        + job.expected_output as f64 / m * rate.output;

    if cfg.prefer_batch && job.can_wait {
        cost * cfg.batch_multiplier
    } else {
        cost
    }
}

/// What the first run costs, before the cache exists.
///
/// Worth knowing separately: the first request of the night is several times
/// the price of the rest, and a plan that ignores that underestimates by a
/// lot.
pub fn first_run_estimate(job: &Job, tier: Tier, cfg: &BudgetConfig) -> f64 {
    let Some(rate) = cfg.rate(tier) else { return 0.0 };
    let m = 1_000_000.0;
    let write = job.cached_input as f64 / m * rate.input * cfg.cache_write_multiplier;
    let cost = write
        + job.fresh_input as f64 / m * rate.input
        + job.expected_output as f64 / m * rate.output;
    if cfg.prefer_batch && job.can_wait {
        cost * cfg.batch_multiplier
    } else {
        cost
    }
}

/// How hard the task is, judged before spending anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Difficulty {
    /// A config value, a phrase, a threshold. The local model handles it.
    Trivial,
    /// Reading a diff, summarising a failure, classifying.
    Simple,
    /// Writing or changing real code.
    Real,
    /// Design work across many files.
    Hard,
}

impl Difficulty {
    /// Said, rather than debug-printed.
    pub fn plain(&self) -> &'static str {
        match self {
            Difficulty::Trivial => "a config value or a phrase",
            Difficulty::Simple => "reading, summarising or classifying",
            Difficulty::Real => "writing or changing real code",
            Difficulty::Hard => "design work across many files",
        }
    }
}

/// Pick the cheapest tier that can actually do the job.
pub fn route(d: Difficulty, cfg: &BudgetConfig) -> Tier {
    if !cfg.enabled {
        return Tier::Local;
    }
    let wanted = match d {
        Difficulty::Trivial => Tier::Local,
        Difficulty::Simple => Tier::Haiku,
        Difficulty::Real => Tier::Sonnet,
        Difficulty::Hard => Tier::Opus,
    };
    // Never above the ceiling you set, whatever Atlas thinks it needs.
    wanted.min(cfg.ceiling)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spend {
    pub at: u64,
    pub tier: Tier,
    pub dollars: f64,
    pub what: String,
    pub batched: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Ledger {
    pub spends: Vec<Spend>,
}

const DAY: u64 = 86_400;
const MONTH: u64 = 30 * DAY;

impl Ledger {
    pub fn record(&mut self, s: Spend) {
        self.spends.push(s);
        if self.spends.len() > 2000 {
            let drop = self.spends.len() - 2000;
            self.spends.drain(0..drop);
        }
    }

    fn spent_since(&self, since: u64) -> f64 {
        // The `+ 0.0` is not decoration. Rust's float `Sum` uses `-0.0` as its
        // identity, so summing no spends at all gives negative zero, and
        // `{:.2}` renders that as "-0.00" — a ledger with nothing in it
        // reporting that you are owed money. Found the first time anything
        // printed this, which was the day it was wired up.
        let total: f64 = self.spends.iter().filter(|s| s.at >= since).map(|s| s.dollars).sum();
        total + 0.0
    }

    pub fn today(&self, now: u64) -> f64 {
        self.spent_since(now.saturating_sub(DAY))
    }
    pub fn this_month(&self, now: u64) -> f64 {
        self.spent_since(now.saturating_sub(MONTH))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Approval {
    /// Go ahead, and what it will cost.
    Go { tier: Tier, dollars: f64, batched: bool },
    /// Do it locally instead.
    Local(String),
    /// Stop. The cap is a stop, not a warning.
    Refused(String),
}

/// The gate every hosted call goes through.
pub fn allow(job: &Job, d: Difficulty, ledger: &Ledger, cfg: &BudgetConfig, now: u64) -> Approval {
    let tier = route(d, cfg);
    if !tier.is_hosted() {
        return Approval::Local("the local model can handle this".into());
    }

    let dollars = estimate(job, tier, cfg);
    let today = ledger.today(now);
    let month = ledger.this_month(now);

    // "Spent $X so far" is only true of spends that were actually recorded.
    // See `untracked()`: nothing in production records, so on a real machine
    // the ledger is empty and the sentence would state a total nobody
    // measured.
    //
    // Keyed on the ledger being empty rather than on `untracked()` alone,
    // which is the correction to the first version of this. A caller that
    // *has* put spends in the ledger has measured something, and refusing to
    // report it — "I'm not counting what has gone out" said about six
    // recorded spends — is the same kind of false statement pointing the
    // other way. The caveat belongs to an empty file, not to the absence of
    // an automatic writer.
    let so_far = |amount: f64| match (untracked(), ledger.spends.is_empty()) {
        (Some(_), true) => {
            "And I can't tell you what has already gone out today: nothing writes to \
             the spend ledger, so the running total is always zero."
                .to_string()
        }
        (Some(_), false) => {
            format!("Spent ${amount:.2} so far, counting only what was recorded.")
        }
        (None, _) => format!("Spent ${amount:.2} so far."),
    };
    if today + dollars > cfg.daily_cap {
        return Approval::Refused(format!(
            "that would take today past ${:.2}. {}",
            cfg.daily_cap,
            so_far(today)
        ));
    }
    if month + dollars > cfg.monthly_cap {
        return Approval::Refused(format!(
            "that would take this month past ${:.2}. {}",
            cfg.monthly_cap,
            so_far(month)
        ));
    }
    Approval::Go { tier, dollars, batched: cfg.prefer_batch && job.can_wait }
}

/// What a night's work would cost, so you can decide before it happens.
pub fn overnight_estimate(jobs: &[Job], d: Difficulty, cfg: &BudgetConfig) -> (f64, String) {
    let tier = route(d, cfg);
    if !tier.is_hosted() || jobs.is_empty() {
        return (0.0, "nothing to pay for".into());
    }
    // The first request pays to fill the cache; the rest read from it.
    let first = jobs.first().map(|j| first_run_estimate(j, tier, cfg)).unwrap_or(0.0);
    let rest: f64 = jobs.iter().skip(1).map(|j| estimate(j, tier, cfg)).sum();
    let total = first + rest;

    let note = format!(
        "{} tasks on {}{}{}. About ${total:.2}.",
        jobs.len(),
        tier.name(),
        if cfg.use_caching { ", codebase cached" } else { "" },
        if cfg.prefer_batch && jobs.iter().all(|j| j.can_wait) { ", overnight rate" } else { "" }
    );
    (total, note)
}

/// A plain answer to "what is this costing me?"
pub fn report(ledger: &Ledger, cfg: &BudgetConfig, now: u64) -> String {
    if !cfg.enabled {
        return "Nothing — everything runs on your machine.".into();
    }
    let today = ledger.today(now);
    let month = ledger.this_month(now);
    if month == 0.0 {
        // "Nothing yet this month" is only a measurement if something was
        // measured. On an EMPTY ledger it is not: `Ledger::record` has no
        // caller anywhere in `src/`, and nothing saves the `budget_ledger`
        // record that `atlas budget` loads, so the month is zero on a machine
        // that has spent nothing and on a machine that has spent four hundred
        // dollars. The two are indistinguishable from here, and the sentence
        // reads as a measurement of your spending while being a measurement
        // of an empty file.
        //
        // A ledger with spends in it that fall outside this month is the
        // other case, and there "nothing yet this month" is exactly true of
        // what was recorded. Hence the `is_empty` rather than a flat
        // `untracked()` check — see the same correction in `allow`.
        //
        // See `untracked()` for the sentence and for what closes it.
        return match untracked() {
            Some(why) if ledger.spends.is_empty() => format!("I can't tell you. {why}"),
            Some(_) => "Nothing yet this month, of what was recorded.".into(),
            None => "Nothing yet this month.".into(),
        };
    }
    format!(
        "${today:.2} today, ${month:.2} this month. Cap is ${:.2} a month.",
        cfg.monthly_cap
    )
}

/// Why the running total cannot be trusted, while it cannot.
///
/// `None` the moment something records into the ledger. Until then this is
/// the honest answer to "what have I spent", and
/// `tests/the_budget_knows_what_it_does_not_know.rs` fails if the note and
/// the wiring ever disagree — so whoever wires the recorder is told to delete
/// this.
///
/// ## What is actually missing
///
/// Everything else in this module is built and correct: `estimate` prices a
/// job from the configured per-token rates, `route` picks a tier, `allow`
/// compares a prospective spend against the caps, and `a_night` costs a
/// night's work before it happens. What does not exist is the one line at
/// the end of a hosted model call that turns what it cost into a `Spend` and
/// hands it to `Ledger::record`.
///
/// Until that line exists the caps bind on **one job at a time** and never
/// on a total, which is the opposite of what a daily cap is for.
pub fn untracked() -> Option<&'static str> {
    Some(
        "nothing writes to the spend ledger yet, so the running total is always \
         zero — the caps can still refuse a single job that is too big on its own, \
         but they are not counting what has already gone out. Wire `Ledger::record` \
         at the end of a hosted model call and this becomes a real number.",
    )
}
