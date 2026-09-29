//! Receipts, read: a photo or a PDF of one becomes a line you can find and
//! a spend `money` can count -- merchant, date, total.
//!
//! "Keep this receipt" (a photo handed to the tray, or a screenshot);
//! "what did I spend at Costco this month?"; "receipts for work".
//!
//! **Sources:** the receipt layouts in the SROIE dataset (ICDAR 2019) were
//! read for where the three fields sit -- merchant in the first lines, the
//! total on a line labelled TOTAL / AMOUNT DUE / BALANCE, usually the last
//! such line and the largest amount near the bottom -- and for the usual
//! traps: SUBTOTAL, TOTAL SAVINGS, TOTAL ITEMS, CHANGE and TENDERED all
//! carry the word or a bigger number.
//!
//! **Soundproofing.**
//! - A total is only taken when it's labelled; the largest-number fallback
//!   is offered as "probably", with a question, never filed silently.
//! - When subtotal + tax is on the receipt and doesn't make the total, it
//!   says so rather than picking one.
//! - A card number on the receipt (the full PAN some printers still print)
//!   is scrubbed from the kept text; only the last four stay.
//! - The same receipt kept twice (merchant, date, total) is one receipt.

use serde::{Deserialize, Serialize};

pub const MAX_RECEIPTS: usize = 5000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Receipt {
    pub merchant: String,
    /// In the receipt's currency, in cents (or the smallest unit).
    pub total_cents: i64,
    pub currency: String,
    /// Local day number (days since 1970), when the receipt says.
    pub day: Option<i64>,
    /// When it was kept, UTC seconds.
    pub kept: u64,
    /// A few words to find it by, tidied and scrubbed.
    pub text: String,
    /// The total was a guess (no TOTAL line), confirmed by you.
    #[serde(default)]
    pub guessed: bool,
    #[serde(default)]
    pub bucket: Option<crate::money::Bucket>,
}

/// How sure the reading is.
#[derive(Debug, Clone, PartialEq)]
pub enum Total {
    /// A labelled TOTAL line.
    Labelled(i64),
    /// No label; the largest amount near the end. Ask before keeping.
    Probably(i64),
    /// Subtotal and tax don't make the total: both said.
    Disagrees { total: i64, subtotal: i64, tax: i64 },
    None,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub merchant: String,
    pub total: Total,
    pub currency: String,
    pub day: Option<i64>,
}

/// Amounts on a line, in cents: "12.99", "1,234.50", "$4.00", "4,00 €".
pub fn amounts(line: &str) -> Vec<i64> {
    let mut out = Vec::new();
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if !b[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && (b[i].is_ascii_digit() || b[i] == b',' || b[i] == b'.') {
            i += 1;
        }
        let tok = line[start..i].trim_end_matches(['.', ',']);
        // The decimal separator is the last '.' or ',' followed by exactly
        // two digits; any other separator is grouping.
        let last = tok.rfind(['.', ',']);
        let cents = match last {
            Some(p) if tok.len() - p - 1 == 2 => {
                let whole: String = tok[..p].chars().filter(|c| c.is_ascii_digit()).collect();
                let frac = &tok[p + 1..];
                whole.parse::<i64>().ok().zip(frac.parse::<i64>().ok()).map(|(w, f)| w * 100 + f)
            }
            _ => None,
        };
        // A bare integer isn't taken as money: it's a quantity, a store
        // number, a time.
        if let Some(c) = cents {
            if c < 100_000_000 {
                out.push(c);
            }
        }
    }
    out
}

const NOT_THE_TOTAL: &[&str] = &[
    "subtotal", "sub total", "sub-total", "total savings", "you saved", "savings", "total items", "items sold", "item count",
    "change", "tendered", "cash", "tip suggestion", "points", "discount", "total tax", "before tax",
];

