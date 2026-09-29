//! Times with other people.
//!
//! Atlas finding a slot and putting it in the calendar is the version that
//! looks impressive and is wrong: a time with someone else is a small promise
//! made in your name, and the only person who can make it is you.
//!
//! So Atlas does the tedious half — reading what was proposed, checking it
//! against what you already have, working out what else would fit — and then
//! stops. You say yes, no, or a different time, and only then does anything
//! get written down or sent.

use serde::{Deserialize, Serialize};

/// Someone wants a time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Proposal {
    pub id: u64,
    /// Who asked.
    pub from: String,
    /// The client or project it belongs to, if any.
    pub about: Option<String>,
    /// What they said, so you can read it rather than Atlas's summary.
    pub their_words: String,
    /// Times they offered.
    pub times: Vec<Slot>,
    pub at: u64,
    pub state: State,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Slot {
    pub start: u64,
    pub mins: u32,
}

impl Slot {
    fn end(&self) -> u64 {
        self.start + self.mins as u64 * 60
    }
    fn clashes_with(&self, other: &Slot) -> bool {
        self.start < other.end() && other.start < self.end()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Waiting for you.
    NeedsYou,
    Accepted,
    Declined,
    /// You offered something else.
    CounterOffered,
    /// The times have passed while it sat there.
    WentStale,
}

/// What Atlas worked out about each offered time.
#[derive(Debug, Clone, PartialEq)]
pub struct Assessed {
    pub slot: Slot,
    pub verdict: Fit,
    /// Why, in a few words.
    pub because: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    Free,
    /// Something's there but you could move it.
    Awkward,
    Clashes,
    /// Outside the hours you work.
    OffHours,
    /// Already gone.
    Past,
}

impl Fit {
    fn workable(&self) -> bool {
        matches!(self, Fit::Free | Fit::Awkward)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct BookingConfig {
    pub enabled: bool,
    /// Hours you'd take a meeting, as 24-hour start and end.
    pub hours: (u32, u32),
    /// Don't offer anything sooner than this, in hours.
    pub notice_hours: u32,
    /// Atlas never accepts, declines or books on its own. Not configurable —
    /// a time with someone else is a promise made in your name.
    #[serde(skip, default = "never")]
    pub may_book_alone: bool,
    /// Nor send the reply.
    #[serde(skip, default = "never")]
    pub may_send_alone: bool,
}

fn never() -> bool {
    false
}

impl Default for BookingConfig {
    fn default() -> Self {
        BookingConfig {
            enabled: false,
            hours: (9, 18),
            notice_hours: 12,
            may_book_alone: false,
            may_send_alone: false,
        }
    }
}

/// Judge each offered time against what you already have.
/// `zone` is your clock: "your hours" are 9 to 6 where you are (UTC when
/// no time zone is set, as before).
pub fn assess(p: &Proposal, busy: &[Slot], now: u64, cfg: &BookingConfig, zone: &crate::tz::Zone) -> Vec<Assessed> {
    p.times
        .iter()
        .map(|s| {
            let hour = zone.hour(s.start as i64);
            let (verdict, because) = if s.start < now {
                (Fit::Past, "already gone".to_string())
            } else if s.start < now + cfg.notice_hours as u64 * 3600 {
                (Fit::Awkward, format!("under {} hours' notice", cfg.notice_hours))
            } else if hour < cfg.hours.0 || hour >= cfg.hours.1 {
                (Fit::OffHours, format!("{hour}:00 is outside your hours"))
            } else if busy.iter().any(|b| b.clashes_with(s)) {
                (Fit::Clashes, "you've got something then".to_string())
            } else {
                (Fit::Free, "clear".to_string())
            };
            Assessed { slot: *s, verdict, because }
        })
        .collect()
}

/// Times you could offer instead.
///
/// Only inside your hours and only with notice — a counter-offer of eleven at
/// night is worse than saying no.
pub fn could_offer(busy: &[Slot], from: u64, mins: u32, cfg: &BookingConfig, how_many: usize, zone: &crate::tz::Zone) -> Vec<Slot> {
    let mut out = Vec::new();
    let start_looking = from + cfg.notice_hours as u64 * 3600;
    // Days and hours on your clock, each slot brought back to UTC.
    let mut day = zone.to_local(start_looking as i64).div_euclid(86_400) * 86_400;

    for _ in 0..14 {
        for hour in cfg.hours.0..cfg.hours.1 {
            let s = Slot { start: zone.to_utc(day + hour as i64 * 3600).max(0) as u64, mins };
            if s.start < start_looking {
                continue;
            }
            if busy.iter().any(|b| b.clashes_with(&s)) {
                continue;
            }
            out.push(s);
            if out.len() >= how_many {
                return out;
            }
        }
        day += 86_400;
    }
    out
}

/// What Atlas puts in front of you.
///
/// Their words, what fits, and what to offer instead — then it stops.
pub fn to_decide(p: &Proposal, assessed: &[Assessed], alternatives: &[Slot]) -> String {
    let mut s = format!("{} wants a time", p.from);
    if let Some(a) = &p.about {
        s.push_str(&format!(" about {a}"));
    }
    s.push_str(". ");

    let workable: Vec<&Assessed> = assessed.iter().filter(|a| a.verdict.workable()).collect();
    match workable.len() {
        0 => {
            s.push_str("None of their times work");
            if let Some(first) = assessed.first() {
                s.push_str(&format!(" — {}", first.because));
            }
            s.push('.');
            if !alternatives.is_empty() {
                s.push_str(&format!(" I've got {} you could offer.", alternatives.len()));
            }
        }
        1 => s.push_str("One of their times is clear."),
        n => s.push_str(&format!("{n} of their times work.")),
    }
    // The line that keeps this honest.
    s.push_str(" Nothing's booked — say yes, no, or a different time.");
    s
}

/// You answered.
pub fn answered(said: &str) -> Option<State> {
    // A comma after the word is the normal way people answer — "no, can't do
    // that week" is a no, and requiring a bare word makes Atlas look thick.
    let t = said.trim().to_lowercase().replace(',', " ");
    let t = t.trim().to_string();
    if ["yes", "accept", "book it", "that works", "fine", "go ahead"]
        .iter()
        .any(|w| t == *w || t.starts_with(&format!("{w} ")))
    {
        return Some(State::Accepted);
    }
    if ["no", "decline", "can't", "cant", "not that", "pass"]
        .iter()
        .any(|w| t == *w || t.starts_with(&format!("{w} ")))
    {
        return Some(State::Declined);
    }
    if t.contains("instead") || t.contains("how about") || t.contains("what about") || t.contains("offer") {
        return Some(State::CounterOffered);
    }
    None
}

/// Anything sitting unanswered while its times pass.
///
/// The failure that actually happens: it waits politely on the hub until the
/// meeting it was about is in the past.
pub fn going_stale(proposals: &[Proposal], now: u64) -> Vec<&Proposal> {
    proposals
        .iter()
        .filter(|p| p.state == State::NeedsYou)
        .filter(|p| p.times.iter().all(|t| t.start < now + 48 * 3600))
        .collect()
}

pub fn stale_nudge(p: &Proposal) -> String {
    format!(
        "{}'s times are within two days and you haven't answered. It'll be a no by default, \
         which is a worse answer than a no.",
        p.from
    )
}
