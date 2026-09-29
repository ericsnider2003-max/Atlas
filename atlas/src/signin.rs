//! Signing you in.
//!
//! This is a password manager with autofill, which is ordinary software —
//! 1Password and Bitwarden and your browser all do it. Framed that way, most
//! of my earlier hesitation was misplaced, and the parts that weren't are
//! design constraints rather than reasons not to build it.
//!
//! Three of those constraints do real work:
//!
//! **It fills on the domain and nowhere else.** This is the actual security
//! benefit over you typing it: a person types their password into
//! `paypa1-secure.com` because it looks right. Nothing here will, because it
//! matches the registered domain and not the look of the page. Autofill is
//! better phishing protection than a careful human.
//!
//! **Signing in is not permission to change security settings.** Two separate
//! grants, and holding the first never implies the second. This is what stops
//! "log me into my bank" from becoming "and now you can move money".
//!
//! **Access is per-site and revocable from one page**, without touching
//! anything else and without your having to remember what you granted.

use serde::{Deserialize, Serialize};

/// What Atlas may do on a site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Allowed {
    /// Nothing. The default for anything not granted.
    Nothing,
    /// Fill the login and sign in.
    SignIn,
    /// Sign in and act as you within the site.
    SignInAndUse,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Grant {
    /// The registered domain. Not a URL, not a page.
    pub domain: String,
    /// Which account, when there's more than one on the same site.
    ///
    /// A work Gmail and a personal one are the same domain and different
    /// logins, and getting that wrong signs you into the wrong life.
    pub account: String,
    /// What you'd call it.
    pub name: String,
    pub allowed: Allowed,
    /// The vault entry holding the credential.
    pub vault_entry: String,
    /// May Atlas sign in while you're not at the machine?
    pub while_you_are_away: bool,
    pub granted_at: u64,
    pub last_used: Option<u64>,
    pub times_used: u32,
    /// The stored credential stopped working — the password was probably
    /// changed somewhere else.
    #[serde(default)]
    pub looks_superseded: bool,
    /// Sign-ins that failed since it last worked.
    #[serde(default)]
    pub failures_in_a_row: u32,
}

/// Someone used a credential. Every time, without exception.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Use {
    pub domain: String,
    pub at: u64,
    pub worked: bool,
    /// Was it you at the machine, or unattended?
    pub you_were_here: bool,
    /// The page it filled on, so a wrong one is visible afterwards.
    pub page: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SignInConfig {
    pub enabled: bool,
    /// Sign in without asking each time, on sites you've granted.
    pub fill_without_asking: bool,
    /// Allow unattended sign-in at all. Off by default: the risk of a stored
    /// credential concentrates in what can use it while nobody's watching.
    pub allow_while_away: bool,
    /// Never fill on a page reached from a link Atlas didn't choose.
    #[serde(skip, default = "always")]
    pub only_on_pages_it_navigated_to: bool,
}

fn always() -> bool {
    true
}

impl Default for SignInConfig {
    fn default() -> Self {
        SignInConfig {
            enabled: false,
            fill_without_asking: true,
            allow_while_away: false,
            only_on_pages_it_navigated_to: true,
        }
    }
}

/// Why a fill was refused, which matters more than that it was.
#[derive(Debug, Clone, PartialEq)]
pub enum Refused {
    NotGranted(String),
    /// The domain doesn't match. This is the one that saves you.
    WrongDomain { expected: String, got: String },
    /// Not a real login page.
    NotALoginPage,
    /// The vault is locked.
    Locked,
    /// You're not here and this site isn't allowed unattended.
    YouAreNotHere,
    /// Reached by following a link from somewhere untrusted.
    ArrivedBadly,
    Disabled,
}

impl Refused {
    pub fn say(&self) -> String {
        match self {
            Refused::NotGranted(d) => format!("I don't have access to {d}. Want to give me it?"),
            Refused::WrongDomain { expected, got } => format!(
                "That page is {got}, not {expected}. I won't put your password into it — and \
                 that difference is exactly what a phishing page counts on you not noticing."
            ),
            Refused::NotALoginPage => "That isn't a sign-in form.".into(),
            Refused::Locked => "The vault's locked — say the passphrase.".into(),
            Refused::YouAreNotHere => {
                "That one only signs in while you're at the machine.".into()
            }
            Refused::ArrivedBadly => {
                "I got to that page by following a link rather than typing the address, so I'm \
                 not filling anything into it."
                    .into()
            }
            Refused::Disabled => "Signing in is switched off.".into(),
        }
    }
}

