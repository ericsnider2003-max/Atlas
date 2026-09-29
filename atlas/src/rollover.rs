//! What it costs to hold, and the worst ninety seconds of the day to decide in.
//!
//! Two things happen at 17:00 New York and they are the same event seen from
//! two sides.
//!
//! ## 1. The book goes thin
//!
//! Every desk rolls its positions at once, the outgoing session has gone home
//! and the incoming one has not arrived. Quoted spreads at the turn are
//! routinely several times their normal width, and they are quoted on a book
//! nobody is really making.
//!
//! A stop sitting in that window gets filled at the wide price. So does a
//! market order. The trade is not worse — the *execution* is worse, and the
//! record afterwards shows a losing trade with a perfectly good reason behind
//! it, which is exactly the way a system learns the wrong lesson.
//!
//! This module will not say *how* wide, because that is a broker fact and
//! Atlas has no feed. It says **when**, which is the half that can be known
//! offline and is the half that matters for deciding whether to act now or in
//! twenty minutes.
//!
//! ## 2. Swap is charged — and on Wednesday it is charged three times
//!
//! Spot FX settles two business days out. A position held through Wednesday's
//! 17:00 New York rolls its value date from Friday to **Monday**, so it is
//! charged or paid **three days** of interest in one go.
//!
//! This is not a subtlety and it is not rare: it happens every single week.
//! On a carry-negative pair it can be the difference between a small winner
//! and a small loser, and it lands on the one night in five that nobody
//! remembers. A swing trade opened Wednesday morning is a different trade from
//! the same setup opened Tuesday morning, and nothing in a chart says so.
//!
//! ## What is modelled, plainly
//!
//! - Rollover at **17:00 New York**, from [`crate::fxday`], so it follows New
//!   York's clock rather than a fixed UTC hour.
//! - Rollovers on **Monday to Thursday**. Friday's 17:00 is the week's close
//!   — nothing is held through it into a trading session — and the weekend
//!   nights are the ones Wednesday already paid for.
//! - **Triple on Wednesday.**
//!
//! Some brokers differ, most often on pairs whose value dates fall on a local
//! holiday, and a few roll a day early around New Year. None of that is
//! derivable without that broker's calendar, so none of it is invented here.
//! What is here is the rule that holds for the overwhelming majority of nights
//! at the overwhelming majority of brokers, and [`Nights::caveat`] says so out
//! loud rather than letting the number look more certain than it is.

use crate::fxday::{close_on, day_start};
use crate::market::time::{civil_from_days, days_from_civil, MS_PER_DAY, MS_PER_HOUR, MS_PER_MIN};

/// How long either side of the turn the book is worth calling thin.
///
/// Thirty minutes, and it is a **chosen** number, not a measured one — Atlas
/// has no tick data and cannot measure a spread it never sees. It is named
/// here as a choice so nobody later reads it as a finding.
pub const THIN_MINUTES: i64 = 30;

/// One rollover.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rollover {
    /// The instant, in UTC.
    pub at: i64,
    /// Wednesday's, which carries three nights instead of one.
    pub triple: bool,
}

impl Rollover {
    pub fn nights(&self) -> i64 {
        if self.triple {
            3
        } else {
            1
        }
    }

    pub fn say(&self) -> String {
        if self.triple {
            "Wednesday's rollover, which charges three nights at once".into()
        } else {
            "a rollover, one night".into()
        }
    }
}

/// Day of the week of an instant. 0 is Sunday.
fn weekday(ms: i64) -> i64 {
    (ms.div_euclid(MS_PER_DAY) + 4).rem_euclid(7)
}

/// The rollover that ends the FX day containing `ms`, if there is one.
///
/// `None` on the days there isn't: Friday's 17:00 closes the week rather than
/// rolling a position into another session, and the weekend has no sessions to
/// roll between.
pub fn rollover_ending(ms: i64) -> Option<Rollover> {
    let start = day_start(ms);
    let (y, m, d) = civil_from_days(start.div_euclid(MS_PER_DAY));
    let (ny, nm, nd) = civil_from_days(days_from_civil(y, m, d) + 1);
    let at = close_on(ny, nm, nd);
    // The FX day is named by the day it CLOSES on: the day that opens 17:00
    // Tuesday closes 17:00 Wednesday and is Wednesday's day. So the weekday of
    // the closing instant is the one that decides both questions.
    match weekday(at - MS_PER_MIN) {
        1 | 2 | 4 => Some(Rollover { at, triple: false }), // Mon, Tue, Thu
        3 => Some(Rollover { at, triple: true }),          // Wed
        _ => None,                                         // Fri close, weekend
    }
}

/// What a position opened at `from` and closed at `to` is charged.
///
/// `Nights` rather than `Held` or `Holding`: `firewall::Held` and
/// `handshape::Holding` both already exist and mean something else entirely.
#[derive(Debug, Clone, PartialEq)]
pub struct Nights {
    pub rollovers: Vec<Rollover>,
}

impl Nights {
    /// Calendar nights crossed.
    pub fn nights(&self) -> i64 {
        self.rollovers.len() as i64
    }

