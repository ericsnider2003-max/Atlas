//! How long a turn actually took, and where it went.
//!
//! Nothing measured this. `perf` decides how hard to work when the machine is
//! busy; `budget` counts what a model call costs. Neither answers the question
//! you ask when it feels slow: **which part was slow?**
//!
//! Without that, every latency improvement is a guess. You can shorten the
//! wake word, warm the model, stream the speech — and have no way to tell
//! which of the three did anything, or whether the thing you shortened was
//! ever the problem.
//!
//! It is also the missing producer for `selfaudit::Kind::GotSlower`, which has
//! sat in the taxonomy since it was written with nothing able to raise it. A
//! signal nothing can produce is a signal that will never fire.
//!
//! ## What it deliberately doesn't do
//!
//! No averages. A mean turn time hides the turn that took nine seconds, and
//! the nine-second turn is the entire complaint — nobody has ever been annoyed
//! by an average. It keeps the worst recent turns and the typical one, which
//! are the two numbers that answer different questions.
//!
//! No storage. This lives in memory and is lost on restart. Turn latency is
//! only interesting while it is happening or shortly after, and writing it to
//! disk would make the thing that measures overhead into a source of it.

use serde::{Deserialize, Serialize};

/// The parts of a turn, in the order they happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// Waiting for you to stop speaking.
    Listening,
    /// Speech to text.
    Hearing,
    /// Working out what you meant.
    Understanding,
    /// Doing it.
    Doing,
    /// Text to speech.
    Speaking,
    /// Actually playing the audio.
    Playing,
}

impl Stage {
    pub fn all() -> [Stage; 6] {
        [
            Stage::Listening,
            Stage::Hearing,
            Stage::Understanding,
            Stage::Doing,
            Stage::Speaking,
            Stage::Playing,
        ]
    }

    /// The one-word key a log line carries for this stage.
    pub fn key(&self) -> &'static str {
        match self {
            Stage::Listening => "listening",
            Stage::Hearing => "hearing",
            Stage::Understanding => "understanding",
            Stage::Doing => "doing",
            Stage::Speaking => "speaking",
            Stage::Playing => "playing",
        }
    }

    pub fn plain(&self) -> &'static str {
        match self {
            Stage::Listening => "waiting for you to finish",
            Stage::Hearing => "turning speech into words",
            Stage::Understanding => "working out what you meant",
            Stage::Doing => "doing it",
            Stage::Speaking => "turning words into speech",
            Stage::Playing => "playing it back",
        }
    }

    /// Is this one Atlas can do anything about?
    ///
    /// `Listening` is you talking and `Playing` is the audio's real length.
    /// Neither is a fault, and counting them as slowness would mean a long
    /// question looking like a slow assistant.
    pub fn atlas_controls_it(&self) -> bool {
        !matches!(self, Stage::Listening | Stage::Playing)
    }
}

/// One turn, timed.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Turn {
    /// Milliseconds per stage. A stage that didn't run is absent rather than
    /// zero — a stage that took no time and a stage that never happened are
    /// different facts, and averaging them together is how a skipped step
    /// looks fast.
    pub ms: Vec<(Stage, u32)>,
    /// What was asked, short, so a slow turn can be recognised.
    pub about: String,
}

impl Turn {
    pub fn note(&mut self, stage: Stage, ms: u32) {
        match self.ms.iter_mut().find(|(s, _)| *s == stage) {
            Some(slot) => slot.1 = ms,
            None => self.ms.push((stage, ms)),
        }
    }

    pub fn get(&self, stage: Stage) -> Option<u32> {
        self.ms.iter().find(|(s, _)| *s == stage).map(|(_, m)| *m)
    }

    /// Only the parts Atlas could have made faster.
    ///
    /// The number worth improving. Total time includes you talking, and a long
    /// question is not a slow assistant.
    pub fn atlas_ms(&self) -> u32 {
        self.ms
            .iter()
            .filter(|(s, _)| s.atlas_controls_it())
            .map(|(_, m)| *m)
            .sum()
    }

