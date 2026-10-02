//! Pushing you toward progress.
//!
//! `proactive` answers "may I speak?". This answers "is there anything worth
//! saying?" — and unlike every other detector in the system, these are not
//! triggered by something you did. Nothing here is a reaction. That is the
//! whole point, and it is also the whole danger.
//!
//! The failure mode is not that Atlas says something useless once. It is that
//! it says something useless four times, you mute it, and then it is worth
//! less than nothing because you have stopped listening to the one channel
//! that would have told you something real. So every nudge here is built
//! around three rules:
//!
//! 1. **A nudge that cannot offer to take work is a nag.** Every nudge carries
//!    a `relief` — the specific thing Atlas will do instead of you. "You've
//!    slowed down" is a nag. "You've slowed down, I can take the invoice
//!    chase off you" is help.
//! 2. **Silence backs off, it does not repeat.** Ignoring a nudge widens the
//!    interval. It never shortens it.
//! 3. **Asking why is a one-shot.** After enough silence Atlas may ask, once,
//!    what it got wrong — because the answer is worth more than the nudge was.
//!    It may never ask twice about the same subject. Asking repeatedly why you
//!    are ignoring something is the purest form of nagging there is.

use serde::{Deserialize, Serialize};

/// What sets a nudge off. Each is a genuinely different situation and gets a
/// different tone — a stalled goal is not the same problem as a slow afternoon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Trigger {
    /// A thing you said you wanted has not moved in a while.
    Stalled,
    /// Right now: on one window a long time, saying nothing, getting nowhere.
    Drifting,
    /// Morning, afternoon or evening. The one nudge that is allowed to be
    /// routine, because a predictable check-in is not an interruption.
    Daypart,
    /// No signal at all. Occasionally, just asking.
    CheckIn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Part {
    Morning,
    Afternoon,
    Evening,
}

impl Part {
    pub fn from_hour(h: u8) -> Option<Part> {
        match h {
            5..=11 => Some(Part::Morning),
            12..=17 => Some(Part::Afternoon),
            18..=22 => Some(Part::Evening),
            // Small hours get nothing. If you are up at 3am Atlas is not
            // going to be the thing that comments on it.
            _ => None,
        }
    }

    pub fn greeting(&self) -> &'static str {
        match self {
            Part::Morning => "Good morning",
            Part::Afternoon => "Good afternoon",
            Part::Evening => "Good evening",
        }
    }
}

/// How you answered. `Ignored` is not `Declined` — saying no is information,
/// saying nothing is a hint that the nudge was badly aimed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Response {
    Accepted,
    Declined,
    Ignored,
}

/// Something you want to happen, that Atlas watches for movement on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Goal {
    pub id: String,
    /// In your words, so it can be quoted back rather than paraphrased.
    pub what: String,
    /// Last time anything happened on this.
    pub last_movement: u64,
    /// What Atlas could take over, if anything. `None` means it can only
    /// point — and a nudge that can only point has to clear a higher bar.
    pub relief: Option<String>,
    /// Consecutive nudges about this that got no answer at all.
    #[serde(default)]
    pub ignored: u32,
    /// Atlas has already asked once what it was getting wrong here.
    #[serde(default)]
    pub asked_why: bool,
    /// You said stop. Ends it permanently.
    #[serde(default)]
    pub muted: bool,
}

impl Goal {
    pub fn new(id: &str, what: &str, last_movement: u64) -> Goal {
        Goal {
            id: id.into(),
            what: what.into(),
            last_movement,
            relief: None,
            ignored: 0,
            asked_why: false,
            muted: false,
        }
    }

    pub fn offering(mut self, relief: &str) -> Goal {
        self.relief = Some(relief.into());
        self
    }
}

/// A single thing Atlas wants to say, unprompted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Nudge {
    pub trigger: Trigger,
    /// Which goal this is about, where there is one.
    pub subject: Option<String>,
    pub message: String,
    /// The work Atlas is offering to absorb. Present on every nudge that
    /// names a problem — see rule 1.
    pub relief: Option<String>,
    /// This is the one-shot "what am I getting wrong?" nudge.
    pub asking_why: bool,
    pub confidence: f32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct NudgeConfig {
    /// A goal is stalled after this long with no movement.
    pub stalled_after_secs: u64,
    /// Dwelling this long with nothing happening reads as drift.
    pub drift_dwell_secs: u64,
    /// Silences before Atlas is allowed its single "what am I missing?".
    pub ask_why_after: u32,
    /// Silences before a goal stops being raised at all.
    pub give_up_after: u32,
    /// Each ignored nudge multiplies the wait before the next one.
    pub backoff: f32,
    /// Nudges about habits and health at all.
    pub personal: bool,
}

