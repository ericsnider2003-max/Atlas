//! Finding another way in.
//!
//! "I'd rather not, we've tried that twice" is honest and useless. If Atlas
//! knows an approach fails, the useful next move is a different approach —
//! and most problems have several ways in that fail independently.
//!
//! The other half of what you asked for is speed. Quality and speed only
//! conflict when everything gets the same treatment; the way out is to spend
//! effort where it changes the answer and nowhere else. So a route is chosen
//! by what's cheapest among the things that could actually work, rather than
//! by trying them in the order they were written down.

use crate::learned::{Advice, Cause, Learned};
use serde::{Deserialize, Serialize};

/// A way of getting something done.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Route {
    pub name: String,
    /// What sort of problem it's for.
    pub for_what: Kind,
    /// Roughly how long, in seconds.
    pub costs_secs: u64,
    /// How often it works, from experience. Starts at a guess.
    pub reliability: f32,
    /// It needs something that may not be there.
    pub needs: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Getting data out of a site or app.
    Extract,
    /// Making something happen in an app.
    Act,
    /// Finding something out.
    Learn,
    /// Changing code.
    Fix,
}

/// The ways Atlas knows to get at something. Ordered cheapest first *within*
/// each kind, which is what makes picking by cost sensible.
pub fn known_routes() -> Vec<Route> {
    vec![
        // Extracting.
        Route { name: "read the file that's already there".into(), for_what: Kind::Extract,
            costs_secs: 1, reliability: 0.95, needs: vec!["a downloaded file".into()] },
        Route { name: "read the window's text".into(), for_what: Kind::Extract,
            costs_secs: 2, reliability: 0.8, needs: vec!["the app open".into()] },
        Route { name: "use the site's own export".into(), for_what: Kind::Extract,
            costs_secs: 20, reliability: 0.85, needs: vec!["a logged-in session".into()] },
        Route { name: "read the page through the browser".into(), for_what: Kind::Extract,
            costs_secs: 15, reliability: 0.7, needs: vec!["the browser".into()] },
        Route { name: "screenshot and read the text off it".into(), for_what: Kind::Extract,
            costs_secs: 40, reliability: 0.6, needs: vec!["OCR".into()] },
        Route { name: "ask you to save it and point me at it".into(), for_what: Kind::Extract,
            costs_secs: 60, reliability: 0.99, needs: vec![] },

        // Acting.
        Route { name: "keyboard shortcut".into(), for_what: Kind::Act,
            costs_secs: 1, reliability: 0.9, needs: vec!["the app focused".into()] },
        Route { name: "click the control by its name".into(), for_what: Kind::Act,
            costs_secs: 3, reliability: 0.8, needs: vec!["an accessible control".into()] },
        Route { name: "the app's own command line".into(), for_what: Kind::Act,
            costs_secs: 5, reliability: 0.85, needs: vec!["a command line".into()] },
        Route { name: "walk you through it".into(), for_what: Kind::Act,
            costs_secs: 45, reliability: 0.99, needs: vec![] },

        // Learning.
        Route { name: "what I already have indexed".into(), for_what: Kind::Learn,
            costs_secs: 1, reliability: 0.6, needs: vec![] },
        Route { name: "the local model".into(), for_what: Kind::Learn,
            costs_secs: 6, reliability: 0.55, needs: vec!["a model".into()] },
        Route { name: "search the web".into(), for_what: Kind::Learn,
            costs_secs: 20, reliability: 0.85, needs: vec!["the internet".into()] },
        Route { name: "read the source or spec directly".into(), for_what: Kind::Learn,
            costs_secs: 60, reliability: 0.9, needs: vec!["the document".into()] },

        // Fixing.
        Route { name: "change the setting rather than the code".into(), for_what: Kind::Fix,
            costs_secs: 2, reliability: 0.5, needs: vec![] },
        Route { name: "the local model on the one file".into(), for_what: Kind::Fix,
            costs_secs: 30, reliability: 0.45, needs: vec!["a model".into()] },
        Route { name: "work it out from the failing test".into(), for_what: Kind::Fix,
            costs_secs: 90, reliability: 0.7, needs: vec![] },
        Route { name: "write it up and hand it over".into(), for_what: Kind::Fix,
            costs_secs: 300, reliability: 0.95, needs: vec![] },
    ]
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RouteConfig {
    /// Don't bother with anything below this hit rate.
    pub min_reliability: f32,
    /// Give up after this many routes rather than exhausting every one.
    pub max_routes: usize,
    /// Prefer speed over reliability by this much, 0 to 1.
    pub impatience: f32,
}

impl Default for RouteConfig {
    fn default() -> Self {
        RouteConfig { min_reliability: 0.35, max_routes: 4, impatience: 0.5 }
    }
}

/// What Atlas does when the first way in is closed.
#[derive(Debug, Clone, PartialEq)]
pub enum Plan {
    /// Try this.
    Try { route: Route, why: String },
    /// Nothing left that could work, and this is why.
    Stuck { tried: Vec<String>, why: String },
}

/// Does this route need the internet?
///
/// The point of naming these is that being offline should *narrow* the
/// options rather than close them. Every kind of problem has at least one
/// local route, and there's a test for it.
pub fn needs_internet(r: &Route) -> bool {
    r.needs.iter().any(|n| n == "the internet")
}

/// How many ways in exist with and without a connection.
///
/// Online is strictly better, never differently better — nothing is only
/// possible offline.
pub fn coverage(kind: Kind) -> (usize, usize) {
    let all: Vec<Route> = known_routes().into_iter().filter(|r| r.for_what == kind).collect();
    let offline = all.iter().filter(|r| !needs_internet(r)).count();
    (offline, all.len())
}

/// Pick a way in.
///
/// Three things decide it: whether the route is even possible here, whether
/// Atlas has already learned it fails, and what it costs against how often it
/// works. That last part is why this is fast — the expensive routes exist, but
/// they are last, and most problems never reach them.
pub fn plan(
    kind: Kind,
    context: &str,
    have: &[String],
    learned: &Learned,
    cfg: &RouteConfig,
    now: u64,
) -> Plan {
    plan_with(kind, context, have, learned, &Record::default(), cfg, now)
}

/// The same, using what Atlas has actually observed here.
pub fn plan_with(
    kind: Kind,
    context: &str,
    have: &[String],
    learned: &Learned,
    record: &Record,
    cfg: &RouteConfig,
    now: u64,
) -> Plan {
    let mut ruled_out: Vec<String> = Vec::new();

    let mut candidates: Vec<(Route, f32, String)> = Vec::new();
    for r in known_routes().into_iter().filter(|r| r.for_what == kind) {
        // Does it need something that isn't here?
        if let Some(missing) = r.needs.iter().find(|n| !have.contains(n)) {
            ruled_out.push(format!("{} (no {missing})", r.name));
            continue;
        }
        // What actually happened here beats what I guessed when writing this.
        let reliability = record.rate(&r.name, context, r.reliability);
        if reliability < cfg.min_reliability {
            continue;
        }
        // Has it failed here before?
        let why = match learned.advise(&r.name, context, now) {
            Advice::Dont { because, .. } => {
                ruled_out.push(format!("{} ({because})", r.name));
                continue;
            }
            Advice::TryAgain { because } => format!("worth another go — {because}"),
            Advice::Fresh => {
                let learned_here = record.attempts(context) >= 5;
                if r.costs_secs <= 5 && !learned_here {
                    "quickest thing that could work".to_string()
                } else if r.costs_secs <= 5 {
                    format!("quickest thing that has worked here — {:.0}% of the time",
                        reliability * 100.0)
                } else if learned_here {
                    format!("about {}s, and it's worked {:.0}% of the time here",
                        r.costs_secs, reliability * 100.0)
                } else {
                    format!("about {}s, works {:.0}% of the time", r.costs_secs, reliability * 100.0)
                }
            }
        };

        // Cheap and reliable wins. The weighting is what stops Atlas
        // reaching for the thorough option on a problem the quick one solves.
        let seconds = r.costs_secs as f32;
        let speed = 1.0 / (1.0 + seconds / 10.0);
        let score = reliability * (1.0 - cfg.impatience) + speed * cfg.impatience;
        candidates.push((r, score, why));
    }

    candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    match candidates.into_iter().next() {
        Some((route, _, why)) => Plan::Try { route, why },
        None => Plan::Stuck {
            why: if ruled_out.is_empty() {
                "I don't have a way to do that here".into()
            } else {
                format!("{} way{} in, all closed", ruled_out.len(),
                    if ruled_out.len() == 1 { "" } else { "s" })
            },
            tried: ruled_out,
        },
    }
}

/// Everything worth trying, in order — so a failure moves straight on rather
/// than starting the thinking again.
pub fn all_routes(
    kind: Kind,
    context: &str,
    have: &[String],
    learned: &Learned,
    cfg: &RouteConfig,
    now: u64,
) -> Vec<Route> {
    let mut out = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut work = learned.clone();

    for _ in 0..cfg.max_routes {
        match plan(kind, context, have, &work, cfg, now) {
            Plan::Try { route, .. } => {
                if seen.contains(&route.name) {
                    break;
                }
                seen.push(route.name.clone());
                // Pretend it failed, to see what Atlas would reach for next.
                work.record(&route.name, context, Cause::Outside, "assumed failed", now);
                work.record(&route.name, context, Cause::Outside, "assumed failed", now);
                out.push(route);
            }
            Plan::Stuck { .. } => break,
        }
    }
    out
}

/// How a route has actually performed here, as opposed to how it was guessed
/// to perform.
///
/// The shipped reliability numbers are estimates. After a dozen real attempts
/// Atlas knows better than I did, and that knowledge is worth more than the
/// guess — this is most of how it gets better without anything changing about
/// your machine.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Record {
    /// Route name and context to attempts and successes.
    pub tallies: Vec<(String, String, u32, u32)>,
}

