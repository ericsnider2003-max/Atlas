//! Getting your accounts ready before you can't reach them.
//!
//! ## The problem, stated properly
//!
//! You go away. You can't receive a text. Two-factor locks you out of your own
//! accounts — so turning it off looks like the fix.
//!
//! It isn't, and the reason matters: **the thing that breaks when you travel
//! is SMS, not two-factor.** A text needs your number, your carrier and a
//! signal. An authenticator code needs none of those — it's a clock and a
//! secret, and it works on a plane, in a facility with no signal, on a device
//! that has never been online. A printed recovery code works with no phone at
//! all. A hardware key works with no phone, no signal and no battery.
//!
//! So the answer is not less security. It's **the same security on something
//! that survives where you're going** — which happens to be both more
//! available and harder to steal than what you have now.
//!
//! Turning it off is also the worst possible option for someone who is away:
//! the account is least protected exactly when you are least able to notice
//! something wrong with it and least able to fix it.

use crate::accounts::{Account, SecondFactor, Stakes};
use serde::{Deserialize, Serialize};

/// Does this survive being somewhere with no signal and no phone?
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Survives {
    /// Works anywhere, no signal, no phone, no battery.
    Anywhere,
    /// Works with no signal, but you need the device.
    NeedsYourDevice,
    /// Needs a signal on your own number. This is what breaks.
    NeedsYourNumber,
    /// Needs to reach your email, which may itself be locked.
    NeedsEmail,
}

impl Survives {
    pub fn of(f: SecondFactor) -> Survives {
        match f {
            // A clock and a secret. No network involved at all.
            SecondFactor::App => Survives::NeedsYourDevice,
            SecondFactor::Key | SecondFactor::Passkey => Survives::NeedsYourDevice,
            SecondFactor::Sms => Survives::NeedsYourNumber,
            SecondFactor::Email => Survives::NeedsEmail,
            SecondFactor::None => Survives::Anywhere,
        }
    }

    pub fn plain(&self) -> &'static str {
        match self {
            Survives::Anywhere => "works anywhere",
            Survives::NeedsYourDevice => "works with no signal, as long as you have the device",
            Survives::NeedsYourNumber => "needs a signal on your own number — this is what breaks",
            Survives::NeedsEmail => "needs your email, which may be locked too",
        }
    }

    pub fn breaks_when_away(&self) -> bool {
        matches!(self, Survives::NeedsYourNumber | Survives::NeedsEmail)
    }
}

/// What to do about an account before you go.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Prepare {
    pub site: String,
    pub what: String,
    pub why: String,
    /// Do this before you leave, not after.
    pub before_you_go: bool,
    pub urgency: f32,
    pub open: Option<String>,
}

/// Everything that would lock you out, and what to do instead.
pub fn plan(accounts: &[Account]) -> Vec<Prepare> {
    let mut out = Vec::new();

    for a in accounts {
        let survives = Survives::of(a.second_factor);
        let weight = match a.stakes {
            Stakes::Keystone => 1.0,
            Stakes::High => 0.8,
            Stakes::Medium => 0.5,
            Stakes::Low => 0.2,
        };

        // The actual problem, named.
        if survives.breaks_when_away() {
            out.push(Prepare {
                site: a.site.clone(),
                what: "move this from a text code to an authenticator app".into(),
                why: format!(
                    "{} — an app code is a clock and a secret, so it works with no signal at all",
                    survives.plain()
                ),
                before_you_go: true,
                urgency: weight,
                open: a.settings_url.clone(),
            });
        }

        // The thing that actually saves you, and almost nobody does.
        if !a.has_recovery_codes && a.second_factor != SecondFactor::None {
            out.push(Prepare {
                site: a.site.clone(),
                what: "print the recovery codes and put them somewhere you'll have".into(),
                why: "ten one-time codes on paper work with no phone, no signal and no battery. \
                      This is the single thing that stops a lockout"
                    .into(),
                before_you_go: true,
                urgency: weight * 0.95,
                open: a.settings_url.clone(),
            });
        }

        // Worth saying once for the accounts that matter.
        if a.stakes >= Stakes::High && a.second_factor.strength() < 5 {
            out.push(Prepare {
                site: a.site.clone(),
                what: "consider a hardware key for this one".into(),
                why: "no phone, no signal, no battery — it's the option built for exactly your \
                      situation, and a spare kept at home covers losing it"
                    .into(),
                before_you_go: true,
                urgency: weight * 0.5,
                open: None,
            });
        }
    }

    out.sort_by(|a, b| b.urgency.partial_cmp(&a.urgency).unwrap_or(std::cmp::Ordering::Equal));
    out.dedup_by(|a, b| a.site == b.site && a.what == b.what);
    out
}