    /// Nights actually charged, Wednesday counting three.
    pub fn charged(&self) -> i64 {
        self.rollovers.iter().map(|r| r.nights()).sum()
    }

    pub fn crosses_a_triple(&self) -> bool {
        self.rollovers.iter().any(|r| r.triple)
    }

    /// What this costs, given a per-night swap the caller got from the broker.
    ///
    /// Takes the rate rather than holding one, because a swap rate is a broker
    /// fact that changes with the policy rate and Atlas has no feed. A number
    /// invented here would be confidently wrong every time a central bank
    /// moved.
    pub fn cost(&self, per_night: f64) -> f64 {
        per_night * self.charged() as f64
    }

    pub fn caveat(&self) -> &'static str {
        "Rollovers on Monday to Thursday, triple on Wednesday. A few brokers \
         differ around local holidays and the new year, and I have no way to \
         know which — so treat this as the usual case rather than as your \
         broker's calendar."
    }

    pub fn say(&self) -> String {
        if self.rollovers.is_empty() {
            return "Closed the same session it opened, so no swap at all.".into();
        }
        let mut said = format!(
            "{} night(s) held, charged as {}.",
            self.nights(),
            self.charged()
        );
        if self.crosses_a_triple() {
            said.push_str(
                " That includes Wednesday's rollover, which charges three nights in one go — on \
                 a pair that pays you to be short, being long over that one night costs what \
                 three ordinary nights cost, and nothing on the chart says so.",
            );
        }
        said
    }
}

/// Every rollover a position would sit through.
pub fn held_through(from: i64, to: i64) -> Nights {
    let mut out = Vec::new();
    if to <= from {
        return Nights { rollovers: out };
    }
    // Walk day by day. Bounded by the span rather than open-ended, so a wild
    // timestamp produces a wrong answer rather than a hang — and a span of
    // more than a year is refused below by the bound itself.
    let days = ((to - from) / MS_PER_DAY) + 2;
    let mut cursor = from;
    for _ in 0..days.min(400) {
        if let Some(r) = rollover_ending(cursor) {
            if r.at > from && r.at <= to && !out.iter().any(|x: &Rollover| x.at == r.at) {
                out.push(r);
            }
        }
        cursor += MS_PER_DAY;
        if cursor > to + MS_PER_DAY {
            break;
        }
    }
    out.sort_by_key(|r| r.at);
    Nights { rollovers: out }
}

/// Is the book thin right now, and why?
///
/// `None` means nothing structural is wrong with the moment. It says nothing
/// about whether the spread is *actually* wide — that needs a feed — only that
/// this is one of the moments where it structurally is.
#[derive(Debug, Clone, PartialEq)]
pub enum Thin {
    /// Inside the rollover window.
    Rollover { minutes_away: i64, triple: bool },
    /// Nobody's desk is open.
    NobodyOpen,
    /// One centre, and it is the quiet one.
    OneDesk(&'static str),
}

impl Thin {
    pub fn plain(&self) -> String {
        match self {
            Thin::Rollover { minutes_away, triple } => format!(
                "this is {} minute(s) from the 17:00 New York rollover{}. Every desk rolls at \
                 once, the spread widens several times over, and a stop sitting in that window \
                 gets filled at the wide price — the trade isn't worse, the fill is",
                minutes_away.abs(),
                if *triple { ", and it is Wednesday's — three nights of swap" } else { "" }
            ),
            Thin::NobodyOpen => "no major centre is open, so what's quoted is a price nobody is \
                                 really making"
                .into(),
            Thin::OneDesk(name) => format!(
                "{name} is the only desk open, which is the thinnest book of the trading day"
            ),
        }
    }
}

/// The thin-book check for one instant.
pub fn thin(ms: i64) -> Option<Thin> {
    if let Some(r) = rollover_ending(ms) {
        let gap = (r.at - ms) / MS_PER_MIN;
        if gap.abs() <= THIN_MINUTES {
            return Some(Thin::Rollover { minutes_away: gap, triple: r.triple });
        }
    }
    // And the previous rollover, for the half-hour after it.
    let before = day_start(ms);
    if (ms - before) / MS_PER_MIN <= THIN_MINUTES && rollover_ending(before - MS_PER_HOUR).is_some()
    {
        return Some(Thin::Rollover {
            minutes_away: -((ms - before) / MS_PER_MIN),
            triple: rollover_ending(before - MS_PER_HOUR).map(|r| r.triple).unwrap_or(false),
        });
    }

    let session = crate::market::session::session_at(ms);
    match session.depth() {
        0 => Some(Thin::NobodyOpen),
        1 => {
            let name = session.centres[0].name();
            (name == "Sydney").then_some(Thin::OneDesk("Sydney"))
        }
        _ => None,
    }
}

/// Said out loud, for one moment and one holding period.
pub fn spoken(now: i64, until: Option<i64>) -> String {
    let mut said = match thin(now) {
        Some(t) => format!("Careful with the fill: {}.", t.plain()),
        None => "The book should be normal depth right now.".into(),
    };
    if let Some(until) = until {
        let held = held_through(now, until);
        said.push(' ');
        said.push_str(&held.say());
    }
    said
}
