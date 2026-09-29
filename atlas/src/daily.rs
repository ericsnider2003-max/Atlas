//! The day as a unit.
//!
//! A task list that never ends is one you stop trusting, because "outstanding"
//! quietly comes to mean "everything I have ever thought of". Closing it at
//! midnight and opening a fresh one forces the useful question every morning:
//! is this still worth doing today?
//!
//! **The thinking does not reset.** That's the important half. A task carried
//! forward six times carries its whole history with it — what was tried, what
//! it's stuck on, why it keeps slipping — because that history is the reason
//! it's still there and the thing that tells you to drop it.

use serde::{Deserialize, Serialize};

/// A day's list, once it's closed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Closed {
    /// Midnight at the start of the day.
    pub day: u64,
    pub finished: Vec<String>,
    /// Still open at the end, and moved forward.
    pub carried: Vec<String>,
    /// You said you weren't doing it.
    pub dropped: Vec<String>,
}

impl Closed {
    /// Did the day go anywhere?
    pub fn moved(&self) -> bool {
        !self.finished.is_empty() || !self.dropped.is_empty()
    }
}

/// How many days something has been carried.
///
/// This is the number that matters and no task app shows it. Something on its
/// eighth day is not a task you keep forgetting, it's a task you have decided
/// eight times not to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Carried(pub u32);

impl Carried {
    /// Worth saying something about?
    ///
    /// `after_days` is your `daily.ask_after_days`. It was a hardcoded 5 here
    /// and a hardcoded `0..=4` in `nudge`, while the shipped file carried
    /// `ask_after_days: 5` that nothing read — the two agreed by coincidence,
    /// and someone who wanted to be asked on day three got asked on day five.
    pub fn worth_mentioning(&self, after_days: u32) -> bool {
        self.0 >= after_days.max(1)
    }