/// The registered domain, which is what a grant is actually against.
///
/// `accounts.google.com` and `mail.google.com` are the same account.
/// `google.com.evil.co` is not, and this is where the whole thing lives or
/// dies.
pub fn registered_domain(host: &str) -> String {
    let h = host.trim().trim_end_matches('.').to_lowercase();
    let parts: Vec<&str> = h.split('.').collect();
    if parts.len() < 2 {
        return h;
    }
    // Two-part public suffixes that would otherwise fool a naive split.
    const TWO_PART: &[&str] = &[
        "co.uk", "org.uk", "ac.uk", "gov.uk", "com.au", "co.jp", "co.nz", "com.br",
    ];
    let last_two = parts[parts.len() - 2..].join(".");
    if TWO_PART.contains(&last_two.as_str()) && parts.len() >= 3 {
        return parts[parts.len() - 3..].join(".");
    }
    last_two
}

/// A login unused for this long is worth one mention.
pub const QUIET_AFTER_DAYS: u64 = 90;
/// And the mention comes round no more than this often.
pub const QUIET_EVERY_DAYS: u64 = 30;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Access {
    pub grants: Vec<Grant>,
    pub log: Vec<Use>,
}

impl Access {
    /// Where the grants live.
    ///
    /// They had nowhere to live until 19 Sep 2026: `Daemon` built an
    /// `Access::default()` at startup and nothing ever loaded or saved one,
    /// so a grant could not survive a restart — and nothing called `grant`
    /// either, so there was never one to lose. `may_start` refusing with
    /// `NotGranted` was the only answer this module could give.
    pub const RECORD: &'static str = "site_access";

    pub fn load(store: &crate::store::Store) -> Access {
        store.load(Self::RECORD)
    }

    pub fn save(&self, store: &crate::store::Store) -> crate::error::Result<()> {
        store.save(Self::RECORD, self)
    }

    pub fn grant(
        &mut self,
        domain: &str,
        account: &str,
        name: &str,
        allowed: Allowed,
        entry: &str,
        now: u64,
    ) {
        let d = registered_domain(domain);
        // Replace only the same account on the same site, never the site.
        self.grants.retain(|g| !(g.domain == d && g.account == account));
        self.grants.push(Grant {
            domain: d,
            account: account.into(),
            name: name.into(),
            allowed,
            vault_entry: entry.into(),
            while_you_are_away: false,
            granted_at: now,
            last_used: None,
            times_used: 0,
            looks_superseded: false,
            failures_in_a_row: 0,
        });
    }

    /// Every account you have on a site.
    pub fn accounts_on(&self, host: &str) -> Vec<&Grant> {
        let d = registered_domain(host);
        self.grants.iter().filter(|g| g.domain == d).collect()
    }

    /// Which one did you mean?
    ///
    /// With one, no question. With several, Atlas asks rather than picking —
    /// signing into the wrong account is a mess that's hard to see and hard to
    /// undo.
    pub fn which_account(&self, host: &str, hint: Option<&str>) -> Which {
        let all = self.accounts_on(host);
        match all.len() {
            0 => Which::None,
            1 => Which::One(all[0].account.clone()),
            _ => {
                if let Some(h) = hint {
                    let h = h.to_lowercase();
                    let matched: Vec<&&Grant> = all
                        .iter()
                        .filter(|g| {
                            g.account.to_lowercase().contains(&h)
                                || g.name.to_lowercase().contains(&h)
                        })
                        .collect();
                    if matched.len() == 1 {
                        return Which::One(matched[0].account.clone());
                    }
                }
                Which::Several(all.iter().map(|g| g.account.clone()).collect())
            }
        }
    }

    /// Take it away. One site, immediately, without touching anything else.
    pub fn revoke(&mut self, domain: &str) -> bool {
        let d = registered_domain(domain);
        let before = self.grants.len();
        self.grants.retain(|g| g.domain != d);
        before != self.grants.len()
    }

    /// Take everything away at once.
    pub fn revoke_all(&mut self) -> usize {
        let n = self.grants.len();
        self.grants.clear();
        n
    }