fn day_of(text: &str) -> Option<i64> {
    // 2026-09-25, 09/25/2026, 25/09/2026 (a day over 12 decides), 09/25/26,
    // 25 Sep 2026, Sep 25, 2026.
    for w in text.split(|c: char| c.is_whitespace() || c == ',') {
        let w = w.trim_matches(|c: char| !c.is_ascii_digit());
        let parts: Vec<&str> = w.split(['/', '-', '.']).collect();
        if parts.len() != 3 || parts.iter().any(|p| p.is_empty() || p.len() > 4) {
            continue;
        }
        let n: Vec<i64> = match parts.iter().map(|p| p.parse::<i64>()).collect::<Result<Vec<_>, _>>() {
            Ok(n) => n,
            Err(_) => continue,
        };
        let (y, m, d) = if parts[0].len() == 4 {
            (n[0], n[1], n[2])
        } else {
            let y = if parts[2].len() == 2 { 2000 + n[2] } else { n[2] };
            // US order unless the first can't be a month.
            if n[0] > 12 { (y, n[1], n[0]) } else { (y, n[0], n[1]) }
        };
        if (2000..=2100).contains(&y) && (1..=12).contains(&m) && d >= 1 && d <= crate::civil::days_in_month(y, m as u32) as i64 {
            return Some(crate::civil::days_from_civil(y, m as u32, d as u32));
        }
    }
    const MON: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    let words: Vec<String> = text.split(|c: char| c.is_whitespace() || c == ',').filter(|w| !w.is_empty()).map(|w| w.to_ascii_lowercase()).collect();
    for i in 0..words.len() {
        let Some(m) = MON.iter().position(|m| words[i].starts_with(m)) else { continue };
        let num = |s: &str| s.trim_end_matches(|c: char| c.is_alphabetic()).parse::<i64>().ok();
        let (d, y) = match (i.checked_sub(1).and_then(|j| num(&words[j])), words.get(i + 1).and_then(|w| num(w)), words.get(i + 2).and_then(|w| num(w))) {
            (Some(d), Some(y), _) if y >= 2000 => (d, y),
            (_, Some(d), Some(y)) if y >= 2000 => (d, y),
            _ => continue,
        };
        if d >= 1 && d <= crate::civil::days_in_month(y, m as u32 + 1) as i64 {
            return Some(crate::civil::days_from_civil(y, m as u32 + 1, d as u32));
        }
    }
    None
}

fn currency_of(text: &str) -> String {
    for (sym, code) in [("€", "EUR"), ("£", "GBP"), ("¥", "JPY"), ("₹", "INR"), ("CAD", "CAD"), ("AUD", "AUD"), ("$", "USD"), ("USD", "USD")] {
        if text.contains(sym) {
            return code.into();
        }
    }
    "USD".into()
}

/// Read OCR'd receipt text.
pub fn read(text: &str) -> Reading {
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let merchant = lines
        .iter()
        .take(6)
        .find(|l| {
            let letters = l.chars().filter(|c| c.is_alphabetic()).count();
            let low = l.to_lowercase();
            letters >= 3
                && letters * 2 >= l.chars().filter(|c| !c.is_whitespace()).count()
                && !["receipt", "welcome", "thank", "invoice", "order", "tel", "phone", "www", "http", "store #", "sale"].iter().any(|w| low.starts_with(w))
        })
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_default();

    let mut labelled: Option<(u8, i64)> = None;
    let mut subtotal = None;
    let mut tax = None;
    for l in &lines {
        let low = l.to_lowercase();
        let am = amounts(l);
        let Some(&last) = am.last() else { continue };
        if low.contains("subtotal") || low.contains("sub total") || low.contains("sub-total") {
            subtotal = Some(last);
            continue;
        }
        if (low.starts_with("tax") || low.contains(" tax") || low.contains("vat") || low.contains("gst") || low.contains("hst")) && !low.contains("total") {
            tax = Some(tax.unwrap_or(0) + last);
            continue;
        }
        if NOT_THE_TOTAL.iter().any(|w| low.contains(w)) {
            continue;
        }
        // Rank the label: "grand total" / "amount due" beat a plain total,
        // which beats a bare "balance"; among equals the last one wins (a
        // running TOTAL before tax is followed by the real one).
        let rank = if ["grand total", "amount due", "balance due", "total due"].iter().any(|w| low.contains(w)) {
            3
        } else if ["total", "amount paid", "to pay"].iter().any(|w| low.contains(w)) {
            2
        } else if low.contains("balance") {
            1
        } else {
            0
        };
        if rank > 0 && labelled.map_or(true, |(r, _)| rank >= r) {
            labelled = Some((rank, last));
        }
    }
    let labelled = labelled.map(|(_, t)| t);
    let total = match (labelled, subtotal, tax) {
        (Some(t), Some(s), Some(x)) if (s + x - t).abs() > 2 && t != s => Total::Disagrees { total: t, subtotal: s, tax: x },
        (Some(t), _, _) => Total::Labelled(t),
        (None, Some(s), Some(x)) => Total::Probably(s + x),
        (None, _, _) => {
            let tail = &lines[lines.len().saturating_sub(8)..];
            match tail.iter().flat_map(|l| amounts(l)).max() {
                Some(m) => Total::Probably(m),
                None => Total::None,
            }
        }
    };
    Reading { merchant, total, currency: currency_of(text), day: day_of(text) }
}

