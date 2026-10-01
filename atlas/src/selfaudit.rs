//! Atlas looking at itself and deciding what to fix.
//!
//! The pipeline needs a diagnosis before it will do anything, and until now
//! that diagnosis had to come from you. That's the wrong way round: you are
//! the person least able to see which of Atlas's own routes keep failing, or
//! which answers you keep correcting.
//!
//! So this is the stage before Thought — noticing. It reads what Atlas already
//! records about itself, turns the strongest signals into diagnoses, and hands
//! them to the pipeline in the shape it demands: a symptom, a cause, where it
//! is, and something that would prove it fixed.
//!
//! **It never proposes work on the strength of one observation.** One failed
//! route is a bad day; the same route failing eleven times is a fault.

use serde::{Deserialize, Serialize};

/// Something Atlas can see about itself, without being told.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Signal {
    pub kind: Kind,
    /// What it's about — a route, a module, an intent.
    pub subject: String,
    /// How many times.
    pub seen: u32,
    /// Out of how many chances.
    pub of: u32,
    /// The most recent example, in your words.
    pub example: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A route that keeps failing.
    RouteFails,
    /// You keep correcting the same answer.
    YouKeepCorrecting,
    /// Something takes far longer than it used to.
    GotSlower,
    /// An intent that reaches Unknown a lot.
    NotUnderstood,
    /// A capability that says it works and has never been used.
    NeverUsed,
    /// The same question asked repeatedly, meaning the answer didn't stick.
    AskedAgain,
    /// A test that has never failed since it was written.
    NeverFailed,
    /// Atlas's own self-test found a command broken (`regressions`).
    SelfTestFails,
}

impl Kind {
    /// How many times before it's a fault rather than a bad day.
    ///
    /// Different for each, because the cost of being wrong differs. Acting on
    /// one correction would make Atlas skittish; ignoring eleven failures
    /// makes it useless.
    pub fn enough(&self) -> u32 {
        match self {
            Kind::RouteFails => 5,
            Kind::YouKeepCorrecting => 3,
            Kind::GotSlower => 4,
            Kind::NotUnderstood => 6,
            // A capability claiming to work and never once used is worth one
            // look — but it's the weakest signal here, so it never leads.
            Kind::NeverUsed => 1,
            Kind::AskedAgain => 3,
            Kind::NeverFailed => 1,
            // A run on a copy of the install that broke is a fault already.
            Kind::SelfTestFails => 1,
        }
    }

    /// Where the cause usually is, which is what turns a signal into a
    /// diagnosis rather than a complaint.
    fn usually_because(&self) -> &'static str {
        match self {
            Kind::RouteFails => "the route's preconditions aren't being checked before it's chosen",
            Kind::YouKeepCorrecting => "the rule that produces this answer is fitted to the wrong signal",
            Kind::GotSlower => "something is being recomputed that used to be cached",
            Kind::NotUnderstood => "the phrase list doesn't cover how you actually say it",
            Kind::NeverUsed => "it isn't reachable, or nothing points at it",
            Kind::AskedAgain => "the answer isn't being kept, so it's derived again each time",
            Kind::NeverFailed => "it asserts something that was always true",
            Kind::SelfTestFails => "what the command does was changed without the self-test being run against it",
        }
    }

    /// How much it's worth fixing, before evidence.
    pub fn weight(&self) -> f32 {
        match self {
            Kind::YouKeepCorrecting => 1.0,
            Kind::RouteFails => 0.9,
            Kind::NotUnderstood => 0.8,
            Kind::AskedAgain => 0.7,
            Kind::GotSlower => 0.6,
            Kind::SelfTestFails => 0.85,
            Kind::NeverFailed => 0.4,
            Kind::NeverUsed => 0.3,
        }
    }
}

impl Signal {
    /// Enough to act on?
    pub fn is_a_fault(&self) -> bool {
        self.seen >= self.kind.enough()
    }

    /// How bad, from how often it happens rather than how often it's seen.
    ///
    /// Five failures out of six is a fault; five out of five hundred is
    /// weather.
    pub fn rate(&self) -> f32 {
        if self.of == 0 {
            return 0.0;
        }
        self.seen as f32 / self.of as f32
    }

