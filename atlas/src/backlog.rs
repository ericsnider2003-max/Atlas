//! Nothing Atlas couldn't do gets forgotten.
//!
//! When Atlas says "I can't do that right now", the request goes here with the
//! reason. Every tick it checks whether the blocker has cleared. When it has,
//! Atlas offers to pick the task back up rather than either silently doing it
//! or silently dropping it.
//!
//! The two failure modes this exists to prevent:
//!   * "I'll do it when we're back online" and then never doing it.
//!   * Asking about the same stuck task every ninety seconds until you
//!     disable the whole feature.

use crate::error::Result;
use crate::store::{now, Store};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Blocker {
    /// No internet.
    Offline,
    /// You were working; it needed the screen.
    NoScreenGap,
    /// A required program isn't installed or isn't on PATH.
    MissingTool(String),
    /// Needs your yes, and you weren't there to give it.
    NeedsApproval,
    /// Atlas tried and it failed.
    Failed(String),
    /// Atlas has no way to do this at all yet.
    Unsupported(String),
    /// You asked for it to wait until you are not at the machine.
    ///
    /// Distinct from every other blocker here: nothing stopped Atlas. You
    /// said "tonight", or "when I'm out", and being asked to wait is as real
    /// a reason as being offline. Without this, a request to defer had
    /// nowhere to live -- Atlas either did it immediately or forgot it.
    NotWhileYouAreHere,
    /// It found a problem and needs a decision only you can make.
    ///
    /// Distinct from `NeedsApproval`: that one is "may I", this one is "which
    /// of these did you mean". Approval clears when you say yes; this clears
    /// only when you choose, and until then the work around it carries on
    /// without it.
    NeedsYourDecision {
        /// The choice, phrased so it can be answered without reading code.
        question: String,
        /// What has been set aside until you answer.
        set_aside: String,
    },
}

/// Did you ask for this to wait until you are out of the way?
///
/// Read from what you actually said rather than from a separate command, on
/// the same reasoning as the rest of this module: "back up the drive tonight"
/// is one sentence, and making you say it as two ("back up the drive", then
/// "do that later") is the kind of friction that stops people using the
/// feature at all.
///
/// Deliberately narrow. These phrases mean *not now, while I am here*, and
/// nothing else in this list is close enough to catch by accident -- "later"
/// alone is not here, because "I'll look at that later" is something you say
/// about your own work, not an instruction to Atlas.
pub fn asked_to_wait(said: &str) -> Option<Blocker> {
    let t = said.to_lowercase();
    const WAIT: &[&str] = &[
        "tonight",
        "overnight",
        "when i'm out",
        "when im out",
        "when i'm away",
        "when im away",
        "while i'm out",
        "while im out",
        "while i'm away",
        "while im away",
        "when i'm not here",
        "when im not here",
        "when i'm asleep",
        "when im asleep",
        "while i'm asleep",
        "while im asleep",
        "in the night",
    ];
    WAIT.iter()
        .any(|w| t.contains(w))
        .then_some(Blocker::NotWhileYouAreHere)
}

impl Blocker {
    /// Whether this can clear on its own, or needs something from you.
    pub fn self_clearing(&self) -> bool {
        matches!(
            self,
            Blocker::Offline
                | Blocker::NoScreenGap
                | Blocker::Failed(_)
                // Clears by itself the moment you step away -- which is the
                // whole point of asking for it.
                | Blocker::NotWhileYouAreHere
        )
    }

    /// What it needs from you, for the third line of the design's honest
    /// triple on Outstanding: what it tried, what stopped it, what it needs.
    /// Plain, and only what is true of the blocker — a blocker that clears by
    /// itself says so rather than inventing a job for you.
    pub fn needs(&self) -> String {
        match self {
            Blocker::Offline => "Nothing from you — it goes the moment there's a connection.".into(),
            Blocker::NoScreenGap => "Nothing — it waits for a pause in your work, then runs.".into(),
            Blocker::MissingTool(t) => format!("{t} installed, or a go-ahead to do it another way."),
            Blocker::NeedsApproval => "Your yes.".into(),
            Blocker::Failed(_) => "Nothing yet — I'll try again, and tell you if it fails the same way.".into(),
            Blocker::Unsupported(_) => "A way to do it — it isn't built yet. Say if it matters and I'll put it on the list.".into(),
            Blocker::NotWhileYouAreHere => "Nothing — it runs when you step away.".into(),
            Blocker::NeedsYourDecision { question, .. } => question.clone(),
        }
    }

