//! Scheduled news: when it lands, what it moves, how long the damage lasts.
//!
//! ## What the research changed, in order of how wrong this would otherwise be
//!
//! **1. Non-farm payrolls is not the first Friday of the month.** Everyone says
//! it is. The BLS rule is the *third Friday after the Saturday ending the week
//! containing the 12th* of the reference month. They coincide often enough for
//! the folk rule to feel true and they came apart in **four of the twelve
//! months of 2026** — by a full week in January and May, and in February the
//! release was on a WEDNESDAY, which no weekday rule can ever produce. A
//! blackout built on "first Friday" would have been open for business through
//! the single largest scheduled event in FX, four times a year, without ever
//! saying it was wrong.
//!
//! **2. The ±2 minute blackout everyone uses is about ten times too short**,
//! and it is convention with nothing behind it — prop firms range from 0 to 5
//! minutes and none publishes a derivation. Andersen, Bollerslev, Diebold &
//! Vega (2003) found the conditional MEAN adjusts in 5–10 minutes but
//! volatility takes about **60 minutes** to return to baseline; Chaboud et al.
//! measured NFP volume elevated for **120**. So a window is reported per
//! PURPOSE: "avoid the price jump" and "avoid the volatility" are different
//! questions with answers an order of magnitude apart.
//!
//! **3. You cannot dodge the window by predicting a dull print.** Chaboud et
//! al. decomposed the NFP volume spike: the intercept dominates the
//! surprise-sensitivity term by roughly **450 to 1**. Most of the extra trading
//! happens even when the number lands exactly on consensus. The event fires the
//! window, not the surprise.
//!
//! **4. Most of a standard economic calendar is noise.** ABDV found NO
//! significant effect on FX for PPI, housing starts, leading indicators, money
//! supply, personal income, new home sales, factory orders, business
//! inventories, or the GDP second and third estimates. PPI independently had
//! the smallest measured volume impact of anything tested — and sits on nearly
//! every prop firm's restricted list. None of them are in this module.
//!
//! **5. Some "structural" cross-currency links have broken.** Oil-to-CAD was
//! significant 1997–2014 and is **not significant from 2016 on**, with the
//! correlation now slightly positive. Hard-coding it would encode a
//! relationship that stopped working a decade ago, so it is absent. AUD/JPY as
//! a risk proxy, by contrast, is properly evidenced — it is mechanically the
//! two ends of the carry cross-section — so it is wired.
//!
//! ## How the dates are held
//!
//! Three tiers, stored on every event, because the honest thing to do with a
//! date nobody can derive is to say so:
//!
//! - `Rule` — derivable from calendar arithmetic. Computed.
//! - `Semi` — a rule that holds most months and breaks on holidays.
//! - `Fixed` — no rule exists. Published dates, hand-entered, **with an
//!   expiry**. When the table runs out this module refuses rather than
//!   returning a stale answer.
//! - `Provisional` — published, and labelled provisional by the issuing bank
//!   itself. Carried at the confidence the issuer gave it rather than flattened
//!   to the same footing as everything else.
//!
//! The SNB looks like it should be "the third Thursday of the quarter-end
//! month" and in 2026 it was the third, third, **fourth** and **second**. That
//! is why these are lists and not arithmetic. And two reads of the SNB's own
//! 2027 schedule disagreed about September and December, so those two are
//! **absent** and [`gaps`] reports why — a missing event makes the calendar
//! silent, a wrong one makes it confidently wrong.

use super::bars::{refuse, Answer};
use super::session::{offset_hours, Zone};
use super::time::{last_weekday, nth_business_day, nth_weekday, Utc, MS_PER_HOUR, MS_PER_MIN};

/// Why a window is being asked for. Each has a measurement behind it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// The price jump itself. ABDV: the conditional mean adjusts in 5–10
    /// minutes. This is what the industry's ±2 minutes actually protects
    /// against, and for that narrow purpose it is about right.
    Execution,
    /// Tham (2008): the adverse-selection component of the spread up 20–44% on
    /// EUR/USD, normalising over 15–30 minutes, with liquidity withdrawal
    /// beginning 30 minutes BEFORE the release.
    AdverseSelection,
    /// Back to baseline. ABDV: complete only after twelve five-minute periods.
    Volatility,
}

impl Purpose {
    /// (minutes before, minutes after).
    pub fn window(self) -> (i64, i64) {
        match self {
            Purpose::Execution => (1, 5),
            Purpose::AdverseSelection => (30, 30),
            Purpose::Volatility => (30, 60),
        }
    }
}

/// NFP is its own case: volume still elevated at 120 minutes, against ~60 for
/// everything else.
pub const NFP_VOLATILITY_WINDOW: (i64, i64) = (30, 120);

/// The carry cross-section. Brunnermeier, Nagel & Pedersen measured currency
/// skewness 1986–2006: AUD −0.322, NZD −0.297, JPY +0.318, with R² = 0.81
/// between rate differential and skew.
pub const RISK_PROXY: [&str; 3] = ["AUD", "NZD", "JPY"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Rule,
    Semi,
    Fixed,
    /// Published, and labelled provisional by the issuer.
    Provisional,
}

