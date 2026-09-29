//! Learning something you do the same way every time.
//!
//! Phase 3.2. You open the same three tabs every Monday, or check the same
//! four numbers before the market opens, or file the same report the same way.
//! Doing it for you is worth having; the interesting question is how Atlas
//! knows what "it" is without you writing a script.
//!
//! The answer is that it watches, notices repetition, and **asks**. A system
//! that silently decides your Monday morning is a routine and starts doing it
//! is unnerving even when it's right.

use serde::{Deserialize, Serialize};

/// One thing you did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Did {
    /// The action, in Atlas's terms.
    pub what: String,
    pub at: u64,
    /// Hour of the day, 0–23.
    pub hour: u32,
    /// 0 = Monday.
    pub weekday: u32,
}

/// A sequence Atlas thinks is a routine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Routine {
    pub name: String,
    pub steps: Vec<String>,
    /// How many times the whole sequence has been seen.
    pub seen: u32,
    /// When it usually happens.
    pub usual_hour: u32,
    pub usual_weekday: Option<u32>,
    /// You said yes to this being a routine.
    pub confirmed: bool,
    /// Atlas may run it without asking each time.
    pub automatic: bool,
    /// Atlas has asked you about it (once is enough).
    #[serde(default)]
    pub asked: bool,
    /// The day it last ran (days since 1970), so it runs once a day at most.
    #[serde(default)]
    pub last_day: u64,
}

impl Routine {
    /// Is it worth mentioning yet?
    ///
    /// Three is the number. Twice is a coincidence, and asking after two is
    /// how a system becomes tiresome.
    pub fn worth_asking(&self) -> bool {
        self.seen >= 3 && !self.confirmed
    }

