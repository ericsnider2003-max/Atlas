//! Getting into the vault when you can't.
//!
//! The passphrase being only in your head is what makes the vault worth
//! having, and it's also a single point of failure attached to a person who
//! goes places with no phone signal. Both are true at once.
//!
//! The answer is not a backdoor. It's **a second way in that you set up
//! deliberately, that takes effort and time to use, and that you can see has
//! been used.** Three shapes, and you can have more than one.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Route {
    /// The passphrase written down and sealed, somewhere physical.
    ///
    /// Unfashionable and very good: it can't be phished, can't be
    /// brute-forced, and you can tell whether the envelope has been opened.
    SealedEnvelope,
    /// Split into pieces, where some number of them opens it.
    ///
    /// Two of three means no single person can get in and losing one person
    /// doesn't lock you out.
    SplitBetweenPeople { pieces: u8, needed: u8 },
    /// One person holds a full copy.
    ///
    /// Simplest and weakest — that person can open it whenever they like.
    OnePerson,
    /// A file only openable with something physical you keep separately.
    SecondKeyFile,
}

impl Route {
    pub fn plain(&self) -> String {
        match self {
            Route::SealedEnvelope => "written down, sealed, somewhere physical".into(),
            Route::SplitBetweenPeople { pieces, needed } => {
                format!("split {pieces} ways, any {needed} of them opens it")
            }
            Route::OnePerson => "one person holds a copy".into(),
            Route::SecondKeyFile => "a key file you keep somewhere separate".into(),
        }
    }

    /// Can one person get in on their own?
    pub fn needs_more_than_one_person(&self) -> bool {
        matches!(self, Route::SplitBetweenPeople { needed, .. } if *needed > 1)
    }

    /// Would you find out it had been used?
    pub fn visible_if_used(&self) -> bool {
        matches!(self, Route::SealedEnvelope | Route::SplitBetweenPeople { .. })
    }