/// One scheduled release, at a real instant, with what it is known to move.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub name: String,
    pub at: i64,
    pub currency: &'static str,
    /// 1–3, evidence-weighted rather than copied from a vendor's traffic
    /// lights. Anything ABDV found insignificant is not in this module at all.
    pub impact: u8,
    pub tier: Tier,
    /// For events with no fixed announcement time.
    pub window_end: Option<i64>,
}

impl Event {
    /// The BoJ announces somewhere in an hour-wide band rather than at a
    /// timestamp, and **the delay is itself information** — a late statement
    /// correlates with a contentious vote. Modelling it as a point would be
    /// precise and wrong.
    pub fn is_window(&self) -> bool {
        self.window_end.is_some()
    }

    pub fn blackout(&self, purpose: Purpose) -> (i64, i64) {
        let (before, after) = if purpose == Purpose::Volatility && self.name.starts_with("Non-Farm")
        {
            NFP_VOLATILITY_WINDOW
        } else {
            purpose.window()
        };
        let end = self.window_end.unwrap_or(self.at);
        (self.at - before * MS_PER_MIN, end + after * MS_PER_MIN)
    }

    /// Does this event move that pair.
    ///
    /// Two channels, and the second is the one people forget. Directly, if the
    /// pair contains the event's currency. And through RISK: a US event moves
    /// AUD/JPY with no USD in it at all, because the carry cross-section
    /// unwinds — which is evidenced rather than assumed.
    pub fn touches(&self, instrument: &str) -> bool {
        let pair = instrument.to_uppercase().replace('/', "");
        if pair.contains(self.currency) {
            return true;
        }
        if self.impact >= 3 && self.currency == "USD" {
            return RISK_PROXY.iter().filter(|c| pair.contains(**c)).count() >= 2;
        }
        false
    }

    pub fn say(&self) -> String {
        let t = Utc::from_ms(self.at);
        let when = match self.window_end {
            Some(e) => {
                let end = Utc::from_ms(e);
                format!(
                    "{}-{:02}:{:02}Z (window: the delay is itself a signal)",
                    t.say(),
                    end.hour,
                    end.minute
                )
            }
            None => t.say(),
        };
        format!("{} [{}] {} impact {} ({:?})", self.name, self.currency, when, self.impact, self.tier)
    }
}

// ---------------------------------------------------------------------------
// US federal holidays that actually move a release
// ---------------------------------------------------------------------------

fn us_holidays(y: i32) -> Vec<(i32, u32, u32)> {
    let base = [
        (y, 1, 1),
        (y, 1, nth_weekday(y, 1, 0, 3)),   // MLK
        (y, 2, nth_weekday(y, 2, 0, 3)),   // Presidents
        (y, 5, last_weekday(y, 5, 0)),     // Memorial
        (y, 6, 19),                        // Juneteenth
        (y, 7, 4),                         // Independence
        (y, 9, nth_weekday(y, 9, 0, 1)),   // Labor
        (y, 11, nth_weekday(y, 11, 3, 4)), // Thanksgiving
        (y, 12, 25),
    ];
    let mut out: Vec<(i32, u32, u32)> = base.to_vec();
    // A federal holiday on a Saturday is observed the Friday before, which is
    // what actually moves a Friday release -- 3 July 2026 is the reason that
    // month's payrolls landed on a Thursday.
    for &(yy, m, d) in &base {
        let wd = Utc::date(yy, m, d).weekday();
        if wd == 5 {
            let prev = Utc::from_ms(Utc::date(yy, m, d).to_ms() - 86_400_000);
            out.push((prev.year, prev.month, prev.day));
        } else if wd == 6 {
            let next = Utc::from_ms(Utc::date(yy, m, d).to_ms() + 86_400_000);
            out.push((next.year, next.month, next.day));
        }
    }
    out
}

/// The three months the payrolls rule cannot produce, and why.
///
/// All three are fallout from the 2025 appropriations lapse rather than rule
/// failures — and the January 2026 reference month released on a **Wednesday**,
/// which no weekday rule can ever generate. Listed rather than approximated: a
/// wrong date here is worse than an admitted exception, because it would
/// silently unblock the largest scheduled event in FX.
const NFP_EXCEPTIONS: [((i32, u32), (i32, u32, u32)); 3] = [
    ((2025, 11), (2025, 12, 16)),
    ((2025, 12), (2026, 1, 9)),
    ((2026, 1), (2026, 2, 11)),
];

/// The Employment Situation release date for reference month (y, m).
pub fn nfp_date(y: i32, m: u32) -> (i32, u32, u32) {
    if let Some((_, d)) = NFP_EXCEPTIONS.iter().find(|((ey, em), _)| *ey == y && *em == m) {
        return *d;
    }
    let twelfth = Utc::date(y, m, 12);
    let to_sat = (5 + 7 - twelfth.weekday()) % 7;
    let sat_ms = twelfth.to_ms() + to_sat as i64 * 86_400_000;
    let sat = Utc::from_ms(sat_ms);
    let to_fri = {
        let step = (4 + 7 - sat.weekday()) % 7;
        if step == 0 { 7 } else { step }
    };
    let mut out = Utc::from_ms(sat_ms + (to_fri as i64 + 14) * 86_400_000);
    // Releases move EARLIER on a holiday, never later.
    let hol = us_holidays(out.year);
    while hol.contains(&(out.year, out.month, out.day)) {
        out = Utc::from_ms(out.to_ms() - 86_400_000);
    }
    (out.year, out.month, out.day)
}

