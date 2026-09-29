//! Knowing where your accounts stand.
//!
//! ## What this does, and what it deliberately doesn't
//!
//! You asked whether Atlas could turn two-factor authentication off across
//! your accounts, with a confirmation and a list. The answer is no, and it's
//! worth being clear that this isn't about legality — they're your accounts.
//!
//! It's that a system which *can* disable two-factor across your bank, your
//! email and your socials is a system where one confused moment, one bad
//! instruction, or one compromise costs you everything at once. The
//! confirmation doesn't help: the danger is the capability existing, not the
//! click. It's the same reason Atlas doesn't touch the firewall or Windows
//! Defender, and I'd rather be consistent about it.
//!
//! What it does instead is the useful half, and probably the half you actually
//! wanted: **find every account, say what protection each has, and rank what's
//! worth fixing.** Most people have no idea SMS codes are their weakest link
//! or which account has no second factor at all. Atlas can tell you that,
//! open the right settings page, and walk you through it.

use serde::{Deserialize, Serialize};

/// How an account is protected beyond the password.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecondFactor {
    /// Nothing. Password only.
    None,
    /// A code by text. Better than nothing, and the weakest kind — a SIM swap
    /// defeats it and doesn't need your phone.
    Sms,
    /// Emailed code. Only as strong as the email account.
    Email,
    /// An authenticator app.
    App,
    /// A physical key.
    Key,
    /// Fingerprint or face, backed by the device.
    Passkey,
}

impl SecondFactor {
    pub fn strength(&self) -> u8 {
        match self {
            SecondFactor::None => 0,
            SecondFactor::Sms => 2,
            SecondFactor::Email => 2,
            SecondFactor::App => 4,
            SecondFactor::Key => 5,
            SecondFactor::Passkey => 5,
        }
    }

    pub fn plain(&self) -> &'static str {
        match self {
            SecondFactor::None => "nothing but the password",
            SecondFactor::Sms => "a text message",
            SecondFactor::Email => "an emailed code",
            SecondFactor::App => "an authenticator app",
            SecondFactor::Key => "a physical key",
            SecondFactor::Passkey => "a passkey",
        }
    }

    pub fn concern(&self) -> Option<&'static str> {
        match self {
            SecondFactor::None => Some("anyone with the password is in"),
            SecondFactor::Sms => {
                Some("a SIM swap gets past this without touching your phone — it's the weakest kind")
            }
            SecondFactor::Email => {
                Some("this is only as strong as your email account, so it's really one factor")
            }
            _ => None,
        }
    }
}

/// How much it matters if this one is taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stakes {
    Low,
    Medium,
    High,
    /// Losing this loses everything else with it.
    Keystone,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Account {
    pub site: String,
    pub second_factor: SecondFactor,
    pub stakes: Stakes,
    /// Password reused somewhere else.
    pub reused_password: bool,
    /// Recovery codes saved somewhere you could actually reach.
    pub has_recovery_codes: bool,
    /// The settings page, so Atlas can take you there.
    pub settings_url: Option<String>,
}

/// Work out how much an account matters, from what it is.
///
/// Email is a keystone whatever people think of it: every other account resets
/// through it, so it's the account that owns all the others.
pub fn stakes_for(site: &str) -> Stakes {
    let s = site.to_lowercase();
    if ["gmail", "outlook", "icloud", "proton", "yahoo mail", "fastmail"].iter().any(|m| s.contains(m)) {
        return Stakes::Keystone;
    }
    if ["bank", "chase", "wells", "paypal", "coinbase", "broker", "schwab", "fidelity",
        "interactive brokers", "tradestation"]
        .iter()
        .any(|m| s.contains(m))
    {
        return Stakes::High;
    }
    if ["instagram", "tiktok", "youtube", "facebook", "x.com", "twitter", "linkedin",
        "github", "domain", "cloudflare"]
        .iter()
        .any(|m| s.contains(m))
    {
        return Stakes::High;
    }
    if ["amazon", "ebay", "spotify", "netflix"].iter().any(|m| s.contains(m)) {
        return Stakes::Medium;
    }
    Stakes::Low
}

/// Something worth doing about it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Advice {
    pub site: String,
    pub what: String,
    pub why: String,
    /// Higher first.
    pub urgency: f32,
    /// Atlas can open the page. It can't make the change.
    pub can_open: Option<String>,
}