    pub fn find(&self, host: &str) -> Option<&Grant> {
        let d = registered_domain(host);
        self.grants.iter().find(|g| g.domain == d)
    }

    pub fn find_account(&self, host: &str, account: &str) -> Option<&Grant> {
        let d = registered_domain(host);
        self.grants.iter().find(|g| g.domain == d && g.account == account)
    }

    /// The grant for a host, and the checks that are about the grant itself.
    ///
    /// Kept in one place so `may_start` and `may_fill` cannot answer the same
    /// question in two different orders.
    fn grant_for(&self, host: &str, cfg: &SignInConfig) -> Result<&Grant, Refused> {
        if !cfg.enabled {
            return Err(Refused::Disabled);
        }
        let d = registered_domain(host);
        let grant = self
            .grants
            .iter()
            .find(|g| g.domain == d)
            .ok_or_else(|| Refused::NotGranted(d.clone()))?;

        if grant.allowed == Allowed::Nothing {
            return Err(Refused::NotGranted(d));
        }
        // The check that does the work. A lookalike domain fails here.
        if grant.domain != d {
            return Err(Refused::WrongDomain { expected: grant.domain.clone(), got: d });
        }
        Ok(grant)
    }

    /// May Atlas sign you in at all, given what it can check without a page?
    ///
    /// Split out on 19 Sep 2026, because `may_fill` had no caller anywhere in
    /// the running program — the whole sign-in safety check, lookalike domain
    /// and all, was reachable only from its own tests, and `Intent::SignIn`
    /// announced "signing you into X" having checked nothing but whether the
    /// feature was on.
    ///
    /// The reason `may_fill` had no caller is that it asks two things only a
    /// browser can answer: is this a login form, and was it reached by typing
    /// the address rather than following a link. Nothing here drives a
    /// browser to a login page yet, so calling it from the spoken sign-in
    /// would have meant passing `true` for both — a lie in the shape of a
    /// safety check, which is worse than the check not running.
    ///
    /// So this is everything that can be decided without a page, and
    /// `may_fill` is this plus the two page questions, in the order it always
    /// had.
    pub fn may_start(
        &self,
        host: &str,
        vault_open: bool,
        you_are_here: bool,
        cfg: &SignInConfig,
    ) -> Result<&Grant, Refused> {
        let grant = self.grant_for(host, cfg)?;
        if !vault_open {
            return Err(Refused::Locked);
        }
        if !you_are_here && !(cfg.allow_while_away && grant.while_you_are_away) {
            return Err(Refused::YouAreNotHere);
        }
        Ok(grant)
    }

    /// May Atlas fill here, now?
    pub fn may_fill(
        &self,
        host: &str,
        is_login_page: bool,
        navigated_directly: bool,
        vault_open: bool,
        you_are_here: bool,
        cfg: &SignInConfig,
    ) -> Result<&Grant, Refused> {
        let grant = self.grant_for(host, cfg)?;
        if !is_login_page {
            return Err(Refused::NotALoginPage);
        }
        if cfg.only_on_pages_it_navigated_to && !navigated_directly {
            return Err(Refused::ArrivedBadly);
        }
        if !vault_open {
            return Err(Refused::Locked);
        }
        if !you_are_here && !(cfg.allow_while_away && grant.while_you_are_away) {
            return Err(Refused::YouAreNotHere);
        }
        Ok(grant)
    }

    /// Fill it, or check with you first?
    ///
    /// A different question from whether it is allowed, and the one
    /// `fill_without_asking` was written for. It ships `true` — sign in
    /// without asking each time, on sites you have already granted — and
    /// nothing read it, so a person who turned it off got asked exactly as
    /// often as before: never.
    pub fn asks_first(cfg: &SignInConfig) -> bool {
        !cfg.fill_without_asking
    }

