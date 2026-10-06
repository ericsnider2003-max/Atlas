//! Walking you through a change on your own accounts.
//!
//! You offered this yourself as the alternative — Atlas pulls up the page and
//! you make the change — and it's the right shape, so here it is properly.
//!
//! Atlas finds the setting, opens the exact page, tells you where the switch
//! is, waits, and moves to the next one. Fifteen accounts becomes fifteen
//! clicks instead of an afternoon of hunting through settings menus that are
//! all laid out differently on purpose.
//!
//! ## One thing worth noticing about turning two-factor off
//!
//! You can only change it from inside the account. Which means in every
//! scenario where you'd want it off, you can already get in — and in the
//! scenario you're actually worried about, being locked out, having it off
//! wouldn't have helped because you'd have needed access to turn it off.
//!
//! That's not an argument against doing it. It's the reason the preparation
//! path solves the problem and this one only feels like it does.

use serde::{Deserialize, Serialize};

/// One step of a change you're making.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stop {
    pub site: String,
    /// The exact page, not the homepage.
    pub url: String,
    /// Where the control actually is.
    pub where_to_look: String,
    /// What to do there.
    pub do_this: String,
    /// What you'll need in hand before you start.
    pub have_ready: Option<String>,
    /// Worth knowing before you click.
    pub warning: Option<String>,
    pub done: bool,
}

/// A run of them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Walk {
    pub what_for: String,
    pub stops: Vec<Stop>,
    pub at: usize,
}

impl Walk {
    pub fn new(what_for: &str, stops: Vec<Stop>) -> Walk {
        Walk { what_for: what_for.into(), stops, at: 0 }
    }

    pub fn current(&self) -> Option<&Stop> {
        self.stops.get(self.at)
    }

    /// What Atlas says at each stop.
    pub fn say(&self) -> String {
        match self.current() {
            None => format!("{} — all {} done.", self.what_for, self.stops.len()),
            Some(s) => {
                let mut out = format!(
                    "{} of {}: {}. {}",
                    self.at + 1,
                    self.stops.len(),
                    s.site,
                    s.where_to_look
                );
                out.push_str(&format!(" {}", s.do_this));
                if let Some(need) = &s.have_ready {
                    out.push_str(&format!(" You'll need {need}."));
                }
                if let Some(w) = &s.warning {
                    out.push_str(&format!(" {w}"));
                }
                out
            }
        }
    }

    #[allow(clippy::should_implement_trait, reason = "moves the walkthrough to its next stop and says whether there was one; not an Iterator")]
    pub fn next(&mut self) -> bool {
        if let Some(s) = self.stops.get_mut(self.at) {
            s.done = true;
        }
        self.at += 1;
        self.at < self.stops.len()
    }

    /// Come back to it. Half-finished is the normal state of these.
    pub fn skip(&mut self) -> bool {
        self.at += 1;
        self.at < self.stops.len()
    }

    pub fn progress(&self) -> (usize, usize) {
        (self.stops.iter().filter(|s| s.done).count(), self.stops.len())
    }

    /// What's left, for picking it up later.
    pub fn remaining(&self) -> Vec<&Stop> {
        self.stops.iter().filter(|s| !s.done).collect()
    }
}

/// Where the setting actually lives, per site.
///
/// The value here is entirely in being exact: "Google security settings" is
/// something you can find yourself, and the reason this is tedious is that
/// every site buries it differently.
pub fn where_2fa_lives(site: &str) -> Option<(&'static str, &'static str)> {
    let s = site.to_lowercase();
    Some(match () {
        _ if s.contains("google") || s.contains("gmail") => (
            "https://myaccount.google.com/signinoptions/two-step-verification",
            "Under 2-Step Verification. Turning it off needs your password again.",
        ),
        _ if s.contains("instagram") => (
            "https://accountscenter.instagram.com/password_and_security/",
            "Accounts Centre, then Password and security, then Two-factor authentication.",
        ),
        _ if s.contains("facebook") => (
            "https://accountscenter.facebook.com/password_and_security/",
            "Accounts Centre, then Password and security. It covers Instagram too if they're linked.",
        ),
        _ if s.contains("tiktok") => (
            "https://www.tiktok.com/setting/",
            "Settings, then Security and permissions, then 2-step verification.",
        ),
        _ if s.contains("x.com") || s.contains("twitter") => (
            "https://x.com/settings/account/login_verification",
            "Settings, Security and account access, Security, Two-factor authentication.",
        ),
        _ if s.contains("github") => (
            "https://github.com/settings/security",
            "Password and authentication. Turning it off may remove your SSH access too.",
        ),
        _ if s.contains("microsoft") || s.contains("outlook") => (
            "https://account.microsoft.com/security",
            "Security, then Advanced security options.",
        ),
        _ if s.contains("apple") || s.contains("icloud") => (
            "https://appleid.apple.com/account/manage",
            "Sign-In and Security. Apple no longer lets most accounts turn it off at all.",
        ),
        _ if s.contains("amazon") => (
            "https://www.amazon.com/a/settings/approval",
            "Login and security, then Two-step verification settings.",
        ),
        _ => return None,
    })
}