    pub fn worth(&self) -> f32 {
        self.kind.weight() * self.rate()
    }
}

/// What Atlas proposes doing about it.
#[derive(Debug, Clone, PartialEq)]
pub struct Recommendation {
    /// The diagnosis, in the shape the pipeline demands.
    pub symptom: String,
    pub cause: String,
    pub where_: String,
    pub proof: String,
    /// How sure, from the evidence rather than from confidence.
    pub certainty: f32,
    /// What it's based on, so you can disagree with the evidence rather than
    /// the conclusion.
    pub because: String,
}

/// Turn signals into diagnoses.
///
/// Only the ones that clear their own bar, ranked, and never more than a
/// handful — a list of thirty recommendations is a list nobody reads.
pub fn recommend(signals: &[Signal], most: usize) -> Vec<Recommendation> {
    let mut faults: Vec<&Signal> = signals.iter().filter(|s| s.is_a_fault()).collect();
    faults.sort_by(|a, b| b.worth().partial_cmp(&a.worth()).unwrap_or(std::cmp::Ordering::Equal));

    faults
        .into_iter()
        .take(most)
        .map(|s| Recommendation {
            symptom: symptom_for(s),
            cause: s.kind.usually_because().to_string(),
            where_: s.subject.clone(),
            proof: proof_for(s),
            // Confidence comes from the rate, not from the count. Something
            // that fails every time is a clearer fault than something that
            // fails often.
            certainty: (s.rate() * 0.7 + s.kind.weight() * 0.3).min(0.95),
            because: format!("{} times out of {} — most recently {}", s.seen, s.of, s.example),
        })
        .collect()
}

fn symptom_for(s: &Signal) -> String {
    match s.kind {
        Kind::RouteFails => format!("{} keeps failing", s.subject),
        Kind::YouKeepCorrecting => format!("you keep correcting what I say about {}", s.subject),
        Kind::GotSlower => format!("{} has got slower", s.subject),
        Kind::NotUnderstood => format!("I keep not understanding {}", s.subject),
        Kind::NeverUsed => format!("there's {} in two weeks", s.subject),
        Kind::AskedAgain => format!("you keep asking me {} again", s.subject),
        Kind::NeverFailed => format!("the test for {} has never once failed", s.subject),
        Kind::SelfTestFails => format!("my self-test found {} broken", s.subject),
    }
}

fn proof_for(s: &Signal) -> String {
    match s.kind {
        Kind::RouteFails => format!("a test that {} refuses to be chosen without its preconditions", s.subject),
        Kind::YouKeepCorrecting => format!("a test asserting the corrected answer for {}", s.subject),
        Kind::GotSlower => format!("a test that {} finishes inside its old budget", s.subject),
        Kind::NotUnderstood => format!("a test that the phrasings you actually use reach {}", s.subject),
        Kind::NeverUsed => format!("a test that calls {} and gets something back", s.subject),
        Kind::AskedAgain => format!("a test that the answer to {} is kept and reused", s.subject),
        Kind::NeverFailed => format!("break {} deliberately and watch the test fail", s.subject),
        Kind::SelfTestFails => format!("a test that \"{}\" passes the self-test's judgement again", s.example),
    }
}

/// What Atlas says about its own state.
///
/// One thing, with the evidence, and an offer. Not a report — a report is what
/// you write when you don't intend to fix anything.
pub fn spoken(recs: &[Recommendation]) -> String {
    match recs.first() {
        None => "Nothing about myself I'd change.".into(),
        Some(r) => {
            let mut s = format!("{} — I think {}. {}.", r.symptom, r.cause, r.because);
            if recs.len() > 1 {
                s.push_str(&format!(" {} other things.", recs.len() - 1));
            }
            s.push_str(" Want me to have a go?");
            s
        }
    }
}

/// The strongest recommendation, as a pipeline diagnosis.
///
/// The whole point: it goes straight into the loop rather than into a
/// document.
pub fn as_thought(r: &Recommendation) -> crate::pipeline::Thought {
    crate::pipeline::Thought {
        symptom: r.symptom.clone(),
        cause: r.cause.clone(),
        where_: r.where_.clone(),
        proof: r.proof.clone(),
        // Never assumed. The pipeline checks this itself, and a diagnosis
        // Atlas wrote is not more trustworthy than one you did.
        proof_fails_now: false,
        not_doing: Vec::new(),
    }
}

