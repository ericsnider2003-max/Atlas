//! Everyday money, not just trading.
//!
//! Trading has its own module because its rules are strange — wash sales,
//! Section 1256, mark-to-market. This is the other 95% of your financial life,
//! which has no strange rules and is still where most of the money goes.
//!
//! The point isn't budgeting. Budgets are a plan you fail at in month two.
//! This is about **knowing where it actually went**, which is a different and
//! much more answerable question.

use serde::{Deserialize, Serialize};

/// What a transaction is, in the terms you'd use talking about it.
///
/// Deliberately not an accounting chart — nobody thinks in "cost of goods
/// sold". These are the buckets people actually reason about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Bucket {
    /// Rent, mortgage, council tax, utilities.
    RoofAndBills,
    Food,
    /// Getting places.
    Travel,
    /// Insurance, phone, subscriptions — the ones that go out whether you
    /// think about them or not.
    Standing,
    /// Things you chose to buy.
    Spending,
    /// For the business or the content.
    Work,
    /// Into savings or investments.
    Kept,
    /// Money in.
    Income,
    /// Between your own accounts. Not spending, and counting it as such is
    /// the single commonest way a summary lies to you.
    MovingYourOwnMoney,
    Unknown,
}

impl Bucket {
    pub fn plain(&self) -> &'static str {
        match self {
            Bucket::RoofAndBills => "roof and bills",
            Bucket::Food => "food",
            Bucket::Travel => "getting about",
            Bucket::Standing => "things that go out anyway",
            Bucket::Spending => "things you chose",
            Bucket::Work => "work",
            Bucket::Kept => "kept",
            Bucket::Income => "in",
            Bucket::MovingYourOwnMoney => "moved between your own accounts",
            Bucket::Unknown => "not sorted",
        }
    }

    /// Does this count as money leaving?
    fn is_spending(&self) -> bool {
        !matches!(
            self,
            Bucket::Income | Bucket::Kept | Bucket::MovingYourOwnMoney | Bucket::Unknown
        )
    }

    /// Could you change it this month if you had to?
    ///
    /// The distinction that makes a summary useful: telling someone their rent
    /// is their biggest expense is not information.
    fn changeable_quickly(&self) -> bool {
        matches!(self, Bucket::Food | Bucket::Spending | Bucket::Travel)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub description: String,
    /// Negative is out.
    pub amount: f32,
    pub at: u64,
    pub bucket: Bucket,
    /// You corrected the bucket, so it stays corrected.
    pub confirmed: bool,
    /// Which account.
    pub account: String,
}

const ROOF: &[&str] = &["rent", "mortgage", "council tax", "electric", "gas ", "water", "internet", "broadband"];
const FOOD: &[&str] = &["tesco", "sainsbury", "aldi", "lidl", "asda", "waitrose", "grocer", "supermarket",
                        "kroger", "safeway", "whole foods", "trader joe", "restaurant", "cafe", "coffee",
                        "deliveroo", "uber eats", "doordash", "just eat"];
const TRAVEL: &[&str] = &["uber", "lyft", "petrol", "fuel", "shell", "bp ", "chevron", "parking",
                          "train", "rail", "transit", "airline", "flight"];
const STANDING: &[&str] = &["insurance", "netflix", "spotify", "subscription", "membership",
                            "phone", "mobile", "gym", "icloud", "adobe", "microsoft 365"];
const WORK: &[&str] = &["adobe", "hosting", "domain", "aws", "camera", "lens", "microphone",
                        "software", "vps", "server"];
const KEPT: &[&str] = &["transfer to savings", "isa", "pension", "401k", "vanguard", "investment"];
const OWN_ACCOUNTS: &[&str] = &["transfer", "internal", "to savings", "from savings", "own account", "payment to card"];

/// Where the month being answered about is kept.
///
/// Two of them, because the useful thing a money summary finds is what
/// changed, and that needs something to change from.
pub const THIS_MONTH: &str = "money_this_month";
pub const LAST_MONTH: &str = "money_last_month";

