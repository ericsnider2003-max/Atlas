//! What Atlas is doing right now, and why.
//!
//! Two things this makes possible. You can ask what it's working on and get a
//! real answer rather than "working on it". And you can watch the reasoning as
//! it happens, which is the difference between a system you trust and one you
//! hope about.
//!
//! It's also the thing that lets you talk to Atlas mid-task without stopping
//! the task. Background work has its own thread of thought; a question from
//! you is a separate one, and neither disturbs the other unless you say so.

use serde::{Deserialize, Serialize};

/// A stage of working on something. Deliberately the words a person would
/// use — this gets read aloud and displayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// Working out what the task actually needs before touching anything.
    Gathering,
    /// Deciding how to do it.
    Planning,
    Doing,
    /// Checking it worked.
    Verifying,
    /// Didn't work; deciding what to try instead.
    Rethinking,
    /// Waiting on something outside its control.
    Waiting,
    Done,
    Stuck,
}

impl Stage {
    pub fn label(&self) -> &'static str {
        match self {
            Stage::Gathering => "gathering context",
            Stage::Planning => "working out how",
            Stage::Doing => "doing it",
            Stage::Verifying => "checking it worked",
            Stage::Rethinking => "trying another way",
            Stage::Waiting => "waiting",
            Stage::Done => "done",
            Stage::Stuck => "stuck",
        }
    }
    pub fn finished(&self) -> bool {
        matches!(self, Stage::Done | Stage::Stuck)
    }
}

/// One line of reasoning, as it happened.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Thought {
    pub at: u64,
    pub stage: Stage,
    /// Plain language. Not a log line.
    pub text: String,
}

/// Something Atlas is working on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Work {
    pub id: u64,
    /// What you asked for, in your words.
    pub asked: String,
    pub stage: Stage,
    /// Steps it decided on, and which are done.
    pub steps: Vec<Step>,
    pub thoughts: Vec<Thought>,
    pub started: u64,
    /// Runs in the background — talking to Atlas doesn't disturb it.
    pub background: bool,
    /// Needs the screen, so it can't be pushed into the background.
    pub needs_screen: bool,
    /// What it's blocked on, if anything.
    pub blocked_on: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub what: String,
    pub done: bool,
    /// Failed, with the reason.
    pub failed: Option<String>,
}

impl Work {
    pub fn new(id: u64, asked: &str, background: bool, t: u64) -> Work {
        Work {
            id,
            asked: asked.to_string(),
            stage: Stage::Gathering,
            steps: Vec::new(),
            thoughts: Vec::new(),
            started: t,
            background,
            needs_screen: false,
            blocked_on: None,
        }
    }

    pub fn think(&mut self, stage: Stage, text: &str, t: u64) {
        self.stage = stage;
        self.thoughts.push(Thought { at: t, stage, text: text.to_string() });
        // Bounded: a long job shouldn't grow without limit, and the recent
        // thinking is the part anyone reads.
        if self.thoughts.len() > 200 {
            let drop = self.thoughts.len() - 200;
            self.thoughts.drain(0..drop);
        }
    }

    pub fn plan(&mut self, steps: &[String]) {
        self.steps = steps
            .iter()
            .map(|s| Step { what: s.clone(), done: false, failed: None })
            .collect();
    }

    pub fn finish_step(&mut self, index: usize, failed: Option<String>) {
        if let Some(s) = self.steps.get_mut(index) {
            s.done = failed.is_none();
            s.failed = failed;
        }
    }

    pub fn progress(&self) -> (usize, usize) {
        (self.steps.iter().filter(|s| s.done).count(), self.steps.len())
    }

    /// The step it's on now.
    fn current_step(&self) -> Option<&Step> {
        self.steps.iter().find(|s| !s.done && s.failed.is_none())
    }