    /// What Atlas says about it, once.
    ///
    /// Twice the threshold is where "still worth doing?" becomes "I'd drop
    /// it". Relative rather than a second number, so moving the first one
    /// moves both and the escalation keeps its shape; at the shipped 5 this
    /// is the 5–9 / 10+ split it has always been.
    pub fn nudge(&self, title: &str, after_days: u32) -> Option<String> {
        let after = after_days.max(1);
        match self.0 {
            d if d < after => None,
            d if d < after * 2 => Some(format!(
                "\"{title}\" has moved forward {} days. Still worth doing, or is it done being \
                 a task?",
                self.0
            )),
            _ => Some(format!(
                "\"{title}\" is on day {}. I'd drop it — you can always put it back, and it's \
                 costing you a line every morning.",
                self.0
            )),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct DailyConfig {
    pub enabled: bool,
    /// When the day rolls over, worked out rather than set.
    ///
    /// Asking someone to declare their own working hours is asking them to
    /// maintain a setting they'll get wrong twice a year. Atlas watches when
    /// you actually stop and rolls the list in the quiet part of your night.
    pub rolls_at_hour: u32,
    /// Carry unfinished work forward automatically.
    pub carry_forward: bool,
    /// Ask about anything carried this many times.
    pub ask_after_days: u32,
    /// Keep closed days for this long.
    pub keep_days: u32,
    /// A gap this long means you went away and came back, rather than carried on.
    ///
    /// The whole of what tells working through the night from starting a new
    /// day. Your day turning over is not the same as you arriving: if the
    /// rollover happens at four in the morning while you are still typing,
    /// nothing has started — you are four hours into a night. What starts a
    /// day is coming back after being gone.
    ///
    /// Sleep-sized rather than break-sized. Long enough that lunch, a
    /// meeting, or an afternoon out is still the same stretch of work.
    pub back_after_minutes: u32,
    /// A gap this long means you are properly gone, not just not-typing.
    ///
    /// Separate from `back_after_minutes`, and longer, because *coming back*
    /// and *being safely gone* are different questions. Ninety minutes of
    /// silence means you left the desk. It does not mean Atlas should begin
    /// an hour of work you will interrupt in five minutes.
    ///
    /// This is what replaces `overnight.start_hour`/`stop_hour`: the night's
    /// work used to run on a timetable, so it ran while you were up at two in
    /// the morning and did not run on the Saturday you were out all day.
    pub away_a_while_minutes: u32,
}

impl Default for DailyConfig {
    fn default() -> Self {
        DailyConfig {
            enabled: true,
            rolls_at_hour: 0,
            carry_forward: true,
            ask_after_days: 5,
            keep_days: 90,
            back_after_minutes: 150,
            away_a_while_minutes: 90,
        }
    }
}

/// Working out when your day ends, from when you actually stop.
///
/// Two things make this work. The gap it looks for is the *longest* quiet
/// stretch, not the first — everyone has a quiet hour in the afternoon.
/// And **Atlas working overnight is not you working**: a machine that ran a
/// job at three in the morning has told you nothing about your day.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Rhythm {
    /// Hours you were active, 0–23, counted over the last few weeks.
    pub active_by_hour: [u32; 24],
    pub days_watched: u32,
}

impl Rhythm {
    /// Note that you did something. Only you — Atlas's own work is passed in
    /// as `by_you: false` and ignored.
    pub fn saw(&mut self, hour: u32, by_you: bool) {
        if by_you && hour < 24 {
            self.active_by_hour[hour as usize] += 1;
        }
    }

    /// A day watched. Older days also count for a little less each day
    /// (about 3%, so a month-old day counts for under half), so the hour your
    /// day ends follows your hours as they change. Eric, 25 Sep 2026 (F2):
    /// "it needs to still be adaptive because I work a lot of random hours".
    pub fn note_day(&mut self) {
        self.days_watched += 1;
        for n in self.active_by_hour.iter_mut() {
            *n -= *n / 32;
        }
    }

    /// The hour your day turns over.
    ///
    /// The middle of your longest quiet stretch, so a list that rolls at 4am
    /// for a night owl and 3am for everyone else does it without being told.
    pub fn quiet_hour(&self) -> Option<u32> {
        // A fortnight before trusting it. Less than that and one late night
        // moves your whole day.
        if self.days_watched < 14 {
            return None;
        }
        let total: u32 = self.active_by_hour.iter().sum();
        if total < 50 {
            return None;
        }
        // Quiet means genuinely quiet, not merely quieter.
        let threshold = (total / 24).max(1);
        let quiet: Vec<bool> = self
            .active_by_hour
            .iter()
            .map(|n| *n < threshold / 2)
            .collect();

        let (mut best_start, mut best_len) = (0usize, 0usize);
        let (mut start, mut len) = (0usize, 0usize);
        // Twice round, so a stretch spanning midnight is seen as one stretch
        // rather than two — which is the case that matters here.
        for i in 0..48 {
            if quiet[i % 24] {
                if len == 0 {
                    start = i;
                }
                len += 1;
                if len > best_len {
                    best_len = len;
                    best_start = start;
                }
            } else {
                len = 0;
            }
        }
        if best_len < 3 {
            return None;
        }
        Some(((best_start + best_len / 2) % 24) as u32)
    }

    /// Is this hour inside the stretch you are usually not about?
    ///
    /// The same measurement `quiet_hour` takes the middle of, asked of one
    /// hour rather than reduced to a single number. `rolls_at` needs the
    /// midpoint because a day has to turn over at *an* hour;
    /// `daily::whereabouts` needs the whole stretch, because whether you are
    /// asleep is true across all of it and not only at its middle.
    ///
    /// `false` until there is a fortnight of watching, which is the cautious
    /// direction: not-knowing lands on `Out`, and `Out` is the state that
    /// assumes you may be back at any moment.
    pub fn quiet_at(&self, hour: u32, cfg: &crate::judgment::JudgmentConfig) -> bool {
        if hour > 23 {
            return false;
        }
        // A fortnight, and this is about **coverage** rather than sample
        // size, which is why it is here and not folded into `min_seen`.
        //
        // A thousand observations from three days is a large sample of three
        // days. It says nothing about a Saturday, and the question being
        // asked -- when are you reliably not about -- is a question about the
        // shape of a week. `min_seen` below then asks the separate question
        // of whether there is enough to measure a spread from at all.
        //
        // Two gates because they are two failures: too few days is a
        // confident answer about a week nobody watched, and too few
        // observations is arithmetic on noise.
        if self.days_watched < 14 {
            return false;
        }
        // Measured against your own hours rather than against a number
        // chosen here.
        //
        // The first version of this said `count < (total / 24) / 2` -- an
        // invented threshold. It had no baseline behind it and no account of
        // how much your hours actually vary, so on a life with a flat
        // routine it called ordinary hours quiet, and on a ragged one it
        // called nothing quiet at all.
        //
        // `judgment::how_unusual` asks the defensible question instead: how
        // far below an ordinary hour is this one, counted in the variations
        // your own week actually shows. `None` when there is not enough
        // behind it, which lands on "not quiet" -- the cautious direction,
        // because not-quiet is what stops Atlas starting work.
        let counts: Vec<f64> = self.active_by_hour.iter().map(|n| *n as f64).collect();
        // The median and the mean distance from it, from one place --
        // `judgment::ordinary_for_counts` -- rather than worked out here.
        // The counts form, because these are counts: an hourly week where
        // every working hour has exactly the same number in it has no
        // observed spread at all, and without a floor one quiet hour then
        // reads as infinitely unusual. The first
        // version of this centred on the mean, and an hourly week is about as
        // skewed as data gets: fifteen busy hours and nine empty ones drag
        // the mean down into the gap between them, so a busy hour reads as
        // only mildly above ordinary and a dead hour as only mildly below.
        let Some((usually, varies_by)) = crate::judgment::ordinary_for_counts(&counts) else {
            return false;
        };
        let m = crate::judgment::Measured {
            name: "this hour",
            value: counts[hour as usize],
            usually,
            varies_by,
            // Observations, not days. `min_seen` asks whether there is
            // enough behind the mean and the spread to measure them from,
            // and what they are measured from is every time Atlas saw you do
            // something -- which is this sum. `days_watched` answers the
            // different question above, about covering a whole week.
            //
            // Passing days here was the first version and it was wrong twice
            // over: 30 days is a month before Atlas would work overnight at
            // all, and it is not the quantity the spread rests on.
            seen: counts.iter().sum::<f64>() as usize,
        };
        match crate::judgment::how_unusual(&m, cfg) {
            // Below by more than an ordinary variation. Said in the
            // baseline's own units, so it means the same thing for somebody
            // who works twelve-hour days and somebody who does not.
            Some(u) => u <= -1.0,
            None => false,
        }
    }

    /// What Atlas uses, falling back to something sensible.
    pub fn rolls_at(&self, cfg: &DailyConfig) -> u32 {
        self.quiet_hour().unwrap_or(cfg.rolls_at_hour)
    }

    /// Said when it works it out, and again only when your hours have moved
    /// (see `worth_saying_again`).
    pub fn noticed(&self) -> Option<String> {
        let h = self.quiet_hour()?;
        Some(format!(
            "You're usually done by {h}:00 lately, so that's when I'll turn the list over. It keeps \
             adjusting as your hours change — nothing to set."
        ))
    }
}

/// Which day a moment belongs to.
///
/// Not simply the calendar day: someone working until two in the morning is
/// still on yesterday's list, and telling them otherwise is pedantry.
pub fn day_of(at: u64, cfg: &DailyConfig) -> u64 {
    day_of_with(at, cfg.rolls_at_hour)
}

/// The same, with a rollover hour Atlas worked out.
///
/// On this machine's clock: the day turns at `rolls_at_hour` *your* time, not
/// UTC's. Returns the real moment of the local midnight that starts that day.
pub fn day_of_with(at: u64, rolls_at_hour: u32) -> u64 {
    let shifted = at.saturating_sub(rolls_at_hour as u64 * 3600);
    crate::localclock::midnight(shifted, crate::localclock::offset_secs())
}

/// Has the day turned since this was last looked at?
pub fn has_rolled(last_seen: u64, now: u64, cfg: &DailyConfig) -> bool {
    day_of(last_seen, cfg) != day_of(now, cfg)
}

/// What this turn is: the start of a day, or the middle of a night.
///
/// Atlas had no way to tell these apart, and the difference is the whole of
/// whether the morning brief is welcome or an interruption.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrival {
    /// You have not stopped. Whatever the clock says, this is the same
    /// stretch of work — four hours into a night is not a new morning, even
    /// when the date changed two of those hours ago.
    StillGoing,
    /// You went away and came back, and the day has turned since the last
    /// time Atlas caught you up. This is the one that earns a brief.
    Starting,
    /// You went away and came back, and you have already had today's.
    BackAgain,
}

/// Whether you are here, briefly gone, or properly gone.
///
/// `Arrival` answers "is this turn the start of a day", which needs you to
/// *be here* to have a turn at all. This answers the other question, and it
/// is the one that decides whether Atlas may get on with something: **are you
/// gone, and gone long enough that starting an hour of work will not be
/// interrupted?**
///
/// Two kinds of gone, told apart because they are different permissions:
///
/// * **Asleep** — a gap that matches the quiet stretch `Rhythm` worked out
///   from when you actually stop. Long, predictable, and ending at an hour
///   Atlas can estimate.
/// * **Out** — a gap in hours you are normally active. A workday. Also long,
///   but it can end at any moment, because leaving the house is not a thing
///   you announce.
///
/// Nothing here asks you to declare anything. The whole point is that being
/// told "I'm stepping away" is a thing people do not do, and a system that
/// needs it does nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Whereabouts {
    /// You have done something recently enough that you are at the machine.
    Here,
    /// Long enough to start something, and it is your quiet stretch.
    Asleep,
    /// Long enough to start something, in hours you are usually about.
    Out,
}

