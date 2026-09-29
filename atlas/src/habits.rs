//! Habits, counted kindly: "did my reading", "how are my habits?".
//!
//! **Sources:** Loop Habit Tracker (GPL-3.0; read for its ideas only). The
//! thing it gets right, and streak apps get wrong, is the **strength**
//! score: an exponential average that one missed day dents rather than
//! zeroes, so a habit that's mostly kept reads as mostly kept. The update is
//! Loop's -- `score = score·m + checked·(1−m)`, `m = 0.5^(√f / 13)` for a
//! habit of frequency `f` a day -- and for "3 times a week" a day counts as
//! checked by how much of the week's target the last 7 days met. The code is
//! new.
//!
//! **Soundproofing.**
//! - **Pauses** -- ill, travelling, a holiday -- leave the score where it
//!   was instead of counting as misses.
//! - **Never about the body's numbers.** A habit whose name touches
//!   `nudge::NEVER_NUDGES_ABOUT` (weight, calories, a dose…) can be tracked
//!   if you ask, but is *never* reminded about and never appears in the
//!   brief -- the same line every nudge holds.
//! - Reminders come at most once a day per habit, only when it's due and
//!   not yet done, and only from the brief -- never a pop-up.
//! - Bounded: 50 habits, two years of days each.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_HABITS: usize = 50;
pub const KEEP_DAYS: i64 = 730;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Habit {
    pub name: String,
    /// `times` in every `days` days: (1,1) daily, (3,7) three a week.
    pub times: u32,
    pub days: u32,
    /// Local day numbers it was done.
    #[serde(default)]
    pub done: BTreeSet<i64>,
    /// Paused day ranges, inclusive.
    #[serde(default)]
    pub paused: Vec<(i64, i64)>,
    pub since: i64,
}

impl Habit {
    pub fn frequency(&self) -> f64 {
        self.times.max(1) as f64 / self.days.max(1) as f64
    }

    pub fn is_paused(&self, day: i64) -> bool {
        self.paused.iter().any(|(a, b)| (*a..=*b).contains(&day))
    }

    /// Never reminded about: a body number or a dose.
    pub fn never_raised(&self) -> bool {
        crate::nudge::is_medical(&self.name)
    }

    /// How much of the target the window ending on `day` met, 0..1.
    fn checked(&self, day: i64) -> f64 {
        if self.days <= 1 {
            return if self.done.contains(&day) { 1.0 } else { 0.0 };
        }
        let from = day - self.days as i64 + 1;
        let n = self.done.range(from..=day).count() as f64;
        (n / self.times.max(1) as f64).min(1.0)
    }

    /// Strength on `today`, 0..1. Costs one pass over the days since it
    /// started (at most two years), so it's computed when asked, not kept.
    pub fn strength(&self, today: i64) -> f64 {
        let m = 0.5f64.powf(self.frequency().sqrt() / 13.0);
        let mut score = 0.0;
        let start = self.since.max(today - KEEP_DAYS);
        for d in start..=today {
            if self.is_paused(d) {
                continue;
            }
            // Today isn't a miss yet: it counts only once it's done.
            if d == today && !self.done.contains(&d) {
                continue;
            }
            score = score * m + self.checked(d) * (1.0 - m);
        }
        score
    }

    /// Days in a row it was done (daily habits) -- paused days neither
    /// break nor extend it; today not yet done doesn't break it.
    pub fn streak(&self, today: i64) -> u32 {
        let mut n = 0;
        let mut d = if self.done.contains(&today) { today } else { today - 1 };
        while d >= self.since {
            if self.is_paused(d) {
                d -= 1;
                continue;
            }
            if self.checked(d) >= 1.0 {
                n += 1;
                d -= 1;
            } else {
                break;
            }
        }
        n
    }

    /// Due today and not done: a daily habit not ticked, or a weekly one
    /// whose window hasn't met its target.
    pub fn due(&self, today: i64) -> bool {
        !self.is_paused(today) && !self.done.contains(&today) && self.checked(today) < 1.0
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Habits {
    pub habits: Vec<Habit>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Refused {
    Empty,
    Full,
    Exists,
    NotFound,
    /// More than one habit answers to that.
    Which(Vec<String>),
}

impl Habits {
    fn index(&self, name: &str) -> Result<usize, Refused> {
        let n = name.trim().to_lowercase();
        if let Some(i) = self.habits.iter().position(|h| h.name.to_lowercase() == n) {
            return Ok(i);
        }
        let hits: Vec<usize> = (0..self.habits.len())
            .filter(|i| {
                let h = self.habits[*i].name.to_lowercase();
                h.contains(&n) || n.contains(&h)
            })
            .collect();
        match hits.as_slice() {
            [i] => Ok(*i),
            [] => Err(Refused::NotFound),
            many => Err(Refused::Which(many.iter().map(|i| self.habits[*i].name.clone()).collect())),
        }
    }

    pub fn add(&mut self, name: &str, times: u32, days: u32, today: i64) -> Result<(), Refused> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Refused::Empty);
        }
        if self.habits.iter().any(|h| h.name.eq_ignore_ascii_case(name)) {
            return Err(Refused::Exists);
        }
        if self.habits.len() >= MAX_HABITS {
            return Err(Refused::Full);
        }
        let days = days.clamp(1, 31);
        self.habits.push(Habit { name: name.to_string(), times: times.clamp(1, days), days, done: BTreeSet::new(), paused: vec![], since: today });
        Ok(())
    }

