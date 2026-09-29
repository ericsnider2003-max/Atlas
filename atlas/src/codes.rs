//! Recovery codes — the thing built for exactly your situation.
//!
//! ## Why this and not turning it off
//!
//! Your problem is precise: you need to sign in from a government computer,
//! where you can't receive a text and may not have your phone. That is the
//! *specific* problem recovery codes were invented for. They are ten one-time
//! strings, printed on paper, that work as the second factor on any machine,
//! with no phone, no signal, no app and no key. You type one, you're in, and
//! that code is spent.
//!
//! They beat turning two-factor off on every axis that matters to you:
//!
//! * They work on a locked-down machine where an authenticator app can't be
//!   installed.
//! * They don't require you to already be signed in to arrange — which
//!   disabling does, and which is the hole in that plan.
//! * The account stays protected for everyone who isn't holding your paper.
//! * You can regenerate a fresh ten before each deployment.
//!
//! Ten logins per account is usually a deployment's worth. Where it isn't,
//! most services let you print a new set, and Atlas tracks how many you have
//! left so you find out before you're down to your last one.

use serde::{Deserialize, Serialize};

/// A set of codes for one account.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Set {
    pub site: String,
    /// How many the service issued.
    pub issued: u32,
    /// How many you've spent.
    pub used: u32,
    /// When you generated them.
    pub at: u64,
    /// Where the paper is. Atlas never stores the codes themselves.
    pub kept_where: Option<String>,
    /// You've confirmed they're printed and on you.
    pub in_hand: bool,
}

impl Set {
    pub fn left(&self) -> u32 {
        self.issued.saturating_sub(self.used)
    }

    /// Running low, with enough warning to do something about it.
    ///
    /// Takes the config since 19 Sep 2026. It used to hardcode three and had
    /// no caller, while `gaps` compared against `cfg.warn_at` — two answers
    /// to the same question, and the one a person could change was not the
    /// one the named predicate gave. `gaps` calls this now, so there is one
    /// rule and `warn_at` is it.
    pub fn running_low(&self, cfg: &CodesConfig) -> bool {
        self.left() <= cfg.warn_at && self.left() > 0
    }

    fn spent(&self) -> bool {
        self.left() == 0
    }
}