/// The months that used to fall off the edge.
///
/// `--keep` shifted this month into last and dropped what was there — so the
/// two-month window was the entire memory, and every question of the shape
/// "is this NORMAL for me?" was unanswerable. `judgment::ordinary_for` has
/// been ready to answer it the whole time; this record is the retained months
/// it never had.
pub const MONTHS: &str = "money_months";

/// How many kept months to retain. Two years: enough that an annual bill has
/// been seen twice, small enough that the file stays trivial.
pub const MONTHS_KEPT: usize = 24;

/// Months of history before a per-bucket baseline is worth stating.
///
/// Six, not `judgment`'s default sample floor — that default is tuned for
/// trades, which arrive daily. Months arrive monthly, and below six the
/// median of them is a fact about which months happened rather than about
/// you.
pub const MONTHS_FLOOR: usize = 6;

/// One kept month, summarised at the moment it was kept.
///
/// The summary, not the entries: the baseline needs one number per bucket
/// per month, and keeping the raw entries here would grow a second copy of
/// the statement store with a different retention.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeptMonth {
    pub at: u64,
    pub by_bucket: Vec<(Bucket, f32)>,
}

/// File one more month into the retained series, oldest out first.
pub fn remember_month(months: &mut Vec<KeptMonth>, entries: &[Entry], at: u64) {
    let month = summarise(entries);
    months.push(KeptMonth { at, by_bucket: month.by_bucket });
    if months.len() > MONTHS_KEPT {
        let drop = months.len() - MONTHS_KEPT;
        months.drain(0..drop);
    }
}

/// Buckets unusual against YOUR months, judged rather than thresholded.
///
/// This is what `finance.category_jump`'s single fraction was the wrong
/// shape for, in `judgment`'s own words: groceries moving 50% is an
/// emergency and "things you chose" moving 50% is a Tuesday, because the two
/// buckets have different ordinary variation. Each bucket is measured
/// against its own retained months — median and mean absolute deviation via
/// `judgment::ordinary_for` — and reported in units of its own wobble.
///
/// Quiet below [`MONTHS_FLOOR`] months and below two spreads: a baseline
/// report that speaks every month is one you stop reading.
pub fn unusual_buckets(months: &[KeptMonth], current: &[Entry]) -> Vec<String> {
    if months.len() < MONTHS_FLOOR {
        return Vec::new();
    }
    let now = summarise(current);
    let mut out = Vec::new();
    for (b, total) in &now.by_bucket {
        if !b.is_spending() {
            continue;
        }
        // A month with nothing in this bucket is a real observation of
        // zero, not a missing one — skipping it would teach the baseline
        // that you spend every month on the thing you sometimes buy.
        let past: Vec<f64> = months
            .iter()
            .map(|m| {
                m.by_bucket
                    .iter()
                    .find(|(pb, _)| pb == b)
                    .map(|(_, t)| *t as f64)
                    .unwrap_or(0.0)
            })
            .collect();
        let Some((usually, varies_by)) = crate::judgment::ordinary_for(&past) else {
            continue;
        };
        let m = crate::judgment::Measured {
            name: b.plain(),
            value: *total as f64,
            usually,
            varies_by,
            seen: past.len(),
        };
        let cfg = crate::judgment::JudgmentConfig {
            min_seen: MONTHS_FLOOR,
            ..Default::default()
        };
        let Some(spreads) = crate::judgment::how_unusual(&m, &cfg) else {
            continue;
        };
        if spreads.abs() < 2.0 {
            continue;
        }
        out.push(format!(
            "{} is {:.0} this month against your usual {:.0} — {:.1}× its own \
             ordinary variation, over {} kept months",
            b.plain(),
            total,
            usually,
            spreads.abs(),
            past.len()
        ));
    }
    out
}