    /// The stage that cost the most, of the ones Atlas controls.
    pub fn worst(&self) -> Option<(Stage, u32)> {
        self.ms
            .iter()
            .filter(|(s, _)| s.atlas_controls_it())
            .max_by_key(|(_, m)| *m)
            .copied()
    }

    /// Was this turn slow enough to be worth saying something about?
    pub fn was_slow(&self, over_ms: u32) -> bool {
        self.atlas_ms() > over_ms
    }
}

/// Above this, a turn stops feeling like a conversation.
///
/// Not a target. It is the point at which someone starts wondering whether it
/// heard them, which is a different and lower bar than "too slow to be useful".
pub const FEELS_SLOW_MS: u32 = 2_000;

/// How many turns to keep.
///
/// Enough to see a pattern, few enough that it costs nothing. A window rather
/// than a total, so a bad afternoon shows up instead of being diluted by a
/// good month.
pub const KEEP: usize = 40;

/// Recent turns.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Recent {
    pub turns: Vec<Turn>,
}

impl Recent {
    pub fn add(&mut self, t: Turn) {
        self.turns.push(t);
        if self.turns.len() > KEEP {
            let cut = self.turns.len() - KEEP;
            self.turns.drain(..cut);
        }
    }

    /// The middle turn, not the mean.
    ///
    /// A mean is dragged around by one bad turn and tells you nothing about
    /// either the usual case or the worst one.
    pub fn typical_ms(&self) -> Option<u32> {
        if self.turns.is_empty() {
            return None;
        }
        let mut v: Vec<u32> = self.turns.iter().map(|t| t.atlas_ms()).collect();
        v.sort_unstable();
        Some(v[v.len() / 2])
    }

    /// The turns that annoyed you, worst first.
    pub fn slow_ones(&self) -> Vec<&Turn> {
        let mut v: Vec<&Turn> = self
            .turns
            .iter()
            .filter(|t| t.was_slow(FEELS_SLOW_MS))
            .collect();
        v.sort_by_key(|t| std::cmp::Reverse(t.atlas_ms()));
        v
    }

    /// Which stage is costing the most across recent turns.
    ///
    /// Summed rather than averaged. A stage that is slow every single time
    /// costs more than one that is occasionally terrible, and summing says so
    /// while an average of averages does not.
    pub fn worst_stage(&self) -> Option<(Stage, u32)> {
        let mut totals: Vec<(Stage, u32)> = Vec::new();
        for t in &self.turns {
            for (s, ms) in &t.ms {
                if !s.atlas_controls_it() {
                    continue;
                }
                match totals.iter_mut().find(|(x, _)| x == s) {
                    Some(slot) => slot.1 += ms,
                    None => totals.push((*s, *ms)),
                }
            }
        }
        totals.into_iter().max_by_key(|(_, m)| *m)
    }

    /// The middle duration for one specific stage, across whichever recent
    /// turns actually ran it. `None` when nothing has run it yet -- a stage
    /// that never happened is a different fact from one that took no time,
    /// same reasoning as `Turn::note`'s own doc.
    pub fn typical_for(&self, stage: Stage) -> Option<u32> {
        let mut v: Vec<u32> = self.turns.iter().filter_map(|t| t.get(stage)).collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        Some(v[v.len() / 2])
    }

    /// What to say when asked why it's slow.
    ///
    /// Names the stage in your words rather than reporting six numbers. Six
    /// numbers is a thing you have to interpret; one sentence is an answer.
    pub fn why_slow(&self) -> String {
        let Some(typical) = self.typical_ms() else {
            return "I haven't handled enough turns yet to tell you.".into();
        };
        let slow = self.slow_ones();
        if slow.is_empty() {
            return format!("Nothing's been slow — a turn is taking about {typical}ms of my time.");
        }
        match self.worst_stage() {
            Some((stage, _)) => format!(
                "{} of the last {} turns were slow. Most of it is {} — the usual turn is {}ms.",
                slow.len(),
                self.turns.len(),
                stage.plain(),
                typical
            ),
            None => format!("{} recent turns were slow.", slow.len()),
        }
    }