pub fn audit(accounts: &[Account]) -> Vec<Advice> {
    let mut out = Vec::new();

    for a in accounts {
        let weight = match a.stakes {
            Stakes::Keystone => 1.0,
            Stakes::High => 0.8,
            Stakes::Medium => 0.5,
            Stakes::Low => 0.2,
        };

        if a.second_factor == SecondFactor::None {
            out.push(Advice {
                site: a.site.clone(),
                what: "turn on two-factor".into(),
                why: format!(
                    "{} has {} — {}",
                    a.site,
                    a.second_factor.plain(),
                    a.second_factor.concern().unwrap_or("")
                ),
                urgency: weight,
                can_open: a.settings_url.clone(),
            });
        } else if a.second_factor <= SecondFactor::Email && a.stakes >= Stakes::High {
            out.push(Advice {
                site: a.site.clone(),
                what: "move from a text code to an app".into(),
                why: a.second_factor.concern().unwrap_or("").to_string(),
                urgency: weight * 0.8,
                can_open: a.settings_url.clone(),
            });
        }

        if a.reused_password && a.stakes >= Stakes::High {
            out.push(Advice {
                site: a.site.clone(),
                what: "change this password to something used nowhere else".into(),
                why: "one leak somewhere else opens this one too".into(),
                urgency: weight * 0.9,
                can_open: a.settings_url.clone(),
            });
        }

        // The thing that turns a lost phone into a lost account.
        if a.second_factor.strength() >= 4 && !a.has_recovery_codes {
            out.push(Advice {
                site: a.site.clone(),
                what: "save the recovery codes somewhere you could reach without your phone".into(),
                why: "strong two-factor with no recovery is how a lost phone becomes a lost account"
                    .into(),
                urgency: weight * 0.6,
                can_open: a.settings_url.clone(),
            });
        }
    }

    out.sort_by(|a, b| b.urgency.partial_cmp(&a.urgency).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// The one that matters most, and why it's first.
pub fn spoken(accounts: &[Account], advice: &[Advice]) -> String {
    if advice.is_empty() {
        return format!("{} accounts, all reasonably protected.", accounts.len());
    }
    let first = &advice[0];
    let mut s = format!("{} of {} accounts could be better. Start with {}: {}.",
        advice.len(), accounts.len(), first.site, first.what);

    // Naming the keystone is the single most useful thing here — most people
    // haven't thought about it.
    if let Some(key) = accounts.iter().find(|a| a.stakes == Stakes::Keystone) {
        if advice.iter().any(|x| x.site == key.site) {
            s.push_str(&format!(
                " {} is the one that matters most — every other account resets through it.",
                key.site
            ));
        }
    }
    if first.can_open.is_some() {
        s.push_str(" I can open the page; you make the change.");
    }
    s
}

/// Asked to weaken something.
///
/// Answered the same way every time, with the reason rather than a refusal.
pub const WONT_WEAKEN: &str =
    "I won't turn off two-factor, and it isn't about it being your account — it is. It's that \
     something able to disable two-factor across your bank, your email and your socials is one \
     bad instruction away from costing you all of them at once, and a confirmation doesn't fix \
     that. Same reason I don't touch the firewall. I'll open any of those settings pages and \
     tell you exactly where the switch is.";

/// Is that what's being asked?
pub fn asked_to_weaken(said: &str) -> bool {
    let t = said.to_lowercase();
    let off = ["turn off", "disable", "remove", "get rid of", "switch off"]
        .iter()
        .any(|w| t.contains(w));
    let what = ["two factor", "2fa", "two-factor", "authentication", "verification",
                "security", "passkey"]
        .iter()
        .any(|w| t.contains(w));
    off && what
}

/// What Atlas offers instead, which is most of what was wanted.
pub fn instead(accounts: &[Account]) -> String {
    let weak = accounts.iter().filter(|a| a.second_factor.strength() < 4).count();
    format!(
        "What I can do: tell you where two-factor is on and where it isn't — {weak} of your {} \
         accounts are on something weaker than an app — and open any of those pages for you.",
        accounts.len()
    )
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AccountsConfig {
    pub enabled: bool,
    /// Never changes a security setting. Not configurable — see the note at
    /// the top of this file.
    #[serde(skip, default = "never")]
    pub may_change_security: bool,
}

fn never() -> bool {
    false
}

impl Default for AccountsConfig {
    fn default() -> Self {
        AccountsConfig { enabled: false, may_change_security: false }
    }
}

// ===========================================================================
// The book: which accounts Atlas actually knows about.
//
// `audit`, `stakes_for`, `spoken` and the rest of this file were complete and
// tested from the day they were written, and nothing ever held an `Account`.
// The hub's Accounts page returned an empty list — not because you have no
// accounts, but because there was nowhere to put one. Every sentence below
// about SIM swaps and keystone email was written against a `&[Account]` that
// was always `&[]`.
//
// This is the missing half: somewhere for them to live, and a way to say what
// you know without ever handing over a password. Nothing here holds a secret.
// The passwords, seeds and recovery codes belong in `vault.rs`, sealed; this
// records only what *kind* of protection each site has, which is exactly what
// `audit` needs and is not itself worth stealing.
// ===========================================================================

/// The accounts Atlas knows about.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Book {
    pub accounts: Vec<Account>,
}

/// Where the book is kept.
pub const FILE: &str = "accounts";

/// What `Book::record_codes` did.
///
/// An enum rather than a bool because there are four outcomes and three of
/// them need saying out loud: a typo'd site must not read as success, and a
/// failed write must not read as "already so".
#[derive(Debug, Clone, PartialEq)]
pub enum Recorded {
    Changed,
    AlreadySo,
    NoSuchSite,
    CouldNotWrite(String),
}

impl Book {
    pub fn load(store: &crate::store::Store) -> Book {
        store.load::<Book>(FILE)
    }

    pub fn save(&self, store: &crate::store::Store) -> crate::error::Result<()> {
        store.save(FILE, self)
    }

    fn index_of(&self, site: &str) -> Option<usize> {
        let s = site.trim().to_lowercase();
        self.accounts
            .iter()
            .position(|a| a.site.trim().to_lowercase() == s)
    }

    pub fn get(&self, site: &str) -> Option<&Account> {
        self.index_of(site).map(|i| &self.accounts[i])
    }

    /// Start tracking a site.
    ///
    /// Stakes are worked out from the name rather than asked for, because the
    /// answer is knowable and a question you have to answer before the tool is
    /// useful is a question that stops it being used. You can still change it.
    ///
    /// Adding one you already have does not reset what you have told Atlas
    /// about it — that would quietly undo the security picture the audit is
    /// built from.
    pub fn note(&mut self, site: &str) -> bool {
        let site = site.trim();
        if site.is_empty() || self.index_of(site).is_some() {
            return false;
        }
        self.accounts.push(Account {
            site: site.to_string(),
            second_factor: SecondFactor::None,
            stakes: stakes_for(site),
            reused_password: false,
            has_recovery_codes: false,
            settings_url: settings_page_for(site),
        });
        self.accounts.sort_by(|a, b| b.stakes.cmp(&a.stakes).then(a.site.cmp(&b.site)));
        true
    }

    pub fn forget(&mut self, site: &str) -> bool {
        match self.index_of(site) {
            Some(i) => {
                self.accounts.remove(i);
                true
            }
            None => false,
        }
    }

    /// Record what protects this account.
    pub fn set_second_factor(&mut self, site: &str, f: SecondFactor) -> bool {
        match self.index_of(site) {
            Some(i) => {
                let changed = self.accounts[i].second_factor != f;
                self.accounts[i].second_factor = f;
                changed
            }
            None => false,
        }
    }

    pub fn set_reused(&mut self, site: &str, reused: bool) -> bool {
        match self.index_of(site) {
            Some(i) => {
                let changed = self.accounts[i].reused_password != reused;
                self.accounts[i].reused_password = reused;
                changed
            }
            None => false,
        }
    }

    /// Record whether this account's recovery codes are somewhere reachable,
    /// and write the book.
    ///
    /// # Why this exists rather than callers doing both steps
    ///
    /// `set_recovery_codes` had **no caller anywhere in the program**, so
    /// `has_recovery_codes` was false for every account that ever existed --
    /// and `goingaway::would_lock_you_out` clears an account off the list
    /// exactly when that flag is true. Atlas therefore told you that every
    /// second-factor account would strand you abroad. Not a silence: a
    /// confident, wrong security answer, given every time it was asked.
    ///
    /// Setting and saving are one operation here because they were two things
    /// a caller had to remember to do in order. A flag that records nothing is
    /// worse than a flag nobody sets -- the person watched Atlas accept it.
    pub fn record_codes(
        &mut self,
        store: &crate::store::Store,
        site: &str,
        has: bool,
    ) -> Recorded {
        if self.index_of(site).is_none() {
            return Recorded::NoSuchSite;
        }
        if !self.set_recovery_codes(site, has) {
            return Recorded::AlreadySo;
        }
        match self.save(store) {
            Ok(()) => Recorded::Changed,
            // Put back, so the book in memory matches the book on disk. An
            // in-memory flag that survives a failed write is how the next
            // question gets answered from a state that was never stored.
            Err(e) => {
                self.set_recovery_codes(site, !has);
                Recorded::CouldNotWrite(e.to_string())
            }
        }
    }

    pub fn set_recovery_codes(&mut self, site: &str, has: bool) -> bool {
        match self.index_of(site) {
            Some(i) => {
                let changed = self.accounts[i].has_recovery_codes != has;
                self.accounts[i].has_recovery_codes = has;
                changed
            }
            None => false,
        }
    }

    /// What to fix, worst first.
    pub fn advice(&self) -> Vec<Advice> {
        audit(&self.accounts)
    }

    /// The one sentence version.
    pub fn spoken(&self) -> String {
        spoken(&self.accounts, &self.advice())
    }

    /// Accounts Atlas has no security picture for.
    ///
    /// An account added and never described looks identical to a perfectly
    /// safe one — `audit` finds nothing wrong with it because it has been told
    /// nothing about it. That is the same shape as an unread instrument
    /// reading as a healthy machine, so it is reported rather than left to
    /// pass as a clean bill of health.
    pub fn undescribed(&self) -> Vec<&Account> {
        self.accounts
            .iter()
            .filter(|a| {
                a.second_factor == SecondFactor::None
                    && !a.reused_password
                    && !a.has_recovery_codes
            })
            .collect()
    }
}

/// The page where you would actually change this site's security.
///
/// Only sites where the address is stable and well known. A guessed URL that
/// four-oh-fours is worse than no link, because it looks like Atlas checked.
fn settings_page_for(site: &str) -> Option<String> {
    let s = site.to_lowercase();
    let known: &[(&str, &str)] = &[
        ("gmail", "https://myaccount.google.com/security"),
        ("google", "https://myaccount.google.com/security"),
        ("outlook", "https://account.microsoft.com/security"),
        ("microsoft", "https://account.microsoft.com/security"),
        ("icloud", "https://appleid.apple.com/account/manage"),
        ("apple", "https://appleid.apple.com/account/manage"),
        ("github", "https://github.com/settings/security"),
        ("proton", "https://account.proton.me/u/0/account/security"),
        ("discord", "https://discord.com/channels/@me"),
        ("dropbox", "https://www.dropbox.com/account/security"),
        ("amazon", "https://www.amazon.com/a/settings/approval"),
    ];
    known
        .iter()
        .find(|(k, _)| s.contains(k))
        .map(|(_, url)| (*url).to_string())
}

/// What a request to change the book asked for.
///
/// Parsed away from HTTP so the rules are testable without a socket, and so an
/// unrecognised field does nothing rather than being taken as a default. A
/// mistyped value silently recording "password only" against your email would
/// be a security picture that is wrong in the dangerous direction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Note(String),
    Forget(String),
    Factor(String, SecondFactor),
    Reused(String, bool),
}

impl Change {
    pub fn parse(what: Option<&str>, site: Option<&str>, to: Option<&str>) -> Option<Change> {
        let site = site?.trim().to_string();
        if site.is_empty() {
            return None;
        }
        match what? {
            "note" => Some(Change::Note(site)),
            "forget" => Some(Change::Forget(site)),
            "reused" => Some(Change::Reused(site, true)),
            "unique" => Some(Change::Reused(site, false)),
            "factor" => {
                let f = match to? {
                    "none" => SecondFactor::None,
                    "sms" => SecondFactor::Sms,
                    "email" => SecondFactor::Email,
                    "app" => SecondFactor::App,
                    "key" => SecondFactor::Key,
                    "passkey" => SecondFactor::Passkey,
                    _ => return None,
                };
                Some(Change::Factor(site, f))
            }
            _ => None,
        }
    }
}

impl Book {
    /// Apply a change. `true` when something actually changed, so the caller
    /// can skip writing a file that would be identical.
    pub fn apply(&mut self, c: &Change) -> bool {
        match c {
            Change::Note(site) => self.note(site),
            Change::Forget(site) => self.forget(site),
            Change::Factor(site, f) => self.set_second_factor(site, *f),
            Change::Reused(site, r) => self.set_reused(site, *r),
        }
    }
}
