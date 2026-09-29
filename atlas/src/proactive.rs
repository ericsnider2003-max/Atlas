//! Proactive assistance.
//!
//! The whole difficulty here is not detecting opportunities — it is *not*
//! taking most of them. An assistant that speaks whenever it notices something
//! is Clippy, and you will turn it off within a day. So every offer must clear
//! four independent bars, and the engine learns to stop offering things you
//! decline.
//!
//! Hard rule: proactive assistance **offers**, it never acts. Autonomous
//! execution comes from the scheduler, where you put it deliberately.

use crate::awareness::Signals;
use crate::memory::Memory;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Offer {
    /// Stable identifier used to learn acceptance per kind of offer.
    pub kind: String,
    /// Spoken aloud. One sentence, phrased as a question.
    pub message: String,
    /// The command to run if you say yes.
    pub command: String,
    pub confidence: f32,
    /// 0 = free to interrupt, 3 = you are clearly deep in something.
    pub cost: u8,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ProactiveConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Minimum seconds between any two offers.
    #[serde(default = "d_cooldown")]
    pub cooldown_secs: u64,
    /// Cap on how often Atlas may speak *first*, unprompted, per hour.
    ///
    /// This is not a resource quota and has nothing to do with any external
    /// service — Atlas is entirely local. It is an interruption limit: a
    /// safeguard against the failure mode where a proactive assistant becomes
    /// noise and you mute it forever. Set to 0 for no limit.
    #[serde(default = "d_max")]
    pub max_interruptions_per_hour: u32,
    #[serde(default = "d_conf")]
    pub min_confidence: f32,
    /// How much declining an offer raises the bar for that kind next time,
    /// and accepting one lowers it. Atlas never stops offering — it gets
    /// choosier about which offers are worth your attention.
    #[serde(default = "d_step")]
    pub learning_step: f32,
    /// The bar can never rise past this, so nothing is silenced forever.
    #[serde(default = "d_ceiling")]
    pub confidence_ceiling: f32,
    /// Don't interrupt until you've been quiet this long.
    #[serde(default = "d_quiet")]
    pub min_idle_secs: u64,
    /// Hold offers while you're mid-task at the keyboard, until a natural
    /// break — or until `defer_max_secs` has passed (bounded deferral).
    #[serde(default = "d_yes")]
    pub defer_while_working: bool,
    /// The most an offer waits for a break before it's said anyway.
    #[serde(default = "d_defer")]
    pub defer_max_secs: u64,
}
fn d_cooldown() -> u64 { 900 }
fn d_max() -> u32 { 4 }
fn d_conf() -> f32 { 0.7 }
fn d_step() -> f32 { 0.08 }
fn d_ceiling() -> f32 { 0.95 }
fn d_quiet() -> u64 { 30 }
fn d_yes() -> bool { true }
fn d_defer() -> u64 { 1200 }

impl Default for ProactiveConfig {
    fn default() -> Self {
        ProactiveConfig {
            enabled: false,
            cooldown_secs: d_cooldown(),
            max_interruptions_per_hour: d_max(),
            min_confidence: d_conf(),
            learning_step: d_step(),
            confidence_ceiling: d_ceiling(),
            min_idle_secs: d_quiet(),
            defer_while_working: d_yes(),
            defer_max_secs: d_defer(),
        }
    }
}

#[derive(Debug, Default)]
pub struct Proactive {
    pub cfg: ProactiveConfig,
    last_offer: u64,
    /// Timestamps of recent offers, for the hourly budget.
    recent: Vec<u64>,
    /// Since when something has been waiting for a break.
    held_since: Option<u64>,
    /// This tick's answer from `holds_for_your_work`, so the half-dozen
    /// gates that ask in one tick get one answer, and the bound's clock is
    /// moved once per tick rather than reset by the second asker.
    decided: Option<(u64, bool)>,
}

impl Proactive {
    pub fn new(cfg: ProactiveConfig) -> Self {
        Proactive { cfg, last_offer: 0, recent: Vec::new(), held_since: None, decided: None }
    }