// ---------------------------------------------------------------------------
// rules
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Rule {
    name: &'static str,
    currency: &'static str,
    impact: u8,
    tier: Tier,
    zone: Zone,
    hour: u32,
    minute: u32,
    when: fn(i32, u32) -> Option<(i32, u32, u32)>,
}

fn r_nfp(y: i32, m: u32) -> Option<(i32, u32, u32)> {
    Some(nfp_date(y, m))
}
fn r_first_bd(y: i32, m: u32) -> Option<(i32, u32, u32)> {
    Some((y, m, nth_business_day(y, m, 1, &us_holidays(y))))
}
fn r_third_bd(y: i32, m: u32) -> Option<(i32, u32, u32)> {
    Some((y, m, nth_business_day(y, m, 3, &us_holidays(y))))
}
fn r_first_bd_plain(y: i32, m: u32) -> Option<(i32, u32, u32)> {
    Some((y, m, nth_business_day(y, m, 1, &[])))
}
fn r_third_bd_plain(y: i32, m: u32) -> Option<(i32, u32, u32)> {
    Some((y, m, nth_business_day(y, m, 3, &[])))
}

/// Only releases ABDV (2003) found significant. PPI, housing starts, leading
/// indicators, money supply and the GDP revisions are deliberately absent: each
/// was measured and found to have no effect on FX.
const RULES: [Rule; 7] = [
    Rule { name: "Non-Farm Payrolls", currency: "USD", impact: 3, tier: Tier::Rule,
           zone: Zone::NewYork, hour: 8, minute: 30, when: r_nfp },
    Rule { name: "ISM Manufacturing", currency: "USD", impact: 2, tier: Tier::Rule,
           zone: Zone::NewYork, hour: 10, minute: 0, when: r_first_bd },
    Rule { name: "ISM Services", currency: "USD", impact: 2, tier: Tier::Rule,
           zone: Zone::NewYork, hour: 10, minute: 0, when: r_third_bd },
    Rule { name: "Euro area flash HICP", currency: "EUR", impact: 3, tier: Tier::Semi,
           zone: Zone::Brussels, hour: 11, minute: 0, when: r_first_bd_plain },
    Rule { name: "Euro area services PMI", currency: "EUR", impact: 2, tier: Tier::Rule,
           zone: Zone::Brussels, hour: 10, minute: 0, when: r_third_bd_plain },
    Rule { name: "UK manufacturing PMI", currency: "GBP", impact: 2, tier: Tier::Rule,
           zone: Zone::London, hour: 9, minute: 30, when: r_first_bd_plain },
    Rule { name: "UK services PMI", currency: "GBP", impact: 2, tier: Tier::Rule,
           zone: Zone::London, hour: 9, minute: 30, when: r_third_bd_plain },
];

fn to_utc(zone: Zone, y: i32, m: u32, d: u32, hour: u32, minute: u32) -> i64 {
    // The release time is defined in the agency's LOCAL clock, so the UTC
    // instant moves by an hour twice a year -- and the US, EU, AU and NZ move on
    // different dates. Converting through the verified offsets is the only way
    // this stays right through those weeks.
    let guess = Utc::at(y, m, d, hour, minute).to_ms();
    guess - offset_hours(zone, guess) * MS_PER_HOUR
}

impl Rule {
    fn on(&self, y: i32, m: u32) -> Option<Event> {
        let (dy, dm, dd) = (self.when)(y, m)?;
        Some(Event {
            name: self.name.into(),
            at: to_utc(self.zone, dy, dm, dd, self.hour, self.minute),
            currency: self.currency,
            impact: self.impact,
            tier: self.tier,
            window_end: None,
        })
    }
}

struct Bank {
    name: &'static str,
    currency: &'static str,
    zone: Zone,
    hour: u32,
    minute: u32,
    /// Minutes of announcement window; 0 for a fixed time.
    window: i64,
    dates: &'static [(i32, u32, u32)],
    /// Years the issuer itself labels provisional.
    provisional: &'static [i32],
}