    /// One spoken line: what it's doing and how far in.
    pub fn spoken(&self) -> String {
        if self.stage == Stage::Stuck {
            return format!(
                "Stuck on {} — {}.",
                self.asked,
                self.blocked_on.clone().unwrap_or_else(|| "not sure why yet".into())
            );
        }
        let (done, total) = self.progress();
        match (self.current_step(), total) {
            (Some(step), t) if t > 0 => {
                format!("{}: {} — step {} of {t}.", self.asked, step.what, done + 1)
            }
            _ => format!("{}: {}.", self.asked, self.stage.label()),
        }
    }

    /// The last few lines of reasoning, for when you ask why.
    pub fn recent_thinking(&self, n: usize) -> Vec<&Thought> {
        let from = self.thoughts.len().saturating_sub(n);
        self.thoughts[from..].iter().collect()
    }
}

/// Everything in flight.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Mind {
    pub work: Vec<Work>,
    next_id: u64,
}

/// What happened when you asked for something while it was already busy.
#[derive(Debug, Clone, PartialEq)]
pub enum Started {
    /// Nothing was running; it just started.
    Straight { id: u64 },
    /// The previous job carried on out of sight and the new one has the floor.
    Demoted { id: u64, moved: String },
    /// Both need the screen, so both are running.
    Alongside { id: u64, with: usize },
}

impl Started {
    pub fn id(&self) -> u64 {
        match self {
            Started::Straight { id } | Started::Demoted { id, .. } | Started::Alongside { id, .. } => *id,
        }
    }
    /// What Atlas says. Nothing at all when there was nothing to say.
    pub fn spoken(&self) -> String {
        match self {
            Started::Straight { .. } => String::new(),
            Started::Demoted { moved, .. } => format!("Carrying on with {moved} in the background."),
            Started::Alongside { with, .. } => {
                format!("Running that alongside {with} other{}.", if *with == 1 { "" } else { "s" })
            }
        }
    }
}

impl Mind {
    pub fn begin(&mut self, asked: &str, background: bool, t: u64) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.work.push(Work::new(id, asked, background, t));
        id
    }

    /// Take on something new while something else is running.
    ///
    /// You having an idea should never be limited by what Atlas happens to be
    /// doing. So anything that doesn't need the screen is pushed out of sight
    /// and carries on; anything that does runs alongside. Either way the new
    /// request starts now.
    pub fn take_on(&mut self, asked: &str, needs_screen: bool, t: u64) -> Started {
        // Demote whatever is in front, if it can carry on without the screen.
        let demotable: Vec<u64> = self
            .work
            .iter()
            .filter(|w| !w.stage.finished() && !w.background && !w.needs_screen)
            .map(|w| w.id)
            .collect();

        let mut moved: Option<String> = None;
        for id in &demotable {
            if let Some(w) = self.get_mut(*id) {
                w.background = true;
                moved = Some(w.asked.clone());
            }
        }

        // Anything still holding the screen has to share it.
        let sharing = self
            .work
            .iter()
            .filter(|w| !w.stage.finished() && !w.background)
            .count();

        self.next_id += 1;
        let id = self.next_id;
        let mut w = Work::new(id, asked, false, t);
        w.needs_screen = needs_screen;
        self.work.push(w);

        match (moved, sharing) {
            (Some(m), _) => Started::Demoted { id, moved: m },
            (None, 0) => Started::Straight { id },
            (None, n) => Started::Alongside { id, with: n },
        }
    }

    /// Bring something back to the front.
    pub fn promote(&mut self, id: u64) -> bool {
        match self.get_mut(id) {
            Some(w) => {
                w.background = false;
                true
            }
            None => false,
        }
    }

    pub fn get(&self, id: u64) -> Option<&Work> {
        self.work.iter().find(|w| w.id == id)
    }
    pub fn get_mut(&mut self, id: u64) -> Option<&mut Work> {
        self.work.iter_mut().find(|w| w.id == id)
    }

    pub fn active(&self) -> Vec<&Work> {
        self.work.iter().filter(|w| !w.stage.finished()).collect()
    }

    /// Work that keeps going while you talk to Atlas about something else.
    pub fn background(&self) -> Vec<&Work> {
        self.active().into_iter().filter(|w| w.background).collect()
    }

    /// The thing to answer "what are you doing?" with.
    ///
    /// Foreground work first — that's what you're waiting on.
    pub fn focus(&self) -> Option<&Work> {
        self.active()
            .into_iter()
            .find(|w| !w.background)
            .or_else(|| self.active().into_iter().next())
    }

    /// A short answer to "what are you working on?"
    pub fn now(&self) -> String {
        let active = self.active();
        match active.len() {
            0 => "Nothing at the moment.".into(),
            1 => active[0].spoken(),
            n => {
                let focus = self.focus().map(|w| w.spoken()).unwrap_or_default();
                format!("{focus} And {} other{} running.", n - 1, if n == 2 { "" } else { "s" })
            }
        }
    }

    /// Finished work, cleared out so the list stays about now.
    pub fn tidy(&mut self, keep: usize) {
        let finished: Vec<u64> =
            self.work.iter().filter(|w| w.stage.finished()).map(|w| w.id).collect();
        if finished.len() > keep {
            let drop: Vec<u64> = finished[..finished.len() - keep].to_vec();
            self.work.retain(|w| !drop.contains(&w.id));
        }
    }
}

