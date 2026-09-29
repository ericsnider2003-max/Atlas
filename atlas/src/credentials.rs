//! Every credential Atlas holds, in one place you can check.
//!
//! You asked a fair question and the answer had drifted, so here it is
//! pinned down: **Atlas has never had your passwords, and there is no code in
//! it that logs into anything.** What it does instead is ride sessions you
//! established yourself — you're signed into LinkedIn in your browser, so
//! Atlas can post there; you're not, so it can't and says so.
//!
//! That's a real distinction rather than a technicality. A session is scoped
//! to one browser profile on one machine, expires, and can be revoked from the
//! site. A password is the account itself.
//!
//! The one exception is mail, which needs a credential because IMAP has no
//! concept of "the session you already have". That one is an **app password**:
//! issued separately, revocable from the account without changing anything
//! else, and scoped to mail only. It is not your password, and if it leaks you
//! revoke it and nothing else moves.
//!
//! This module exists so that stays true. Every credential is registered here
//! with what it opens and where it lives, and there's a test that nothing
//! sensitive is sitting in a config file.

use serde::{Deserialize, Serialize};

/// What a credential actually gets you.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Opens {
    /// Nothing on its own.
    Nothing,
    /// One narrow thing, revocable without touching the account.
    OneThing,
    /// A whole service.
    AService,
    /// The account, and everything that resets through it.
    Everything,
}

/// Where it's kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kept {
    /// Not held at all — you type it when needed.
    NotHeld,
    /// In the browser, as a session cookie you created by signing in.
    YourBrowserSession,
    /// In the passphrase-locked vault.
    Vault,
    /// In a config file, in plain text.
    ///
    /// Nothing that opens more than nothing may live here, and there's a test.
    PlainConfig,
}

impl Kept {
    pub fn safe_for(&self, opens: Opens) -> bool {
        match self {
            Kept::NotHeld => true,
            Kept::YourBrowserSession => opens <= Opens::AService,
            Kept::Vault => true,
            // The line that matters.
            Kept::PlainConfig => opens == Opens::Nothing,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Credential {
    pub name: &'static str,
    /// What it's for.
    pub used_for: &'static str,
    pub opens: Opens,
    pub kept: Kept,
    /// How you take it away.
    pub revoke: &'static str,
    /// Does Atlas need it unattended?
    pub needed_overnight: bool,
}

/// Everything Atlas holds or touches. If it isn't here, it doesn't exist.
pub fn all() -> Vec<Credential> {
    vec![
        Credential {
            name: "browser session",
            used_for: "posting, and reading a page you're already signed into",
            opens: Opens::AService,
            kept: Kept::YourBrowserSession,
            revoke: "sign out, or clear the profile Atlas uses — it has its own",
            needed_overnight: false,
        },
        Credential {
            name: "mail app password",
            used_for: "reading and sorting your inbox over IMAP",
            opens: Opens::OneThing,
            kept: Kept::Vault,
            revoke: "revoke it in your account's app-password list — nothing else changes",
            needed_overnight: false,
        },
        Credential {
            name: "hosted model key",
            used_for: "the paid model, if you ever turn it on",
            opens: Opens::OneThing,
            kept: Kept::Vault,
            revoke: "delete the key in the console",
            needed_overnight: true,
        },
        Credential {
            name: "authenticator seeds",
            used_for: "nothing — held only if you choose to put them there",
            opens: Opens::Everything,
            kept: Kept::Vault,
            revoke: "regenerate the second factor on the account",
            needed_overnight: false,
        },
    ]
}

/// Things Atlas deliberately does not hold, and won't.
pub fn never_held() -> Vec<(&'static str, &'static str)> {
    vec![
        ("your account passwords",
         "Atlas has no code that signs into anything. It uses sessions you created"),
        ("your banking credentials",
         "financial sites are read-only by construction — it can't submit a form on one"),
        ("your master password",
         "for a password manager, which is the right place for passwords and isn't this"),
        ("recovery codes",
         "it tracks how many you have left, never the codes themselves"),
    ]
}

/// The check that keeps this honest.
///
/// Anything that opens more than nothing must not be in a config file. This is
/// the rule that got broken quietly, and the reason it's a test rather than a
/// convention.
pub fn misplaced() -> Vec<&'static Credential> {
    let all: &'static [Credential] = Box::leak(all().into_boxed_slice());
    all.iter().filter(|c| !c.kept.safe_for(c.opens)).collect()
}

/// Something needed unattended can't live behind a passphrase.
///
/// Worth surfacing rather than discovering at 3am when overnight work stops:
/// if the hosted model is on, its key has to be reachable without you, and
/// that's a real trade.
pub fn needs_you_awake() -> Vec<Credential> {
    // Owned, not `&'static`: the old version leaked a fresh copy of the whole
    // list every time it was asked.
    all().into_iter().filter(|c| c.needed_overnight && c.kept == Kept::Vault).collect()
}

/// What Atlas says when you ask what it's holding.
pub fn spoken() -> String {
    let held: Vec<Credential> =
        all().into_iter().filter(|c| c.kept != Kept::NotHeld).collect();
    let vault = held.iter().filter(|c| c.kept == Kept::Vault).count();
    format!(
        "{} things: {} in the locked vault, and your browser session, which you made by signing \
         in yourself. No passwords — I've got no code that signs into anything.",
        held.len(),
        vault
    )
}

/// The full account, for reading.
pub fn written() -> String {
    let mut s = String::from("What I hold\n\n");
    for c in all() {
        s.push_str(&format!(
            "{}\n  for: {}\n  kept: {:?}\n  revoke: {}\n\n",
            c.name, c.used_for, c.kept, c.revoke
        ));
    }
    s.push_str("What I don't\n\n");
    for (what, why) in never_held() {
        s.push_str(&format!("{what}\n  {why}\n\n"));
    }
    s
}

/// Asked whether Atlas can sign into something.
pub const CAN_IT_LOG_IN: &str =
    "No — there's no code in me that signs into anything, and no password store. What I use is \
     the session you already made: if you're signed into LinkedIn in the browser, I can post \
     there; if you're not, I'll tell you rather than trying. The one credential I hold is a mail \
     app password, which is issued separately, only works for mail, and can be revoked without \
     changing your actual password.";

/// The difference, since it sounds like a technicality and isn't.
pub const SESSION_VS_PASSWORD: &str =
    "A session is one browser profile on one machine, it expires, and you can revoke it from the \
     site's own security page. A password is the account itself — it works from anywhere, forever, \
     and revoking it means changing it everywhere. That's why one of those is worth holding and \
     the other isn't.";