// ============ looking without being asked ============
//
// Both `every_days` and `act_without_asking` were settings a person could
// change and get nothing from, and for the same reason: the only caller of
// `recommend` was the `Intent::WorkOnYourself` branch, which runs because you
// asked. "How often to look" described a looking that never happened, and
// "act on the best one without asking" described an asking that was the only
// way in.
//
// `every_days` is a cadence in days, and a daemon is restarted far more often
// than that, so the clock is kept in the store rather than in memory —
// otherwise the setting would still do nothing, one layer down.

/// When Atlas last looked at itself.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LastLook {
    pub at: u64,
}

/// Where that is kept.
pub const LOOK_RECORD: &str = "self_audit_last_look";

/// Is it time to look again?
///
/// Named `time_to_look` rather than `due`: `brief` already has a free
/// function called `due`, and a second one would leave the deadness scan
/// unable to tell which of them an unqualified call reached.
pub fn time_to_look(cfg: &SelfAuditConfig, last: u64, now: u64) -> bool {
    if !cfg.enabled || cfg.every_days == 0 {
        return false;
    }
    // No record means it has never looked, which is overdue rather than
    // recent. A record from the future — a clock that moved back, which
    // happens on a laptop that has been asleep across a timezone — reads as
    // due rather than as a lockout lasting until the clock catches up.
    if last == 0 || last > now {
        return true;
    }
    now.saturating_sub(last) >= cfg.every_days as u64 * 86_400
}

/// What a look nobody asked for turns into.
#[derive(Debug, Clone, PartialEq)]
pub struct Unprompted {
    /// What to say.
    pub said: String,
    /// The goal to start on, when Atlas has been told not to ask first.
    pub goal: Option<String>,
    /// The diagnosis already worked out, so the work starts from it rather
    /// than asking the four questions it already has answers to (Eric, E2).
    pub thought: Option<crate::pipeline::Thought>,
}

/// What to do with what it found, having looked on its own.
///
/// `None` when there is nothing worth saying. A scheduled check that reports
/// "nothing about myself I'd change" every week is a weekly interruption
/// carrying no information, and the fastest way to teach someone to ignore
/// the one week it matters.
pub fn unprompted(cfg: &SelfAuditConfig, recs: &[Recommendation]) -> Option<Unprompted> {
    let top = recs.first()?;
    if !cfg.act_without_asking {
        return Some(Unprompted { said: spoken(recs), goal: None, thought: None });
    }
    // Told not to ask. It still says what it is doing and how to stop it —
    // acting without asking is not the same as acting without saying, and a
    // system that diagnoses itself and then acts on the diagnosis has no
    // outside check on either half unless you can hear it happening.
    Some(Unprompted {
        said: format!(
            "{} — I think {}. {}. You've told me not to ask, so I'm having a go at it now. \
             Say stop and I'll leave it.",
            top.symptom, top.cause, top.because
        ),
        goal: Some(top.symptom.clone()),
        thought: Some(as_thought(top)),
    })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SelfAuditConfig {
    pub enabled: bool,
    /// How often to look, in days.
    pub every_days: u32,
    /// Most recommendations at once.
    pub most_at_once: usize,
    /// Act on the best one without asking.
    ///
    /// Off, and the default matters: a system that diagnoses itself and then
    /// acts on the diagnosis has no outside check on either half.
    pub act_without_asking: bool,
}

impl Default for SelfAuditConfig {
    fn default() -> Self {
        SelfAuditConfig {
            enabled: false,
            every_days: 7,
            // Three. A list of thirty is a list nobody reads.
            most_at_once: 3,
            act_without_asking: false,
        }
    }
}

/// Why Atlas doesn't just fix what it finds.
pub const WHY_IT_ASKS: &str =
    "I can see which of my own routes keep failing better than you can, and I'm worse than you at \
     knowing whether fixing one is worth the risk. A system that writes its own diagnosis and \
     then acts on it has no outside check on either half — so I bring you the evidence and you \
     decide.";