    pub fn remove(&mut self, name: &str) -> Result<String, Refused> {
        let i = self.index(name)?;
        Ok(self.habits.remove(i).name)
    }

    /// Mark done on `day`; returns the habit's name.
    pub fn did(&mut self, name: &str, day: i64) -> Result<String, Refused> {
        let i = self.index(name)?;
        let h = &mut self.habits[i];
        h.done.insert(day);
        let cut = day - KEEP_DAYS;
        h.done = h.done.split_off(&cut);
        h.paused.retain(|(_, b)| *b >= cut);
        Ok(h.name.clone())
    }

    pub fn undo(&mut self, name: &str, day: i64) -> Result<String, Refused> {
        let i = self.index(name)?;
        self.habits[i].done.remove(&day);
        Ok(self.habits[i].name.clone())
    }

    /// Pause all habits (or one) for days [from, to].
    pub fn pause(&mut self, name: Option<&str>, from: i64, to: i64) -> Result<usize, Refused> {
        let (from, to) = (from.min(to), from.max(to));
        let idx: Vec<usize> = match name {
            Some(n) => vec![self.index(n)?],
            None => (0..self.habits.len()).collect(),
        };
        for i in &idx {
            self.habits[*i].paused.push((from, to));
        }
        Ok(idx.len())
    }

    /// Due today and allowed to be mentioned.
    pub fn due_today(&self, today: i64) -> Vec<&Habit> {
        self.habits.iter().filter(|h| h.due(today) && !h.never_raised()).collect()
    }

    pub fn said(&self, today: i64) -> String {
        if self.habits.is_empty() {
            return "No habits yet. \"New habit: read 20 minutes, 5 times a week\" starts one.".into();
        }
        self.habits
            .iter()
            .map(|h| {
                let freq = if h.days == 1 { "daily".to_string() } else { format!("{} in {} days", h.times, h.days) };
                let done = if h.done.contains(&today) { ", done today" } else if h.is_paused(today) { ", paused" } else { "" };
                let streak = h.streak(today);
                let streak = if h.days == 1 && streak >= 2 { format!(", {streak} days running") } else { String::new() };
                format!("{} ({freq}): {:.0}% strong{streak}{done}.", h.name, h.strength(today) * 100.0)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// What a sentence asked.
#[derive(Debug, Clone, PartialEq)]
pub enum Asked {
    Add { name: String, times: u32, days: u32 },
    Did { name: String },
    Undo { name: String },
    Remove { name: String },
    Show,
}

fn number(w: &str) -> Option<u32> {
    match w {
        "once" | "one" | "a" => Some(1),
        "twice" | "two" => Some(2),
        "three" => Some(3),
        "four" => Some(4),
        "five" => Some(5),
        "six" => Some(6),
        "seven" => Some(7),
        x => x.parse().ok(),
    }
}

/// "new habit: read 20 minutes, 5 times a week" / "new habit stretch daily".
fn frequency_in(text: &str) -> (String, u32, u32) {
    let low = text.to_ascii_lowercase();
    for (suffix, t, d) in [(" every day", 1, 1), (" daily", 1, 1), (" each day", 1, 1), (" weekly", 1, 7), (" once a week", 1, 7), (" every week", 1, 7)] {
        if let Some(stripped) = low.strip_suffix(suffix) {
            return (text[..stripped.len()].trim().trim_end_matches(',').trim().to_string(), t, d);
        }
    }
    // "…, N times a week" / "… N times per week" / "… twice a week"
    let words: Vec<&str> = low.split_whitespace().collect();
    if words.len() >= 3 {
        let n = words.len();
        let per_week = matches!(words[n - 1], "week" | "weekly") && matches!(words[n - 2], "a" | "per" | "each" | "every");
        if per_week {
            let (count, cut) = if words[n - 3] == "times" && n >= 4 { (number(words[n - 4]), n - 4) } else { (number(words[n - 3]), n - 3) };
            if let Some(c) = count {
                let name: Vec<&str> = text.split_whitespace().take(cut).collect();
                return (name.join(" ").trim_end_matches(',').to_string(), c.clamp(1, 7), 7);
            }
        }
    }
    (text.trim().to_string(), 1, 1)
}

pub fn read(said: &str) -> Option<Asked> {
    let s = said.trim().trim_end_matches(['.', '!']);
    let low = s.to_ascii_lowercase();
    if matches!(low.as_str(), "how are my habits" | "how are my habits?" | "habits" | "my habits" | "show my habits") {
        return Some(Asked::Show);
    }
    for lead in ["new habit: ", "new habit ", "add a habit: ", "add habit ", "track the habit "] {
        if low.starts_with(lead) {
            let (name, times, days) = frequency_in(&s[lead.len()..]);
            return (!name.is_empty()).then_some(Asked::Add { name, times, days });
        }
    }
    for lead in ["stop tracking ", "delete habit ", "remove habit "] {
        if low.starts_with(lead) {
            return Some(Asked::Remove { name: s[lead.len()..].trim().to_string() });
        }
    }
    for lead in ["undo habit ", "i didn't do my ", "i didn't do "] {
        if low.starts_with(lead) {
            return Some(Asked::Undo { name: s[lead.len()..].trim().trim_end_matches(" today").to_string() });
        }
    }
    for lead in ["did my ", "done my ", "i did my ", "habit done ", "tick off ", "check off "] {
        if low.starts_with(lead) {
            let name = s[lead.len()..].trim();
            let name = name.strip_suffix(" today").unwrap_or(name).trim();
            return (!name.is_empty()).then(|| Asked::Did { name: name.to_string() });
        }
    }
    None
}
