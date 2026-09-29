//! Working while you're asleep.
//!
//! You go to bed, Atlas works through the things it couldn't finish, and in
//! the morning there's a brief and a set of changes waiting for you to accept
//! or throw away.
//!
//! The important design choice: **this doesn't care where the answers come
//! from.** A session works through problems using whatever brain it's been
//! given — the local model, a hosted one, or a queue of briefs for a person to
//! answer over coffee. Swapping that later changes one line of config and
//! nothing else.
//!
//! Two rules make unattended work safe rather than alarming:
//!
//! 1. **Nothing is applied while you're asleep.** Everything lands in the
//!    sandbox with its tests run. You accept in the morning.
//! 2. **It stops when it stops being useful** — a budget, a time limit, and a
//!    rule that repeated failure on the same problem ends that problem rather
//!    than burning the night on it.

use crate::handoff::Problem;
use serde::{Deserialize, Serialize};

/// Where answers come from. Atlas doesn't care which.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Brain {
    /// The model on your machine. Free, limited.
    Local,
    /// A hosted model, paid per use. See COST.md.
    Hosted,
    /// Write the brief and leave it for you. Free, and you answer in the
    /// morning.
    AskYouLater,
    /// Carry on an existing conversation in an app you already have open, the
    /// same way `delegate` does when you step out of the room.
    ///
    /// The bound is the turn budget, not your presence. It continues a thread
    /// that already exists; it does not open new ones.
    Delegate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Fixed, tests pass, waiting for you to accept.
    Solved,
    /// Tried and couldn't. Written up.
    Stuck,
    /// Ran out of time or budget before getting to it.
    NotReached,
    /// Deliberately left — it needs a decision only you can make.
    NeedsYou,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Result_ {
    pub problem: String,
    pub outcome: Outcome,
    pub attempts: u32,
    /// The change, waiting in the sandbox.
    pub sandbox_path: Option<String>,
    /// Tests that passed with the change in place.
    pub tests_passed: Option<usize>,
    /// One line, for the morning.
    pub note: String,
    /// What it cost, if anything.
    pub dollars: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct OvernightConfig {
    pub enabled: bool,
    pub brain: Brain,
    // `start_hour` and `stop_hour` were deleted on 19 Sep 2026, and deleted
    // rather than pinned because what they described is now *observed*.
    //
    // They were a clock window -- 23 to 6 -- and a clock cannot tell you
    // asleep from you at a desk at two in the morning, nor know you left for
    // work at eight. So the night ran on a timetable: it ran while you were
    // up working, and did not run on the Saturday you were out all day.
    //
    // `daily::whereabouts` answers the question they were standing in for:
    // are you gone, and gone long enough that an hour of work will not be
    // interrupted. `daily.away_a_while_minutes` is the one number left to
    // set, and it is about *you* rather than about the clock.
    //
    // Not moved to PROMISES_ABOUT_WHAT_IS_NOT_BUILT: that list is for
    // decisions recorded ahead of a capability that does not exist. This
    // capability exists and works better without them.
    /// Attempts on one problem before moving on. Wandering on a hard problem
    /// costs the whole night.
    pub attempts_each: u32,
    /// Never work on more than this in one night.
    pub max_problems: usize,
    /// Stop the night if this many *separate problems* in a row get nowhere.
    ///
    /// Not the same as attempts on one problem — that's the twelve-angle
    /// ladder in `strategy.rs`. This is about the night as a whole: three
    /// different problems all going nowhere means something is wrong with the
    /// setup, not that the problems are hard.
    pub give_up_after_failures: u32,
    /// Nothing is ever applied unattended. Not configurable.
    #[serde(skip, default = "never")]
    pub apply_while_asleep: bool,
    /// The window to carry on in, when `brain` is `delegate`. A window that
    /// already has the conversation in it.
    pub delegate_window: String,
    /// Turns per problem in that window. This is the actual bound on the
    /// whole arrangement, so it is deliberately small.
    pub delegate_turns_each: u32,
    /// Total turns across the night, whatever the per-problem budget allows.
    pub delegate_turns_total: u32,
}

fn never() -> bool {
    false
}

impl Default for OvernightConfig {
    fn default() -> Self {
        OvernightConfig {
            enabled: true,
            brain: Brain::AskYouLater,
            attempts_each: 3,
            max_problems: 10,
            give_up_after_failures: 3,
            apply_while_asleep: false,
            delegate_window: String::new(),
            delegate_turns_each: 6,
            delegate_turns_total: 40,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Session {
    pub started_at: u64,
    pub ended_at: Option<u64>,
    /// Why the night stopped, kept for the morning.
    ///
    /// `Step::Finish` and `Step::Abandon` both carry a reason, and until the
    /// night had a caller there was nowhere for it to go. It matters: "worked
    /// through everything" and "three in a row got nowhere, so I stopped" are
    /// very different mornings, and without this the brief read the same
    /// either way.
    pub ended_because: Option<String>,
    pub results: Vec<Result_>,
    consecutive_failures: u32,
    pub spent: f64,
    /// Turns used in the delegated window, across the whole night.
    pub delegate_turns_used: u32,
}

/// What the session should do next.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// Work on this one.
    Work(String),
    /// Nothing left, or time to stop.
    Finish(String),
    /// Stop early — something is wrong beyond the individual problems.
    Abandon(String),
}

impl Session {
    pub fn start(t: u64) -> Session {
        Session { started_at: t, ..Default::default() }
    }

    pub fn record(&mut self, r: Result_) {
        if r.outcome == Outcome::Stuck {
            self.consecutive_failures += 1;
        } else if r.outcome == Outcome::Solved {
            self.consecutive_failures = 0;
        }
        self.spent += r.dollars;
        self.results.push(r);
    }

    /// Decide whether to keep going.
    pub fn next(
        &self,
        queue: &[String],
        cfg: &OvernightConfig,
        where_you_are: crate::daily::Whereabouts,
        budget_left: f64,
    ) -> Step {
        // The same question the caller asked to start the night, asked again
        // each step. Taking an hour here and a `Whereabouts` there is how the
        // two halves come to disagree -- and they would have: Atlas would
        // have begun work at two in the afternoon because you were out, then
        // stopped on the next step because two in the afternoon is not
        // between 23 and 6.
        if !where_you_are.free_to_work() {
            return Step::Finish("you came back".into());
        }
        // Several failures in a row means the setup is wrong, not that the
        // problems are hard. Carrying on wastes the night and the money.
        if self.consecutive_failures >= cfg.give_up_after_failures {
            return Step::Abandon(format!(
                "{} in a row got nowhere, so I stopped rather than keep trying",
                self.consecutive_failures
            ));
        }
        if self.results.len() >= cfg.max_problems {
            return Step::Finish("did as many as I set out to".into());
        }
        if cfg.brain == Brain::Hosted && budget_left <= 0.0 {
            return Step::Finish("ran out of budget".into());
        }
        if cfg.brain == Brain::Delegate {
            if cfg.delegate_window.trim().is_empty() {
                return Step::Abandon("no window to carry on in".into());
            }
            // The turn budget is the whole bound. Running past it turns a
            // delegated conversation into something else.
            if self.delegate_turns_used >= cfg.delegate_turns_total {
                return Step::Finish(format!(
                    "used the {} turns you allowed",
                    cfg.delegate_turns_total
                ));
            }
        }
        let done: Vec<&String> = self.results.iter().map(|r| &r.problem).collect();
        match queue.iter().find(|p| !done.contains(p)) {
            Some(p) => Step::Work(p.clone()),
            None => Step::Finish("worked through everything".into()),
        }
    }

    /// Start a delegated continuation for one problem, bounded by whatever
    /// budget is left for the night.
    pub fn delegation_for(
        &self,
        problem: &str,
        cfg: &OvernightConfig,
    ) -> Option<crate::delegate::Delegation> {
        if cfg.brain != Brain::Delegate || cfg.delegate_window.trim().is_empty() {
            return None;
        }
        let left = cfg.delegate_turns_total.saturating_sub(self.delegate_turns_used);
        if left == 0 {
            return None;
        }
        Some(
            crate::delegate::Delegation::new(
                &cfg.delegate_window,
                problem,
                crate::delegate::Reach::Converse,
                cfg.delegate_turns_each.min(left),
            )
            .stopping_on(&["that should fix it", "let me know how that goes", "does that work"]),
        )
    }

    pub fn spend_turns(&mut self, n: u32) {
        self.delegate_turns_used = self.delegate_turns_used.saturating_add(n);
    }

    fn solved(&self) -> Vec<&Result_> {
        self.results.iter().filter(|r| r.outcome == Outcome::Solved).collect()
    }
    pub fn stuck(&self) -> Vec<&Result_> {
        self.results.iter().filter(|r| r.outcome == Outcome::Stuck).collect()
    }
    pub fn needs_you(&self) -> Vec<&Result_> {
        self.results.iter().filter(|r| r.outcome == Outcome::NeedsYou).collect()
    }
}

/// The morning brief.
///
/// Leads with what's waiting for a decision, because that's the only part that
/// needs you. A list of everything it did is a report; this is a handover.
/// The night's work as steps, so the report can be checked against it.
///
/// `Solved` is the only outcome that backs a claim, and even then only of the
/// change existing — not of it being applied. Everything else is something
/// that did not happen, and the mapping says so rather than letting a count
/// of successes stand in for the whole night.
fn steps_of(s: &Session) -> Vec<crate::faithful::Step> {
    s.results
        .iter()
        .map(|r| crate::faithful::Step {
            what: r.problem.clone(),
            outcome: match r.outcome {
                Outcome::Solved => crate::faithful::Outcome::Did,
                Outcome::Stuck => crate::faithful::Outcome::Failed(r.note.clone()),
                Outcome::NotReached => {
                    crate::faithful::Outcome::Skipped("ran out of time or budget".into())
                }
                Outcome::NeedsYou => {
                    crate::faithful::Outcome::Skipped("needs a decision from you".into())
                }
            },
        })
        .collect()
}

/// The write-up for the stretch of work that just ended.
///
/// `while_you_were` says where you actually were, because since 19 Sep 2026
/// the night's work no longer runs on a clock — it runs whenever you have
/// been gone long enough, which on a Tuesday is the six hours you spent at
/// the office. Calling that "overnight" is a small lie that makes you trust
/// the rest of the report less.
pub fn morning_brief(s: &Session, while_you_were: crate::daily::Whereabouts) -> String {
    let when = while_you_were.plain();
    if s.results.is_empty() {
        return format!("Nothing to report — I didn't get to anything {when}.");
    }
    let solved = s.solved();
    let stuck = s.stuck();
    let needs = s.needs_you();

    let mut parts: Vec<String> = Vec::new();

    if !solved.is_empty() {
        let names: Vec<&str> = solved.iter().take(3).map(|r| r.problem.as_str()).collect();
        parts.push(format!(
            "{} fixed and waiting for you to look at: {}{}",
            solved.len(),
            names.join(", "),
            if solved.len() > 3 { format!(", and {} more", solved.len() - 3) } else { String::new() }
        ));
    }
    if !needs.is_empty() {
        parts.push(format!(
            "{} needs a decision from you: {}",
            needs.len(),
            needs[0].problem
        ));
    }
    if !stuck.is_empty() {
        parts.push(format!("{} I couldn't work out — written up", stuck.len()));
    }
    if s.spent > 0.0 {
        parts.push(format!("cost ${:.2}", s.spent));
    }

    // Ordered by what happened, not by what reads best. This used to open
    // with "N fixed" and close with "N I couldn't work out", which is the
    // shape of a report that has disclosed a problem without communicating
    // it — the reader stops before the last clause.
    // Capitalised from `plain()`'s own words rather than a second string,
    // so the two cannot drift into saying different things.
    let mut heading: String = when.to_string();
    if let Some(first) = heading.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    let drafted = format!("{heading}: {}.", parts.join(", "));
    let mut out = crate::faithful::lead_with_the_problem(&steps_of(s), &drafted);
    // The reminder that matters: nothing has actually changed yet.
    if !solved.is_empty() {
        out.push_str(" Nothing's been applied — say the word and I'll put them in.");
    }
    out
}

/// The full account, for reading rather than hearing.
pub fn morning_detail(s: &Session) -> String {
    let mut out = String::from("Overnight session\n\n");
    for r in &s.results {
        let mark = match r.outcome {
            Outcome::Solved => "fixed",
            Outcome::Stuck => "stuck",
            Outcome::NeedsYou => "needs you",
            Outcome::NotReached => "not reached",
        };
        out.push_str(&format!("[{mark}] {}\n", r.problem));
        out.push_str(&format!("    {} after {} attempt{}\n", r.note, r.attempts,
            if r.attempts == 1 { "" } else { "s" }));
        if let Some(n) = r.tests_passed {
            out.push_str(&format!("    {n} tests pass with the change in\n"));
        }
        if let Some(p) = &r.sandbox_path {
            out.push_str(&format!("    waiting in {p}\n"));
        }
    }
    if s.spent > 0.0 {
        out.push_str(&format!("\nSpent ${:.2}.\n", s.spent));
    }
    out.push_str("\nNothing has been applied to your machine.\n");
    out
}

/// Which problems are worth an unattended night.
///
/// Not everything is. Anything needing a judgement call, or that would change
/// something outside the sandbox, is better left for when you're there.
/// The one line the morning brief carries for a problem the night reached.
///
/// Written here rather than at the call site so that what the brief says
/// stays with the module that decides what a night is. Under the
/// `ask_you_later` brain -- the shipped default, free and offline -- reaching
/// a problem means writing it up, so the note says exactly that rather than
/// implying an attempt that never happened.
pub fn note_for(problem: &str, cfg: &OvernightConfig) -> String {
    match cfg.brain {
        Brain::AskYouLater => {
            format!("left for you: {problem}")
        }
        // The other brains are not wired yet. If one is selected in config,
        // the night still runs and the brief says plainly that it did not
        // attempt anything, rather than reporting silence as success.
        _ => format!("{problem} -- I only have the write-it-up brain wired, so I left it"),
    }
}

pub fn worth_doing_overnight(p: &Problem) -> bool {
    let t = format!("{} {}", p.goal, p.theory.clone().unwrap_or_default()).to_lowercase();
    const NEEDS_YOU: &[&str] = &[
        "which", "prefer", "should i", "decide", "choose", "design",
        "delete", "remove permanently", "send", "post", "publish", "pay",
    ];
    !NEEDS_YOU.iter().any(|w| t.contains(w))
}
