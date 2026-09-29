//! Whether each connection is still working.
//!
//! `connectivity` asks whether the internet is reachable. `doctor` checks
//! everything, once, when you ask. Neither answers the question that actually
//! bites: **is this one integration still working right now?**
//!
//! The failure it exists for is quiet. A token expires, a service changes an
//! endpoint, a machine goes off the network — and Atlas carries on, because
//! nothing that needed that integration happened to run. You find out days
//! later when something you cared about silently did not happen.
//!
//! Three ideas do the work here.
//!
//! **Last success, not last attempt.** An integration that has failed forty
//! times in a row is not "recently active". The clock that matters runs from
//! the last time it actually worked.
//!
//! **Silence is not health.** An integration nobody has called in a week is
//! unknown, not fine. Reporting it as working is the mistake this module was
//! written to stop making, and it is the same mistake `hollow` names: absence
//! of a finding read as absence of a problem.
//!
//! **A stale check is worse than no check.** A monitor whose own last look was
//! yesterday will tell you yesterday's answer in the present tense.

use serde::{Deserialize, Serialize};

/// How an integration stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Health {
    /// Worked recently.
    Working,
    /// Worked, but not lately. Might be fine, might be broken since.
    Quiet,
    /// Tried and failed.
    Failing,
    /// Never worked, or nothing has ever called it.
    Unknown,
    /// You switched it off. Not a fault.
    Off,
}

impl Health {
    /// Does this need you to do something?
    ///
    /// `Quiet` deliberately does not. Nagging about every integration you
    /// have not used this week is how a status page stops being read.
    pub fn wants_attention(&self) -> bool {
        matches!(self, Health::Failing | Health::Unknown)
    }

    pub fn plain(&self) -> &'static str {
        match self {
            Health::Working => "working",
            Health::Quiet => "no recent use, so I can't say",
            Health::Failing => "failing",
            Health::Unknown => "never seen it work",
            Health::Off => "switched off",
        }
    }
}

/// One connection Atlas depends on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Integration {
    pub name: String,
    /// What stops working if this does. Written when the integration is added,
    /// so a failure reads as a consequence rather than a name.
    pub if_it_breaks: String,
    pub enabled: bool,
    /// When it last actually worked.
    pub last_ok: Option<u64>,
    /// When it last failed, and why.
    pub last_fail: Option<(u64, String)>,
    /// Consecutive failures since the last success.
    #[serde(default)]
    pub failures_running: u32,
}

/// Beyond this with no success, an integration is quiet rather than working.
pub const QUIET_AFTER_SECS: u64 = 60 * 60 * 24 * 3;

/// A monitor that has not looked in this long is reporting history.
pub const MONITOR_STALE_AFTER_SECS: u64 = 60 * 60 * 6;

impl Integration {
    pub fn new(name: &str, if_it_breaks: &str) -> Integration {
        Integration {
            name: name.to_string(),
            if_it_breaks: if_it_breaks.to_string(),
            enabled: true,
            last_ok: None,
            last_fail: None,
            failures_running: 0,
        }
    }

    pub fn worked(&mut self, now: u64) {
        self.last_ok = Some(now);
        self.failures_running = 0;
    }

    pub fn failed(&mut self, now: u64, why: &str) {
        self.last_fail = Some((now, why.to_string()));
        self.failures_running = self.failures_running.saturating_add(1);
    }

    pub fn health(&self, now: u64) -> Health {
        if !self.enabled {
            return Health::Off;
        }
        // A failure since the last success is the current state, however long
        // ago the success was.
        if self.failures_running > 0 {
            return Health::Failing;
        }
        match self.last_ok {
            None => Health::Unknown,
            Some(t) if now.saturating_sub(t) <= QUIET_AFTER_SECS => Health::Working,
            Some(_) => Health::Quiet,
        }
    }

