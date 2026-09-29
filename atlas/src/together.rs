//! Three trades that are one bet.
//!
//! ## One opinion counted three times
//!
//! Two opinions that share an input are one opinion counted twice. Positions
//! are the same: **long EURUSD, long GBPUSD and long AUDUSD is not three
//! positions. It is short dollar, three times.**
//!
//! It has every property that makes a failure expensive:
//!
//! - The risk rules pass. Each trade is one per cent, and one per cent is the
//!   rule.
//! - The trade log looks diversified. Three instruments, three setups.
//! - It only shows up on the days it matters, when one dollar print takes all
//!   three out together and the account loses three per cent from a rule that
//!   said one.
//!
//! And it gets **worse** as Atlas gets better, which is the part worth sitting
//! with. A reader that is genuinely good at spotting dollar strength will
//! spot it on every dollar pair at once, and will be right, and will put on
//! three correlated trades because it was right. Skill concentrates this risk
//! rather than diluting it.
//!
//! ## How it is measured
//!
//! Not with a correlation matrix. A correlation is a number about the past
//! that needs a long history and is unstable exactly when it matters — every
//! correlation goes to one in a crisis, which is the day the number was
//! supposed to warn you.
//!
//! Instead: a pair is two currencies, and a position is an opinion about both
//! of them. Add up the opinions. That is not a model of anything; it is
//! arithmetic on what is actually held, and it cannot be wrong about the past
//! because it is not about the past.

use serde::{Deserialize, Serialize};

/// What a pair is made of, and which way round.
///
/// The base currency is bought and the quote is sold when you go long. Listed
/// rather than parsed out of the name for one reason: a pair whose name Atlas
/// does not recognise must be **reported**, not silently treated as having no
/// currency exposure at all. A silent zero here is the whole failure this
/// module exists to prevent, arriving through the door marked "unknown
/// instrument".
pub const PAIRS: [(&str, &str, &str); 14] = [
    ("EURUSD", "EUR", "USD"),
    ("GBPUSD", "GBP", "USD"),
    ("AUDUSD", "AUD", "USD"),
    ("NZDUSD", "NZD", "USD"),
    ("USDJPY", "USD", "JPY"),
    ("USDCHF", "USD", "CHF"),
    ("USDCAD", "USD", "CAD"),
    ("EURGBP", "EUR", "GBP"),
    ("EURJPY", "EUR", "JPY"),
    ("GBPJPY", "GBP", "JPY"),
    ("EURAUD", "EUR", "AUD"),
    ("AUDJPY", "AUD", "JPY"),
    ("CADJPY", "CAD", "JPY"),
    ("CHFJPY", "CHF", "JPY"),
];

/// The two currencies in a pair, base first.
pub fn legs(pair: &str) -> Option<(&'static str, &'static str)> {
    let want = pair.trim().to_uppercase().replace(['/', '_', '-'], "");
    PAIRS
        .iter()
        .find(|(name, _, _)| *name == want)
        .map(|(_, base, quote)| (*base, *quote))
}

/// One position, in R rather than lots.
///
/// R, because the question is how much is at risk and lots do not answer it.
/// Two one-lot positions with different stops are two different amounts of
/// money, and adding them up gives a number that is about neither.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub pair: String,
    pub long: bool,
    /// What is at risk, in R.
    pub risk: f64,
}

/// What is actually held, once the pairs are taken apart.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Netted {
    /// Currency, and net R one way. Positive is long that currency.
    pub per_currency: Vec<(String, f64)>,
    /// Positions whose pair Atlas does not know. **Never silently dropped.**
    pub unknown: Vec<String>,
    /// The sum of every position's risk, as the risk rules counted it.
    pub as_counted: f64,
}

impl Netted {
    /// The largest one-way bet, whatever it is called on the ticket.
    pub fn biggest(&self) -> Option<(&str, f64)> {
        self.per_currency
            .iter()
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            .map(|(c, r)| (c.as_str(), *r))
    }