fn d_stalled() -> u64 { 3 * 24 * 3600 }

impl Default for NudgeConfig {
    fn default() -> Self {
        NudgeConfig {
            stalled_after_secs: d_stalled(),
            drift_dwell_secs: 2700,
            ask_why_after: 3,
            give_up_after: 6,
            backoff: 2.0,
            personal: false,
        }
    }
}

/// Subjects Atlas will not nudge about, whatever the config says.
///
/// You asked for habits and health to be in scope and they are — Atlas can
/// tell you that you have not stood up in four hours, or that you said you
/// wanted to be off the machine by seven and it is nine.
///
/// What it must never do is cross from *your stated habits* into *medicine*.
/// A local assistant reading a symptom and forming a view is not a doctor with
/// bad handwriting, it is a confident stranger. The distinction is that Atlas
/// pushes on the behaviour you told it you wanted, and says nothing about what
/// any of it might mean.
///
/// Enforced as a `const` rather than a config field, which is why there is no
/// `serde(skip)` here to point at: there is no field at all. `personal` turns
/// the whole personal category off. Nothing turns this part back on, because
/// there is nothing to turn. `tests/guards.rs` fails the build if either this
/// list or `is_medical` stops existing.
pub const NEVER_NUDGES_ABOUT: &[&str] = &[
    "symptom",
    "diagnos",
    "medication",
    "dose",
    "dosage",
    "prescription",
    "blood pressure",
    "blood sugar",
    "weight",
    "calorie",
    "test result",
    "scan",
];

/// True if this subject is out of bounds regardless of settings.
pub fn is_medical(subject: &str) -> bool {
    let s = subject.to_lowercase();
    NEVER_NUDGES_ABOUT.iter().any(|w| s.contains(w))
}

/// Where your goals are kept.
pub const GOALS: &str = "goals";

/// What a goal said out loud comes down to: the words after the phrase.
pub fn goal_words(said: &str) -> String {
    let t = said.trim().trim_end_matches(['.', '!']);
    let lower = t.to_lowercase();
    for p in ["my goal is to", "my goal is", "set a goal to", "set a goal", "new goal", "i worked on my goal",
              "made progress on", "i worked on", "drop the goal", "forget the goal", "i'm done with the goal",
              "im done with the goal"] {
        if let Some(i) = lower.find(p) {
            return t[i + p.len()..].trim().trim_start_matches([':', '-', ',']).trim().to_string();
        }
    }
    t.to_string()
}

/// Which goal someone means: the one sharing the most words with what they
/// said.
pub fn which_goal<'a>(goals: &'a [Goal], words: &str) -> Option<&'a Goal> {
    let said: Vec<String> = words.to_lowercase().split_whitespace().filter(|w| w.len() > 2).map(str::to_string).collect();
    goals
        .iter()
        .filter(|g| !g.muted)
        .map(|g| {
            let hits = said.iter().filter(|w| g.what.to_lowercase().contains(w.as_str())).count();
            (g, hits)
        })
        .filter(|(_, h)| *h > 0)
        .max_by_key(|(_, h)| *h)
        .map(|(g, _)| g)
}

#[derive(Debug, Default)]
pub struct Nudger {
    pub cfg: NudgeConfig,
    pub goals: Vec<Goal>,
    /// Last daypart greeted, so a morning brief happens once per morning.
    last_part: Option<(u64, Part)>,
}

impl Nudger {
    pub fn new(cfg: NudgeConfig) -> Nudger {
        Nudger { cfg, goals: Vec::new(), last_part: None }
    }

    /// The part of the day last greeted, kept across a restart: on 30 Sep
    /// "Good evening. Nothing outstanding on my side" was said six times in
    /// one evening, once after every restart, because this started empty.
    pub fn last_daypart(&self) -> Option<(u64, Part)> {
        self.last_part
    }

    /// Put back the part of the day already greeted (from the store).
    pub fn set_last_daypart(&mut self, p: Option<(u64, Part)>) {
        self.last_part = p;
    }