impl Record {
    pub fn note(&mut self, route: &str, context: &str, worked: bool) {
        match self
            .tallies
            .iter_mut()
            .find(|(r, c, _, _)| r == route && c == context)
        {
            Some((_, _, tries, wins)) => {
                *tries += 1;
                if worked {
                    *wins += 1;
                }
            }
            None => self.tallies.push((route.into(), context.into(), 1, worked as u32)),
        }
    }

    /// The rate to use, blending the guess with what actually happened.
    ///
    /// Weighted by how much evidence there is: one success doesn't mean 100%,
    /// and twenty attempts should outweigh whatever I assumed.
    pub fn rate(&self, route: &str, context: &str, guess: f32) -> f32 {
        match self
            .tallies
            .iter()
            .find(|(r, c, _, _)| r == route && c == context)
        {
            None => guess,
            Some((_, _, tries, wins)) => {
                let observed = *wins as f32 / *tries as f32;
                let weight = (*tries as f32 / (*tries as f32 + 5.0)).min(0.95);
                guess * (1.0 - weight) + observed * weight
            }
        }
    }

    /// How much Atlas has actually learned here.
    pub fn attempts(&self, context: &str) -> u32 {
        self.tallies.iter().filter(|(_, c, _, _)| c == context).map(|(_, _, t, _)| t).sum()
    }
}

/// "I seem to be stuck on X for Y." The one sentence for being stuck, used
/// wherever Atlas is.
pub fn stuck_on(what: &str, why: &str) -> String {
    format!("I seem to be stuck on {} for {}.", what.trim(), why.trim().trim_end_matches('.'))
}

/// What Atlas says when it changes approach mid-task.
///
/// Naming what it's abandoning and what it's moving to is the difference
/// between looking adaptive and looking confused.
pub fn switching(from: &str, to: &str, why: &str) -> String {
    format!("{from} didn't work — {why}. Trying {to} instead.")
}

/// What it says when there is genuinely nothing left, in Eric's words (25
/// Sep 2026, F7): "I seem to be stuck on X for Y."
pub fn stuck_spoken(what: &str, p: &Plan) -> String {
    match p {
        Plan::Try { route, why } => format!("{} — {why}.", route.name),
        Plan::Stuck { tried, why } => {
            let mut s = stuck_on(what, why);
            if let Some(first) = tried.first() {
                s.push_str(&format!(" The closest was {first}."));
            }
            s.push_str(" Tell me how you'd do it and I'll learn it.");
            s
        }
    }
}