impl Whereabouts {
    /// May Atlas get on with something long?
    pub fn free_to_work(&self) -> bool {
        matches!(self, Whereabouts::Asleep | Whereabouts::Out)
    }

    /// How it would say where you are.
    pub fn plain(&self) -> &'static str {
        match self {
            Whereabouts::Here => "you're about",
            Whereabouts::Asleep => "overnight",
            Whereabouts::Out => "while you were out",
        }
    }
}

/// Where you are, from when you were last here and what your hours are.
///
/// `overnight::in_window` was `start_hour`/`stop_hour` — a fixed clock
/// window, and exactly the mistake `brief.at_hour` was. It cannot tell you
/// asleep from you at a desk at two in the morning, and it has no idea you
/// left for work at eight. So the night's work ran on a timetable rather than
/// on whether you were there, which means it ran while you were working and
/// did not run on the Saturday you were out all day.
///
/// The gap is measured against `away_a_while_minutes` — longer than
/// `back_after_minutes`, because *coming back* and *being safely gone* are
/// different questions. A ninety-minute gap means you left; it does not mean
/// Atlas should start an hour of work you will interrupt in five minutes.
///
/// Which kind of gone is decided by your own hours, not by the clock: if
/// right now is inside the quiet stretch `Rhythm` found, you are asleep. It
/// falls back to `Out` when there is no rhythm yet, which is the cautious
/// direction — `Out` can end at any moment and nothing here assumes
/// otherwise.
pub fn whereabouts(
    last_turn: u64,
    now: u64,
    rhythm: &Rhythm,
    cfg: &DailyConfig,
    jcfg: &crate::judgment::JudgmentConfig,
) -> Whereabouts {
    // Never seen you. Not "asleep" -- unknown, and `Here` is the answer that
    // starts nothing.
    if last_turn == 0 {
        return Whereabouts::Here;
    }
    let gone_for = now.saturating_sub(last_turn);
    if gone_for < cfg.away_a_while_minutes as u64 * 60 {
        return Whereabouts::Here;
    }
    let hour = crate::localclock::hour_here(now) as u32;
    if rhythm.quiet_at(hour, jcfg) {
        Whereabouts::Asleep
    } else {
        Whereabouts::Out
    }
}