    /// Everything that must be true before Atlas is allowed to speak first.
    pub fn may_interrupt(&mut self, s: &Signals, t: u64) -> bool {
        if !self.cfg.enabled {
            return false;
        }
        // Never talk over an active conversation.
        if s.in_conversation {
            return false;
        }
        // Let a thought finish.
        if s.idle_secs < self.cfg.min_idle_secs {
            return false;
        }
        if t.saturating_sub(self.last_offer) < self.cfg.cooldown_secs {
            return false;
        }
        self.recent.retain(|x| t.saturating_sub(*x) < 3600);
        if self.cfg.max_interruptions_per_hour > 0
            && self.recent.len() as u32 >= self.cfg.max_interruptions_per_hour
        {
            return false;
        }
        if !self.holds_for_your_work(s, t) {
            return false;
        }
        true
    }

    /// Bounded deferral (Horvitz, Apacible & Subramani 2005): an offer that
    /// would land while you're mid-task waits for a natural break — an app
    /// switch, a return to the keyboard, a pause in typing (Iqbal & Bailey,
    /// CHI 2008) — but never longer than `defer_max_secs`. While the OS says
    /// you're presenting, in a full-screen app or away, it waits with no
    /// bound: a pop-up over a presentation is the one interruption that
    /// can't be taken back. Where the OS can't say how long since your last
    /// input, nothing is held, which is how it always was.
    fn holds_for_your_work(&mut self, s: &Signals, t: u64) -> bool {
        if let Some((at, answer)) = self.decided {
            if at == t {
                return answer;
            }
        }
        let answer = self.decide_hold(s, t);
        self.decided = Some((t, answer));
        answer
    }

    fn decide_hold(&mut self, s: &Signals, t: u64) -> bool {
        if !self.cfg.defer_while_working {
            return true;
        }
        if s.os_quiet.map(|q| q.holds_offers()).unwrap_or(false) {
            // Held with no bound -- and the working bound starts afresh
            // afterwards, so time spent presenting doesn't let an offer
            // straight through the first keystroke after it.
            self.held_since = None;
            return false;
        }
        if s.input_idle_secs.is_some() && !s.at_breakpoint {
            let since = *self.held_since.get_or_insert(t);
            if t.saturating_sub(since) < self.cfg.defer_max_secs {
                return false;
            }
        }
        self.held_since = None;
        true
    }


    /// Pick the best offer that clears every bar, or none.
    pub fn consider(&mut self, s: &Signals, mem: &Memory, t: u64) -> Option<Offer> {
        self.consider_with(s, mem, t, None)
    }

    /// `consider`, with one more candidate from outside — a nudge — weighed
    /// against the offers on the same footing rather than always winning.
    pub fn consider_with(&mut self, s: &Signals, mem: &Memory, t: u64, also: Option<Offer>) -> Option<Offer> {
        if !self.may_interrupt(s, t) {
            return None;
        }
        let mut eligible: Vec<Offer> = also.into_iter().collect();
        for o in detect(s) {
            if o.confidence < self.threshold_for(&o.kind, mem) {
                continue;
            }
            // Deep focus raises the bar rather than blocking outright.
            if o.cost >= 2 && o.confidence < 0.9 {
                continue;
            }
            eligible.push(o);
        }
        // One clears the bars: that's the offer. Several: the most confident
        // used to win every time, so a kind you'd welcome that is never the
        // most confident never got asked and never got learned. Thompson
        // sampling (`bandit`) over how you've answered each kind still
        // favours what you've welcomed and keeps trying the rest. Seeded by
        // the tick, so a given moment is reproducible.
        let best: Option<Offer> = if eligible.len() <= 1 {
            eligible.pop()
        } else {
            let record: Vec<(u32, u32)> = eligible.iter().map(|o| answered(&o.kind, mem)).collect();
            let mut rng = crate::bandit::Rng(t ^ 0x0FFE_5EED);
            crate::bandit::pick(&record, &mut rng).map(|i| eligible.swap_remove(i))
        };
        if let Some(o) = &best {
            self.last_offer = t;
            self.recent.push(t);
            let _ = o;
        }
        best
    }