/// Sort one entry.
///
/// Order matters: a transfer that mentions a supermarket name is still a
/// transfer, and a work expense that's also a subscription is a work expense
/// if you said so.
pub fn sort_one(description: &str, amount: f32, work_words: &[String]) -> Bucket {
    let d = description.to_lowercase();

    if amount > 0.0 && !OWN_ACCOUNTS.iter().any(|w| d.contains(w)) {
        return Bucket::Income;
    }
    // Checked before anything else. Counting a transfer as spending is how a
    // summary tells you that you spent twice what you did.
    if OWN_ACCOUNTS.iter().any(|w| d.contains(w)) {
        return Bucket::MovingYourOwnMoney;
    }
    if KEPT.iter().any(|w| d.contains(w)) {
        return Bucket::Kept;
    }
    // Your own list wins over the guesses.
    if work_words.iter().any(|w| d.contains(&w.to_lowercase())) {
        return Bucket::Work;
    }
    if ROOF.iter().any(|w| d.contains(w)) {
        return Bucket::RoofAndBills;
    }
    if FOOD.iter().any(|w| d.contains(w)) {
        return Bucket::Food;
    }
    if TRAVEL.iter().any(|w| d.contains(w)) {
        return Bucket::Travel;
    }
    if STANDING.iter().any(|w| d.contains(w)) {
        return Bucket::Standing;
    }
    if WORK.iter().any(|w| d.contains(w)) {
        return Bucket::Work;
    }
    Bucket::Unknown
}

/// A month, summarised.
#[derive(Debug, Clone, PartialEq)]
pub struct Month {
    pub in_total: f32,
    pub out_total: f32,
    pub by_bucket: Vec<(Bucket, f32)>,
    /// Things that go out every month whether you think about them or not.
    pub standing_total: f32,
    /// Couldn't be sorted.
    pub unsorted: usize,
}

pub fn summarise(entries: &[Entry]) -> Month {
    let mut by: Vec<(Bucket, f32)> = Vec::new();
    let (mut inn, mut out) = (0.0f32, 0.0f32);

    for e in entries {
        if e.bucket == Bucket::MovingYourOwnMoney {
            continue;
        }
        if e.amount > 0.0 {
            inn += e.amount;
        } else if e.bucket.is_spending() {
            out += -e.amount;
        }
        match by.iter_mut().find(|(b, _)| *b == e.bucket) {
            Some((_, t)) => *t += e.amount.abs(),
            None => by.push((e.bucket, e.amount.abs())),
        }
    }
    by.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    Month {
        in_total: inn,
        out_total: out,
        standing_total: by
            .iter()
            .find(|(b, _)| *b == Bucket::Standing)
            .map(|(_, t)| *t)
            .unwrap_or(0.0),
        by_bucket: by,
        unsorted: entries.iter().filter(|e| e.bucket == Bucket::Unknown).count(),
    }
}

/// Something that started going out and you may not have noticed.
///
/// The most useful thing a money summary can find, and the thing nobody looks
/// for: a subscription that appeared, or one whose price went up.
/// `rose_by` is `finance.category_jump` — the fraction a thing has to climb
/// before it is worth saying. It hardcoded 15% here and was 50% in the config,
/// so the file said one thing and the code did another, which is the shape
/// this campaign keeps finding.
pub fn new_or_grown(this_month: &[Entry], last_month: &[Entry], rose_by: f32) -> Vec<String> {
    let mut out = Vec::new();
    // One line per thing, not per charge. A subscription billed twice in a
    // month is one subscription, and saying "Netflix is new" twice is how a
    // short, useful list turns into one you skim.
    let mut already: Vec<String> = Vec::new();
    for e in this_month.iter().filter(|e| e.bucket == Bucket::Standing) {
        let key = e.description.to_lowercase();
        if already.contains(&key) {
            continue;
        }
        already.push(key);
        let before = last_month
            .iter()
            .find(|p| p.description.to_lowercase() == e.description.to_lowercase());
        match before {
            None => out.push(format!("{} is new — {:.2} a month", e.description, -e.amount)),
            Some(p) if -e.amount > -p.amount * (1.0 + rose_by) => out.push(format!(
                "{} went from {:.2} to {:.2}",
                e.description, -p.amount, -e.amount
            )),
            _ => {}
        }
    }
    out
}

