//! Reading your statements, and knowing enough about money to be useful.
//!
//! Phase 2.5, and the oldest item in your spec. Entirely local: files you
//! already have, no credentials, nothing leaves the machine.
//!
//! ## On the tax knowledge
//!
//! Atlas knowing the rules is the difference between "you spent $4,200 on
//! software" and "that's likely deductible, and the receipts need keeping for
//! three years". But it is knowledge, not advice — it can tell you what a
//! category usually is and what a rule generally says. It cannot tell you what
//! your situation is, and for a trader the rules are genuinely unusual, so it
//! says so rather than pretending.
//!
//! Everything here is US federal, general, and current to the model's
//! knowledge. Anything that matters is worth checking with someone whose job
//! it is.

use crate::finance::Transaction;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Software,
    Hardware,
    Internet,
    Phone,
    Office,
    Travel,
    Meals,
    Education,
    Fees,
    Professional,
    Marketing,
    Subscription,
    Groceries,
    Housing,
    Utilities,
    Transport,
    Health,
    Entertainment,
    Income,
    Transfer,
    /// Money moving in or out of a brokerage.
    Investment,
    Unknown,
}

impl Category {
    /// Is this the sort of thing a business commonly deducts?
    ///
    /// "Commonly" is doing real work here. Whether *your* spend qualifies
    /// depends on whether it's ordinary and necessary for your trade, which is
    /// a judgement about your situation and not about the category.
    pub fn often_deductible(&self) -> bool {
        matches!(
            self,
            Category::Software
                | Category::Hardware
                | Category::Internet
                | Category::Phone
                | Category::Office
                | Category::Travel
                | Category::Education
                | Category::Fees
                | Category::Professional
                | Category::Marketing
                | Category::Subscription
        )
    }

    /// What Atlas can say about it, honestly.
    pub fn note(&self) -> Option<&'static str> {
        Some(match self {
            Category::Meals =>
                "Business meals are generally 50% deductible, and you need to record who and why. \
                 Meals on your own while working usually don't count.",
            Category::Internet | Category::Phone =>
                "Only the business share is deductible, and you need a defensible way of working \
                 out what that share is.",
            Category::Office =>
                "A home office has to be used regularly and exclusively for business — a desk in \
                 the corner of a room you also live in generally doesn't qualify.",
            Category::Hardware =>
                "Equipment can often be deducted in the year you buy it rather than depreciated, \
                 but there are limits and elections involved.",
            Category::Travel =>
                "Travel needs a business purpose and the days need to be mostly business. Keep \
                 the itinerary, not just the receipt.",
            Category::Education =>
                "Training that maintains or improves skills in your current trade is usually \
                 deductible. Training that qualifies you for a new one usually isn't.",
            Category::Investment =>
                "Brokerage transfers aren't income or expense — they're your own money moving. \
                 The gains are what matter, and they're on the 1099.",
            Category::Fees =>
                "Platform, data and exchange fees are usually a business expense if you're \
                 trading as a business. If you're not, the treatment is different.",
            _ => return None,
        })
    }
}

/// What Atlas knows about the rules that catch people out.
///
/// Deliberately short and about *traps* rather than a tax course. These are
/// the ones that cost money when nobody mentions them.
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub name: &'static str,
    pub what: &'static str,
    /// Why it matters to you specifically.
    pub why_you: &'static str,
}

pub fn trading_rules() -> Vec<Rule> {
    vec![
        Rule {
            name: "wash sales",
            what: "Selling at a loss and buying the same or a substantially identical security \
                   within 30 days either side disallows the loss — it gets added to the basis of \
                   the new position instead.",
            why_you: "If you trade the same instruments repeatedly, this can quietly disallow a \
                      lot of losses you thought you'd realised.",
        },
        Rule {
            name: "short vs long term",
            what: "Held a year or less, gains are taxed as ordinary income. Over a year, at \
                   long-term rates, which are lower.",
            why_you: "Almost everything an active trader does is short-term, so the lower rate \
                      rarely applies.",
        },
        Rule {
            name: "Section 1256 contracts",
            what: "Regulated futures and some index options are marked to market at year end and \
                   taxed 60% long-term, 40% short-term, whatever the holding period.",
            why_you: "If you trade futures, this is usually better than ordinary income \
                      treatment — and it applies whether or not you closed the position.",
        },
        Rule {
            name: "trader tax status",
            what: "Trading as a business rather than as an investor changes what's deductible and \
                   opens up a mark-to-market election. The bar is substantial, frequent, \
                   continuous activity.",
            why_you: "It's the difference between your platform and data costs being deductible \
                      and not. It is also frequently claimed and frequently denied.",
        },
        Rule {
            name: "mark-to-market election",
            what: "Traders with trader status can elect to treat gains and losses as ordinary, \
                   which removes wash sale rules and the capital loss limit.",
            why_you: "The election has to be made by the filing deadline for the *previous* \
                      year — miss the date and you wait a year.",
        },
        Rule {
            name: "the capital loss limit",
            what: "Net capital losses beyond gains are limited to $3,000 against ordinary income \
                   per year; the rest carries forward.",
            why_you: "A bad year doesn't offset your other income the way people assume.",
        },
        Rule {
            name: "estimated payments",
            what: "Income without withholding generally needs quarterly estimated payments, or \
                   there's an underpayment penalty even if you pay in full at filing.",
            why_you: "Trading income has no withholding, so this catches almost everyone in \
                      their first profitable year.",
        },
        Rule {
            name: "self-employment tax",
            what: "Business income is subject to self-employment tax on top of income tax. \
                   Trading gains generally are not.",
            why_you: "Worth knowing which of your income is which before budgeting for a bill.",
        },
    ]
}