    /// How much more the account is really carrying than the ticket says.
    ///
    /// `None` when nothing is held. One is "the positions are genuinely
    /// independent"; two is "half of what looks like diversification is one
    /// bet".
    pub fn really_carrying(&self) -> Option<f64> {
        let (_, worst) = self.biggest()?;
        (self.as_counted > 0.0).then(|| worst.abs() / self.as_counted)
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(default)]
pub struct TogetherConfig {
    /// One-way exposure to a single currency above this, in R, is said out
    /// loud.
    pub say_above: f64,
    /// And above this it is an alarm.
    pub alarm_above: f64,
}

impl Default for TogetherConfig {
    fn default() -> Self {
        // Two R is the first number where the ticket and the truth have
        // meaningfully parted company: two "one per cent" trades that are the
        // same bet are a two per cent trade, and the rule said one.
        TogetherConfig { say_above: 2.0, alarm_above: 3.0 }
    }
}

/// Take the positions apart into what is actually being bet on.
pub fn net(held: &[Position]) -> Netted {
    let mut per: Vec<(String, f64)> = Vec::new();
    let mut unknown = Vec::new();
    let mut as_counted = 0.0;

    for p in held {
        if !p.risk.is_finite() || p.risk <= 0.0 {
            continue;
        }
        as_counted += p.risk;
        let Some((base, quote)) = legs(&p.pair) else {
            // Named, never dropped. A pair Atlas does not recognise
            // contributing nothing to the totals would make the exposure look
            // *better* than it is, which is the failure this file exists for
            // arriving through a side door.
            unknown.push(p.pair.clone());
            continue;
        };
        let way = if p.long { 1.0 } else { -1.0 };
        for (ccy, sign) in [(base, 1.0), (quote, -1.0)] {
            let amount = way * sign * p.risk;
            match per.iter_mut().find(|(c, _)| c == ccy) {
                Some((_, v)) => *v += amount,
                None => per.push((ccy.to_string(), amount)),
            }
        }
    }
    per.retain(|(_, v)| v.abs() > 1e-9);
    per.sort_by(|a, b| b.1.abs().total_cmp(&a.1.abs()).then(a.0.cmp(&b.0)));
    Netted { per_currency: per, unknown, as_counted }
}

/// Would adding this trade make it worse?
///
/// Asked **before** the trade, which is the only time the answer is useful.
/// After it is on, this is a report; before it, it is a decision.
pub fn if_i_add(held: &[Position], adding: &Position, cfg: &TogetherConfig) -> Option<String> {
    let before = net(held);
    let mut with = held.to_vec();
    with.push(adding.clone());
    let after = net(&with);

    let (worst, amount) = after.biggest()?;
    if amount.abs() <= cfg.say_above {
        return None;
    }
    let was = before
        .per_currency
        .iter()
        .find(|(c, _)| c == worst)
        .map(|(_, v)| *v)
        .unwrap_or(0.0);
    // Only worth saying if this trade is what pushed it. A warning that fires
    // on a position somebody already holds, every time they look at anything
    // else, is a warning that gets ignored.
    if amount.abs() <= was.abs() {
        return None;
    }
    Some(format!(
        "That would put you {} {worst} at {:.1} R across {} position(s). It's booked as {} \
         separate trades and it's one bet — if {worst} moves against you they all go together, \
         and the risk rules will have counted it as {:.1} R.",
        if amount > 0.0 { "long" } else { "short" },
        amount.abs(),
        with.len(),
        with.len(),
        adding.risk
    ))
}

/// What is held, said out loud.
pub fn spoken(held: &[Position], cfg: &TogetherConfig) -> String {
    let n = net(held);
    if held.is_empty() {
        return "Nothing open.".into();
    }
    let mut out = format!(
        "{} position(s), {:.1}R of risk deployed by the ticket (initial risk per position, not \
         live P&L).\n",
        held.len(),
        n.as_counted
    );
    for (ccy, amount) in &n.per_currency {
        let mark = if amount.abs() > cfg.alarm_above {
            "  <- that is one bet, not several"
        } else if amount.abs() > cfg.say_above {
            "  <- worth noticing"
        } else {
            ""
        };
        out.push_str(&format!(
            "  {} {ccy} {:.1} R{mark}\n",
            if *amount > 0.0 { "long " } else { "short" },
            amount.abs()
        ));
    }
    if let Some(ratio) = n.really_carrying() {
        if ratio > 0.6 {
            out.push_str(&format!(
                "The biggest single bet is {:.0}% of everything at risk. The ticket says \
                 diversified; the exposure says one trade.\n",
                ratio * 100.0
            ));
        }
    }
    if !n.unknown.is_empty() {
        out.push_str(&format!(
            "I don't know what {} is made of, so it isn't in any of the numbers above. That \
             makes this look safer than it is.\n",
            n.unknown.join(", ")
        ));
    }
    out
}