    /// Has it got slower?
    ///
    /// The producer `selfaudit::Kind::GotSlower` never had. Compares the older
    /// half of the window against the newer half — a fixed threshold would
    /// only ever say "this machine is slow", and the question is whether
    /// something changed.
    pub fn got_slower(&self) -> Option<crate::selfaudit::Signal> {
        if self.turns.len() < 10 {
            return None;
        }
        let half = self.turns.len() / 2;
        let median = |v: &[Turn]| {
            let mut m: Vec<u32> = v.iter().map(|t| t.atlas_ms()).collect();
            m.sort_unstable();
            m[m.len() / 2]
        };
        let before = median(&self.turns[..half]);
        let after = median(&self.turns[half..]);
        // Half again, not a few percent. Turn times are noisy and a small
        // change means nothing.
        if before == 0 || after < before + before / 2 {
            return None;
        }
        let stage = self.worst_stage().map(|(s, _)| s.plain()).unwrap_or("something");
        Some(crate::selfaudit::Signal {
            kind: crate::selfaudit::Kind::GotSlower,
            subject: stage.to_string(),
            seen: after,
            of: before,
            example: format!("turns went from about {before}ms to about {after}ms"),
        })
    }
}

/// Where one pass of the tick spent its time, by named part (30 Sep 2026).
///
/// The laptop's log said "tick took 1777ms" and nothing about which part:
/// every slow tick now names its three slowest parts, so the next look at a
/// log says where the time goes instead of guessing.
#[derive(Debug)]
pub struct Laps {
    last: std::time::Instant,
    parts: Vec<(&'static str, u32)>,
}

impl Default for Laps {
    fn default() -> Self {
        Laps::start()
    }
}

impl Laps {
    pub fn start() -> Laps {
        Laps { last: std::time::Instant::now(), parts: Vec::new() }
    }

    /// The part just finished, by name.
    pub fn mark(&mut self, name: &'static str) {
        let now = std::time::Instant::now();
        let ms = now.duration_since(self.last).as_millis().min(u32::MAX as u128) as u32;
        self.last = now;
        match self.parts.iter_mut().find(|(n, _)| *n == name) {
            Some(p) => p.1 = p.1.saturating_add(ms),
            None => self.parts.push((name, ms)),
        }
    }

    /// The slowest `n` parts, slowest first, that took any time at all.
    pub fn slowest(&self, n: usize) -> Vec<(&'static str, u32)> {
        let mut p: Vec<(&'static str, u32)> = self.parts.iter().copied().filter(|(_, ms)| *ms > 0).collect();
        p.sort_by(|a, b| b.1.cmp(&a.1));
        p.truncate(n);
        p
    }

    /// Every part so far, in the order first marked.
    pub fn parts(&self) -> &[(&'static str, u32)] {
        &self.parts
    }

    /// "work_for_you 900ms, observe 400ms, health 120ms".
    pub fn plain(&self, n: usize) -> String {
        self.slowest(n).iter().map(|(name, ms)| format!("{name} {ms}ms")).collect::<Vec<_>>().join(", ")
    }
}

#[cfg(test)]
mod laps_tests {
    use super::*;

    #[test]
    fn the_slowest_parts_are_named_slowest_first() {
        let mut l = Laps { last: std::time::Instant::now(), parts: vec![] };
        l.parts = vec![("a", 5), ("b", 50), ("c", 0), ("d", 20)];
        assert_eq!(l.slowest(2), vec![("b", 50), ("d", 20)]);
        assert_eq!(l.plain(5), "b 50ms, d 20ms, a 5ms");
    }
}