/// How long to keep things.
pub fn keep_for(what: &str) -> &'static str {
    let w = what.to_lowercase();
    if w.contains("receipt") || w.contains("expense") || w.contains("statement") {
        "Generally three years from filing — longer if it supports a position that could be \
         questioned, and seven if a loss is involved."
    } else if w.contains("basis") || w.contains("purchase") || w.contains("asset") {
        "Keep basis records until three years after you sell the thing, not three years after \
         you bought it."
    } else {
        "Three years is the usual answer; seven if it involves a loss."
    }
}

/// Work out what a transaction is, from its description.
pub fn categorise(t: &Transaction) -> Category {
    let d = t.description.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| d.contains(w));

    if t.amount > 0.0 && has(&["payroll", "salary", "deposit from", "invoice", "stripe", "payment from"]) {
        return Category::Income;
    }
    // Fees first: a charge from a broker is a fee, not money moving into a
    // brokerage. Matching the broker's name first gets that backwards.
    if has(&["exchange fee", "data feed", "market data", "commission", "platform fee", "clearing", "cme "]) {
        return Category::Fees;
    }
    if has(&["schwab", "fidelity", "interactive brokers", "tastytrade", "tradestation", "coinbase", "brokerage"]) {
        return Category::Investment;
    }
    if has(&["transfer", "zelle", "venmo", "payment to card", "autopay"]) {
        return Category::Transfer;
    }
    if has(&["aws", "azure", "github", "jetbrains", "adobe", "microsoft 365", "google workspace", "figma", "notion"]) {
        return Category::Software;
    }
    if has(&["dell", "apple store", "newegg", "micro center", "best buy", "monitor", "laptop"]) {
        return Category::Hardware;
    }
    if has(&["comcast", "xfinity", "spectrum", "fiber", "broadband", "internet"]) {
        return Category::Internet;
    }
    if has(&["verizon", "at&t", "t-mobile", "mint mobile"]) {
        return Category::Phone;
    }
    if has(&["netflix", "spotify", "hulu", "disney", "prime video", "patreon", "substack"]) {
        return Category::Subscription;
    }
    if has(&["udemy", "coursera", "pluralsight", "books", "course", "training"]) {
        return Category::Education;
    }
    if has(&["accountant", "cpa", "attorney", "legal", "bookkeep"]) {
        return Category::Professional;
    }
    if has(&["rent", "mortgage", "hoa", "landlord"]) {
        return Category::Housing;
    }
    if has(&["electric", "gas company", "water", "utility", "power"]) {
        return Category::Utilities;
    }
    if has(&["uber", "lyft", "shell", "chevron", "parking", "toll", "transit"]) {
        return Category::Transport;
    }
    if has(&["kroger", "safeway", "whole foods", "trader joe", "aldi", "grocer"]) {
        return Category::Groceries;
    }
    if has(&["restaurant", "cafe", "coffee", "doordash", "grubhub", "pizza", "bar "]) {
        return Category::Meals;
    }
    if has(&["pharmacy", "cvs", "walgreens", "doctor", "dental", "clinic"]) {
        return Category::Health;
    }
    if has(&["hotel", "airline", "delta", "united", "airbnb", "flight"]) {
        return Category::Travel;
    }
    Category::Unknown
}