    pub fn track(&mut self, g: Goal) {
        self.goals.retain(|x| x.id != g.id);
        self.goals.push(g);
    }

    /// Something happened on this goal. Resets the clock and, importantly, the
    /// silence count — movement is the answer Atlas was hoping for, so it
    /// should not still be treating you as unresponsive afterwards.
    pub fn moved(&mut self, id: &str, t: u64) {
        if let Some(g) = self.goals.iter_mut().find(|g| g.id == id) {
            g.last_movement = t;
            g.ignored = 0;
        }
    }

    /// How long to wait before raising this goal again. Grows with every
    /// silence and never shrinks.
    pub fn wait_for(&self, g: &Goal) -> u64 {
        let mult = self.cfg.backoff.powi(g.ignored as i32);
        (self.cfg.stalled_after_secs as f32 * mult) as u64
    }

    /// The one nudge most worth making right now, if any.
    ///
    /// `dwell_secs` and `idle_secs` come from `awareness::Signals`. `hour` is
    /// local wall-clock. Ordering is deliberate: a stalled commitment beats a
    /// slow afternoon beats a greeting beats nothing.
    pub fn consider(&mut self, t: u64, hour: u8, dwell_secs: u64, idle_secs: u64) -> Option<Nudge> {
        if let Some(n) = self.stalled(t) {
            return Some(n);
        }
        if let Some(n) = self.drifting(dwell_secs, idle_secs) {
            return Some(n);
        }
        self.daypart(t, hour)
    }

    fn stalled(&mut self, t: u64) -> Option<Nudge> {
        let cfg_give_up = self.cfg.give_up_after;
        let cfg_ask_why = self.cfg.ask_why_after;
        let personal = self.cfg.personal;
        let waits: Vec<u64> = self.goals.iter().map(|g| self.wait_for(g)).collect();

        let mut pick: Option<usize> = None;
        for (i, g) in self.goals.iter().enumerate() {
            if g.muted || g.ignored >= cfg_give_up {
                continue;
            }
            if is_medical(&g.what) {
                continue;
            }
            if !personal && g.id.starts_with("personal:") {
                continue;
            }
            if t.saturating_sub(g.last_movement) < waits[i] {
                continue;
            }
            // Oldest stall first — the thing that has been sitting longest is
            // the thing most likely to be quietly dead.
            let better = pick.map(|p| g.last_movement < self.goals[p].last_movement);
            if better.unwrap_or(true) {
                pick = Some(i);
            }
        }

        let i = pick?;
        let g = &mut self.goals[i];
        let days = t.saturating_sub(g.last_movement) / 86_400;

        // The one-shot. Once only, and only after real silence.
        if g.ignored >= cfg_ask_why && !g.asked_why {
            g.asked_why = true;
            return Some(Nudge {
                trigger: Trigger::Stalled,
                subject: Some(g.id.clone()),
                message: format!(
                    "I've raised {} a few times and I don't think I'm being useful about it. \
                     What am I getting wrong — is it dead, or is it just not now?",
                    g.what
                ),
                relief: g.relief.clone(),
                asking_why: true,
                confidence: 0.9,
            });
        }

        let message = match (&g.relief, days) {
            (Some(r), 0) => format!("{} has gone quiet. I can take {r} if that helps.", g.what),
            (Some(r), d) => format!(
                "{} hasn't moved in {d} days. I can take {r} off you so you can focus on the rest.",
                g.what
            ),
            (None, 0) => format!("{} has gone quiet. Still worth doing?", g.what),
            (None, d) => format!("{} hasn't moved in {d} days. Still worth doing?", g.what),
        };

        Some(Nudge {
            trigger: Trigger::Stalled,
            subject: Some(g.id.clone()),
            message,
            relief: g.relief.clone(),
            asking_why: false,
            // A nudge that can only point has to be more certain than one that
            // comes with an offer of help.
            confidence: if g.relief.is_some() { 0.82 } else { 0.74 },
        })
    }