/// Whether this turn is you starting a day.
///
/// Two conditions, and both are needed:
///
/// 1. **A real gap.** `back_after_minutes` since your last turn. Without
///    this, the day rolling at four in the morning hands a brief to somebody
///    who never went to bed.
/// 2. **A day that has turned**, measured against the last brief rather than
///    the calendar, and by `rolls_at` — the middle of *your* longest quiet
///    stretch, which `Rhythm` works out from when you actually stop. A night
///    owl's day ends at four; nobody has to say so.
///
/// The case worth following through: you work eleven until six, straight
/// through the four o'clock rollover. At 4am this says `StillGoing`, so no
/// brief. You sleep, and come back at two in the afternoon — a gap, and a day
/// that turned at four — so it says `Starting` and you get the brief then.
/// The brief is held rather than missed, which is the same rule
/// `Daemon::morning_brief` already keeps for the night's work.
pub fn arriving(
    last_turn: u64,
    last_brief: u64,
    now: u64,
    rolls_at: u32,
    cfg: &DailyConfig,
) -> Arrival {
    // A first ever turn has no previous one to be a gap from, and is
    // unambiguously an arrival.
    let been_away = last_turn == 0
        || now.saturating_sub(last_turn) >= cfg.back_after_minutes as u64 * 60;
    if !been_away {
        return Arrival::StillGoing;
    }
    if last_brief != 0 && day_of_with(last_brief, rolls_at) == day_of_with(now, rolls_at) {
        return Arrival::BackAgain;
    }
    Arrival::Starting
}