const BANKS: [Bank; 8] = [
    Bank { name: "FOMC", currency: "USD", zone: Zone::NewYork, hour: 14, minute: 0, window: 0,
        dates: &[(2026,1,28),(2026,3,18),(2026,4,29),(2026,6,17),(2026,7,29),(2026,9,16),(2026,10,28),(2026,12,9),
                 (2027,1,27),(2027,3,17),(2027,4,28),(2027,6,9),(2027,7,28),(2027,9,15),(2027,10,27),(2027,12,8)],
        provisional: &[] },
    Bank { name: "ECB", currency: "EUR", zone: Zone::Brussels, hour: 14, minute: 15, window: 0,
        dates: &[(2026,2,5),(2026,3,19),(2026,4,30),(2026,6,11),(2026,7,23),(2026,9,10),(2026,10,29),(2026,12,17),
                 (2027,2,4),(2027,3,18),(2027,4,29),(2027,6,10),(2027,7,22),(2027,9,9),(2027,10,28),(2027,12,16)],
        provisional: &[] },
    // The Bank publishes 2027 under the heading "2027 provisional dates" while
    // showing 2026 as confirmed. Flattening that would present a date the
    // issuer hedged at the same confidence as one it did not.
    Bank { name: "BoE", currency: "GBP", zone: Zone::London, hour: 12, minute: 0, window: 0,
        dates: &[(2026,2,5),(2026,3,19),(2026,4,30),(2026,6,18),(2026,7,30),(2026,9,17),(2026,11,5),(2026,12,17),
                 (2027,2,4),(2027,3,18),(2027,4,29),(2027,6,17),(2027,7,29),(2027,9,16),(2027,11,4),(2027,12,16)],
        provisional: &[2027] },
    // The one entry with a window rather than a time.
    Bank { name: "BoJ", currency: "JPY", zone: Zone::Tokyo, hour: 11, minute: 30, window: 60,
        dates: &[(2026,1,23),(2026,3,19),(2026,4,28),(2026,6,16),(2026,7,31),(2026,9,18),(2026,10,30),(2026,12,18),
                 (2027,1,22),(2027,3,18),(2027,4,28),(2027,6,11),(2027,7,22),(2027,9,22),(2027,10,29),(2027,12,17)],
        provisional: &[] },
    Bank { name: "RBA", currency: "AUD", zone: Zone::Sydney, hour: 14, minute: 30, window: 0,
        dates: &[(2026,2,3),(2026,3,17),(2026,5,5),(2026,6,16),(2026,8,11),(2026,9,29),(2026,11,3),(2026,12,8),
                 (2027,2,9),(2027,3,23),(2027,5,4),(2027,6,22),(2027,8,10),(2027,9,28),(2027,11,2),(2027,12,14)],
        provisional: &[] },
    // SEVEN scheduled decisions a year becomes EIGHT in 2027. Code that assumed
    // the 2026 shape would be wrong about a whole extra decision.
    Bank { name: "RBNZ", currency: "NZD", zone: Zone::Auckland, hour: 14, minute: 0, window: 0,
        dates: &[(2026,9,2),(2026,10,28),(2026,12,9),
                 (2027,2,10),(2027,3,17),(2027,5,5),(2027,6,16),(2027,8,4),(2027,9,15),(2027,10,27),(2027,12,8)],
        provisional: &[] },
    Bank { name: "BoC", currency: "CAD", zone: Zone::Toronto, hour: 9, minute: 45, window: 0,
        dates: &[(2026,9,2),(2026,10,28),(2026,12,9),
                 (2027,1,27),(2027,3,3),(2027,4,28),(2027,6,2),(2027,7,21),(2027,9,8),(2027,10,27),(2027,12,8)],
        provisional: &[] },
    // Only March and June 2027 could be confirmed from the SNB's own pages;
    // two reads of its event schedule disagreed on September and December.
    // Those two are ABSENT rather than carried on one unconfirmed reading.
    Bank { name: "SNB", currency: "CHF", zone: Zone::Zurich, hour: 9, minute: 30, window: 0,
        dates: &[(2026,3,19),(2026,6,18),(2026,9,24),(2026,12,10),(2027,3,18),(2027,6,24)],
        provisional: &[] },
];

/// Canada's Labour Force Survey. Here for one reason: it releases at 08:30
/// Eastern, the same instant as US payrolls, and the two shared a date in
/// **seven of twelve months in 2026**. USD/CAD takes two independent shocks at
/// once on those days, and sizing risk for one of them is sizing for half the
/// event.
const CANADA_LFS: [(i32, u32, u32); 12] = [
    (2026, 1, 9), (2026, 2, 6), (2026, 3, 13), (2026, 4, 10),
    (2026, 5, 8), (2026, 6, 5), (2026, 7, 10), (2026, 8, 7),
    (2026, 9, 4), (2026, 10, 9), (2026, 11, 6), (2026, 12, 4),
];

/// The last date the hand-entered tables are known good for.
pub const TABLE_GOOD_UNTIL: (i32, u32, u32) = (2027, 12, 31);

/// The FIRST month the hand-entered tables cover.
///
/// `BANKS` starts at 2026-01-28 and `CANADA_LFS` at 2026-01-09, so nothing
/// before January 2026 has a central-bank decision or a Canadian LFS date in
/// it at all.
///
/// This exists because `month` had a ceiling and no floor, and the asymmetry
/// was not deliberate. Above the ceiling it refused, loudly and for the right
/// reason — *"a calendar that keeps answering after its data runs out is
/// worse than one that stops."* Below the floor it answered: `RULES` are
/// algorithmic (payrolls is "the first Friday", and was in 2019 too), so a
/// 2019 month came back with payrolls and CPI in it and **no Fed meeting, no
/// ECB, no BoE, no Canadian LFS** — because those are tables, and the tables
/// start in 2026.
///
/// A partial calendar is worse than no calendar, and worse in the specific
/// way this tree keeps finding: it does not look partial. Ask "is there a
/// central-bank decision in this month" of March 2019 and the honest answer
/// is "I have no idea"; what came back was an empty list, which reads as
/// "no". Every consumer then treats a Fed day as an ordinary Wednesday.
///
/// So the floor refuses on the same principle the ceiling already did.
pub const TABLE_GOOD_FROM: (i32, u32) = (2026, 1);

/// Where the tables are known to be INCOMPLETE, and why.
///
/// A missing event looks exactly like a quiet quarter, and that is the failure
/// this reports rather than allows.
pub fn gaps(year: i32) -> Vec<(String, &'static str)> {
    let mut out = Vec::new();
    if year == 2027 {
        out.push((
            "SNB 2027".to_string(),
            "only the March and June assessments could be confirmed from the \
             SNB's own pages; two reads of its event schedule disagreed on \
             September and December",
        ));
    }
    out
}