/// A whole bucket that climbed, which is what `finance.category_jump` says it
/// is for.
///
/// The setting reads "flag a category whose spend rises more than this
/// fraction month on month" and nothing did that: `finance::review` checks
/// large transactions, duplicates and repeat billing, and never compares two
/// months. `new_or_grown` compares months and works one standing charge at a
/// time. Neither is the check the setting describes, and a threshold for a
/// check nobody wrote is the worst kind of dead setting — the file reads as
/// though the check exists.
pub fn buckets_that_jumped(this_month: &[Entry], last_month: &[Entry], rose_by: f32) -> Vec<String> {
    let this = summarise(this_month);
    let last = summarise(last_month);
    let mut out = Vec::new();
    for (b, now) in &this.by_bucket {
        if !b.is_spending() {
            continue;
        }
        let Some((_, before)) = last.by_bucket.iter().find(|(o, _)| o == b) else {
            // A bucket with nothing in it last month is not a rise of
            // infinity. It is a bucket you did not have, which `new_or_grown`
            // says better, one line at a time.
            continue;
        };
        if *before > 0.0 && *now > *before * (1.0 + rose_by) {
            out.push(format!(
                "{} went from {:.0} to {:.0}",
                b.plain(),
                before,
                now
            ));
        }
    }
    out
}

/// What Atlas says about a month.
///
/// Not the total — you have a banking app for that. The biggest thing you
/// could actually change, and anything that appeared without you noticing.
pub fn spoken(m: &Month, changes: &[String]) -> String {
    if m.by_bucket.is_empty() {
        return "Nothing to go on.".into();
    }
    let mut s = String::new();

    // The biggest changeable thing, not the biggest thing. Telling you your
    // rent is your largest expense is not information.
    if let Some((b, amount)) = m
        .by_bucket
        .iter()
        .find(|(b, _)| b.is_spending() && b.changeable_quickly())
    {
        s.push_str(&format!("{:.0} on {} this month.", amount, b.plain()));
    }

    if m.standing_total > 0.0 {
        s.push_str(&format!(
            " {:.0} went out on things you don't think about.",
            m.standing_total
        ));
    }
    // The thing nobody looks for.
    if let Some(first) = changes.first() {
        s.push_str(&format!(" {first}."));
    }
    if m.unsorted > 3 {
        s.push_str(&format!(" {} I couldn't place.", m.unsorted));
    }
    s
}

/// Money going out for the business or the content, kept apart.
///
/// Not because the tax rules differ from trading — they do — but because
/// mixing them means you find out what the content actually cost you at the
/// end of the year rather than as you go.
pub fn work_spend(entries: &[Entry]) -> f32 {
    entries
        .iter()
        .filter(|e| e.bucket == Bucket::Work && e.amount < 0.0)
        .map(|e| -e.amount)
        .sum()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct MoneyConfig {
    pub enabled: bool,
    /// Words that mean a transaction is for work, in your terms.
    pub work_words: Vec<String>,
    /// Never advises. Not configurable — Atlas reads statements, it doesn't
    /// tell you what to do with money.
    #[serde(skip, default = "never")]
    pub gives_advice: bool,
}

fn never() -> bool {
    false
}

impl Default for MoneyConfig {
    fn default() -> Self {
        MoneyConfig { enabled: false, work_words: Vec::new(), gives_advice: false }
    }
}

/// The line Atlas holds.
pub const NOT_ADVICE: &str =
    "I sort what already happened and tell you what I see. What to do about it is yours — I'm \
     not a financial adviser and a system that reads your statements is the last thing that \
     should be telling you where to put your money.";