    /// Record it. Every use, whether it worked or not.
    pub fn note_use(
        &mut self,
        domain: &str,
        account: &str,
        page: &str,
        worked: bool,
        you_were_here: bool,
        now: u64,
    ) {
        let d = registered_domain(domain);
        if let Some(g) = self
            .grants
            .iter_mut()
            .find(|g| g.domain == d && (g.account == account || account.is_empty()))
        {
            g.times_used += 1;
            if worked {
                g.last_used = Some(now);
                g.failures_in_a_row = 0;
                g.looks_superseded = false;
            } else {
                g.failures_in_a_row += 1;
                // Two failures in a row on a credential that used to work
                // means the password changed somewhere else, not that the
                // site is down.
                if g.failures_in_a_row >= 2 && g.times_used > g.failures_in_a_row {
                    g.looks_superseded = true;
                }
            }
        }
        self.log.push(Use {
            domain: d,
            at: now,
            worked,
            you_were_here,
            page: page.to_string(),
        });
        if self.log.len() > 1000 {
            self.log.drain(0..200);
        }
    }

    /// Grants you haven't used in a while.
    ///
    /// **Mentioned, never removed.** Not using a site for four months is
    /// normal — being away is normal — and dropping the credential is exactly
    /// the wrong thing to do to someone who's about to need it after a long
    /// absence. Time you were away doesn't count against it.
    pub fn quiet(&self, now: u64, days: u64, days_you_were_away: u64) -> Vec<&Grant> {
        let allowance = (days + days_you_were_away) * 86_400;
        self.grants
            .iter()
            .filter(|g| match g.last_used {
                None => now.saturating_sub(g.granted_at) > allowance,
                Some(t) => now.saturating_sub(t) > allowance,
            })
            .collect()
    }

    /// The once-a-month line about logins you haven't used (Eric, B4:
    /// "saved passwords for things not used yes but shouldn't be annoying").
    ///
    /// At most three named, the rest counted, nothing removed, and never
    /// more often than `QUIET_EVERY_DAYS` — it goes in the brief, not as an
    /// interruption.
    pub fn quiet_line(&self, now: u64) -> Option<String> {
        let quiet = self.quiet(now, QUIET_AFTER_DAYS, 0);
        if quiet.is_empty() {
            return None;
        }
        let names: Vec<String> = quiet.iter().take(3).map(|g| g.name.clone()).collect();
        let more = quiet.len().saturating_sub(3);
        let listed = match names.len() {
            1 => names[0].clone(),
            2 => format!("{} and {}", names[0], names[1]),
            _ => format!("{}, {} and {}", names[0], names[1], names[2]),
        };
        let rest = if more > 0 { format!(" (and {more} more)") } else { String::new() };
        Some(format!(
            "you haven't used your saved login for {listed}{rest} in three months or more — I'm keeping \
             them; the Access page is where to drop any you don't need"
        ))
    }

    /// Credentials that have stopped working.
    ///
    /// This is the useful thing to surface, rather than age: a password
    /// changed elsewhere fails silently and repeatedly until someone notices.
    pub fn superseded(&self) -> Vec<&Grant> {
        self.grants.iter().filter(|g| g.looks_superseded).collect()
    }

    /// How many distinct sites, as opposed to logins.
    pub fn accounts_on_all_sites(&self) -> usize {
        let mut d: Vec<&str> = self.grants.iter().map(|g| g.domain.as_str()).collect();
        d.sort();
        d.dedup();
        d.len()
    }

    /// You changed the password. Point the grant at the new one.
    pub fn superseded_by(&mut self, host: &str, account: &str, new_entry: &str) -> bool {
        let d = registered_domain(host);
        match self.grants.iter_mut().find(|g| g.domain == d && g.account == account) {
            Some(g) => {
                g.vault_entry = new_entry.into();
                g.looks_superseded = false;
                g.failures_in_a_row = 0;
                true
            }
            None => false,
        }
    }

    /// Anything that failed, which is how you notice something is wrong.
    fn failures(&self, since: u64) -> Vec<&Use> {
        self.log.iter().filter(|u| !u.worked && u.at >= since).collect()
    }
}

/// Which account you meant.
#[derive(Debug, Clone, PartialEq)]
pub enum Which {
    None,
    One(String),
    /// Ask. Signing into the wrong account is hard to see and hard to undo.
    Several(Vec<String>),
}

impl Which {
    pub fn ask(&self, site: &str) -> Option<String> {
        match self {
            Which::Several(accounts) => Some(format!(
                "You've got {} accounts on {site}: {}. Which one?",
                accounts.len(),
                accounts.join(", ")
            )),
            _ => None,
        }
    }
}