/// A period's worth of money, sorted out.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Summary {
    pub in_total: f64,
    pub out_total: f64,
    pub by_category: Vec<(Category, f64)>,
    /// Spend that a business commonly deducts.
    pub possibly_deductible: f64,
    /// Things Atlas couldn't place.
    pub unrecognised: usize,
}

impl Summary {
    pub fn net(&self) -> f64 {
        self.in_total + self.out_total
    }
}

pub fn summarise(txns: &[Transaction]) -> Summary {
    let mut s = Summary::default();
    let mut totals: std::collections::BTreeMap<Category, f64> = Default::default();

    for t in txns {
        let c = t.category.as_deref().map(parse_category).unwrap_or_else(|| categorise(t));
        if c == Category::Unknown {
            s.unrecognised += 1;
        }
        // Transfers and brokerage movements are your own money moving. Counting
        // them as income or spend makes every total wrong.
        if matches!(c, Category::Transfer | Category::Investment) {
            *totals.entry(c).or_insert(0.0) += t.amount;
            continue;
        }
        if t.amount > 0.0 {
            s.in_total += t.amount;
        } else {
            s.out_total += t.amount;
            if c.often_deductible() {
                s.possibly_deductible += -t.amount;
            }
        }
        *totals.entry(c).or_insert(0.0) += t.amount;
    }

    let mut by: Vec<(Category, f64)> = totals.into_iter().collect();
    by.sort_by(|a, b| a.1.abs().partial_cmp(&b.1.abs()).unwrap_or(std::cmp::Ordering::Equal).reverse());
    s.by_category = by;
    s
}

fn parse_category(s: &str) -> Category {
    match s.to_lowercase().as_str() {
        "software" => Category::Software,
        "hardware" => Category::Hardware,
        "meals" => Category::Meals,
        "travel" => Category::Travel,
        "fees" => Category::Fees,
        _ => Category::Unknown,
    }
}

/// What Atlas says about a period. One line, then the useful detail.
pub fn spoken(s: &Summary) -> String {
    if s.by_category.is_empty() {
        return "Nothing to go on.".into();
    }
    // The biggest outgoing, not the biggest line: income being the largest
    // number tells you nothing.
    let biggest = s
        .by_category
        .iter()
        .find(|(c, amount)| *amount < 0.0 && *c != Category::Transfer && *c != Category::Investment);
    let mut out = format!(
        "In {:.0}, out {:.0}, net {:.0}.",
        s.in_total,
        s.out_total.abs(),
        s.net()
    );
    if let Some((c, amount)) = biggest {
        out.push_str(&format!(" Biggest was {c:?} at {:.0}.", amount.abs()));
    }
    if s.possibly_deductible > 100.0 {
        out.push_str(&format!(
            " About {:.0} of that is the sort of thing a business deducts — worth checking.",
            s.possibly_deductible
        ));
    }
    if s.unrecognised > 0 {
        out.push_str(&format!(" {} I couldn't place.", s.unrecognised));
    }
    out
}

/// The rule worth mentioning, given what's actually in the statements.
///
/// Volunteering all eight would be a lecture. One, when it's relevant, is
/// useful.
pub fn relevant_rule(s: &Summary, txns: &[Transaction]) -> Option<&'static Rule> {
    // `OnceLock`, not `Box::leak`.
    //
    // This was `Box::leak(trading_rules().into_boxed_slice())` and it leaked
    // the eight `Rule` structs **on every call**, permanently, in a process
    // designed to run for months. Nothing freed them and nothing bounded how
    // often this is asked. A `OnceLock` gives the same `&'static` lifetime
    // the return type needs, from one allocation for the life of the process.
    static RULES: std::sync::OnceLock<Vec<Rule>> = std::sync::OnceLock::new();
    let rules: &'static [Rule] = RULES.get_or_init(trading_rules).as_slice();
    let trading = s
        .by_category
        .iter()
        .any(|(c, _)| *c == Category::Investment || *c == Category::Fees);

    if trading && txns.len() > 30 {
        // Frequent trading is where wash sales bite.
        return rules.iter().find(|r| r.name == "wash sales");
    }
    if s.in_total > 5000.0 && s.by_category.iter().any(|(c, _)| *c == Category::Income) {
        return rules.iter().find(|r| r.name == "estimated payments");
    }
    if s.possibly_deductible > 1000.0 {
        return rules.iter().find(|r| r.name == "trader tax status");
    }
    None
}

/// The disclaimer, said once and meant.
pub const NOT_ADVICE: &str =
    "I know the general rules, not your situation — and for trading they're genuinely unusual. \
     Treat this as a prompt to ask someone, not as an answer.";