    /// How confident Atlas must be to raise this kind of offer, given how you
    /// have responded before. Declines push it up, acceptances pull it down.
    /// It is clamped at both ends, so an offer you keep refusing becomes rare
    /// but never impossible — and one you keep accepting becomes eager without
    /// becoming automatic.
    pub fn threshold_for(&self, kind: &str, mem: &Memory) -> f32 {
        let (yes, no) = answered(kind, mem);
        let (yes, no) = (yes as i32, no as i32);
        let adjusted =
            self.cfg.min_confidence + (no - yes) as f32 * self.cfg.learning_step;
        adjusted.clamp(0.25, self.cfg.confidence_ceiling)
    }

    /// Record what you said back, so it stops asking about things you refuse.
    pub fn record_response(&self, kind: &str, accepted: bool, mem: &mut Memory) {
        mem.record_approval(&format!("offer:{kind}"), accepted, None);
    }
}

/// How you've answered this kind of offer: (welcomed, declined). Counts the
/// compacted history too — `Memory` folds old approvals into `summaries`, and
/// reading only the recent list forgot every answer older than that.
fn answered(kind: &str, mem: &Memory) -> (u32, u32) {
    let key = format!("offer:{kind}");
    let (total, approved) = mem.summaries.get(&key).copied().unwrap_or((0, 0));
    let (mut yes, mut no) = (approved, total.saturating_sub(approved));
    for a in mem.approvals.iter().filter(|a| a.kind == key) {
        if a.approved {
            yes += 1
        } else {
            no += 1
        }
    }
    (yes, no)
}

/// Signal → candidate offers. Kept as a plain function so the rules are
/// readable and testable in isolation from the budget logic.
pub fn detect(s: &Signals) -> Vec<Offer> {
    let mut out = Vec::new();

    let n = s.recent_changes.added.len();
    if n >= 3 {
        out.push(Offer {
            kind: "index_new_files".into(),
            message: format!("{n} new files showed up. Want me to file them?"),
            command: "index refresh".into(),
            confidence: 0.75 + (n.min(20) as f32 * 0.01),
            cost: 1,
        });
    }

    if let Some(a) = &s.active {
        let t = a.title.to_lowercase();
        // Long dwell on one document with no speech: likely stuck, not busy.
        if s.dwell_secs > 1800 && s.idle_secs > 900 && t.contains(".pdf") {
            out.push(Offer {
                kind: "summarize_document".into(),
                message: "You've been on that document a while. Want a summary?".into(),
                command: "view my display".into(),
                confidence: 0.72,
                cost: 2,
            });
        }
    }

    out
}

/// Turn a nudge into an offer so it travels the same path as every other
/// thing Atlas says first — same interruption budget, same accept/decline
/// bookkeeping, same learning from refusals.
///
/// Initiative gets no privileged channel. If it did, it would be the one
/// thing that could not be turned down.
pub fn from_nudge(n: &crate::nudge::Nudge) -> Offer {
    let kind = match n.trigger {
        crate::nudge::Trigger::Stalled if n.asking_why => "nudge_why",
        crate::nudge::Trigger::Stalled => "nudge_stalled",
        crate::nudge::Trigger::Drifting => "nudge_drifting",
        crate::nudge::Trigger::Daypart => "nudge_daypart",
        crate::nudge::Trigger::CheckIn => "nudge_checkin",
    };
    Offer {
        kind: kind.into(),
        message: n.message.clone(),
        // Relief is an offer to do the work; without one there is nothing to
        // run, so the brief is the most Atlas can put behind it.
        command: n.relief.clone().unwrap_or_else(|| "brief".into()),
        confidence: n.confidence,
        // Never rated deep-focus cost: a nudge that only fires when you are
        // already idle does not need a second idleness test.
        cost: 1,
    }
}