    fn drifting(&self, dwell_secs: u64, idle_secs: u64) -> Option<Nudge> {
        if dwell_secs < self.cfg.drift_dwell_secs || idle_secs < 600 {
            return None;
        }
        // Only offer relief that actually exists.
        let relief = self
            .goals
            .iter()
            .find(|g| !g.muted && g.relief.is_some())
            .and_then(|g| g.relief.clone());
        let message = match &relief {
            Some(r) => format!("Looks like we've slowed down. I can take {r} off your plate so you can focus."),
            None => "Looks like we've slowed down. Want to talk through what's next?".into(),
        };
        Some(Nudge {
            trigger: Trigger::Drifting,
            subject: None,
            message,
            relief,
            asking_why: false,
            confidence: 0.7,
        })
    }

    fn daypart(&mut self, t: u64, hour: u8) -> Option<Nudge> {
        let part = Part::from_hour(hour)?;
        let day = crate::localclock::day_here(t) as u64;
        if self.last_part == Some((day, part)) {
            return None;
        }
        self.last_part = Some((day, part));
        let open = self.goals.iter().filter(|g| !g.muted).count();
        let message = match open {
            0 => format!("{}. Nothing outstanding on my side — what do you want to start on?", part.greeting()),
            1 => format!("{}. One thing open. Want the brief?", part.greeting()),
            n => format!(
                "{}. There's a fair bit on — {n} things open. Let's start with a brief. Where would you like to start?",
                part.greeting()
            ),
        };
        Some(Nudge {
            trigger: Trigger::Daypart,
            subject: None,
            message,
            relief: None,
            asking_why: false,
            confidence: 0.8,
        })
    }

    /// The nudge `consider` returned was not said — another offer on the same
    /// tick was chosen instead (`proactive::consider_with`). Undo what
    /// returning it marked as done: the once-only "what am I getting wrong"
    /// question is still unasked, and the part-of-day greeting still ungiven.
    pub fn unsaid(&mut self, n: &Nudge) {
        if n.asking_why {
            if let Some(g) = self.goals.iter_mut().find(|g| Some(&g.id) == n.subject.as_ref()) {
                g.asked_why = false;
            }
        }
        if n.trigger == Trigger::Daypart {
            self.last_part = None;
        }
    }

    /// Record how you answered. This is the only thing that changes how often
    /// Atlas speaks, and it can only ever make it quieter or reset it.
    /// May Atlas raise this subject right now?
    ///
    /// Exists so a nudge built outside `consider` — `link_broke` is the first —
    /// still obeys the rules Eric set for every other nudge, rather than
    /// getting its own private path that ignores them. Speaking directly from
    /// the daemon without this would mean a failing connection re-announced
    /// itself every tick, and a "no" would not stick.
    ///
    /// A subject nobody has raised before is allowed through: the back-off
    /// curve is about repetition, and there has not been any yet.
    pub fn may_raise(&self, subject: &str, t: u64) -> bool {
        match self.goals.iter().find(|g| g.id == subject) {
            None => true,
            Some(g) => !g.muted && t.saturating_sub(g.last_movement) >= self.wait_for(g),
        }
    }

    /// Atlas has just raised this subject. Counts as unanswered until
    /// `record` says otherwise, so silence widens the gap before the next one.
    pub fn raised(&mut self, subject: &str, what: &str, t: u64) {
        match self.goals.iter_mut().find(|g| g.id == subject) {
            Some(g) => {
                g.last_movement = t;
                g.ignored = g.ignored.saturating_add(1);
            }
            None => self.goals.push(Goal::new(subject, what, t)),
        }
    }

    pub fn record(&mut self, subject: Option<&str>, r: Response, t: u64) {
        let Some(id) = subject else { return };
        let Some(g) = self.goals.iter_mut().find(|g| g.id == id) else { return };
        match r {
            Response::Accepted => {
                g.ignored = 0;
                g.last_movement = t;
            }
            // Saying no is a clear answer and deserves to be believed the
            // first time. It is not silence and does not accumulate.
            Response::Declined => {
                g.muted = true;
            }
            Response::Ignored => {
                g.ignored = g.ignored.saturating_add(1);
            }
        }
    }
}