/// Where a service hides its recovery codes, and what it calls them.
///
/// They all call it something different, which is most of why people never
/// find them.
pub fn where_to_get(site: &str) -> Option<(&'static str, &'static str, &'static str)> {
    let s = site.to_lowercase();
    Some(match () {
        _ if s.contains("google") || s.contains("gmail") => (
            "https://myaccount.google.com/signinoptions/two-step-verification",
            "Backup codes",
            "Scroll to Backup codes and choose Get backup codes. Ten of them, and you can \
             regenerate whenever you like.",
        ),
        _ if s.contains("instagram") || s.contains("facebook") => (
            "https://accountscenter.facebook.com/password_and_security/",
            "Recovery codes",
            "Two-factor authentication, pick the account, then Additional methods and \
             Recovery codes.",
        ),
        _ if s.contains("tiktok") => (
            "https://www.tiktok.com/setting/",
            "Backup codes",
            "Security and permissions, then 2-step verification. TikTok's are easy to miss.",
        ),
        _ if s.contains("x.com") || s.contains("twitter") => (
            "https://x.com/settings/account/login_verification",
            "Backup code",
            "It gives you a single code rather than ten, and regenerating replaces it.",
        ),
        _ if s.contains("github") => (
            "https://github.com/settings/security",
            "Recovery codes",
            "Sixteen of them, and it nags you to download them, which is unusually sensible.",
        ),
        _ if s.contains("microsoft") || s.contains("outlook") => (
            "https://account.microsoft.com/security",
            "Recovery code",
            "Advanced security options. Microsoft gives one long code, not a list.",
        ),
        _ if s.contains("apple") || s.contains("icloud") => (
            "https://appleid.apple.com/account/manage",
            "Recovery Key",
            "Sign-In and Security, then Account Recovery. Apple's is a single key and losing \
             it can lock you out permanently — read the warning.",
        ),
        _ if s.contains("amazon") => (
            "https://www.amazon.com/a/settings/approval",
            "Backup methods",
            "Two-step verification settings, then Backup methods.",
        ),
        _ => return None,
    })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct CodesConfig {
    pub enabled: bool,
    /// Warn when a set drops to this many.
    pub warn_at: u32,
    /// Check before a trip, this many days out.
    pub check_days_before: u32,
}

impl Default for CodesConfig {
    fn default() -> Self {
        CodesConfig { enabled: false, warn_at: 3, check_days_before: 21 }
    }
}

/// What still needs doing before you go.
#[derive(Debug, Clone, PartialEq)]
pub struct Gap {
    pub site: String,
    pub what: String,
    pub url: Option<String>,
    pub urgency: f32,
}

/// Check the whole set.
pub fn gaps(sets: &[Set], accounts: &[String], cfg: &CodesConfig) -> Vec<Gap> {
    let mut out = Vec::new();

    // Accounts with no codes at all — the real hole.
    for site in accounts {
        if !sets.iter().any(|s| s.site.eq_ignore_ascii_case(site)) {
            out.push(Gap {
                site: site.clone(),
                what: "no recovery codes at all — this is the one that locks you out".into(),
                url: where_to_get(site).map(|(u, _, _)| u.to_string()),
                urgency: 1.0,
            });
        }
    }

    for s in sets {
        if s.spent() {
            out.push(Gap {
                site: s.site.clone(),
                what: "all used — generate a new set".into(),
                url: where_to_get(&s.site).map(|(u, _, _)| u.to_string()),
                urgency: 0.95,
            });
        } else if s.running_low(cfg) {
            out.push(Gap {
                site: s.site.clone(),
                what: format!("{} left — print a fresh set before you go", s.left()),
                url: where_to_get(&s.site).map(|(u, _, _)| u.to_string()),
                urgency: 0.8,
            });
        }
        if !s.in_hand {
            out.push(Gap {
                site: s.site.clone(),
                what: "generated but you haven't said they're printed and on you".into(),
                url: None,
                urgency: 0.7,
            });
        }
    }

    out.sort_by(|a, b| b.urgency.partial_cmp(&a.urgency).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// Spend one.
pub fn used_one(sets: &mut [Set], site: &str) -> Option<u32> {
    let s = sets.iter_mut().find(|s| s.site.eq_ignore_ascii_case(site))?;
    s.used += 1;
    Some(s.left())
}

/// What Atlas says before a deployment.
pub fn before_you_go(sets: &[Set], accounts: &[String], cfg: &CodesConfig) -> String {
    let g = gaps(sets, accounts, cfg);
    if g.is_empty() {
        let total: u32 = sets.iter().map(|s| s.left()).sum();
        return format!(
            "All {} accounts have codes in hand — {total} logins between them. You're covered.",
            sets.len()
        );
    }
    let missing = g.iter().filter(|x| x.what.contains("no recovery codes")).count();
    let mut s = if missing > 0 {
        format!("{missing} accounts have no recovery codes at all. ")
    } else {
        String::new()
    };
    s.push_str(&format!("{} things to sort, starting with {}: {}.",
        g.len(), g[0].site, g[0].what));
    s.push_str(" I'll open each page — you print them and tell me they're on you.");
    s
}

/// How many logins you actually have, which is the number that matters.
///
/// Only the sets you have said are printed and on you. A set you generated
/// and left on the screen is not a login you have from a government computer,
/// and counting it would make the number comforting and wrong.
pub fn logins_available(sets: &[Set]) -> u32 {
    sets.iter().filter(|s| s.in_hand).map(|s| s.left()).sum()
}

/// Is the trip close enough that this is worth raising?
///
/// `check_days_before` is the setting, and nothing read it — the module could
/// say what was missing, and nothing knew when to ask. It is deliberately a
/// longer window than the accounts one: printing a fresh set and getting the
/// paper into your bag is a thing you do over a weekend, not on the way out.
pub fn worth_raising_now(cfg: &CodesConfig, days_until_you_go: Option<u32>) -> bool {
    match days_until_you_go {
        _ if !cfg.enabled => false,
        Some(days) => days <= cfg.check_days_before,
        None => false,
    }
}

/// The honest limit of this approach.
pub const THE_LIMIT: &str =
    "Ten codes is ten sign-ins. If you'll be logging into something every day for six months, \
     that isn't enough on its own — regenerate a set the week you leave, and I'll tell you when \
     you're down to three. Some services only give one code rather than ten, and I'll say which.";

/// Why this beats the thing you asked for, said once and specifically.
pub const WHY_THIS_WORKS: &str =
    "This is the case recovery codes exist for: signing in from a machine that isn't yours, with \
     no phone and no signal. They work on a locked-down government computer where you couldn't \
     install an authenticator. And unlike turning it off, you don't need to already be signed in \
     to arrange them — which is the hole in the other plan, because you can only disable it from \
     inside the account.";