pub fn money(cents: i64, currency: &str) -> String {
    let sym = match currency {
        "USD" | "CAD" | "AUD" => "$",
        "EUR" => "€",
        "GBP" => "£",
        _ => "",
    };
    let s = format!("{sym}{}.{:02}", cents.abs() / 100, cents.abs() % 100);
    if sym.is_empty() { format!("{s} {currency}") } else { s }
}

impl Reading {
    /// Said back, with the question when there is one.
    pub fn said(&self) -> String {
        let at = if self.merchant.is_empty() { "a receipt".to_string() } else { self.merchant.clone() };
        let day = self.day.map(|d| {
            let c = crate::civil::Civil::from_local(d * 86_400);
            format!(" on {:04}-{:02}-{:02}", c.year, c.month, c.day)
        }).unwrap_or_default();
        match &self.total {
            Total::Labelled(t) => format!("{at}{day}: {}. Kept.", money(*t, &self.currency)),
            Total::Probably(t) => format!("{at}{day}: probably {} -- there's no TOTAL line I can read. Say \"yes\" to keep it, or the right amount.", money(*t, &self.currency)),
            Total::Disagrees { total, subtotal, tax } => format!(
                "{at}{day}: it says total {}, but subtotal {} plus tax {} is {}. Which is right?",
                money(*total, &self.currency), money(*subtotal, &self.currency), money(*tax, &self.currency), money(subtotal + tax, &self.currency)
            ),
            Total::None => "I couldn't find an amount on that. A sharper photo, flat and in good light, usually does it.".into(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Receipts {
    pub kept: Vec<Receipt>,
}

impl Receipts {
    /// Keep one. False when it's already here.
    pub fn keep(&mut self, r: Reading, total_cents: i64, guessed: bool, source_text: &str, now: u64) -> bool {
        if self.kept.iter().any(|k| k.merchant.eq_ignore_ascii_case(&r.merchant) && k.total_cents == total_cents && k.day == r.day) {
            return false;
        }
        let scrubbed = crate::redact::Scrubber::default().scrub(source_text);
        let text: String = scrubbed.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(600).collect();
        let bucket = Some(crate::money::sort_one(&r.merchant, -(total_cents as f32 / 100.0), &[]));
        self.kept.push(Receipt { merchant: r.merchant, total_cents, currency: r.currency, day: r.day, kept: now, text, guessed, bucket });
        if self.kept.len() > MAX_RECEIPTS {
            self.kept.remove(0);
        }
        true
    }

    /// Receipts matching words (merchant or text), within days [from, to].
    pub fn find(&self, words: &str, from: Option<i64>, to: Option<i64>) -> Vec<&Receipt> {
        let w: Vec<String> = words.split_whitespace().map(|x| x.to_lowercase()).collect();
        self.kept
            .iter()
            .filter(|r| {
                let d = r.day.unwrap_or((r.kept / 86_400) as i64);
                from.map_or(true, |f| d >= f) && to.map_or(true, |t| d <= t)
            })
            .filter(|r| {
                let hay = format!("{} {}", r.merchant, r.text).to_lowercase();
                w.iter().all(|x| hay.contains(x.as_str()))
            })
            .collect()
    }

    /// "12 receipts, $412.30 (USD)" -- totals kept apart per currency.
    pub fn summed(found: &[&Receipt]) -> String {
        if found.is_empty() {
            return "No receipts match.".into();
        }
        let mut by: std::collections::BTreeMap<&str, i64> = Default::default();
        for r in found {
            *by.entry(r.currency.as_str()).or_default() += r.total_cents;
        }
        let sums: Vec<String> = by.iter().map(|(c, t)| money(*t, c)).collect();
        format!("{} receipt{}, {}.", found.len(), if found.len() == 1 { "" } else { "s" }, sums.join(" + "))
    }
}