/// The morning nudge, with the day's brief already run.
///
/// The daypart greeting on its own says "there is a lot on". That is a
/// notification. With the brief attached it says what to start with, which is
/// the difference between being told you are busy and being helped.
/// Takes a brief that has already been run, which is what its own first line
/// always said it did. The first version took raw items and ran the brief
/// itself, and when it finally got a caller that turned into running the same
/// brief twice a tick — once to find out whether there was anything worth
/// greeting you about, and again in here to say it. Worse than wasteful: the
/// second run was handed the output of the first, so `drafted` was always
/// empty by the time the relief was read off it, and the morning greeting
/// could never offer the one thing it is built to offer.
pub fn daypart_with_brief(part: Part, b: &crate::brief::Brief) -> Nudge {
    Nudge {
        trigger: Trigger::Daypart,
        subject: None,
        message: format!("{}. {}", part.greeting(), crate::brief::spoken(&b)),
        relief: b.drafted.first().map(|i| format!("the reply to {}", i.from)),
        asking_why: false,
        confidence: 0.85,
    }
}

/// Put a decision to the room instead of answering it alone.
///
/// Reached from here rather than from its own command because the moment a
/// council is worth convening is exactly when Atlas was about to nudge you
/// about something it cannot settle by itself.
pub fn convene(question: &str) -> (crate::council::Council, Vec<(String, String)>) {
    convene_with(crate::council::Council::default_room(), question)
}

/// Convene a specific room rather than the general one.
///
/// The room that matters here is `council::hardware_room`, whose seats have
/// been handed the measured machine — because five seats arguing about "should
/// I upgrade" in the abstract is five opinions, and five seats arguing about
/// 15.7GB with no discrete graphics is a decision.
pub fn convene_with(
    c: crate::council::Council,
    question: &str,
) -> (crate::council::Council, Vec<(String, String)>) {
    let prompts = c.blind_prompts(question);
    (c, prompts)
}

/// A correction you have now made twice, offered as a change to Atlas itself.
///
/// This is a nudge rather than a silent write on purpose. Atlas changing its
/// own instructions without saying so is the one kind of self-improvement you
/// cannot audit, so it goes through the same door as everything else it says
/// first — and can be refused like anything else.
pub fn offer_to_mend(e: &crate::revise::Edit) -> Nudge {
    Nudge {
        trigger: Trigger::CheckIn,
        subject: Some(e.about.clone()),
        message: crate::revise::proposal(e),
        relief: Some(format!("updating {}", e.home.plain())),
        asking_why: false,
        // High, because you said it twice. Not certain, because Atlas is
        // proposing to rewrite its own instructions and that always asks.
        confidence: 0.88,
    }
}

/// Boot knowing the map rather than carrying the territory.
///
/// This used to say it was reached from the daypart brief. It was not reached
/// from anywhere, and when it was finally given callers the daypart greeting
/// turned out to be the wrong home: a morning greeting that recites an
/// inventory count every day is a notification, which is the thing this whole
/// module is written against. It is reached instead from the two moments
/// where the count is an answer rather than an announcement — being asked
/// what Atlas has written down, and having just rebuilt the index.
pub fn what_i_know_of(master: &crate::contents::Contents) -> String {
    crate::contents::boot(master).plain()
}

/// An index that has drifted from its folder, raised as a nudge.
///
/// A wrong index is worse than no index, because Atlas trusts it and stops
/// looking — so this is a problem to be told about rather than a tidy-up to
/// run quietly.
pub fn drifted(d: &crate::contents::Drift) -> Option<Nudge> {
    if d.is_clean() {
        return None;
    }
    Some(Nudge {
        trigger: Trigger::CheckIn,
        subject: Some("index".into()),
        message: format!("{} I can rebuild the index if you like.", d.plain()),
        relief: Some("rebuilding the index".into()),
        asking_why: false,
        confidence: 0.8,
    })
}

/// What the model has been doing, when you ask.
pub fn trace_line(t: &crate::trace::Trace) -> String {
    t.spoken()
}

/// A connection that just stopped working, said out loud.
///
/// The failure this prevents is not an outage — it is weeks of subtly worse
/// answers from a source that detached and never mentioned it. A break has to
/// interrupt, because the alternative is a dashboard nobody opens.
pub fn link_broke(i: &crate::integrations::Integration) -> Nudge {
    Nudge {
        trigger: Trigger::CheckIn,
        subject: Some(format!("integration:{}", i.name)),
        message: format!(
            "{} has stopped working. That means {} until it's back.",
            i.name, i.if_it_breaks
        ),
        relief: Some(format!("working around {} for now", i.name)),
        asking_why: false,
        // Certain, because this is a measurement rather than a judgement.
        confidence: 0.95,
    }
}