pub fn bank_events(year: i32) -> Vec<Event> {
    let mut out = Vec::new();
    for b in &BANKS {
        for &(y, m, d) in b.dates {
            if y != year {
                continue;
            }
            let at = to_utc(b.zone, y, m, d, b.hour, b.minute);
            out.push(Event {
                name: format!("{} decision", b.name),
                at,
                currency: b.currency,
                impact: 3,
                tier: if b.provisional.contains(&year) { Tier::Provisional } else { Tier::Fixed },
                window_end: if b.window > 0 { Some(at + b.window * MS_PER_MIN) } else { None },
            });
        }
    }
    out.sort_by_key(|e| e.at);
    out
}

fn fixed_events(year: i32) -> Vec<Event> {
    CANADA_LFS
        .iter()
        .filter(|(y, _, _)| *y == year)
        .map(|&(y, m, d)| Event {
            name: "Canada Labour Force Survey".into(),
            at: to_utc(Zone::Toronto, y, m, d, 8, 30),
            currency: "CAD",
            impact: 2,
            tier: Tier::Fixed,
            window_end: None,
        })
        .collect()
}

/// Every modelled event landing in one calendar month, earliest first.
///
/// Keyed on when the release LANDS, not on the month it describes. Payrolls for
/// September comes out in October, and a calendar that filed it under September
/// would black out the wrong week.
pub fn month(y: i32, m: u32) -> Answer<Vec<Event>> {
    if (y, m) > (TABLE_GOOD_UNTIL.0, TABLE_GOOD_UNTIL.1) {
        return refuse(format!(
            "the hand-entered tables end {}-{:02}; asking about {}-{:02} would \
             return dates nobody has checked. Refusing rather than answering -- \
             a calendar that keeps answering after its data runs out is worse \
             than one that stops",
            TABLE_GOOD_UNTIL.0, TABLE_GOOD_UNTIL.1, y, m
        ));
    }
    // The same rule at the other end. See `TABLE_GOOD_FROM`: before it the
    // algorithmic rules still fire and the tables do not, so the answer was a
    // calendar with payrolls in it and no central bank anywhere -- which does
    // not read as incomplete, it reads as quiet.
    if (y, m) < TABLE_GOOD_FROM {
        return refuse(format!(
            "the hand-entered tables start {}-{:02}; asking about {}-{:02} would \
             give you the algorithmic releases -- payrolls, CPI -- and no central \
             bank decisions at all, because those are tables and the tables do \
             not go back that far. An empty list of Fed meetings reads as \"none\" \
             rather than \"unknown\", so this refuses instead",
            TABLE_GOOD_FROM.0, TABLE_GOOD_FROM.1, y, m
        ));
    }
    let mut out = Vec::new();
    for (ry, rm) in [
        if m == 1 { (y - 1, 12) } else { (y, m - 1) },
        (y, m),
        if m == 12 { (y + 1, 1) } else { (y, m + 1) },
    ] {
        for r in &RULES {
            if let Some(e) = r.on(ry, rm) {
                let t = Utc::from_ms(e.at);
                if t.year == y && t.month == m {
                    out.push(e);
                }
            }
        }
    }
    for e in bank_events(y).into_iter().chain(fixed_events(y)) {
        let t = Utc::from_ms(e.at);
        if t.year == y && t.month == m {
            out.push(e);
        }
    }
    out.sort_by_key(|e| e.at);
    out.dedup_by(|a, b| a.name == b.name && a.at == b.at);
    Ok(out)
}

/// Events whose disturbance window overlaps a SPAN of time.
///
/// A forward look from an instant cannot answer this and should not be bent to.
/// A BAR is a span: the H4 bar closing at 16:00 covers 12:00–16:00, so a
/// release at 12:30 whose window shut at 14:30 is inside that bar even though
/// it is firmly in the past by the time the bar closes. Above M5, "was this bar
/// disturbed" is a question about an interval, and asking it about an instant
/// gets the most disturbed bar of the week wrong.
pub fn overlapping(from: i64, to: i64, instrument: &str, purpose: Purpose) -> Answer<Vec<Event>> {
    if to < from {
        return refuse("a span cannot end before it starts");
    }
    let a = Utc::from_ms(from);
    let b = Utc::from_ms(to);
    let mut out = Vec::new();
    let mut seen: Vec<(String, i64)> = Vec::new();
    for (y, m) in [(a.year, a.month), (b.year, b.month)] {
        for e in month(y, m)? {
            if !instrument.is_empty() && !e.touches(instrument) {
                continue;
            }
            let (wf, wt) = e.blackout(purpose);
            if wf < to && from < wt && !seen.contains(&(e.name.clone(), e.at)) {
                seen.push((e.name.clone(), e.at));
                out.push(e);
            }
        }
    }
    out.sort_by_key(|e| e.at);
    Ok(out)
}

/// (inside, the events responsible) at one instant.
///
/// Says WHICH, not just whether: a refusal that cannot name its reason is not
/// reviewable.
pub fn inside_blackout(at: i64, instrument: &str, purpose: Purpose) -> Answer<(bool, Vec<Event>)> {
    let hit = overlapping(at, at, instrument, purpose)?;
    Ok((!hit.is_empty(), hit))
}