    /// Does now look like the time?
    pub fn due(&self, hour: u32, weekday: u32) -> bool {
        if !self.confirmed {
            return false;
        }
        let hour_matches = hour == self.usual_hour;
        match self.usual_weekday {
            Some(d) => hour_matches && weekday == d,
            None => hour_matches,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RoutineConfig {
    pub enabled: bool,
    /// Repeats before Atlas asks.
    pub times_before_asking: u32,
    /// Longest sequence it will treat as one routine.
    pub max_steps: usize,
    /// Gap that ends a sequence, in seconds.
    pub gap_secs: u64,
    /// Never offer to automate anything that sends or posts.
    pub never_automate_outgoing: bool,
}

impl Default for RoutineConfig {
    fn default() -> Self {
        RoutineConfig {
            enabled: false,
            times_before_asking: 3,
            max_steps: 6,
            gap_secs: 600,
            // A routine that posts something is a routine that posts something
            // wrong eventually.
            never_automate_outgoing: true,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Watcher {
    pub history: Vec<Did>,
    pub routines: Vec<Routine>,
}

impl Watcher {
    pub fn did(&mut self, what: &str, at: u64, hour: u32, weekday: u32) {
        self.history.push(Did { what: what.into(), at, hour, weekday });
        if self.history.len() > 2000 {
            self.history.drain(0..500);
        }
    }

    /// Break the history into sequences separated by real gaps.
    fn sequences(&self, cfg: &RoutineConfig) -> Vec<Vec<&Did>> {
        let mut out: Vec<Vec<&Did>> = Vec::new();
        let mut current: Vec<&Did> = Vec::new();
        for d in &self.history {
            match current.last() {
                Some(prev) if d.at.saturating_sub(prev.at) <= cfg.gap_secs => current.push(d),
                Some(_) => {
                    if current.len() > 1 {
                        out.push(std::mem::take(&mut current));
                    } else {
                        current.clear();
                    }
                    current.push(d);
                }
                None => current.push(d),
            }
        }
        if current.len() > 1 {
            out.push(current);
        }
        out
    }

    /// Look for sequences you've repeated.
    pub fn find(&mut self, cfg: &RoutineConfig) -> Vec<Routine> {
        if !cfg.enabled {
            return Vec::new();
        }
        let sequences = self.sequences(cfg);
        let mut counts: std::collections::BTreeMap<Vec<String>, Vec<(u32, u32)>> = Default::default();

        for seq in &sequences {
            let steps: Vec<String> =
                seq.iter().take(cfg.max_steps).map(|d| d.what.clone()).collect();
            if steps.len() < 2 {
                continue;
            }
            counts
                .entry(steps)
                .or_default()
                .push((seq[0].hour, seq[0].weekday));
        }

        let mut found = Vec::new();
        for (steps, whens) in counts {
            if whens.len() < cfg.times_before_asking as usize {
                continue;
            }
            let hour = commonest(whens.iter().map(|(h, _)| *h));
            let days: Vec<u32> = whens.iter().map(|(_, d)| *d).collect();
            let same_day = days.windows(2).all(|w| w[0] == w[1]);

            found.push(Routine {
                name: name_for(&steps, hour),
                steps,
                seen: whens.len() as u32,
                usual_hour: hour,
                usual_weekday: if same_day { days.first().copied() } else { None },
                confirmed: false,
                automatic: false,
                asked: false,
                last_day: 0,
            });
        }
        found
    }

    pub fn confirm(&mut self, name: &str, automatic: bool) -> bool {
        match self.routines.iter_mut().find(|r| r.name == name) {
            Some(r) => {
                r.confirmed = true;
                r.automatic = automatic;
                true
            }
            None => false,
        }
    }

    pub fn due_now(&self, hour: u32, weekday: u32) -> Vec<&Routine> {
        self.routines.iter().filter(|r| r.due(hour, weekday)).collect()
    }
}

fn commonest(items: impl Iterator<Item = u32>) -> u32 {
    let mut counts: std::collections::BTreeMap<u32, u32> = Default::default();
    for i in items {
        *counts.entry(i).or_insert(0) += 1;
    }
    counts.into_iter().max_by_key(|(_, n)| *n).map(|(v, _)| v).unwrap_or(0)
}

/// A name you'd recognise: when it happens ("morning setup"), the way you'd
/// say it yourself (Eric: "Your usual morning setup?").
fn name_for(steps: &[String], hour: u32) -> String {
    let part = match hour {
        5..=11 => "morning",
        12..=16 => "afternoon",
        17..=21 => "evening",
        _ => "late-night",
    };
    match steps.first() {
        Some(_) => format!("{part} setup"),
        None => "routine".into(),
    }
}

/// Steps that are concrete, ordinary things: adding to the calendar, a
/// report, a backup, checking something, opening your apps. Eric, 25 Sep 2026
/// (E4): "if it's a normal thing like adding something to a calendar getting
/// a report then automatic. If it's something more abstract then ask."
const CONCRETE: &[&str] = &[
    "schedule", "remind", "calendar", "agenda", "brief", "report", "back up", "backup",
    "check", "how's", "hows", "what's", "whats", "show", "open", "workspace", "mode",
    "messages", "mail", "queued", "outstanding", "recap", "health", "status",
];

/// Is every step one of those? Anything that sends never is.
pub fn is_concrete(steps: &[String]) -> bool {
    !steps.is_empty()
        && !sends_something(steps)
        && steps.iter().all(|s| {
            let l = s.to_lowercase();
            CONCRETE.iter().any(|w| l.contains(w))
        })
}

impl Watcher {
    /// New routines found this time, merged in (by their steps), and the one
    /// worth asking about now, if any. Asked once each.
    pub fn take_new(&mut self, cfg: &RoutineConfig) -> Option<Routine> {
        for r in self.find(cfg) {
            if !self.routines.iter().any(|x| x.steps == r.steps) {
                let mut r = r;
                // Two routines at the same time of day are told apart by
                // their first step.
                if self.routines.iter().any(|x| x.name == r.name) {
                    r.name = format!("{} ({})", r.name, r.steps[0]);
                }
                self.routines.push(r);
            }
        }
        let r = self.routines.iter_mut().find(|r| r.worth_asking() && !r.asked)?;
        r.asked = true;
        Some(r.clone())
    }

    /// You said no to one: it's forgotten, and the history that made it goes
    /// too, so it isn't found and asked about again next hour.
    pub fn declined(&mut self, name: &str) {
        if let Some(i) = self.routines.iter().position(|r| r.name == name) {
            let steps = self.routines.remove(i).steps;
            self.history.retain(|d| !steps.contains(&d.what));
        }
    }

    /// Routines due now that haven't run today.
    pub fn due_today(&mut self, hour: u32, weekday: u32, day: u64) -> Vec<Routine> {
        let mut out = Vec::new();
        for r in self.routines.iter_mut() {
            if r.due(hour, weekday) && r.last_day != day {
                r.last_day = day;
                out.push(r.clone());
            }
        }
        out
    }
}

/// Does this routine do anything that leaves the machine?
pub fn sends_something(steps: &[String]) -> bool {
    steps.iter().any(|s| {
        let l = s.to_lowercase();
        ["send", "post", "publish", "reply", "tweet", "email", "pay", "buy"]
            .iter()
            .any(|w| l.contains(w))
    })
}

/// What Atlas says when it thinks it's spotted one.
///
/// It asks. A system that silently decides your Monday is a routine and starts
/// doing it is unnerving even when it's right.
pub fn ask_about(r: &Routine, cfg: &RoutineConfig) -> String {
    let when = match r.usual_weekday {
        Some(d) => format!("{} around {}", weekday_name(d), hour_name(r.usual_hour)),
        None => format!("around {}", hour_name(r.usual_hour)),
    };
    let mut s = format!(
        "You've done the same {} things {when} {} times now — {}. Want me to do it?",
        r.steps.len(),
        r.seen,
        r.steps.join(", then ")
    );
    if cfg.never_automate_outgoing && sends_something(&r.steps) {
        // Worth saying up front rather than after it has posted something.
        s.push_str(" I'd stop before anything that sends, and check with you.");
    }
    s
}

fn weekday_name(d: u32) -> &'static str {
    ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"]
        .get(d as usize)
        .copied()
        .unwrap_or("that day")
}

fn hour_name(h: u32) -> String {
    match h {
        0 => "midnight".into(),
        12 => "noon".into(),
        1..=11 => format!("{h}am"),
        _ => format!("{}pm", h - 12),
    }
}

/// What it says when it's about to run one.
pub fn starting(r: &Routine) -> String {
    if r.automatic {
        format!("Doing your {} — say stop if not.", r.name)
    } else {
        format!("Your usual {}?", r.name)
    }
}