// ---------- what gets said first ----------

/// How much it matters that you hear this now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Weight {
    /// Worth knowing, no hurry.
    Passing,
    /// You'll want to deal with it today.
    Soon,
    /// It's holding something up.
    Blocking,
    /// It'll cost you something if you don't hear it.
    Urgent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub what: String,
    pub weight: Weight,
    /// Why it matters, in a few words. Read out only for the top one.
    pub because: String,
}

/// Order a brief so the most important thing is said first, and say it out
/// loud rather than handing over a document.
///
/// A written brief is something you read later. Three sentences in priority
/// order is something you act on now.
pub fn speak_brief(items: &[Item]) -> String {
    if items.is_empty() {
        return "Nothing needs you.".into();
    }
    let mut sorted: Vec<&Item> = items.iter().collect();
    sorted.sort_by_key(|b| std::cmp::Reverse(b.weight));

    let top = sorted[0];
    let mut out = match top.weight {
        Weight::Urgent => format!("First thing: {}. {}.", top.what, top.because),
        Weight::Blocking => format!("{} is holding things up. {}.", top.what, top.because),
        _ => format!("{}.", top.what),
    };

    // Only the top item gets its reasoning. The rest are named so you know
    // they exist — a spoken list of six with justifications is unlistenable.
    let rest: Vec<&str> = sorted[1..].iter().take(3).map(|i| i.what.as_str()).collect();
    if !rest.is_empty() {
        out.push_str(&format!(" Then {}.", rest.join(", ")));
    }
    if sorted.len() > 4 {
        out.push_str(&format!(" {} more after that.", sorted.len() - 4));
    }
    out
}

/// "How's that going?", "is it done yet?", "what are you working on?":
/// asking after work already started. 30 Sep 2026: nothing answered these;
/// they went to the model, which can't see the background work and guessed.
pub fn asks_how_its_going(said: &str) -> bool {
    let t: String = said.to_lowercase().chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '\'' { c } else { ' ' }).collect();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ").replace('\'', "");
    const ASKS: &[&str] = &[
        "hows that going", "hows it going with", "how is that going", "hows the", "how is the research", "hows it coming",
        "how's it coming", "is it done yet", "is that done yet", "is it finished", "is that finished", "are you done yet",
        "are you finished", "whats running", "what are you working on", "what are you doing right now", "what are you up to",
        "still working on it", "any progress", "how far along", "status update", "whats the status",
    ];
    // "how's the weather" / "how's the market" are not about Atlas's work.
    if t.starts_with("hows the ") && !["research", "download", "build", "report", "letter", "search", "writing", "job", "task", "update", "backup", "sort"].iter().any(|w| t.contains(w)) {
        return false;
    }
    ASKS.iter().any(|a| t.starts_with(a) || t == *a)
}
