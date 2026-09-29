//! Remembering what you want to remember: flashcards on a schedule that
//! brings each one back just before you'd forget it.
//!
//! "Make a card: what's the CPI release time | 8:30 Eastern." "Quiz me."
//! Then "again", "hard", "good" or "easy" (or 1–4) after each.
//!
//! **Source:** FSRS-5, the Free Spaced Repetition Scheduler
//! (open-spaced-repetition; the algorithm as its wiki states it, and the
//! default parameters fsrs-rs ships). Written here from those formulas:
//!
//! - retrievability `R(t,S) = (1 + 19/81 · t/S)^−0.5` (so `R(S,S) = 0.9`);
//! - first stability `S₀ = w[G−1]`; first difficulty
//!   `D₀ = w4 − e^(w5·(G−1)) + 1`;
//! - difficulty `D' = D − w6·(G−3)` damped by `(10−D)/9`, then pulled back
//!   toward `D₀(4)` by `w7`;
//! - after a recall `S' = S·(e^w8·(11−D)·S^−w9·(e^(w10·(1−R))−1)·hard·easy + 1)`;
//! - after a lapse `S' = min(S, w11·D^−w12·((S+1)^w13−1)·e^(w14·(1−R)))`;
//! - a second look the same day `S' = S·e^(w17·(G−3+w18))`.
//!
//! The interval for a wanted retention `r` is `S/F·(r^(1/−0.5) − 1)`, which
//! is `S` days at 90%.
//!
//! **Soundproofing.** A card holding a secret-looking string is refused.
//! "Quiz me" takes at most 20 due cards (config) so a missed week doesn't
//! become a wall. Intervals are capped at a year and a half; days are whole
//! local days, so a card is never due at 3 a.m.

use serde::{Deserialize, Serialize};

pub const W: [f64; 19] = [
    0.40255, 1.18385, 3.173, 15.69105, 7.1949, 0.5345, 1.4604, 0.0046, 1.54575, 0.1192, 1.01925, 1.9395, 0.11, 0.29605,
    2.2698, 0.2315, 2.9898, 0.51655, 0.6621,
];
const DECAY: f64 = -0.5;
const FACTOR: f64 = 19.0 / 81.0;
pub const MAX_INTERVAL: i64 = 548;
pub const MAX_CARDS: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Grade {
    Again = 1,
    Hard = 2,
    Good = 3,
    Easy = 4,
}

impl Grade {
    pub fn read(said: &str) -> Option<Grade> {
        match said.trim().trim_end_matches(['.', '!']).to_ascii_lowercase().as_str() {
            "again" | "1" | "forgot" | "no" | "wrong" => Some(Grade::Again),
            "hard" | "2" => Some(Grade::Hard),
            "good" | "3" | "yes" | "got it" | "right" => Some(Grade::Good),
            "easy" | "4" => Some(Grade::Easy),
            _ => None,
        }
    }
}

pub fn retrievability(elapsed_days: f64, stability: f64) -> f64 {
    (1.0 + FACTOR * elapsed_days / stability.max(0.01)).powf(DECAY)
}

/// Days until retention falls to `r`.
pub fn interval(stability: f64, r: f64) -> i64 {
    let r = r.clamp(0.7, 0.97);
    let days = stability / FACTOR * (r.powf(1.0 / DECAY) - 1.0);
    (days.round() as i64).clamp(1, MAX_INTERVAL)
}

