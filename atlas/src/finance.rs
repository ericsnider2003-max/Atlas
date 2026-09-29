//! Money, read-only — and read-only by construction rather than by promise.
//!
//! You want Atlas auditing your finances between statements, which means
//! reaching your accounts. That's reasonable, and it's also the highest-stakes
//! thing in this system, so the guarantee can't be "Atlas is configured not
//! to". It has to be that Atlas *cannot*.
//!
//! So: on any domain marked financial, Atlas may navigate, read, and click
//! things that only fetch. It may not submit a form, click a button whose
//! label suggests moving money, or type into a field that isn't a login or a
//! date filter. That's enforced here, ahead of the browser layer, and it fails
//! closed — an action it doesn't recognise is refused, not allowed.
//!
//! Three ways to get the data, cheapest and safest first. See FINANCE.md.

use serde::{Deserialize, Serialize};

/// Where the numbers came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// A file you already have. No credentials, no risk.
    LocalFile,
    /// Atlas logged in and used the bank's own export button.
    SiteExport,
    /// A read-only aggregation API with a revocable token.
    Aggregator,
    /// An email receipt or card notification.
    Notification,
}

/// Every way in, in the order worth trying: the ones that never hold your
/// login first.
pub fn sources() -> [Source; 4] {
    [Source::LocalFile, Source::Aggregator, Source::Notification, Source::SiteExport]
}

impl Source {
    /// Does getting this involve Atlas holding your login?
    pub fn needs_credentials(&self) -> bool {
        matches!(self, Source::SiteExport)
    }
    pub fn describe(&self) -> &'static str {
        match self {
            Source::LocalFile => "a file you exported yourself",
            Source::SiteExport => "your bank's own download button",
            Source::Aggregator => "a read-only connection you can revoke",
            Source::Notification => "an email receipt",
        }
    }
}

/// What Atlas is being asked to do on a page.
#[derive(Debug, Clone, PartialEq)]
pub enum PageAction {
    Navigate(String),
    /// Read text or a table.
    Read,
    /// Move down the page. Always allowed on a financial site: reading a
    /// statement means reaching the bottom of it, and scrolling commits to
    /// nothing.
    Scroll,
    /// Click something. The label matters enormously.
    Click(String),
    /// Type into a field.
    Type { field: String, text: String },
    /// Submit a sign-in form — the one submit that is safe on a financial
    /// site.
    ///
    /// Refusing every submit meant Atlas could type your username and
    /// password into your bank and then not press the button, which is not a
    /// safety property, it's a broken feature. What makes this safe is that
    /// the form is checked: it may carry only credential fields. A form with
    /// an amount or an account number in it is not a sign-in form, whatever
    /// the page calls it.
    SubmitSignIn { fields: Vec<String> },
    /// Submit any other form.
    Submit,
    Download,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    Allowed,
    /// Refused, with the reason. Never silently dropped.
    Refused(String),
}