    pub fn explain(&self) -> String {
        match self {
            Blocker::Offline => "no connection".into(),
            Blocker::NoScreenGap => "you were busy".into(),
            Blocker::MissingTool(t) => format!("{t} isn't installed"),
            Blocker::NeedsApproval => "it needed your go-ahead".into(),
            Blocker::Failed(why) => format!("it failed: {why}"),
            Blocker::Unsupported(what) => format!("I can't {what} yet"),
            Blocker::NotWhileYouAreHere => "you asked me to wait until you were out".into(),
            // Leads with what is waiting rather than with the question, so a
            // list of these reads as a list of work rather than a quiz.
            Blocker::NeedsYourDecision { question, set_aside } => {
                format!("{set_aside} is waiting on you: {question}")
            }
        }
    }
}

/// What is true right now, for deciding what has become possible.
#[derive(Debug, Clone, Default)]
pub struct Conditions {
    pub online: bool,
    pub screen_free: bool,
    pub you_are_here: bool,
    pub tools: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: u64,
    /// What you actually said, so Atlas can quote it back.
    pub request: String,
    pub blocker: Blocker,
    pub first_seen: u64,
    pub last_offered: u64,
    /// How many times Atlas has raised it. Drives the backoff.
    pub offers: u32,
    /// You said no. Never raised again.
    pub dismissed: bool,
    pub done: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct BacklogConfig {
    /// Wait this long before the first re-offer once unblocked.
    pub first_retry_secs: u64,
    /// Each subsequent offer waits this many times longer.
    pub backoff: f32,
    /// Never wait longer than this between offers.
    pub max_retry_secs: u64,
    /// Stop offering after this many refusals-by-silence.
    pub give_up_after: u32,
    /// Drop items older than this even if never resolved.
    pub expire_days: u64,
    pub max_items: usize,
}

impl Default for BacklogConfig {
    fn default() -> Self {
        BacklogConfig {
            first_retry_secs: 60,
            backoff: 3.0,
            max_retry_secs: 86_400,
            give_up_after: 4,
            expire_days: 30,
            max_items: 200,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Backlog {
    pub items: Vec<Item>,
    next_id: u64,
}

impl Backlog {
    pub fn load(store: &Store) -> Backlog {
        store.load("backlog")
    }
    pub fn save(&self, store: &Store) -> Result<()> {
        store.save("backlog", self)
    }

    /// File a request Atlas couldn't complete.
    ///
    /// Asking for the same thing twice updates the existing entry rather than
    /// creating a duplicate — otherwise a flaky connection turns one task into
    /// twenty and Atlas offers each of them separately.
    pub fn record(&mut self, request: &str, blocker: Blocker, t: u64) -> u64 {
        let key = normalize(request);
        if let Some(i) = self.items.iter_mut().find(|i| normalize(&i.request) == key && !i.done) {
            i.blocker = blocker;
            i.dismissed = false; // asking again un-dismisses it
            return i.id;
        }
        self.next_id += 1;
        self.items.push(Item {
            id: self.next_id,
            request: request.to_string(),
            blocker,
            first_seen: t,
            last_offered: 0,
            offers: 0,
            dismissed: false,
            done: false,
        });
        if self.items.len() > 200 {
            self.items.retain(|i| !i.done);
        }
        self.next_id
    }

    pub fn is_blocked(&self, item: &Item, c: &Conditions) -> bool {
        match &item.blocker {
            Blocker::Offline => !c.online,
            Blocker::NoScreenGap => !c.screen_free,
            Blocker::NeedsApproval => !c.you_are_here,
            Blocker::MissingTool(t) => !c.tools.iter().any(|x| x == t),
            Blocker::Failed(_) => false, // worth another go
            Blocker::Unsupported(_) => true, // nothing has changed
            // Blocked precisely while you are here.
            Blocker::NotWhileYouAreHere => c.you_are_here,
            // Only you clear this, and only by answering. Being back at the
            // machine is not an answer — that is the difference between this
            // and NeedsApproval, and collapsing the two would mean Atlas
            // treating your presence as a decision.
            Blocker::NeedsYourDecision { .. } => true,
        }
    }

    /// Everything whose blocker has cleared and that is due another mention.
    pub fn ready(&self, c: &Conditions, cfg: &BacklogConfig, t: u64) -> Vec<&Item> {
        self.items
            .iter()
            .filter(|i| !i.done && !i.dismissed)
            .filter(|i| !self.is_blocked(i, c))
            .filter(|i| i.offers < cfg.give_up_after)
            .filter(|i| t.saturating_sub(i.last_offered) >= self.wait_for(i, cfg))
            .collect()
    }

    /// Escalating gaps between offers. Two reminders about one stuck task is
    /// helpful; twenty is why people turn assistants off.
    fn wait_for(&self, item: &Item, cfg: &BacklogConfig) -> u64 {
        if item.offers == 0 {
            return cfg.first_retry_secs;
        }
        let secs = cfg.first_retry_secs as f32 * cfg.backoff.powi(item.offers as i32);
        (secs as u64).min(cfg.max_retry_secs)
    }

    /// The single most useful thing to raise, so Atlas mentions one task and
    /// not a list. Oldest first — the thing that has waited longest.
    pub fn next_offer(&mut self, c: &Conditions, cfg: &BacklogConfig, t: u64) -> Option<Item> {
        let id = {
            let mut ready = self.ready(c, cfg, t);
            ready.sort_by_key(|i| i.first_seen);
            ready.first().map(|i| i.id)?
        };
        let item = self.items.iter_mut().find(|i| i.id == id)?;
        item.offers += 1;
        item.last_offered = t;
        Some(item.clone())
    }

    pub fn phrase(item: &Item) -> String {
        format!(
            "Earlier you asked me to {} but {}. Want me to do it now?",
            item.request.trim(),
            item.blocker.explain()
        )
    }

    pub fn complete(&mut self, id: u64) {
        if let Some(i) = self.items.iter_mut().find(|i| i.id == id) {
            i.done = true;
        }
    }

    /// You said no. It stays on record but is never raised again.
    pub fn dismiss(&mut self, id: u64) {
        if let Some(i) = self.items.iter_mut().find(|i| i.id == id) {
            i.dismissed = true;
        }
    }

    /// Everything still outstanding, for "what's on your list?".
    pub fn outstanding(&self) -> Vec<&Item> {
        self.items.iter().filter(|i| !i.done && !i.dismissed).collect()
    }

    pub fn summary(&self) -> String {
        let o = self.outstanding();
        match o.len() {
            0 => "Nothing outstanding.".into(),
            1 => format!("One thing outstanding: {}", o[0].request),
            n => format!("{n} things outstanding, oldest is: {}", o[0].request),
        }
    }

    /// Drop stale and completed entries.
    pub fn tidy(&mut self, cfg: &BacklogConfig, t: u64) {
        let cutoff = cfg.expire_days * 86_400;
        self.items
            .retain(|i| !i.done && t.saturating_sub(i.first_seen) < cutoff);
        if self.items.len() > cfg.max_items {
            self.items.sort_by_key(|i| i.first_seen);
            let excess = self.items.len() - cfg.max_items;
            self.items.drain(0..excess);
        }
    }
}

/// Which outstanding item a "take it off my list" means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Removal {
    /// "Clear my outstanding list", "remove everything from it".
    All,
    /// "Remove the second one", "remove number 2" (1-based, as listed).
    Number(usize),
    /// "Remove the chrome one": words to find it by.
    Words(Vec<String>),
    /// "Remove the item on my outstanding list": which isn't said.
    Unsaid,
}

/// Is this asking to take something off the outstanding list? (1 Oct 2026:
/// Eric couldn't -- "Remove the item on my outstanding list", "chrome
/// should be a removed item off my list" -- there was no way to at all.)
pub fn removal_asked(said: &str) -> Option<Removal> {
    let t = format!(" {} ", normalize(said));
    const VERBS: &[&str] = &[
        " remove ", " removed ", " delete ", " drop ", " clear ", " cross off ", " cross ", " take off ", " take ",
        " scratch ", " get rid of ", " mark done ", " mark as done ", " is done ", " are done ", " dismiss ",
    ];
    const LIST: &[&str] = &[" outstanding ", " outstanding list", " my list ", " your list ", " the list ", " off my ", " off the list", " off your list"];
    let verb = VERBS.iter().any(|v| t.contains(v));
    let list = LIST.iter().any(|l| t.contains(l)) || t.trim_end().ends_with(" outstanding");
    if !verb || !list {
        return None;
    }
    if [" all ", " everything ", " the whole ", " entire "].iter().any(|w| t.contains(w)) || t.trim() == "clear outstanding" || t.trim() == "clear my outstanding list" {
        return Some(Removal::All);
    }
    const ORD: &[(&str, usize)] = &[
        ("first", 1), ("second", 2), ("third", 3), ("fourth", 4), ("fifth", 5), ("sixth", 6), ("seventh", 7), ("eighth", 8),
        ("1st", 1), ("2nd", 2), ("3rd", 3), ("4th", 4), ("5th", 5),
    ];
    let words: Vec<&str> = t.split_whitespace().collect();
    for w in &words {
        if let Some((_, n)) = ORD.iter().find(|(o, _)| o == w) {
            return Some(Removal::Number(*n));
        }
        if let Ok(n) = w.parse::<usize>() {
            if (1..=50).contains(&n) {
                return Some(Removal::Number(n));
            }
        }
    }
    const NOT_A_TARGET: &[&str] = &[
        "remove", "removed", "delete", "drop", "clear", "cross", "take", "scratch", "get", "rid", "mark", "done", "dismiss",
        "off", "from", "my", "your", "the", "list", "outstanding", "item", "items", "thing", "things", "one", "on", "of",
        "a", "an", "please", "atlas", "should", "be", "is", "are", "as", "it", "that", "this", "can", "you", "could", "i",
        "want", "to", "need", "now", "and", "out", "entry", "task", "job",
    ];
    let left: Vec<String> = words.iter().filter(|w| !NOT_A_TARGET.contains(w) && w.len() > 1).map(|w| w.to_string()).collect();
    Some(if left.is_empty() { Removal::Unsaid } else { Removal::Words(left) })
}

impl Backlog {
    /// The outstanding item a removal names, by its place in the list or its
    /// words: `Ok(id)`, or `Err` with what to say instead.
    pub fn find_for_removal(&self, r: &Removal) -> std::result::Result<Vec<u64>, String> {
        let o = self.outstanding();
        if o.is_empty() {
            return Err("There's nothing on your outstanding list.".into());
        }
        let listed = || {
            o.iter().enumerate().map(|(i, it)| format!("{}. {}", i + 1, it.request.trim())).collect::<Vec<_>>().join("; ")
        };
        match r {
            Removal::All => Ok(o.iter().map(|i| i.id).collect()),
            Removal::Number(n) => match o.get(n.saturating_sub(1)) {
                Some(it) => Ok(vec![it.id]),
                None => Err(format!("There are only {} on the list: {}.", o.len(), listed())),
            },
            Removal::Unsaid if o.len() == 1 => Ok(vec![o[0].id]),
            Removal::Unsaid => Err(format!("Which one? {}. Say its number, or some of its words.", listed())),
            Removal::Words(w) => {
                let score = |it: &Item| {
                    let r = normalize(&it.request);
                    w.iter().filter(|x| r.split_whitespace().any(|y| y == x.as_str())).count()
                };
                let best = o.iter().map(|it| score(it)).max().unwrap_or(0);
                let hits: Vec<u64> = o.iter().filter(|it| best > 0 && score(it) == best).map(|it| it.id).collect();
                match hits.len() {
                    0 => Err(format!("I couldn't find that on the list: {}.", listed())),
                    1 => Ok(hits),
                    _ => Err(format!("More than one matches. {}. Say its number.", listed())),
                }
            }
        }
    }
}

fn normalize(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn now_secs() -> u64 {
    now()
}