fn d0(g: f64) -> f64 {
    W[4] - (W[5] * (g - 1.0)).exp() + 1.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Card {
    pub front: String,
    pub back: String,
    #[serde(default)]
    pub deck: String,
    /// 0 until first review.
    pub stability: f64,
    pub difficulty: f64,
    /// Local day numbers.
    pub last: Option<i64>,
    pub due: i64,
    #[serde(default)]
    pub reps: u32,
    #[serde(default)]
    pub lapses: u32,
}

impl Card {
    /// Grade it on `today`; returns the new due day.
    pub fn review(&mut self, g: Grade, today: i64, retention: f64) -> i64 {
        let gf = g as u8 as f64;
        match self.last {
            None => {
                self.stability = W[g as usize - 1];
                self.difficulty = d0(gf).clamp(1.0, 10.0);
            }
            Some(last) => {
                let elapsed = (today - last).max(0) as f64;
                let (s, d) = (self.stability, self.difficulty);
                if elapsed < 1.0 {
                    self.stability = s * (W[17] * (gf - 3.0 + W[18])).exp();
                } else {
                    let r = retrievability(elapsed, s);
                    self.stability = if g == Grade::Again {
                        let f = W[11] * d.powf(-W[12]) * ((s + 1.0).powf(W[13]) - 1.0) * (W[14] * (1.0 - r)).exp();
                        f.min(s)
                    } else {
                        let hard = if g == Grade::Hard { W[15] } else { 1.0 };
                        let easy = if g == Grade::Easy { W[16] } else { 1.0 };
                        s * ((W[8]).exp() * (11.0 - d) * s.powf(-W[9]) * ((W[10] * (1.0 - r)).exp() - 1.0) * hard * easy + 1.0)
                    };
                }
                let delta = -W[6] * (gf - 3.0);
                let damped = d + delta * (10.0 - d) / 9.0;
                self.difficulty = (W[7] * d0(4.0) + (1.0 - W[7]) * damped).clamp(1.0, 10.0);
            }
        }
        self.stability = self.stability.clamp(0.01, 36_500.0);
        self.reps += 1;
        if g == Grade::Again {
            self.lapses += 1;
        }
        self.last = Some(today);
        // A lapse comes back tomorrow; otherwise the interval for the
        // wanted retention.
        self.due = today + if g == Grade::Again { 1 } else { interval(self.stability, retention) };
        self.due
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Deck {
    pub cards: Vec<Card>,
    /// The card being asked, by index, and whether its back was shown.
    #[serde(default)]
    pub asking: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Refused {
    Empty,
    Secret(Vec<&'static str>),
    Exists,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SrsConfig {
    pub enabled: bool,
    /// The chance of remembering a card on the day it comes back.
    pub retention: f64,
    /// Most cards one "quiz me" takes.
    pub per_session: usize,
}

impl Default for SrsConfig {
    fn default() -> Self {
        SrsConfig { enabled: true, retention: 0.9, per_session: 20 }
    }
}

impl Deck {
    pub fn add(&mut self, front: &str, back: &str, deck: &str, today: i64) -> Result<(), Refused> {
        let (f, b) = (front.trim(), back.trim());
        if f.is_empty() || b.is_empty() {
            return Err(Refused::Empty);
        }
        let mut secrets = crate::redact::secrets_in(f);
        secrets.extend(crate::redact::secrets_in(b));
        if !secrets.is_empty() {
            secrets.sort();
            secrets.dedup();
            return Err(Refused::Secret(secrets));
        }
        if self.cards.iter().any(|c| c.front.eq_ignore_ascii_case(f)) {
            return Err(Refused::Exists);
        }
        if self.cards.len() >= MAX_CARDS {
            return Err(Refused::Full);
        }
        self.cards.push(Card { front: f.into(), back: b.into(), deck: deck.trim().into(), stability: 0.0, difficulty: 0.0, last: None, due: today, reps: 0, lapses: 0 });
        Ok(())
    }

    /// Due cards, most overdue first (by how far retrievability has fallen),
    /// new ones after.
    pub fn due(&self, today: i64, n: usize) -> Vec<usize> {
        let mut v: Vec<(f64, usize)> = self
            .cards
            .iter()
            .enumerate()
            .filter(|(_, c)| c.due <= today)
            .map(|(i, c)| (c.last.map(|l| retrievability((today - l) as f64, c.stability)).unwrap_or(2.0), i))
            .collect();
        v.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        v.into_iter().take(n).map(|(_, i)| i).collect()
    }

    /// Start or continue: the next card's front, or `None` when nothing's due.
    pub fn next(&mut self, today: i64, cfg: &SrsConfig) -> Option<String> {
        let i = *self.due(today, cfg.per_session.max(1)).first()?;
        self.asking = Some(i);
        let left = self.due(today, usize::MAX).len();
        Some(format!("{} ({} due) -- say \"show\", or how well you knew it.", self.cards[i].front, left))
    }

    /// The asked card's back.
    pub fn show(&self) -> Option<String> {
        self.asking.and_then(|i| self.cards.get(i)).map(|c| c.back.clone())
    }

    /// Grade the asked card and move on. Returns (answer, next due in days).
    pub fn grade(&mut self, g: Grade, today: i64, cfg: &SrsConfig) -> Option<(String, i64)> {
        let i = self.asking.take()?;
        let c = self.cards.get_mut(i)?;
        let due = c.review(g, today, cfg.retention);
        Some((c.back.clone(), due - today))
    }
}

/// "card: front | back", "make a card: front | back", "flashcard front = back".
pub fn read_card(said: &str) -> Option<(String, String)> {
    let low = said.trim().to_ascii_lowercase();
    for lead in ["make a card:", "make a card", "new card:", "card:", "flashcard:", "flashcard", "add a card:"] {
        if low.starts_with(lead) {
            let rest = said.trim()[lead.len()..].trim();
            let (f, b) = rest.split_once('|').or_else(|| rest.split_once(" = "))?;
            return Some((f.trim().to_string(), b.trim().to_string()));
        }
    }
    None
}