/// Build the walk for a set of accounts.
pub fn to_turn_off(sites: &[String]) -> Walk {
    let stops: Vec<Stop> = sites
        .iter()
        .filter_map(|site| {
            let (url, where_to_look) = where_2fa_lives(site)?;
            Some(Stop {
                site: site.clone(),
                url: url.into(),
                where_to_look: where_to_look.into(),
                do_this: "Turn it off there. I'll wait — say next when you're done.".into(),
                have_ready: Some("your password, and probably a code from the method you're removing".into()),
                warning: warning_for(site),
                done: false,
            })
        })
        .collect();
    Walk::new("Turning off two-factor", stops)
}

/// Turning two-factor on (Eric, B1: "turn it on or off for me").
pub fn to_turn_on(sites: &[String]) -> Walk {
    let stops: Vec<Stop> = sites
        .iter()
        .filter_map(|site| {
            let (url, where_to_look) = where_2fa_lives(site)?;
            Some(Stop {
                site: site.clone(),
                url: url.into(),
                where_to_look: where_to_look.into(),
                do_this: "Turn it on there — an authenticator app if it offers one, and print the \
                          recovery codes it gives you. Say next when you're done."
                    .into(),
                have_ready: Some("your phone".into()),
                warning: None,
                done: false,
            })
        })
        .collect();
    Walk::new("Turning on two-factor", stops)
}

/// Build the walk for the version that actually solves being away.
pub fn to_prepare(sites: &[String]) -> Walk {
    let stops: Vec<Stop> = sites
        .iter()
        .filter_map(|site| {
            let (url, where_to_look) = where_2fa_lives(site)?;
            Some(Stop {
                site: site.clone(),
                url: url.into(),
                where_to_look: where_to_look.into(),
                do_this: "Switch it from a text code to an authenticator app, then print the \
                          recovery codes."
                    .into(),
                have_ready: Some("your phone and somewhere to print".into()),
                warning: None,
                done: false,
            })
        })
        .collect();
    Walk::new("Getting ready to be away", stops)
}

/// The things worth saying before you click, which vary more than people
/// expect.
fn warning_for(site: &str) -> Option<String> {
    let s = site.to_lowercase();
    if s.contains("apple") || s.contains("icloud") {
        return Some("Apple mostly doesn't allow this any more — you may find the option isn't there."
            .into());
    }
    if s.contains("github") {
        return Some("This can also drop your SSH keys and tokens, which breaks anything automated."
            .into());
    }
    if s.contains("bank") || s.contains("chase") || s.contains("coinbase") {
        return Some(
            "Most banks won't let you, and the ones that do may flag the account. Worth a call \
             instead — they can usually note a travel period."
                .into(),
        );
    }
    if s.contains("google") || s.contains("gmail") {
        return Some(
            "This is the account everything else resets through, so it's the one I'd leave on if \
             you leave any on."
                .into(),
        );
    }
    None
}

/// What Atlas says before starting a run to weaken things.
///
/// Once, without lecturing, and then it helps.
pub fn before_turning_off(count: usize) -> String {
    format!(
        "{count} accounts, and I'll open every one. Two things first, then I'll stop saying it. \
         You can only turn it off from inside the account — so in the case you're worried about, \
         being locked out, having it off wouldn't have helped. And if you get a heads-up before \
         you go, moving to an authenticator app takes the same fifteen minutes and doesn't leave \
         the accounts open while you're gone. Say go and I'll start, or say prepare and I'll do \
         that run instead."
    )
}

/// The other thing his situation actually points at.
pub const FAMILY_ACCESS: &str =
    "You mentioned family can help while you're away. Several of these have a proper way to do \
     that — Google has Inactive Account Manager, Apple has Legacy Contact, and most banks have a \
     travel note or a power of attorney form. That gets someone in without the account sitting \
     open for months, and it's the thing your situation actually points at. I can open those too.";

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct WalkConfig {
    pub enabled: bool,
    /// Open the page automatically at each stop.
    pub open_pages: bool,
    /// After the read-back and your yes, Atlas presses the control itself
    /// in its own browser — only when exactly one control on the page is
    /// labelled for the change, never past a password prompt, and never
    /// typing anything. Off until you turn it on.
    ///
    /// This was `#[serde(skip)]` over a function returning false, so no
    /// file could turn it on. Eric, 24 Sep 2026, on security changes: "yes"
    /// — Atlas may make the change itself once you've said yes to it.
    pub atlas_clicks: bool,
}

impl Default for WalkConfig {
    fn default() -> Self {
        WalkConfig { enabled: true, open_pages: true, atlas_clicks: false }
    }
}