/// How many accounts would actually lock you out.
pub fn would_lock_you_out(accounts: &[Account]) -> Vec<&Account> {
    accounts
        .iter()
        .filter(|a| Survives::of(a.second_factor).breaks_when_away() && !a.has_recovery_codes)
        .collect()
}

/// What Atlas says when you tell it you're going away.
pub fn spoken(accounts: &[Account]) -> String {
    let stuck = would_lock_you_out(accounts);
    if stuck.is_empty() {
        return "You'd get into all of these from anywhere. Nothing to do.".into();
    }
    let names: Vec<&str> = stuck.iter().take(3).map(|a| a.site.as_str()).collect();
    let more = stuck.len().saturating_sub(names.len());

    let mut s = format!(
        "{} would lock you out: {}",
        stuck.len(),
        names.join(", ")
    );
    if more > 0 {
        s.push_str(&format!(" and {more} more"));
    }
    s.push_str(
        ". They're on text codes, which need a signal on your number. Two things fix that, and \
         both leave you better protected than turning it off would: move them to an \
         authenticator app, and print the recovery codes.",
    );
    s
}

/// Why not just turn it off, when you've already explained you'll be away.
///
/// The answer is different from the general one, and worth giving properly
/// rather than repeating a policy.
pub const WHY_NOT_OFF: &str =
    "Being away is the worst time to have it off, not the best — the account is least protected \
     exactly when you're least able to notice something's wrong and least able to do anything \
     about it. And it doesn't solve what you're actually hitting, which is that a text needs your \
     number and a signal. An app code needs neither: it's a clock and a shared secret, and it \
     works on a plane. Recovery codes on paper work with no phone at all. I'll set both up with \
     you before you go, and open every page you need.";

/// Could Atlas just hold the codes and log you in?
///
/// Worth answering honestly rather than refusing, because the technical answer
/// is yes and the reason not to isn't obvious.
pub const WHY_NOT_ATLAS_HOLDS_IT: &str =
    "Technically yes — an authenticator is a stored secret and a clock, and I could keep both. \
     But then the thing holding your password also holds your second factor, which makes them one \
     factor wearing two hats. If this machine is taken, everything goes with it. Keep the codes \
     on your phone or a key, and I'll do everything around that: remind you before you travel, \
     check what's on text codes, open the pages, and tell you which accounts have no recovery \
     saved.";

/// The one thing to do if you only do one thing.
pub fn if_you_do_one_thing(accounts: &[Account]) -> Option<String> {
    let keystone = accounts.iter().find(|a| a.stakes == Stakes::Keystone)?;
    Some(format!(
        "If you do one thing: print the recovery codes for {}. Every other account resets \
         through it, so getting locked out of that one locks you out of everything.",
        keystone.site
    ))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AwayConfig {
    pub enabled: bool,
    /// Remind you this many days before a trip you've told it about.
    pub remind_days_before: u32,
    /// Check every so often even without a trip, since deployments move.
    pub check_every_days: u32,
}

impl Default for AwayConfig {
    fn default() -> Self {
        AwayConfig { enabled: false, remind_days_before: 14, check_every_days: 90 }
    }
}

/// A check worth running periodically rather than only before a trip, because
/// dates change and the preparation takes days — recovery codes have to be
/// printed, and a hardware key has to arrive.
pub fn periodic_nudge(accounts: &[Account], days_since_last: u32, cfg: &AwayConfig) -> Option<String> {
    if !cfg.enabled || days_since_last < cfg.check_every_days {
        return None;
    }
    let stuck = would_lock_you_out(accounts);
    if stuck.is_empty() {
        return None;
    }
    Some(format!(
        "{} accounts would still lock you out if you had to leave tomorrow. It takes ten minutes \
         and I can open the pages.",
        stuck.len()
    ))
}

// ============ the trip itself ============
//
// `remind_days_before` is "remind you this many days before a trip you've
// told it about", and there was no way to tell it about a trip — no date
// anywhere in the tree, so the setting was a threshold on a number that was
// never computed. `periodic_nudge` had the same shape one step along: it
// takes `days_since_last` and nothing kept a last.
//
// Both needed the same small thing: somewhere to write down that you are
// going away, and when you were last asked about it.

/// When you're going, and where.
///
/// No detail beyond that on purpose. This exists to answer one question —
/// how many days until you cannot receive a text — and a travel record that
/// grew flight numbers and hotel names would be a second calendar to keep in
/// step with your real one.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Away {
    /// Midnight of the day you leave, or zero for no trip.
    pub leaving_at: u64,
    /// Where, in your words. Only ever said back to you.
    pub going_to: String,
    /// When Atlas last asked about any of this, so the periodic check has a
    /// last to count from.
    pub last_checked: u64,
}