    pub fn honest_weakness(&self) -> &'static str {
        match self {
            Route::SealedEnvelope => {
                "whoever can reach the envelope can open it — so where it lives is the whole \
                 security"
            }
            Route::SplitBetweenPeople { .. } => {
                "it only works if the holders can reach each other, which is exactly what's \
                 hard in an emergency"
            }
            Route::OnePerson => "that person can open it any time, not only when you'd want",
            Route::SecondKeyFile => "lose the key file and this route is gone too",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Setup {
    pub route: Route,
    /// Who or where, in your words.
    pub with: String,
    pub set_up_at: u64,
    /// You last checked it's still there and still works.
    pub last_checked: Option<u64>,
    /// It was used.
    pub used_at: Option<u64>,
}

impl Setup {
    /// Untested recovery isn't recovery. It's a plan.
    fn needs_checking(&self, now: u64, every_days: u64) -> bool {
        match self.last_checked {
            None => now.saturating_sub(self.set_up_at) > every_days * 86_400,
            Some(t) => now.saturating_sub(t) > every_days * 86_400,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RecoveryConfig {
    pub enabled: bool,
    /// Check the routes still work this often.
    pub check_every_days: u64,
    /// Warn if there's only one way in.
    pub want_at_least: usize,
    /// Nothing here is a route Atlas can use by itself. Not configurable.
    #[serde(skip, default = "never")]
    pub atlas_can_use_these: bool,
}

fn never() -> bool {
    false
}

impl Default for RecoveryConfig {
    fn default() -> Self {
        RecoveryConfig {
            enabled: false,
            // Twice a year, which is about as rarely as you can check
            // something and still call it checked.
            check_every_days: 180,
            want_at_least: 2,
            atlas_can_use_these: false,
        }
    }
}

/// What's missing.
#[derive(Debug, Clone, PartialEq)]
pub struct Gap {
    pub what: String,
    pub why: String,
    pub urgency: f32,
}

pub fn gaps(setups: &[Setup], cfg: &RecoveryConfig, now: u64) -> Vec<Gap> {
    let mut out = Vec::new();

    if setups.is_empty() {
        out.push(Gap {
            what: "there's no way into the vault but you".into(),
            why: "if you're not reachable, everything in it is gone — including the recovery \
                  codes you're keeping track of"
                .into(),
            urgency: 1.0,
        });
        return out;
    }

    if setups.len() < cfg.want_at_least {
        out.push(Gap {
            what: "only one way in besides you".into(),
            why: "one route is one thing going wrong away from none".into(),
            urgency: 0.7,
        });
    }

    for s in setups {
        if s.needs_checking(now, cfg.check_every_days) {
            out.push(Gap {
                what: format!("{} hasn't been checked", s.with),
                why: "untested recovery isn't recovery, it's a plan — envelopes get thrown out \
                      and people move"
                    .into(),
                urgency: 0.8,
            });
        }
        if let Some(at) = s.used_at {
            out.push(Gap {
                what: format!("{} was used", s.with),
                why: format!("at {at} — if that wasn't you or wasn't expected, change the passphrase"),
                urgency: 1.0,
            });
        }
    }

    // Everything resting on one person is the common shape and the one worth
    // naming.
    if setups.len() > 1 && setups.iter().all(|s| !s.route.needs_more_than_one_person()) {
        out.push(Gap {
            what: "any one of them can open it alone".into(),
            why: "splitting it so two are needed costs nothing and removes the question of \
                  trusting any single person with it"
                .into(),
            urgency: 0.4,
        });
    }

    out.sort_by(|a, b| b.urgency.partial_cmp(&a.urgency).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// What Atlas suggests, given how you actually live.
///
/// The right answer depends on whether people can reach each other, which for
/// you they often can't.
pub fn suggest(you_go_away: bool, family_reachable: bool) -> Vec<(Route, &'static str)> {
    let mut out = Vec::new();

    // For someone away with no signal, the physical one is the one that works
    // when nothing else does.
    if you_go_away {
        out.push((
            Route::SealedEnvelope,
            "works with no signal, no phone and no coordination — which is the case you keep \
             finding yourself in",
        ));
    }
    if family_reachable {
        out.push((
            Route::SplitBetweenPeople { pieces: 3, needed: 2 },
            "no single person can open it, and losing touch with one doesn't lock you out",
        ));
    }
    out.push((
        Route::SecondKeyFile,
        "a file on a drive you keep elsewhere — no other person involved at all",
    ));
    out
}

/// One way back in, said honestly (Eric, B5): what it is, its weakness,
/// and whether you'd be able to tell it had been used.
pub fn described(s: &Setup) -> String {
    let seen = if s.route.visible_if_used() {
        "You'd be able to tell if it had been used."
    } else {
        "You wouldn't know if it had been used."
    };
    let with = if s.with.trim().is_empty() { String::new() } else { format!(" ({})", s.with.trim()) };
    format!("{}{with}: {}. {seen}", s.route.plain(), s.route.honest_weakness())
}

/// A route from what you typed: `envelope`, `split 3 2` (three pieces, any
/// two), `person`, `keyfile`.
pub fn route_from(words: &[String]) -> Option<Route> {
    match words.first().map(|w| w.to_lowercase()).as_deref() {
        Some("envelope") | Some("sealed") => Some(Route::SealedEnvelope),
        Some("person") | Some("one-person") => Some(Route::OnePerson),
        Some("keyfile") | Some("key-file") | Some("file") => Some(Route::SecondKeyFile),
        Some("split") => {
            let pieces: u8 = words.get(1)?.parse().ok()?;
            let needed: u8 = words.get(2)?.parse().ok()?;
            (needed >= 1 && needed <= pieces).then_some(Route::SplitBetweenPeople { pieces, needed })
        }
        _ => None,
    }
}

/// What Atlas says about the state of it.
pub fn spoken(setups: &[Setup], cfg: &RecoveryConfig, now: u64) -> String {
    let g = gaps(setups, cfg, now);
    if g.is_empty() {
        let each: Vec<String> = setups.iter().map(described).collect();
        return format!("{} ways in besides you, all checked. {}", setups.len(), each.join(" "));
    }
    let first = &g[0];
    let mut s = format!("{} — {}.", first.what, first.why);
    if g.len() > 1 {
        s.push_str(&format!(" {} other thing{}.", g.len() - 1, if g.len() == 2 { "" } else { "s" }));
    }
    s
}

/// The thing that makes this real rather than a plan.
pub const TEST_IT: &str =
    "Set it up and then actually use it once, while you're standing there. Envelopes get thrown \
     out in a clear-out, people change address, and a key file on a drive you can't find is the \
     same as no key file. The check takes five minutes and it's the difference between recovery \
     and the idea of recovery.";

/// Why Atlas can't be one of the routes.
pub const NOT_ATLAS: &str =
    "None of these is something I can use on my own — if I could open the vault without you, the \
     passphrase would be decoration. That's the point of it, and it's why the way back in has to \
     be outside me.";
