//! Learning you.
//!
//! Not a profile you fill in — those go stale and nobody updates them. This is
//! built from what actually happens: when you work, what you come back to,
//! how you phrase things, what you always change about Atlas's output, what
//! you keep and what you throw away.
//!
//! ## What this deliberately isn't
//!
//! It doesn't diagnose you and it doesn't infer things about your inner life.
//! It notices patterns in what you *do* — which is both more useful and more
//! honest than a system guessing at how you feel from your word choice. The
//! difference matters: "you've been at this since six and it's now eleven" is
//! an observation you can check. "You seem stressed" is a guess dressed as
//! insight, and a wrong one is worse than silence.

use serde::{Deserialize, Serialize};

/// Something learned about how you work.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Trait_ {
    /// Plain language, and phrased so you could disagree with it.
    pub what: String,
    /// How many times Atlas has seen it.
    pub seen: u32,
    /// When it last held.
    pub last: u64,
    pub kind: Kind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// When you work, how long, what you do first.
    Rhythm,
    /// How you want things said and shown.
    Taste,
    /// What you're working on, and how it's going.
    Focus,
    /// Words you use, people you mention, projects you name.
    World,
    /// What you always change about Atlas's output.
    Correction,
}

impl Trait_ {
    /// Is this worth acting on, or has Atlas seen it once?
    pub fn confident(&self) -> bool {
        self.seen >= 3
    }
    pub fn stale(&self, now: u64) -> bool {
        // Six weeks. People change how they work, and an old pattern
        // confidently applied is worse than none.
        now.saturating_sub(self.last) > 42 * 86_400
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Person {
    pub traits: Vec<Trait_>,
    /// Hours you're usually active, as a count per hour of the day.
    pub active_hours: Vec<(u32, u32)>,
    /// Projects and when each was last touched.
    pub projects: Vec<(String, u64)>,
    /// Days on which you worked outside the hours you usually keep.
    ///
    /// Days rather than sessions: two late stretches on one night are one
    /// night, and `late_nights_before_saying` counts nights.
    #[serde(default)]
    pub late_days: Vec<u64>,
}

impl Person {
    pub fn load(store: &crate::store::Store) -> Person {
        store.load("person")
    }

    pub fn save(&self, store: &crate::store::Store) -> crate::error::Result<()> {
        store.save("person", self)
    }

    pub fn notice(&mut self, what: &str, kind: Kind, now: u64) {
        match self.traits.iter_mut().find(|t| t.what == what) {
            Some(t) => {
                t.seen += 1;
                t.last = now;
            }
            None => self.traits.push(Trait_ { what: what.into(), seen: 1, last: now, kind }),
        }
    }

    /// You did something at this hour.
    pub fn worked_at(&mut self, hour: u32) {
        match self.active_hours.iter_mut().find(|(h, _)| *h == hour) {
            Some((_, n)) => *n += 1,
            None => self.active_hours.push((hour, 1)),
        }
    }

    /// Note the hour you are working at, and whether it was a late one.
    ///
    /// The producer `Noticed::LateRun` never had. `worked_at` recorded the
    /// hour and nothing turned a pile of hours into "that's three nights
    /// running" — so `late_nights_before_saying` was a threshold on a count
    /// nobody kept.
    ///
    /// Worth knowing: `usual_hours` is derived from the same counts this
    /// increments, so a late hour kept up long enough stops being late. That
    /// is the right behaviour — if midnight is when you work now, Atlas
    /// saying so every night would be wrong — and it is why this is a
    /// observation rather than a rule.
    pub fn working_now(&mut self, hour: u32, now: u64, cfg_j: &crate::judgment::JudgmentConfig) {
        let was_late = self.is_late(hour, cfg_j);
        self.worked_at(hour);
        if !was_late {
            return;
        }
        let day = crate::localclock::day_here(now) as u64;
        if !self.late_days.contains(&day) {
            self.late_days.push(day);
        }
        // Bounded. A year of late nights is not more informative than two
        // months of them, and this is saved on every persist.
        self.late_days.sort_unstable();
        let too_many = self.late_days.len().saturating_sub(60);
        self.late_days.drain(..too_many);
    }

    /// Is this an hour you do not usually keep?
    ///
    /// Compared against the busiest hour you keep rather than against
    /// `usual_hours`, which uses a third of the peak. A third is right for
    /// "when do you work" and too generous here: three nights at 2am against
    /// a five-day week already clears it, so a run would stop counting itself
    /// before the threshold it is measured against could be reached.
    ///
    /// Half the peak, and it still drifts — four nights at 2am and Atlas
    /// stops calling 2am late. That is the right answer rather than a
    /// weakness: at four nights running you work nights, and a laptop that
    /// went on remarking on it every evening would be wrong as well as
    /// annoying.
    fn is_late(&self, hour: u32, cfg_j: &crate::judgment::JudgmentConfig) -> bool {
        // No usual hours yet means nothing to be outside of. `usual_hours`
        // refuses below three hours seen, which is the same refusal.
        let Some((from, to)) = self.usual_hours(cfg_j) else {
            return false;
        };
        let outside = if from <= to {
            hour < from || hour > to
        } else {
            // A range that wraps midnight: usually up until 2am.
            hour > to && hour < from
        };
        if outside {
            return true;
        }
        // Inside the range and still barely kept.
        //
        // This was `kept < peak * 0.5`, and **the peak is the wrong anchor**.
        // One very heavy hour halves every other hour's fraction, so somebody
        // with a single 10am spike reads as "late" through most of their
        // working day, while somebody with a flat nine-to-nine never does.
        // The comment this replaces already half-knew it: it explains why
        // `usual_hours`' third is "too generous here" and answers with
        // another invented fraction rather than with the distribution.
        //
        // Against the spread of your hours instead, in the same units
        // `daily::Rhythm::quiet_at` uses -- one file, one idea of what
        // unusual means.
        let counts: Vec<f64> = self.active_hours.iter().map(|(_, n)| *n as f64).collect();
        let Some((usually, varies_by)) = crate::judgment::ordinary_for_counts(&counts) else {
            return false;
        };
        let kept = self
            .active_hours
            .iter()
            .find(|(h, _)| *h == hour)
            .map(|(_, n)| *n)
            .unwrap_or(0);
        match crate::judgment::how_unusual(
            &crate::judgment::Measured {
                name: "this hour",
                value: kept as f64,
                usually,
                varies_by,
                seen: counts.iter().sum::<f64>() as usize,
            },
            cfg_j,
        ) {
            // More than one ordinary variation below an ordinary hour --
            // the same line `daily::Rhythm::quiet_at` draws, so the two
            // files cannot come to mean different things by "unusually
            // quiet".
            //
            // It is also what makes the drift work. Atlas is meant to stop
            // calling 2am late once you have kept it four nights running:
            // against a settled nine-to-five those four nights put the hour
            // 0.9 variations below ordinary, and the third night puts it
            // 1.3 below. Half a step would have called the fourth night late
            // too, and Atlas would have gone on remarking on your hours
            // forever.
            Some(u) => u <= -1.0,
            // Not enough behind it to say. `false` keeps Atlas quiet, which
            // is the right direction for something that decides whether to
            // remark on your working habits.
            None => false,
        }
    }

    /// Nights in a row past your usual hours, counting back from today.
    pub fn late_run(&self, now: u64) -> u32 {
        let mut day = crate::localclock::day_here(now) as u64;
        let mut nights = 0;
        while self.late_days.contains(&day) {
            nights += 1;
            match day.checked_sub(1) {
                Some(d) => day = d,
                None => break,
            }
        }
        nights
    }

    pub fn touched(&mut self, project: &str, now: u64) {
        match self.projects.iter_mut().find(|(p, _)| p == project) {
            Some((_, t)) => *t = now,
            None => self.projects.push((project.into(), now)),
        }
    }

    /// What Atlas is confident about, and hasn't gone stale.
    pub fn known(&self, now: u64) -> Vec<&Trait_> {
        self.traits.iter().filter(|t| t.confident() && !t.stale(now)).collect()
    }

    pub fn about(&self, kind: Kind, now: u64) -> Vec<&Trait_> {
        self.known(now).into_iter().filter(|t| t.kind == kind).collect()
    }

    /// The hours you're usually working, as a range.
    pub fn usual_hours(&self, cfg_j: &crate::judgment::JudgmentConfig) -> Option<(u32, u32)> {
        if self.active_hours.len() < 3 {
            return None;
        }
        // `*n > peak * 0.3` was here, with the same fault as `is_late`: the
        // busiest hour sets the bar for every other one, so a single spike
        // narrows your working day to almost nothing.
        //
        // An hour is yours if it is not unusually quiet for you. Measured
        // against the middle and spread of all your hours rather than against
        // the top of them.
        let counts: Vec<f64> = self.active_hours.iter().map(|(_, n)| *n as f64).collect();
        let (usually, varies_by) = crate::judgment::ordinary_for_counts(&counts)?;
        let busy: Vec<u32> = self
            .active_hours
            .iter()
            .filter(|(_, n)| {
                crate::judgment::how_unusual(
                    &crate::judgment::Measured {
                        name: "an hour",
                        value: *n as f64,
                        usually,
                        varies_by,
                        seen: counts.iter().sum::<f64>() as usize,
                    },
                    cfg_j,
                )
                // Anything not clearly below ordinary counts as an hour you
                // work. Wider than `is_late`'s test on purpose: "when do you
                // work" should include your quieter hours, and "is this late"
                // should not.
                .map(|u| u > -1.0)
                // Below the floor, the old fraction, and only there.
                .unwrap_or(*n as f32 > usually as f32 * 0.3)
            })
            .map(|(h, _)| *h)
            .collect();
        Some((*busy.iter().min()?, *busy.iter().max()?))
    }

    /// Something you've stopped touching.
    ///
    /// Useful and easy to get wrong: a project untouched for a week is normal,
    /// one untouched for a month when you used to be in it daily is worth
    /// mentioning once.
    /// Returns how long as well as what. It used to return only the names,
    /// and had no caller — "Atlas hasn't moved in 4 weeks" is worth saying and
    /// "Atlas has gone quiet" is not, so a producer for `Noticed::Stalled`
    /// could not be written from the old answer without asking the same
    /// question twice.
    pub fn gone_quiet(&self, now: u64) -> Vec<(&str, u32)> {
        self.projects
            .iter()
            .filter_map(|(p, t)| {
                let days = now.saturating_sub(*t) / 86_400;
                (21..90).contains(&days).then_some((p.as_str(), (days / 7) as u32))
            })
            .collect()
    }

    /// Forget something. Said once, done — no arguing about whether it's true.
    pub fn forget(&mut self, matching: &str) -> usize {
        let before = self.traits.len();
        let m = matching.to_lowercase();
        self.traits.retain(|t| !t.what.to_lowercase().contains(&m));
        before - self.traits.len()
    }

    /// Everything Atlas thinks it knows, so you can read it and argue.
    ///
    /// A system that learns about you and won't show you what it learned is a
    /// system you can't correct.
    pub fn show(&self, now: u64) -> String {
        let known = self.known(now);
        if known.is_empty() {
            return "I haven't worked anything out about you yet.".into();
        }
        let mut s = String::from("What I've picked up:\n");
        for t in known {
            s.push_str(&format!("  · {} (seen {} times)\n", t.what, t.seen));
        }
        s.push_str("\nSay \"forget\" and any of that and I'll drop it.\n");
        s
    }
}

/// The project a request is about (H13h): one it names ("the Northwind
/// project", "project Atlas"), or one you've named before that it mentions.
pub fn project_named(said: &str, known: &[(String, u64)]) -> Option<String> {
    let l = said.to_lowercase();
    let words: Vec<&str> = l.split(|c: char| !c.is_alphanumeric() && c != '-').filter(|w| !w.is_empty()).collect();
    const NOT_NAMES: &[&str] = &[
        "the", "a", "an", "my", "this", "that", "new", "our", "your", "which", "what", "side", "whole", "to", "for",
        "on", "in", "of", "about", "with", "at", "from", "into", "and", "or", "is", "was", "each", "every", "same",
    ];
    for (i, w) in words.iter().enumerate() {
        if *w == "project" {
            // "the Northwind project"
            if let Some(prev) = i.checked_sub(1).and_then(|j| words.get(j)) {
                if !NOT_NAMES.contains(prev) {
                    return Some(title_case(prev));
                }
            }
            // "project Atlas"
            if let Some(next) = words.get(i + 1) {
                if !NOT_NAMES.contains(next) && !["is", "was", "for", "to", "and", "about"].contains(next) {
                    return Some(title_case(next));
                }
            }
        }
    }
    known
        .iter()
        .map(|(p, _)| p)
        .find(|p| {
            let pl = p.to_lowercase();
            pl.len() >= 3 && words.windows(pl.split_whitespace().count().max(1)).any(|w| w.join(" ") == pl)
        })
        .cloned()
}

fn title_case(w: &str) -> String {
    let mut c = w.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

// ---------- learning from what actually happens ----------

/// You changed something Atlas produced. That correction is the most valuable
/// signal there is — it's you saying what you wanted, with an example.
pub fn learn_from_edit(before: &str, after: &str) -> Option<(String, Kind)> {
    let (b, a) = (before.trim(), after.trim());
    if b == a {
        return None;
    }
    let bw = b.split_whitespace().count();
    let aw = a.split_whitespace().count();

    if aw * 2 < bw {
        return Some(("cuts what I write down by about half".into(), Kind::Correction));
    }
    if aw > bw + bw / 2 {
        return Some(("adds detail to what I write".into(), Kind::Correction));
    }
    // Same length, different words: a rewrite for tone.
    let shared = b
        .split_whitespace()
        .filter(|w| a.contains(*w))
        .count();
    if shared * 3 < bw {
        return Some(("rewrites my wording rather than trimming it".into(), Kind::Correction));
    }
    None
}

/// You said no to something. Worth learning once, not inferring a rule from.
pub fn learn_from_refusal(what: &str) -> (String, Kind) {
    (format!("said no to: {what}"), Kind::Taste)
}

// ---------- noticing, without diagnosing ----------

/// Something Atlas has noticed about your working pattern.
///
/// All of these are observations you can check against a clock or a calendar,
/// which is the line this stays on the right side of.
#[derive(Debug, Clone, PartialEq)]
pub enum Noticed {
    /// Working much later than usual, repeatedly.
    LateRun { nights: u32 },
    /// A long stretch without a break.
    NoBreak { hours: u32 },
    /// Something you cared about has stopped moving.
    Stalled { project: String, weeks: u32 },
    /// A lot of starting and not much finishing.
    Scattered { started: u32, finished: u32 },
}

impl Noticed {
    /// What Atlas says. Observation, then an offer — never a diagnosis and
    /// never advice you didn't ask for.
    pub fn spoken(&self) -> String {
        match self {
            Noticed::LateRun { nights } => format!(
                "That's {nights} nights running past your usual hours. Want me to take anything \
                 off tomorrow, or shall I leave it?"
            ),
            Noticed::NoBreak { hours } => {
                format!("You've been at this {hours} hours. Nothing needs you right now if you want to stop.")
            }
            Noticed::Stalled { project, weeks } => format!(
                "{project} hasn't moved in {weeks} weeks. Want me to pick it up, or is it \
                 parked on purpose?"
            ),
            Noticed::Scattered { started, finished } => format!(
                "{started} things started, {finished} finished this week. Want me to line them \
                 up so you can drop some?"
            ),
        }
    }

    /// Some observations are worth making once and then dropping.
    pub fn say_once(&self) -> bool {
        matches!(self, Noticed::Stalled { .. } | Noticed::Scattered { .. })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PersonConfig {
    pub enabled: bool,
    /// Notice working patterns and say something.
    pub notice_patterns: bool,
    /// Nights past your usual hours before mentioning it.
    pub late_nights_before_saying: u32,
    /// Hours straight before mentioning it.
    pub hours_before_saying: u32,
    /// Never say the same observation more than once in this many days.
    pub quiet_days: u64,
}

impl Default for PersonConfig {
    fn default() -> Self {
        PersonConfig {
            enabled: true,
            notice_patterns: true,
            late_nights_before_saying: 3,
            hours_before_saying: 6,
            quiet_days: 7,
        }
    }
}

/// The line Atlas will not cross, and says so plainly when asked.
///
/// Knowing you well is not the same as being qualified to help with the things
/// a person should help with. Atlas can be genuinely useful — it knows your
/// hours, your projects, what you're avoiding, what you said last month — and
/// none of that makes it a substitute for someone whose job this is.
pub const NOT_A_THERAPIST: &str =
    "I'm not the right thing for this, and I'd rather say so than have a go. \
     I can take things off your plate, or just leave you alone for a bit — say which. \
     For the rest of it, a person is better than me, and that isn't me deflecting.";

/// Does this need a person rather than an assistant?
///
/// Deliberately narrow: most difficult conversations are just difficult
/// conversations, and treating every mention of a bad week as a crisis is its
/// own kind of unhelpful.
pub fn beyond_me(said: &str) -> bool {
    let t = said.to_lowercase();
    [
        "want to die", "kill myself", "end it all", "worth living",
        "hurt myself", "harm myself", "no reason to go on", "better off without me",
        "end my life", "not want to be here", "cant go on", "can't go on",
    ]
    .iter()
    .any(|p| t.contains(p))
}

/// Does this read as a hard time that wants doing-something, not redirecting?
///
/// The narrow companion to `beyond_me`: `beyond_me` catches the message that
/// needs a person instead of an assistant, and this catches the ordinary rough
/// day, where the right move is neither to redirect nor to perform concern but
/// to quietly offer to take real work off the plate. Kept deliberately narrow —
/// most mentions of a bad day are just conversation — and checked *after*
/// `beyond_me`, so a genuine crisis is never softened into an offer of errands.
pub fn having_a_hard_time(said: &str) -> bool {
    let t = said.to_lowercase();
    [
        "rough day", "hard day", "long day", "terrible day", "awful day",
        "so stressed", "really stressed", "stressed out", "overwhelmed",
        "burnt out", "burned out", "worn out", "at my limit", "falling apart",
        "had it today", "so drained", "wiped out today",
    ]
    .iter()
    .any(|p| t.contains(p))
}

/// What Atlas says when someone is having a hard time but doesn't need
/// redirecting anywhere.
///
/// The useful thing here is almost never words. It's doing something.
pub fn hard_day(what_it_can_take_on: &[String]) -> String {
    match what_it_can_take_on.split_first() {
        None => "Rough one. I'm here if there's anything I can take off you.".into(),
        Some((first, rest)) => format!(
            "Rough one. I can deal with {first}{} if it helps — say the word.",
            if rest.is_empty() {
                String::new()
            } else {
                format!(" and {} other thing{}", rest.len(), if rest.len() == 1 { "" } else { "s" })
            }
        ),
    }
}

// ============ the producer Noticed never had ============
//
// `Noticed` has four variants, each with a sentence and a rule about whether
// it is worth repeating, and nothing in the tree ever built one. Three
// settings governed it — `notice_patterns`, `late_nights_before_saying`,
// `hours_before_saying` — and a fourth, `quiet_days`, decided how often the
// same observation could be made. All four were thresholds on something
// nobody produced.
//
// What is said here is an observation and an offer. Never a diagnosis, never
// advice nobody asked for, and never anything that cannot be checked against
// a clock or a calendar — which is the line this stays on the right side of.

/// What Atlas has already said about you, and when.
///
/// `quiet_days` is "never say the same observation more than once in this
/// many days", which needs a memory of what was said. Keyed by the kind of
/// observation rather than by its exact words, because "that's 3 nights
/// running" and "that's 4 nights running" are the same remark.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Said {
    pub lines: Vec<(String, u64)>,
}

/// Where that is kept.
pub const SAID_RECORD: &str = "person_said";

fn key_for(n: &Noticed) -> &'static str {
    match n {
        Noticed::LateRun { .. } => "late_run",
        Noticed::NoBreak { .. } => "no_break",
        Noticed::Stalled { .. } => "stalled",
        Noticed::Scattered { .. } => "scattered",
    }
}

impl Said {
    /// Has enough time passed to say this again?
    pub fn may_say(&self, n: &Noticed, cfg: &PersonConfig, now: u64) -> bool {
        let key = key_for(n);
        let Some((_, at)) = self.lines.iter().find(|(k, _)| k == key) else {
            return true;
        };
        // Some observations are worth making once and then dropping.
        // "Nothing has moved on this in a month" does not become more true
        // by being said again next month.
        if n.say_once() {
            return false;
        }
        // A record from the future is a clock that moved, not a remark from
        // tomorrow. Allowed rather than silenced: the failure of a quiet
        // period should be saying something twice, not never again.
        *at > now || now.saturating_sub(*at) >= cfg.quiet_days * 86_400
    }

    pub fn record(&mut self, n: &Noticed, now: u64) {
        let key = key_for(n).to_string();
        match self.lines.iter_mut().find(|(k, _)| *k == key) {
            Some((_, at)) => *at = now,
            None => self.lines.push((key, now)),
        }
    }
}

/// What there is to notice about how you're working, if anything.
///
/// `hours_straight` is how long you have been at it without a break, which
/// only the running daemon can know — it comes from idle time rather than
/// from anything stored here.
///
/// One at a time, most pressing first. Four observations at once is a
/// lecture, and nobody takes a lecture from their laptop.
pub fn noticing(
    p: &Person,
    cfg: &PersonConfig,
    hours_straight: u32,
    now: u64,
) -> Option<Noticed> {
    if !cfg.enabled || !cfg.notice_patterns {
        return None;
    }
    // Zero means never, for both thresholds. That is a real answer and not
    // the same as "say it every time", which is what `>=` alone would give.
    if cfg.late_nights_before_saying > 0 {
        let nights = p.late_run(now);
        if nights >= cfg.late_nights_before_saying {
            return Some(Noticed::LateRun { nights });
        }
    }
    if cfg.hours_before_saying > 0 && hours_straight >= cfg.hours_before_saying {
        return Some(Noticed::NoBreak { hours: hours_straight });
    }
    // Longest quiet first: the one that has been still the longest is the one
    // most likely to be genuinely parked rather than merely paused.
    let mut quiet = p.gone_quiet(now);
    quiet.sort_by_key(|(_, weeks)| std::cmp::Reverse(*weeks));
    if let Some((project, weeks)) = quiet.first() {
        return Some(Noticed::Stalled { project: (*project).to_string(), weeks: *weeks });
    }
    None
}