    /// One line for the hub.
    pub fn line(&self, now: u64) -> String {
        let h = self.health(now);
        match (&h, &self.last_fail) {
            (Health::Failing, Some((_, why))) => {
                format!("{} — failing: {why}. {}", self.name, self.if_it_breaks)
            }
            (Health::Unknown, _) => {
                format!("{} — {}. {}", self.name, h.plain(), self.if_it_breaks)
            }
            _ => format!("{} — {}", self.name, h.plain()),
        }
    }
}

/// Everything Atlas depends on, and when it last looked.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Board {
    pub integrations: Vec<Integration>,
    /// When the monitor itself last ran.
    pub last_swept: Option<u64>,
}

impl Board {
    pub fn add(&mut self, i: Integration) {
        match self.integrations.iter_mut().position(|x| x.name == i.name) {
            Some(n) => self.integrations[n] = i,
            None => self.integrations.push(i),
        }
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut Integration> {
        self.integrations.iter_mut().find(|i| i.name == name)
    }

    pub fn swept(&mut self, now: u64) {
        self.last_swept = Some(now);
    }

    /// Is the monitor itself trustworthy right now?
    ///
    /// Checked before anything else it says. A monitor whose own last look was
    /// yesterday will report yesterday's answer in the present tense, which is
    /// the failure it was built to prevent, committed by the monitor.
    pub fn is_current(&self, now: u64) -> bool {
        match self.last_swept {
            None => false,
            Some(t) => now.saturating_sub(t) <= MONITOR_STALE_AFTER_SECS,
        }
    }

    /// Everything that needs you, worst first.
    pub fn needs_you(&self, now: u64) -> Vec<&Integration> {
        let mut v: Vec<&Integration> = self
            .integrations
            .iter()
            .filter(|i| i.health(now).wants_attention())
            .collect();
        v.sort_by_key(|i| (i.health(now), std::cmp::Reverse(i.failures_running)));
        v
    }

    /// The hub panel.
    ///
    /// Leads with whether the monitor itself is current, then with what is
    /// broken. Working things come last, because a page that opens with
    /// nine greens and one red trains you to skim past the red.
    pub fn panel(&self, now: u64) -> String {
        if self.integrations.is_empty() {
            return "Nothing connected yet.".into();
        }
        let mut lines = Vec::new();
        if !self.is_current(now) {
            lines.push(match self.last_swept {
                None => "I haven't checked any of these yet, so none of it is current.".into(),
                Some(t) => format!(
                    "Last checked {} hours ago, so treat this as history.",
                    now.saturating_sub(t) / 3600
                ),
            });
        }
        let needs = self.needs_you(now);
        for i in &needs {
            lines.push(i.line(now));
        }
        let fine = self.integrations.len() - needs.len();
        if fine > 0 {
            lines.push(format!("{fine} other{} fine.", if fine == 1 { "" } else { "s" }));
        }
        lines.join("\n")
    }

    /// One line, for speaking.
    pub fn spoken(&self, now: u64) -> String {
        if !self.is_current(now) {
            return "I haven't checked the connections recently enough to tell you.".into();
        }
        match self.needs_you(now).as_slice() {
            [] => "Everything's connected.".into(),
            [one] => format!("{} needs a look. {}", one.name, one.if_it_breaks),
            many => format!(
                "{} connections need a look, starting with {}.",
                many.len(),
                many[0].name
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Naming the gap in the answer itself.
//
// This module and Atlas's own `links.rs` were built the same week, from the
// same root cause in `hollow.rs`, for the same failure — a Jarvis build that
// quietly "got dumber" over weeks because connections had detached and it
// answered from what was left without saying so. Two fixes for one problem is
// worse than one, so `links.rs` was retired and this became the only copy.
// This function is the one thing it had that this file did not.
//
// A `Health` printed on a hub page is a warning nobody is looking at when the
// answer actually arrives. It has to be attached to the answer.
// ---------------------------------------------------------------------------

/// The name the internet is recorded under.
pub const INTERNET: &str = "the internet";

/// The name the reasoning model is recorded under.
pub const MODEL: &str = "the language model";

/// The connections Atlas actually watches while it runs.
///
/// **Why this list is short.** A `Board` is only worth reading if every entry
/// on it is genuinely observed. Until this function existed the board was
/// created empty and nothing ever added to it, so `swept()` ran every tick
/// over nothing, `needs_you()` could not return anything, `nudge::link_broke`
/// could never fire, and `mark()` — the whole point of the module — was
/// never called. The feature read as delivered and did nothing. That is the
/// `hollow.rs` failure exactly: an absence of findings looked like an absence
/// of problems.
///
/// The fix is not to list every dependency Atlas has. It is to list the ones
/// with a real success-or-failure moment in the running code, because
/// `Health::Unknown` wants attention, so registering something nothing ever
/// reports on would permanently flag a connection that is probably fine.
///
/// Deliberately **not** here yet, each for the same reason — no call site
/// currently reports an outcome:
/// - speech-to-text and text-to-speech: `doctor` checks them once, on demand,
///   rather than during a run. When `doctor`'s result is routed here, add them.
/// - the headless browser: `browser.rs` has a real failure path, but nothing
///   in the daemon drives it yet.
pub fn dependencies() -> Board {
    let mut b = Board::default();
    b.add(Integration::new(
        INTERNET,
        "research, anything fetched from a page, and any hosted model",
    ));
    b.add(Integration::new(
        MODEL,
        "anything the phrase parser can't recognise on its own",
    ));
    b
}

/// Which registered connections an answer actually leaned on.
///
/// Driven by the same `Need` the connectivity gate already uses, so there is
/// one answer to "does this need the network", not two that can drift apart.
///
/// The model is listed only when it was genuinely consulted. If the phrase
/// parser recognised the request outright, the answer is complete whether or
/// not the model is reachable, and saying otherwise would attach a warning to
/// an answer that has nothing missing from it.
pub fn sources_for(need: crate::connectivity::Need, model: crate::brain::Reached) -> Vec<&'static str> {
    use crate::connectivity::Need;
    let mut used = Vec::new();
    // Only a hard requirement. `PrefersInternet` answers offline from the
    // local model — the answer is complete, just produced by a smaller model,
    // and that is `tier`'s thing to report, not a missing connection. Listing
    // it here attached "I've never seen the internet work" to every ordinary
    // conversational reply on a daemon that had not yet run a tick, which is
    // how a caveat system teaches you to ignore caveats.
    if matches!(need, Need::Internet) {
        used.push(INTERNET);
    }
    if matches!(model, crate::brain::Reached::Yes | crate::brain::Reached::No) {
        used.push(MODEL);
    }
    used
}

/// How a health reads when it follows the name of the thing, mid-sentence.
///
/// `plain()` is written to sit after an em dash ("stripe — never seen it
/// work"), which reads correctly there and not here. `mark` built its
/// sentence as `"{name} is {plain}"` and produced "the internet is never seen
/// it work". Nobody caught it because `mark` was never called and its only
/// tests covered the `Failing` case, where the two phrasings happen to agree.
fn as_clause(name: &str, h: Health) -> String {
    match h {
        Health::Working => format!("{name} is working"),
        Health::Failing => format!("{name} is failing"),
        Health::Off => format!("{name} is switched off"),
        Health::Quiet => format!("I haven't used {name} recently enough to know"),
        Health::Unknown => format!("I've never seen {name} work"),
    }
}

/// An answer, marked with what it could not see while producing it.
///
/// Deliberately worded per-integration rather than lumped into one adjective —
/// "isn't working" is not the same gap as "you switched it off" or "I've
/// never seen it work", and the difference changes what you do next.
pub fn mark(answer: &str, used: &[&str], board: &Board, now: u64) -> String {
    let gaps: Vec<String> = used
        .iter()
        .filter_map(|name| {
            let i = board.integrations.iter().find(|i| i.name == *name)?;
            let h = i.health(now);
            (h != Health::Working).then(|| as_clause(&i.name, h))
        })
        .collect();
    if gaps.is_empty() {
        return answer.to_string();
    }
    format!(
        "{answer} I should say: {} — so this is missing whatever {} would have added.",
        gaps.join(", "),
        if gaps.len() == 1 { "it" } else { "they" }
    )
}