/// Where that is kept.
pub const AWAY_RECORD: &str = "going_away";

impl Away {
    pub fn on_a_trip(&self) -> bool {
        self.leaving_at > 0
    }

    /// Days until you leave. `None` when there is no trip, or it has passed.
    pub fn days_until(&self, now: u64) -> Option<u32> {
        if !self.on_a_trip() || self.leaving_at < now {
            return None;
        }
        u32::try_from((self.leaving_at - now) / 86_400).ok()
    }

    /// Close enough that the preparation has to start.
    ///
    /// The window matters more than it looks: printing recovery codes and
    /// waiting for a hardware key both take days, so being told the morning
    /// you leave is the same as not being told.
    pub fn time_to_get_ready(&self, cfg: &AwayConfig, now: u64) -> bool {
        cfg.enabled && self.days_until(now).is_some_and(|d| d <= cfg.remind_days_before)
    }

    /// How long since Atlas last raised any of this.
    pub fn days_since_asked(&self, now: u64) -> u32 {
        if self.last_checked == 0 || self.last_checked > now {
            // Never asked is a long time, not no time. Zero would mean the
            // periodic check never came due on a fresh install.
            return u32::MAX;
        }
        u32::try_from(now.saturating_sub(self.last_checked) / 86_400).unwrap_or(u32::MAX)
    }
}

/// What Atlas says when the trip is close enough to act on.
///
/// The accounts half. `codes::before_you_go` is the other, and the daemon
/// says both — they answer different questions and either alone is a wrong
/// impression: "every account is reachable" is no comfort with no codes
/// printed, and "ten codes in hand" is no comfort for the account that has
/// none.
pub fn the_trip_is_close(away: &Away, accounts: &[Account], cfg: &AwayConfig, now: u64) -> Option<String> {
    if !away.time_to_get_ready(cfg, now) {
        return None;
    }
    let days = away.days_until(now)?;
    let stuck = would_lock_you_out(accounts);
    if stuck.is_empty() {
        return None;
    }
    let where_ = if away.going_to.trim().is_empty() {
        String::new()
    } else {
        format!(" to {}", away.going_to.trim())
    };
    Some(format!(
        "You're going{where_} in {days} day{}. {} account{} would lock you out from there. {}",
        if days == 1 { "" } else { "s" },
        stuck.len(),
        if stuck.len() == 1 { "" } else { "s" },
        if_you_do_one_thing(accounts).unwrap_or_else(|| "I can open the pages.".into())
    ))
}

/// Read a leaving date from what somebody typed.
///
/// An ISO date, because it is the one form that means the same thing to
/// everybody. `one_way_to_write_a_date` in the test suite is the standing
/// ruling this follows.
pub fn leaving_on(text: &str) -> Option<u64> {
    let t = text.trim();
    let mut parts = t.split('-');
    let y: i32 = parts.next()?.parse().ok()?;
    let m: u32 = parts.next()?.parse().ok()?;
    let d: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let days = crate::market::time::days_from_civil(y, m, d);
    u64::try_from(days).ok().map(|d| d * 86_400)
}