/// Close a day and carry what's left.
///
/// Returns the record, and how many days each carried thing has now been
/// carried.
pub fn close(
    items: &[crate::workspace_view::Item],
    day: u64,
    carried_so_far: &[(String, u32)],
    cfg: &DailyConfig,
) -> (Closed, Vec<(String, Carried)>) {
    use crate::workspace_view::Status;

    let end = day + 86_400;
    let closed = Closed {
        day,
        finished: items
            .iter()
            .filter(|i| i.closed_at.map(|c| c >= day && c < end).unwrap_or(false))
            .filter(|i| i.status == Status::Done)
            .map(|i| i.title.clone())
            .collect(),
        carried: if cfg.carry_forward {
            items
                .iter()
                .filter(|i| i.status.live())
                .map(|i| i.title.clone())
                .collect()
        } else {
            Vec::new()
        },
        dropped: items
            .iter()
            .filter(|i| i.status == Status::Dropped)
            .map(|i| i.title.clone())
            .collect(),
    };

    let counts = closed
        .carried
        .iter()
        .map(|title| {
            let before = carried_so_far
                .iter()
                .find(|(t, _)| t == title)
                .map(|(_, n)| *n)
                .unwrap_or(0);
            (title.clone(), Carried(before + 1))
        })
        .collect();

    (closed, counts)
}

/// What Atlas says when the day turns.
///
/// Short. This arrives when you sit down and a paragraph about yesterday is
/// the wrong thing to read first.
pub fn opening(yesterday: &Closed, carried: &[(String, Carried)], cfg: &DailyConfig) -> String {
    let mut s = if yesterday.finished.is_empty() {
        String::new()
    } else {
        format!("{} finished yesterday. ", yesterday.finished.len())
    };

    if carried.is_empty() {
        s.push_str("Clean list today.");
        return s;
    }
    s.push_str(&format!("{} carried over.", carried.len()));

    // The one that's been moving for a week is worth a sentence; the rest
    // aren't.
    if let Some((title, c)) = carried
        .iter()
        .filter(|(_, c)| c.worth_mentioning(cfg.ask_after_days))
        .max_by_key(|(_, c)| c.0)
    {
        if let Some(n) = c.nudge(title, cfg.ask_after_days) {
            s.push(' ');
            s.push_str(&n);
        }
    }
    s
}