impl Verdict {
    pub fn ok(&self) -> bool {
        *self == Verdict::Allowed
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct FinanceConfig {
    pub enabled: bool,
    /// Domains treated as financial. Anything here gets the strict rules.
    pub financial_domains: Vec<String>,
    /// Where exported statements and CSVs are picked up from.
    pub inbox: String,
    /// Flag a transaction larger than this.
    pub large_transaction: f64,
    /// Flag a category whose spend rises more than this fraction month on
    /// month.
    pub category_jump: f64,
}

impl Default for FinanceConfig {
    fn default() -> Self {
        FinanceConfig {
            enabled: false,
            financial_domains: vec![
                "chase.com".into(),
                "bankofamerica.com".into(),
                "wellsfargo.com".into(),
                "citi.com".into(),
                "capitalone.com".into(),
                "amex.com".into(),
                "paypal.com".into(),
                "venmo.com".into(),
                "coinbase.com".into(),
                // Brokers. Same rules as a bank: Atlas may sign in and read,
                // and may never touch anything that moves a position.
                "schwab.com".into(),
                "fidelity.com".into(),
                "interactivebrokers.com".into(),
                "ibkr.com".into(),
                "tastytrade.com".into(),
                "tastyworks.com".into(),
                "tradestation.com".into(),
                "thinkorswim.com".into(),
                "etrade.com".into(),
                "webull.com".into(),
                "robinhood.com".into(),
                "vanguard.com".into(),
                "ninjatrader.com".into(),
                "tradovate.com".into(),
                "kraken.com".into(),
                "binance.com".into(),
                "gemini.com".into(),
            ],
            inbox: String::new(),
            large_transaction: 500.0,
            category_jump: 0.5,
        }
    }
}

impl FinanceConfig {
    pub fn is_financial(&self, url: &str) -> bool {
        let u = url.to_lowercase();
        self.financial_domains.iter().any(|d| u.contains(&d.to_lowercase()))
    }
}

/// Button labels that move money. Matched loosely on purpose — a false refusal
/// costs you a manual click; a false approval costs you money.
const MOVES_MONEY: &[&str] = &[
    // Banking
    "transfer", "send", "pay", "payment", "wire", "zelle", "venmo", "withdraw",
    "withdrawal", "deposit", "move money", "convert", "exchange", "remit",
    "schedule payment", "make a payment", "bill pay", "autopay", "direct debit",
    "standing order", "new payee", "add payee", "add recipient",
    // Trading. A broker page is a bank page with faster ways to lose money.
    "buy", "sell", "trade", "order", "place order", "review order",
    "market order", "limit order", "stop order", "stop loss", "take profit",
    "bracket", "close position", "close all", "flatten", "liquidate",
    "exercise", "assign", "roll", "short", "cover", "margin", "borrow",
    "leverage", "stake", "unstake", "swap", "bridge", "approve",
    "enable trading", "fund account", "link bank",
    // Generic commit verbs. Loose on purpose.
    "confirm", "submit", "authorize", "authorise", "continue to payment",
    "agree and", "accept and", "place", "execute",
];

/// Fields it's reasonable to type into on a bank site.
/// Field-name words that never appear in a sign-in form.
///
/// Checked first and unconditionally. One of these anywhere in a field name
/// settles it however innocent the rest of the form looks.
const NEVER_FILL: &[&str] = &[
    "amount", "amt", "sum", "value", "total", "balance",
    "account", "acct", "iban", "bic", "swift", "sortcode", "routing", "aba",
    "payee", "recipient", "beneficiary", "counterparty", "destination",
    "transfer", "payment", "pay", "send", "wire", "withdraw", "deposit",
    "quantity", "qty", "shares", "units", "contracts", "price", "limit",
    "stop", "ticker", "symbol", "side", "order", "leverage", "margin",
    "card", "cvv", "cvc", "expiry", "pan", "confirm",
];

/// Field-name words a sign-in or a date filter is made of.
///
/// Matched as **whole words**, not substrings, and that distinction is the
/// entire safety of this module rather than a detail.
///
/// It used to be a substring match, and `"to"` was on the list. `"to"` is
/// inside `total`, `token`, `stop_loss`, `custodian` and `account_to`, so a
/// wire transfer form made of `account_from`, `account_to`, `amount_total`
/// was three-for-three "safe fields" and got submitted. Measured, four out of
/// five realistic transfer and order forms passed this gate.
const SAFE_FIELDS: &[&str] = &[
    // Who you are
    "username", "user", "userid", "name", "email", "mail", "login", "id",
    // What proves it
    "password", "passcode", "pass", "pwd", "pin", "otp", "code", "token2fa",
    "mfa", "totp", "authenticator", "remember", "trust",
    // "Remember this device" / "keep me signed in" are ordinary parts of a
    // sign-in form. Leaving these off refuses real logins, and a guard that
    // blocks the thing you actually do gets switched off.
    "device", "browser", "computer", "machine", "keep", "stay", "signed",
    "logged", "in", "me", "this",
    // Reading, not doing
    "search", "query", "q", "filter", "from", "to", "start", "end", "date",
    "range", "month", "year", "period", "page", "sort",
];

/// Split a field name into words: `account_from`, `accountFrom`,
/// `account-from` and `account.from` all give `["account", "from"]`.
fn field_words(field: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut prev_lower = false;
    for ch in field.chars() {
        if ch.is_alphanumeric() {
            // camelCase boundary
            if ch.is_uppercase() && prev_lower && !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            cur.push(ch.to_ascii_lowercase());
            prev_lower = ch.is_lowercase() || ch.is_numeric();
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
            prev_lower = false;
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Is this a field Atlas may fill or submit on a money page?
///
/// Deny first, then require **every** word to be recognised. Requiring only
/// one recognised word would let `amount_to` through on the strength of `to`,
/// which is the shape of the bug this replaced.
pub fn is_safe_field(field: &str) -> bool {
    let words = field_words(field);
    if words.is_empty() {
        return false;
    }
    if words
        .iter()
        .any(|w| NEVER_FILL.iter().any(|n| w == n))
    {
        return false;
    }
    words
        .iter()
        .all(|w| SAFE_FIELDS.iter().any(|s| w == s))
}

/// Does this button label mean money moves?
///
/// Whole words, for the same reason `is_safe_field` uses them, but failing the
/// other way. Substring matching refused `Buying power`, `Payment history`,
/// `Repayment schedule`, `Enrollment`, `Payee list` and `Wireless statements` —
/// every one of them a read-only view, and reading statements is the thing
/// Atlas is supposed to be for on these sites.
///
/// Over-refusal looks like the safe error and is not free: a guard that blocks
/// what you actually do is a guard you end up switching off.
///
/// Multi-word entries still match as phrases, since `place order` cannot be
/// judged one word at a time.
/// Words that turn an action into a view of past actions.
///
/// `Transfer` is a button. `Transfer history` is a page. The distinguishing
/// word is the qualifier rather than the verb — but only when it comes **last**.
///
/// Position is the whole rule. `Payment schedule` is a view; `Schedule payment`
/// is an action, and they contain exactly the same two words. Checking anywhere
/// in the label let `Schedule payment` and `Limit order` through, both caught
/// by tests already in this repo.
const A_VIEW_OF: &[&str] = &[
    "history", "schedule", "scheduled", "list", "statements", "statement",
    "summary", "activity", "details", "log", "record", "records", "past",
    "recent", "upcoming", "pending", "report", "reports", "overview",
    "power", "settings", "preferences", "help", "about", "limits",
];

pub fn moves_money(label: &str) -> bool {
    let lower = label.to_lowercase();
    // Checked before the verbs, and only on the final word.
    let all: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    if let Some(last) = all.last() {
        if A_VIEW_OF.contains(last) {
            return false;
        }
    }
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    MOVES_MONEY.iter().any(|entry| {
        if entry.contains(' ') {
            lower.contains(entry)
        } else {
            words.iter().any(|w| w == entry)
        }
    })
}

/// The gate. Every browser action on a financial domain passes through here.
///
/// Fails closed: anything it doesn't positively recognise as safe is refused.
pub fn allowed(url: &str, action: &PageAction, cfg: &FinanceConfig) -> Verdict {
    if !cfg.is_financial(url) {
        return Verdict::Allowed;
    }
    match action {
        PageAction::Navigate(_) | PageAction::Read | PageAction::Scroll | PageAction::Download => {
            Verdict::Allowed
        }

        // Signing in is allowed, and only if every field in the form is a
        // credential field. That check is the whole safety of it.
        PageAction::SubmitSignIn { fields } => {
            let odd: Vec<&String> = fields
                .iter()
                .filter(|f| !is_safe_field(f))
                .collect();
            if odd.is_empty() {
                Verdict::Allowed
            } else {
                Verdict::Refused(format!(
                    "that form has {:?} in it, so it isn't a sign-in form — I won't submit it.",
                    odd.first().map(|s| s.as_str()).unwrap_or("")
                ))
            }
        }

        // Everything else. A submit on a banking page could be anything.
        PageAction::Submit => Verdict::Refused(
            "I'll sign you in, but I don't submit anything else on a banking site. Do that \
             yourself."
                .into(),
        ),

        PageAction::Click(label) => {
            if moves_money(label) {
                return Verdict::Refused(format!(
                    "\"{label}\" looks like it moves money — I won't click that."
                ));
            }
            Verdict::Allowed
        }

        PageAction::Type { field, .. } => {
            if is_safe_field(field) {
                Verdict::Allowed
            } else {
                // Fails closed. An unrecognised field on a bank page could be
                // an amount or an account number.
                Verdict::Refused(format!(
                    "I only fill logins and date filters on banking sites, not \"{field}\"."
                ))
            }
        }
    }
}

// ---------- what it does with the numbers ----------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transaction {
    pub date: String,
    pub description: String,
    /// Negative is money out.
    pub amount: f64,
    #[serde(default)]
    pub category: Option<String>,
    pub source: Source,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Flag {
    Large { t: Transaction },
    /// Same description, same amount, twice in a few days.
    PossibleDuplicate { a: Transaction, b: Transaction },
    /// A subscription that renewed after a long gap in use.
    Recurring { description: String, amount: f64, times: usize },
    CategoryJump { category: String, from: f64, to: f64 },
}

impl Flag {
    /// One spoken line.
    pub fn say(&self) -> String {
        match self {
            Flag::Large { t } => {
                format!("{} for {:.2} on {}.", t.description, t.amount.abs(), t.date)
            }
            Flag::PossibleDuplicate { a, .. } => {
                format!("{} charged twice for {:.2}.", a.description, a.amount.abs())
            }
            Flag::Recurring { description, amount, times } => {
                format!("{description} has billed {times} times at {amount:.2}.")
            }
            Flag::CategoryJump { category, from, to } => {
                format!("{category} went from {from:.0} to {to:.0}.")
            }
        }
    }
}

/// Look over transactions and pick out what's worth mentioning.
///
/// Deliberately conservative: an audit that flags forty things is one you stop
/// reading.
pub fn review(txns: &[Transaction], cfg: &FinanceConfig) -> Vec<Flag> {
    let mut out = Vec::new();

    for t in txns {
        if t.amount.abs() >= cfg.large_transaction {
            out.push(Flag::Large { t: t.clone() });
        }
    }

    // Same description and amount within a few days.
    for (i, a) in txns.iter().enumerate() {
        for b in txns.iter().skip(i + 1) {
            if a.description.eq_ignore_ascii_case(&b.description)
                && (a.amount - b.amount).abs() < 0.01
                && a.date != b.date
                && within_days(&a.date, &b.date, 4)
            {
                out.push(Flag::PossibleDuplicate { a: a.clone(), b: b.clone() });
            }
        }
    }

    // Things that bill repeatedly at the same amount.
    let mut seen: std::collections::BTreeMap<String, Vec<f64>> = Default::default();
    for t in txns.iter().filter(|t| t.amount < 0.0) {
        seen.entry(normalise(&t.description)).or_default().push(t.amount);
    }
    for (desc, amounts) in seen {
        if amounts.len() >= 3 {
            let first = amounts[0];
            if amounts.iter().all(|a| (a - first).abs() < 0.01) {
                out.push(Flag::Recurring {
                    description: desc,
                    amount: first.abs(),
                    times: amounts.len(),
                });
            }
        }
    }

    out
}

fn normalise(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_alphabetic() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .take(3)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Dates as YYYY-MM-DD. Crude day arithmetic, adequate for "within a few
/// days" and with no dependency.
fn within_days(a: &str, b: &str, days: i64) -> bool {
    let (Some(x), Some(y)) = (day_number(a), day_number(b)) else { return false };
    (x - y).abs() <= days
}

fn day_number(date: &str) -> Option<i64> {
    let mut parts = date.split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let d: i64 = parts.next()?.parse().ok()?;
    Some(y * 372 + m * 31 + d)
}

/// Parse a CSV export. Column names vary by bank, so they're matched by
/// meaning rather than position.
pub fn parse_csv(text: &str, source: Source) -> Vec<Transaction> {
    let mut lines = text.lines();
    let Some(header) = lines.next() else { return Vec::new() };
    let cols: Vec<String> = split_row(header).iter().map(|c| c.trim().to_lowercase()).collect();

    let find = |names: &[&str]| cols.iter().position(|c| names.iter().any(|n| c.contains(n)));
    let (Some(date_i), Some(desc_i)) = (
        find(&["date", "posted", "transaction date"]),
        find(&["description", "payee", "name", "merchant", "memo"]),
    ) else {
        return Vec::new();
    };
    let amount_i = find(&["amount", "value"]);
    let debit_i = find(&["debit", "withdrawal"]);
    let credit_i = find(&["credit", "deposit"]);

    let mut out = Vec::new();
    for line in lines {
        let f = split_row(line);
        if f.len() <= date_i.max(desc_i) {
            continue;
        }
        let f: Vec<&str> = f.iter().map(|s| s.as_str()).collect();
        let amount = match (amount_i, debit_i, credit_i) {
            (Some(i), _, _) => f.get(i).and_then(|v| clean_number(v)),
            // Some banks use separate debit and credit columns.
            (None, Some(d), Some(c)) => {
                // A blank cell is genuinely nothing — most rows fill one column
                // and leave the other empty. A cell with something in it that
                // will not parse is a number Atlas could not read, and
                // `unwrap_or(0.0)` turned that into a transaction worth zero:
                // the row stayed in the total, silently wrong, and a statement
                // that failed to parse looked like a statement that balanced.
                let read = |i: usize| -> std::result::Result<f64, ()> {
                    match f.get(i) {
                        None => Ok(0.0),
                        Some(v) if v.trim().is_empty() => Ok(0.0),
                        Some(v) => clean_number(v).ok_or(()),
                    }
                };
                match (read(d), read(c)) {
                    (Ok(debit), Ok(credit)) => Some(credit - debit.abs()),
                    // Skip the row rather than book it as zero.
                    _ => None,
                }
            }
            _ => None,
        };
        let Some(amount) = amount else { continue };
        out.push(Transaction {
            date: f[date_i].to_string(),
            description: f[desc_i].trim_matches('"').to_string(),
            amount,
            category: None,
            source,
        });
    }
    out
}

/// Split a CSV row, respecting quoted fields.
///
/// Banks export amounts as `"-1,234.56"` — splitting on every comma turns
/// that into two columns and the amount into nonsense. Doubled quotes inside
/// a quoted field are an escaped quote.
pub fn split_row(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '"' if in_quotes && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                out.push(field.trim().to_string());
                field = String::new();
            }
            _ => field.push(c),
        }
    }
    out.push(field.trim().to_string());
    out
}

fn clean_number(s: &str) -> Option<f64> {
    let cleaned: String = s
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    cleaned.parse().ok()
}

/// What Atlas says after a review. Leads with the count so you know whether to
/// listen to the rest.
pub fn summary(flags: &[Flag]) -> String {
    if flags.is_empty() {
        return "Nothing unusual.".into();
    }
    let first = flags[0].say();
    match flags.len() {
        1 => format!("One thing: {first}"),
        n => format!("{n} things worth a look. First: {first}"),
    }
}