/// The list for the hub page.
///
/// Each row is a site, what Atlas may do there, when it last did, and a way to
/// take it away.
pub fn hub_rows(a: &Access, now: u64) -> Vec<(String, String, String, bool)> {
    a.grants
        .iter()
        .map(|g| {
            let label = if g.account.is_empty() {
                g.name.clone()
            } else {
                format!("{} — {}", g.name, g.account)
            };
            let what = match g.allowed {
                Allowed::Nothing => "nothing",
                Allowed::SignIn => "sign in",
                Allowed::SignInAndUse => "sign in and act as you",
            };
            let when = match g.last_used {
                None => "never used".to_string(),
                Some(t) => {
                    let days = now.saturating_sub(t) / 86_400;
                    if days == 0 {
                        "used today".into()
                    } else {
                        format!("last used {days} days ago")
                    }
                }
            };
            // Flagged only when it has actually stopped working. Age alone
            // isn't a problem.
            let needs_attention = g.looks_superseded;
            let detail = if g.looks_superseded {
                format!("{what} · the password looks like it changed")
            } else {
                format!("{what} · {when}")
            };
            (label, detail, g.domain.clone(), needs_attention)
        })
        .collect()
}

/// What Atlas says about what it can get into.
pub fn spoken(a: &Access, now: u64) -> String {
    if a.grants.is_empty() {
        return "I can't sign into anything. Say \"give me access to\" and a site.".into();
    }
    let sites = a.accounts_on_all_sites();
    let mut s = if sites == a.grants.len() {
        format!("{} sites.", sites)
    } else {
        format!("{} logins across {sites} sites.", a.grants.len())
    };
    // The thing worth saying is what's stopped working, not what's old.
    let stale_creds = a.superseded();
    if !stale_creds.is_empty() {
        s.push_str(&format!(
            " {} where the password looks like it changed: {}.",
            stale_creds.len(),
            stale_creds.iter().map(|g| g.name.as_str()).collect::<Vec<_>>().join(", ")
        ));
    }
    let recent_failures = a.failures(now.saturating_sub(7 * 86_400)).len();
    if recent_failures > 0 {
        s.push_str(&format!(" {recent_failures} sign-ins failed this week."));
    }
    s
}

/// What Atlas says when a credential has stopped working.
///
/// The useful version names what probably happened, because "sign-in failed"
/// sends you to check the site and the answer is usually simpler.
pub fn probably_changed(g: &Grant) -> String {
    format!(
        "{} hasn't worked the last {} times. That usually means the password was changed somewhere else. Want to give me the new one?",
        g.name, g.failures_in_a_row
    )
}

/// Signing into your bank is allowed. Doing anything else there is not.
///
/// These are two separate things and conflating them cost the feature: the
/// old rule refused every form submit on a financial site, which meant Atlas
/// could fill your credentials and then not press the button. That is not a
/// safety property.
///
/// What actually holds the line is `finance::allowed`, which lets a sign-in
/// form through only when every field in it is a credential field, and
/// refuses every click whose label looks like it moves money — including on
/// a broker, where the words are different and the speed is worse.
pub const BANKS_ARE_SITES_TOO: &str =
    "I'll sign you into your bank and your broker. Once you're in I can read it — balances, \
     statements, positions — and that's where it stops. I won't submit anything but the sign-in \
     form, and I won't click a button whose label looks like it moves money, on a bank or a \
     broker.";

/// Why this is safer than you typing it, which is the part that surprises
/// people.
pub const WHY_SAFER: &str =
    "Filling is matched to the registered domain, so it won't put your password into a lookalike. \
     That's the common way accounts are actually lost — someone types it into a page that looks \
     right. I can't be fooled by how a page looks, only by the domain, and the domain is the one \
     thing a phishing page can't fake.";

/// The line that stays, and why it isn't arbitrary.
pub const SIGNIN_IS_NOT_SETTINGS: &str =
    "Signing in doesn't let me change your security settings. They're separate grants and holding \
     one never implies the other — otherwise \"log me into my bank\" would quietly mean \"and you \
     can move money\", which isn't what you asked for.";

/// If you already have one, it's better than this.
pub const IF_YOU_HAVE_ONE: &str =
    "If you already use 1Password or Bitwarden, keep doing that and point me at it instead. \
     They're audited, they sync to your phone, and they survive this laptop dying. Mine is here \
     for the case where you don't.";