/// Everything a carried task brings with it.
///
/// The point of the whole module: closing the day does not close the thread.
pub fn thread<'a>(
    item: &'a crate::workspace_view::Item,
    carried: Carried,
) -> (Carried, &'a [crate::workspace_view::Thought]) {
    (carried, &item.thinking)
}

/// Things you said you weren't doing.
///
/// Kept rather than deleted, because "I dropped that, what was it" is a real
/// question and a deleted thing has no answer. Also because dropping
/// something and picking it up three weeks later is normal, and having to
/// retype it is the reason people don't drop things in the first place.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Dropped {
    pub title: String,
    pub when: u64,
    /// How many days it had been carried before you gave up on it.
    pub carried_for: u32,
    /// The project or client it belonged to.
    pub about: Option<String>,
    /// What Atlas had been thinking, kept with it.
    pub thinking: Vec<crate::workspace_view::Thought>,
}

impl Dropped {
    /// Something dropped after being carried a long time was probably a bad
    /// task rather than a bad week, and that's worth remembering when it
    /// comes back.
    pub fn was_a_slog(&self) -> bool {
        self.carried_for >= 5
    }
}

/// Find one again from anything you remember about it.
pub fn find_dropped<'a>(dropped: &'a [Dropped], asked: &str) -> Vec<&'a Dropped> {
    // Common words match everything, so searching for "the fee thing" would
    // return every dropped item on the strength of "the".
    const NOISE: &[&str] = &[
        "the", "that", "this", "thing", "about", "one", "and", "for", "with",
        "was", "were", "from", "what", "when", "some", "any", "old",
    ];
    let words: Vec<String> = asked
        .to_lowercase()
        .split_whitespace()
        .filter(|w| w.len() > 2 && !NOISE.contains(w))
        .map(str::to_string)
        .collect();

    let mut scored: Vec<(usize, &Dropped)> = dropped
        .iter()
        .map(|d| {
            let hay = format!("{} {}", d.title, d.about.clone().unwrap_or_default()).to_lowercase();
            (words.iter().filter(|w| hay.contains(w.as_str())).count(), d)
        })
        .filter(|(n, _)| *n > 0)
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.when.cmp(&a.1.when)));
    scored.into_iter().take(5).map(|(_, d)| d).collect()
}

/// Bringing one back.
pub fn picking_back_up(d: &Dropped) -> String {
    let mut s = format!("\"{}\" — you dropped it", d.title);
    if let Some(a) = &d.about {
        s.push_str(&format!(" from {a}"));
    }
    s.push('.');
    if d.was_a_slog() {
        // Worth saying, because the reason it was dropped hasn't changed.
        s.push_str(&format!(
            " It had been on the list {} days before that, so it might want to be smaller this \
             time.",
            d.carried_for
        ));
    }
    if !d.thinking.is_empty() {
        s.push_str(&format!(" I've still got {} notes on it.", d.thinking.len()));
    }
    s
}

/// Whether a closed day is worth keeping.
pub fn still_keep(c: &Closed, now: u64, cfg: &DailyConfig) -> bool {
    now.saturating_sub(c.day) < cfg.keep_days as u64 * 86_400
}

/// Is a new quiet hour worth saying, against the last one said? Only a move
/// of two hours or more (round the clock), and not more than once a week:
/// random hours shift it a little all the time, and that isn't news.
pub fn worth_saying_again(now_hour: u32, said: Option<(u32, u64)>, t: u64) -> bool {
    match said {
        None => true,
        Some((h, at)) => {
            let d = (now_hour as i32 - h as i32).rem_euclid(24);
            let apart = d.min(24 - d);
            apart >= 2 && t.saturating_sub(at) >= 7 * 86_400
        }
    }
}