/// Pairs of events landing close enough together to compound.
pub fn collisions(y: i32, m: u32, within_minutes: i64) -> Answer<Vec<(Event, Event)>> {
    let ev = month(y, m)?;
    let mut out = Vec::new();
    for i in 0..ev.len() {
        for j in i + 1..ev.len() {
            if (ev[j].at - ev[i].at).abs() <= within_minutes * MS_PER_MIN
                && ev[i].currency != ev[j].currency
            {
                out.push((ev[i].clone(), ev[j].clone()));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Release dates from the BLS Employment Situation schedule itself, keyed by
    /// the month they describe. Ground truth, not a reconstruction.
    const PUBLISHED: [((i32, u32), (i32, u32, u32)); 13] = [
        ((2025, 11), (2025, 12, 16)), ((2025, 12), (2026, 1, 9)),
        ((2026, 1), (2026, 2, 11)),   ((2026, 2), (2026, 3, 6)),
        ((2026, 3), (2026, 4, 3)),    ((2026, 4), (2026, 5, 8)),
        ((2026, 5), (2026, 6, 5)),    ((2026, 6), (2026, 7, 2)),
        ((2026, 7), (2026, 8, 7)),    ((2026, 8), (2026, 9, 4)),
        ((2026, 9), (2026, 10, 2)),   ((2026, 10), (2026, 11, 6)),
        ((2026, 11), (2026, 12, 4)),
    ];

    #[test]
    fn the_payrolls_rule_reproduces_thirteen_published_dates() {
        for ((y, m), want) in PUBLISHED {
            assert_eq!(nfp_date(y, m), want, "reference {}-{:02}", y, m);
        }
    }

    #[test]
    fn the_arithmetic_covers_every_month_the_shutdown_did_not_move() {
        // Three of thirteen are hand-listed, all three fallout from the same
        // 2025 appropriations lapse, and one released on a WEDNESDAY, which no
        // weekday rule can produce. The arithmetic gets every other month,
        // including the holiday shift in July -- so the exceptions are one event
        // in history, not a pattern the rule missed.
        assert_eq!(NFP_EXCEPTIONS.len(), 3);
        let odd = PUBLISHED.iter().find(|((y, m), _)| (*y, *m) == (2026, 1)).unwrap().1;
        assert_eq!(Utc::date(odd.0, odd.1, odd.2).weekday(), 2, "February 2026 was a Wednesday");
        let clean = PUBLISHED.iter().filter(|(r, _)| !NFP_EXCEPTIONS.iter().any(|(e, _)| e == r));
        assert_eq!(clean.clone().count(), 10);
        for ((y, m), want) in clean {
            assert_eq!(nfp_date(*y, *m), *want);
        }
    }

    #[test]
    fn first_friday_is_wrong_in_four_months_of_the_year() {
        let wrong = PUBLISHED
            .iter()
            .filter(|(_, (y, m, d))| *d != nth_weekday(*y, *m, 4, 1))
            .count();
        assert!(wrong >= 4, "only {} disagree with 'first Friday'", wrong);
    }

    #[test]
    fn a_holiday_pulls_a_release_earlier_never_later() {
        // 3 July 2026 is the observed Independence Day, which is why that
        // month's release was a Thursday. Releases are never postponed.
        let (y, m, d) = nfp_date(2026, 6);
        assert_eq!((y, m, d), (2026, 7, 2));
        assert!(!us_holidays(y).contains(&(y, m, d)));
    }

    #[test]
    fn a_release_time_moves_in_utc_when_the_local_clock_changes() {
        // 08:30 New York is 12:30 UTC in summer and 13:30 in winter -- the
        // whole reason a rule stores a zone and not a UTC hour.
        let summer = month(2026, 7).unwrap();
        let winter = month(2026, 1).unwrap();
        let s = summer.iter().find(|e| e.name.starts_with("Non-Farm")).unwrap();
        let w = winter.iter().find(|e| e.name.starts_with("Non-Farm")).unwrap();
        assert_eq!(Utc::from_ms(s.at).hour, 12, "{}", s.say());
        assert_eq!(Utc::from_ms(w.at).hour, 13, "{}", w.say());
    }

    #[test]
    fn the_blackout_is_asked_for_by_purpose_not_by_convention() {
        let ev = month(2026, 10).unwrap();
        let nfp = ev.iter().find(|e| e.name.starts_with("Non-Farm")).unwrap();
        let ex = nfp.blackout(Purpose::Execution);
        let vol = nfp.blackout(Purpose::Volatility);
        assert_eq!((ex.1 - ex.0) / MS_PER_MIN, 6);
        assert_eq!((vol.1 - vol.0) / MS_PER_MIN, 150);
        assert!(vol.1 > ex.1, "the volatility window must outlast the jump");
    }

    #[test]
    fn the_measured_window_is_an_order_of_magnitude_past_the_convention() {
        let ev = month(2026, 10).unwrap();
        let nfp = ev.iter().find(|e| e.name.starts_with("Non-Farm")).unwrap();
        let measured = nfp.blackout(Purpose::Volatility).1 - nfp.at;
        assert!(measured > 10 * 2 * MS_PER_MIN, "the industry's two minutes is nothing like it");
    }

    #[test]
    fn payrolls_gets_the_longer_window_the_measurement_supports() {
        let ev = month(2026, 10).unwrap();
        let nfp = ev.iter().find(|e| e.name.starts_with("Non-Farm")).unwrap();
        let other = ev.iter().find(|e| e.name.starts_with("ISM")).unwrap();
        assert!(nfp.blackout(Purpose::Volatility).1 - nfp.at
                > other.blackout(Purpose::Volatility).1 - other.at);
    }

    #[test]
    fn an_event_knows_which_pairs_it_moves() {
        let ev = month(2026, 10).unwrap();
        let nfp = ev.iter().find(|e| e.name.starts_with("Non-Farm")).unwrap();
        assert!(nfp.touches("EURUSD") && nfp.touches("USDJPY"));
        assert!(!nfp.touches("EURGBP"));
    }

    #[test]
    fn a_us_event_reaches_a_pair_with_no_dollar_in_it() {
        // Evidenced rather than assumed: AUD/JPY is mechanically the two ends
        // of the carry cross-section, so a US risk event unwinds it.
        let ev = month(2026, 10).unwrap();
        let nfp = ev.iter().find(|e| e.name.starts_with("Non-Farm")).unwrap();
        assert!(nfp.touches("AUDJPY"), "the risk channel is not wired up");
        assert!(!nfp.touches("EURGBP"), "but it must not reach everything");
    }

    #[test]
    fn the_boj_is_a_window_because_it_has_no_fixed_time() {
        let boj: Vec<Event> = bank_events(2026).into_iter().filter(|e| e.name.starts_with("BoJ")).collect();
        assert!(!boj.is_empty());
        assert!(boj.iter().all(|e| e.is_window()));
        assert!(boj[0].say().contains("the delay is itself a signal"));
        let fomc: Vec<Event> = bank_events(2026).into_iter().filter(|e| e.name.starts_with("FOMC")).collect();
        assert!(fomc.iter().all(|e| !e.is_window()), "only the BoJ lacks a fixed time");
    }

    #[test]
    fn all_eight_central_banks_are_in_the_calendar() {
        let mut got: Vec<&str> = bank_events(2026).iter().map(|e| {
            BANKS.iter().find(|b| e.name.starts_with(b.name)).unwrap().name
        }).collect();
        got.sort();
        got.dedup();
        assert_eq!(got.len(), 8, "{:?}", got);
    }

    #[test]
    fn the_calendar_reaches_into_2027() {
        let got = bank_events(2027);
        assert!(got.len() >= 50, "{}", got.len());
    }

    #[test]
    fn the_rbnz_gained_a_meeting_in_2027() {
        // Seven scheduled decisions a year became eight. A rule derived from
        // the 2026 shape would have been wrong about an entire decision.
        let n26 = bank_events(2026).iter().filter(|e| e.name.starts_with("RBNZ")).count();
        let n27 = bank_events(2027).iter().filter(|e| e.name.starts_with("RBNZ")).count();
        assert_eq!(n27, 8);
        assert!(n27 > n26);
    }

    #[test]
    fn a_banks_own_provisional_label_survives_onto_the_record() {
        let boe27: Vec<Event> = bank_events(2027).into_iter().filter(|e| e.name.starts_with("BoE")).collect();
        let boe26: Vec<Event> = bank_events(2026).into_iter().filter(|e| e.name.starts_with("BoE")).collect();
        assert!(boe27.iter().all(|e| e.tier == Tier::Provisional));
        assert!(boe26.iter().all(|e| e.tier == Tier::Fixed));
    }

    #[test]
    fn an_unconfirmable_date_is_left_out_and_the_gap_is_reported() {
        // Carrying them on one unconfirmed reading would make the calendar
        // confidently wrong; dropping them silently would make a missing
        // assessment look like a quiet quarter. So they are absent AND named.
        let snb = bank_events(2027).into_iter().filter(|e| e.name.starts_with("SNB")).count();
        assert_eq!(snb, 2);
        let g = gaps(2027);
        assert!(g.iter().any(|(w, _)| w.contains("SNB")));
        assert!(g.iter().any(|(_, w)| w.contains("disagreed")));
        assert!(gaps(2026).is_empty());
    }

    #[test]
    fn the_snb_dates_are_not_derivable_which_is_why_they_are_listed() {
        // Third, third, fourth and second Thursday in 2026. A "third Thursday
        // of the quarter-end month" rule would be wrong half the time.
        let snb = BANKS.iter().find(|b| b.name == "SNB").unwrap();
        let nths: Vec<u32> = snb.dates.iter().filter(|(y, _, _)| *y == 2026)
            .map(|(_, _, d)| (d - 1) / 7 + 1).collect();
        assert!(nths.windows(2).any(|w| w[0] != w[1]), "{:?}", nths);
    }

    #[test]
    fn asking_past_the_end_of_the_tables_refuses() {
        let e = month(2030, 6);
        assert!(e.is_err());
        assert!(e.unwrap_err().0.contains("runs out"));
    }

    #[test]
    fn asking_before_the_start_of_the_tables_refuses_too() {
        // The ceiling had a floor missing under it, and the asymmetry was not
        // deliberate: above 2027 it refused, below 2026 it answered. What it
        // answered with is the part that matters.
        let e = month(2019, 3);
        assert!(
            e.is_err(),
            "March 2019 was answered. The algorithmic rules fire for any year, so \
             the answer is payrolls and CPI with no central bank in it -- and an \
             empty list of Fed meetings reads as \"none\", not \"unknown\""
        );
        let why = e.unwrap_err().0;
        assert!(
            why.contains("central bank"),
            "the refusal does not say what is missing: {why}"
        );
    }

    #[test]
    fn the_floor_is_where_the_tables_actually_start() {
        // Read off the tables rather than trusted, so moving one and not the
        // other fails here. A floor set later than the data would throw away
        // months that are genuinely covered; earlier, and the refusal is a
        // lie.
        let first_bank = BANKS
            .iter()
            .flat_map(|b| b.dates.iter())
            .map(|&(y, m, _)| (y, m))
            .min()
            .expect("some bank dates");
        let first_lfs = CANADA_LFS.iter().map(|&(y, m, _)| (y, m)).min().expect("some LFS dates");
        assert_eq!(
            TABLE_GOOD_FROM,
            first_bank.min(first_lfs),
            "TABLE_GOOD_FROM does not match the earliest hand-entered date"
        );
        assert!(month(TABLE_GOOD_FROM.0, TABLE_GOOD_FROM.1).is_ok(), "the floor itself refuses");
        let before = if TABLE_GOOD_FROM.1 == 1 {
            (TABLE_GOOD_FROM.0 - 1, 12)
        } else {
            (TABLE_GOOD_FROM.0, TABLE_GOOD_FROM.1 - 1)
        };
        assert!(month(before.0, before.1).is_err(), "the month below the floor is answered");
    }

    #[test]
    fn a_span_finds_a_release_the_forward_look_cannot() {
        // Payrolls at 12:30Z, window 12:00-14:30Z. At 16:00Z an instant check
        // correctly reports nothing -- the window shut ninety minutes ago. But
        // the H4 bar closing at 16:00Z covers 12:00-16:00Z and contains it all.
        let at = Utc::at(2026, 10, 2, 16, 0).to_ms();
        let (inside, _) = inside_blackout(at, "EURUSD", Purpose::Volatility).unwrap();
        assert!(!inside, "the instant check should see nothing here");
        let span = overlapping(at - 4 * MS_PER_HOUR, at, "EURUSD", Purpose::Volatility).unwrap();
        assert!(span.iter().any(|e| e.name.starts_with("Non-Farm")),
                "{:?}", span.iter().map(|e| e.name.clone()).collect::<Vec<_>>());
    }

    #[test]
    fn a_five_minute_bar_at_the_same_close_is_clear_of_it() {
        let at = Utc::at(2026, 10, 2, 16, 0).to_ms();
        let span = overlapping(at - 5 * MS_PER_MIN, at, "EURUSD", Purpose::Volatility).unwrap();
        assert!(span.is_empty());
    }

    #[test]
    fn a_backwards_span_is_refused() {
        let at = Utc::at(2026, 10, 2, 16, 0).to_ms();
        assert!(overlapping(at, at - MS_PER_HOUR, "", Purpose::Volatility).is_err());
    }

    #[test]
    fn a_blackout_names_the_event_responsible() {
        let ev = month(2026, 10).unwrap();
        let nfp = ev.iter().find(|e| e.name.starts_with("Non-Farm")).unwrap();
        let (inside, why) = inside_blackout(nfp.at, "EURUSD", Purpose::Volatility).unwrap();
        assert!(inside);
        assert!(why.iter().any(|e| e.name == nfp.name));
        let (clear, none) = inside_blackout(nfp.at + 5 * 86_400_000, "EURUSD", Purpose::Volatility).unwrap();
        assert!(!clear && none.is_empty());
    }

    #[test]
    fn a_pair_the_event_does_not_move_is_left_trading() {
        let ev = month(2026, 10).unwrap();
        let nfp = ev.iter().find(|e| e.name.starts_with("Non-Farm")).unwrap();
        let (inside, _) = inside_blackout(nfp.at, "EURGBP", Purpose::Volatility).unwrap();
        assert!(!inside, "EURGBP has no dollar in it and is not a carry pair");
    }

    #[test]
    fn payrolls_and_the_canadian_jobs_report_are_seen_landing_together() {
        // Both at 08:30 Eastern, sharing a date in seven of twelve months in
        // 2026. USD/CAD takes two independent shocks; sizing for one is sizing
        // for half the event.
        let months = (1..=12u32)
            .filter(|&m| {
                collisions(2026, m, 5)
                    .unwrap()
                    .iter()
                    .any(|(a, b)| {
                        let mut s = [a.currency, b.currency];
                        s.sort();
                        s == ["CAD", "USD"]
                    })
            })
            .count();
        assert!(months >= 5, "found the USD/CAD double in only {} months", months);
    }

    #[test]
    fn releases_measured_to_have_no_effect_are_not_in_the_calendar() {
        // PPI, housing starts, leading indicators and the GDP revisions were
        // each measured and found to have no significant effect on FX, and they
        // sit on nearly every vendor's high-impact list. Carrying them would
        // mean blacking out trading for events that do not move.
        let names: String = RULES.iter().map(|r| r.name.to_lowercase()).collect::<Vec<_>>().join(" ");
        for dud in ["ppi", "producer price", "housing start", "leading indicator", "money supply"] {
            assert!(!names.contains(dud), "{} was measured to have no effect", dud);
        }
    }
}
