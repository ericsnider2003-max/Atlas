//! Turning corrections into edits.
//!
//! You correct Atlas. The conversation ends. The correction dies with it, and
//! next week you make the same correction again. That is the single most
//! expensive thing a memory system can get wrong, because it wastes the one
//! input that is unambiguously worth keeping: you, telling it directly that it
//! was wrong.
//!
//! Four rules, and the third is the one everything else hangs on:
//!
//! 1. **Where it is written decides whether it works.** A correction about
//!    *how a task is done* belongs in the instructions for that task, not in a
//!    diary. The instructions are read every time the task runs. The diary may
//!    never be read again. Same fact, two places, one of them useless.
//! 2. **Store why it was wrong and what right looks like** — never "you didn't
//!    like it". A note that records displeasure teaches nothing.
//! 3. **Wait for the repeat.** One correction in a session is a note. It earns
//!    an edit only when you have said it twice. Otherwise Atlas rebuilds
//!    itself around a bad day, and a rule made from one irritable evening is
//!    worse than no rule.
//! 4. **Show the exact lines before changing them.** Surgical edits, named,
//!    reviewable. Never a rewrite.
//!
//! The scoreboard is one question: does it make the same mistake twice.
//! `repeat_rate` answers it and nothing else here matters if that number is
//! not falling.

use serde::{Deserialize, Serialize};

/// Where a lesson has to live to actually change anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Home {
    /// The instructions for a task. Read every single time that task runs.
    /// Anything about *how* something is done belongs here.
    HowTo,
    /// A standing preference about you. Read when Atlas is deciding tone,
    /// format, or what you would want.
    AboutYou,
    /// A fact with a date on it. Read when the subject comes up.
    Record,
}

impl Home {
    /// Read every time, or only when something goes looking?
    ///
    /// This is the whole distinction. A lesson in a file nobody opens is a
    /// lesson that was not learned.
    pub fn read_every_time(&self) -> bool {
        matches!(self, Home::HowTo)
    }

    pub fn plain(&self) -> &'static str {
        match self {
            Home::HowTo => "the instructions for that job",
            Home::AboutYou => "what I know about how you like things",
            Home::Record => "the record",
        }
    }
}

/// One thing you said that Atlas got wrong.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Correction {
    /// A slug for the same complaint said two different ways, so "too long"
    /// and "that was way too long" count as the same correction twice.
    pub about: String,
    /// What Atlas did.
    pub did: String,
    /// What you wanted instead. Without this there is nothing to write down —
    /// see `is_actionable`.
    pub wanted: Option<String>,
    /// When, so a repeat can be told from a restatement in the same breath.
    pub at: u64,
    /// Which session, so two corrections in one sitting are one correction.
    pub session: u64,
}

impl Correction {
    pub fn new(about: &str, did: &str, at: u64, session: u64) -> Correction {
        Correction { about: slug(about), did: did.into(), wanted: None, at, session }
    }

    pub fn wanting(mut self, wanted: &str) -> Correction {
        self.wanted = Some(wanted.into());
        self
    }

    /// A correction Atlas can act on says what right looks like.
    ///
    /// "That's wrong" is a signal to ask, not a lesson to file. Filing it
    /// produces a rule that says only what not to do, which forbids one thing
    /// and teaches none.
    pub fn is_actionable(&self) -> bool {
        self.wanted.as_ref().is_some_and(|w| w.trim().len() > 2)
    }
}

/// Same complaint, different wording.
pub fn slug(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .filter(|w| !FILLER.contains(w))
        .collect::<Vec<_>>()
        .join("-")
}

const FILLER: &[&str] = &[
    "that", "was", "way", "too", "a", "bit", "the", "is", "it", "you", "your", "i", "me", "my",
    "please", "again", "really", "very", "just", "so",
];

/// A change Atlas proposes to make to itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edit {
    pub about: String,
    pub home: Home,
    /// The exact existing line, or None when the line is new.
    pub replacing: Option<String>,
    /// What it becomes.
    pub becomes: String,
    /// How many times you said it. Never below `REPEATS_NEEDED`.
    pub said_times: u32,
}

/// One correction is a note. Two is a rule.
pub const REPEATS_NEEDED: u32 = 2;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Mending {
    /// Everything heard, kept whether or not it earned an edit — a note that
    /// was never promoted is still the evidence for the next one.
    pub heard: Vec<Correction>,
    /// Edits already applied, so a repeat rate can be measured.
    pub applied: Vec<Edit>,
    /// Corrections that arrived *after* an edit about the same thing. This is
    /// the number that matters.
    pub repeats_after_fix: u32,
}

impl Mending {
    /// Record a correction. Returns an edit only when it has earned one.
    pub fn heard(&mut self, c: Correction) -> Option<Edit> {
        let prior_fix = self.applied.iter().find(|e| e.about == c.about).cloned();
        if prior_fix.is_some() {
            // Atlas was told, wrote it down, and did it again. That is the
            // failure the whole loop exists to drive to zero, whether this
            // occasion turns out to be the same instruction repeating or a
            // genuinely different one replacing it.
            self.repeats_after_fix += 1;
        }

        let actionable = c.is_actionable();
        // Two corrections in one sitting are one correction. Restating a
        // complaint in the same breath is emphasis, not a second occasion.
        let prior_sessions: std::collections::BTreeSet<u64> = self
            .heard
            .iter()
            .filter(|h| h.about == c.about)
            .map(|h| h.session)
            .collect();
        let occasions = prior_sessions.len() as u32
            + if prior_sessions.contains(&c.session) { 0 } else { 1 };

        let wanted = c.wanted.clone();
        self.heard.push(c.clone());

        if !actionable {
            return None;
        }

        // Already fixed once, and this time you're not repeating the same
        // instruction -- you're saying something different about the same
        // subject. That is not "the same mistake twice" (already counted,
        // above); it's the applied line itself turning out to be wrong or
        // incomplete, and it is offered as a *replacement* rather than a
        // second, contradicting addition. `proposal()` already has the
        // wording for this; nothing before this built the `Edit` that would
        // use it. Still gated behind `apply_lesson`'s approval, same as any
        // other edit -- naming a different line to change is not the same as
        // writing over it.
        if let Some(prior) = &prior_fix {
            let becomes = wanted?;
            if becomes == prior.becomes {
                // The identical instruction, restated. Nothing to replace.
                return None;
            }
            return Some(Edit {
                about: c.about.clone(),
                home: home_for(&c),
                replacing: Some(prior.becomes.clone()),
                becomes,
                said_times: prior.said_times + 1,
            });
        }

        if occasions < REPEATS_NEEDED {
            return None;
        }

        let becomes = wanted?;
        let edit = Edit {
            about: c.about.clone(),
            home: home_for(&c),
            replacing: None,
            becomes,
            said_times: occasions,
        };
        Some(edit)
    }

    /// Confirm an edit was actually written.
    pub fn applied(&mut self, e: Edit) {
        self.applied.retain(|x| x.about != e.about);
        self.applied.push(e);
    }

    /// The only metric. How often did Atlas repeat a mistake it had already
    /// written down a fix for?
    pub fn repeat_rate(&self) -> f32 {
        if self.applied.is_empty() {
            return 0.0;
        }
        self.repeats_after_fix as f32 / self.applied.len() as f32
    }

    /// Notes that never earned an edit and are old enough to be noise.
    ///
    /// Surfaced for review, never deleted silently — a one-off complaint that
    /// keeps not recurring is itself information.
    pub fn stale_notes(&self, now: u64, older_than_secs: u64) -> Vec<&Correction> {
        self.heard
            .iter()
            .filter(|c| {
                now.saturating_sub(c.at) > older_than_secs
                    && !self.applied.iter().any(|e| e.about == c.about)
            })
            .collect()
    }
}

/// Old corrections that bear on what you're asking for now: at least two
/// real words in common with the request, and not already mentioned. Eric,
/// 25 Sep 2026 (F9): "yes, only related to things being worked on." Each is
/// mentioned once; `mentioned` holds the `at` of those already said.
pub fn related_stale<'a>(m: &'a Mending, asked: &str, now: u64, mentioned: &[u64]) -> Vec<&'a Correction> {
    const SKIP: &[&str] = &["the", "and", "that", "this", "with", "for", "you", "your", "was", "too", "about", "from", "what", "have"];
    let words: Vec<String> = asked
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2 && !SKIP.contains(w))
        .map(str::to_string)
        .collect();
    m.stale_notes(now, 14 * 86_400)
        .into_iter()
        .filter(|c| !mentioned.contains(&c.at) && c.wanted.is_some())
        .filter(|c| {
            let hay = format!("{} {} {}", c.about.replace('-', " "), c.did, c.wanted.clone().unwrap_or_default()).to_lowercase();
            words.iter().filter(|w| hay.contains(w.as_str())).count() >= 2
        })
        .take(1)
        .collect()
}

/// Where this lesson has to go to actually be read again.
///
/// Anything phrased as *how* something is done goes to the instructions.
/// Everything else is a preference or a fact.
pub fn home_for(c: &Correction) -> Home {
    let t = format!("{} {}", c.did, c.wanted.clone().unwrap_or_default()).to_lowercase();
    const HOW: &[&str] = &[
        "format", "order", "step", "first", "before", "after", "always", "never", "when you",
        "each time", "every time", "instead of", "use ", "run ", "check",
    ];
    if HOW.iter().any(|k| t.contains(k)) {
        return Home::HowTo;
    }
    const YOU: &[&str] = &["prefer", "like", "hate", "want", "rather", "tone", "voice", "style"];
    if YOU.iter().any(|k| t.contains(k)) {
        return Home::AboutYou;
    }
    Home::Record
}

/// What Atlas says before it changes itself.
///
/// Names the exact line and the reason. An edit you cannot see is an edit you
/// cannot refuse.
pub fn proposal(e: &Edit) -> String {
    let where_ = e.home.plain();
    match &e.replacing {
        Some(old) => format!(
            "You've said this {} times, so I want to change {}: \"{}\" becomes \"{}\". Alright?",
            e.said_times, where_, old, e.becomes
        ),
        None => format!(
            "You've said this {} times, so I want to add to {}: \"{}\". Alright?",
            e.said_times, where_, e.becomes
        ),
    }
}

// ---------------------------------------------------------------------------
// The half that was missing — and the reason it could not simply be "wired".
//
// Nothing ever built a `Correction`, so `Mending::heard` never fired, no
// `Edit` was ever produced, and `nudge::offer_to_mend` had no caller. Same
// shape as `contents`, `trace` and `council`.
//
// But this module could not be honestly wired by adding a caller, because of
// its own **rule 1**: *where it is written decides whether it works*. A lesson
// about how a task is done has to go somewhere that is read **every time**,
// and `daemon::context()` — everything the model actually sees — was displays,
// apps, the focused window, recent files and the conversation. No preferences.
// No instructions. Nothing learned.
//
// So there was nowhere for a lesson to live where it would be read again, and
// filing one anyway would have produced exactly the failure the module header
// names: "Same fact, two places, one of them useless." `standing` below is the
// place. `daemon::context` reads it on every turn.
// ---------------------------------------------------------------------------

/// The most lessons that go in front of the model each turn.
///
/// Bounded for the same reason the brief is: a context that grows with every
/// correction eventually crowds out the thing you just said. Past this, the
/// oldest lesson is the one that stops being carried — a rule you have not
/// re-stated in a hundred corrections has been superseded in practice.
pub const MAX_STANDING: usize = 12;

/// The lessons, as the model should read them.
///
/// `Home::HowTo` first and always: those are the ones `read_every_time` is
/// true for, and if the cap bites it must bite the preferences rather than the
/// instructions. Empty string when there is nothing learned, so a fresh
/// install carries no heading for an empty list.
pub fn standing(applied: &[Edit]) -> String {
    let mut how: Vec<&Edit> = applied.iter().filter(|e| e.home.read_every_time()).collect();
    let mut rest: Vec<&Edit> = applied.iter().filter(|e| !e.home.read_every_time()).collect();
    // Newest first within each group: a later correction about the same thing
    // has already replaced the earlier one in `applied`, so what is left is in
    // the order it was learned and the recent ones matter more.
    how.reverse();
    rest.reverse();

    let mut lines = Vec::new();
    for e in how.into_iter().chain(rest) {
        if lines.len() >= MAX_STANDING {
            break;
        }
        lines.push(format!("- {}", e.becomes.trim()));
    }
    if lines.is_empty() {
        return String::new();
    }
    format!("What you have told me, and expect me to keep doing:\n{}\n", lines.join("\n"))
}

/// What you wanted, pulled out of the sentence you said it in.
///
/// "That was too long, keep it to one line" carries both halves: the
/// complaint and the fix. When it does not — "that's wrong" — this returns
/// `None`, `is_actionable` is false, and the caller has to ask rather than
/// file a rule that says only what not to do.
pub fn wanted_in(said: &str) -> Option<String> {
    const AFTER: &[&str] = &[
        " instead ", " instead,", "instead of that", "should have ", "should've ",
        " i wanted ", " i want ", " just ", " next time ", " from now on ",
    ];
    // Searched against a space-padded copy, so a marker written with a leading
    // space still matches a sentence that *starts* with it: "next time lead
    // with the number" has no space before "next time" and the plain `find`
    // missed every such sentence.
    //
    // Second time this exact shape has bitten in one session -- `council`'s
    // opinion parser had it with `" for "` and a seat saying "For -- ship it"
    // was recorded as Against. A word-boundary marker needs a boundary that
    // exists at position 0, and the start of the string is one.
    let lower = format!(" {} ", said.to_ascii_lowercase());
    for marker in AFTER {
        if let Some(at) = lower.find(marker) {
            // Back into the original string: the pad shifted everything by one.
            let after = (at + marker.len()).saturating_sub(1).min(said.len());
            let rest = said[after..].trim().trim_start_matches(',').trim();
            if rest.len() > 2 {
                return Some(strip_leading_filler(rest));
            }
        }
    }
    // A bare imperative after a comma is the commonest shape of all: "too
    // long, keep it to one line". Taken only when the tail actually reads as
    // an instruction, so "that's wrong, again" does not become a rule.
    if let Some((_, tail)) = said.split_once(',') {
        let t = tail.trim();
        if t.len() > 2 && starts_imperative(t) {
            return Some(strip_leading_filler(t));
        }
    }
    None
}

/// Does this read as an instruction rather than more complaint?
fn starts_imperative(t: &str) -> bool {
    const VERBS: &[&str] = &[
        "keep", "use", "put", "say", "give", "make", "do", "don't", "dont", "start", "stop",
        "always", "never", "just", "check", "ask", "show", "send", "write", "run", "open",
        "lead", "leave", "add", "drop", "cut", "shorten", "list",
    ];
    let first = t.split_whitespace().next().unwrap_or("").trim_matches(|c: char| !c.is_alphanumeric());
    VERBS.contains(&first.to_lowercase().as_str())
}

fn strip_leading_filler(s: &str) -> String {
    s.trim_start_matches(|c: char| c == ',' || c == '.' || c.is_whitespace())
        .trim_end_matches(['.', '!'])
        .to_string()
}

/// What the complaint is *about*, for matching one correction to the next.
///
/// The whole sentence would never match twice — "that was too long" and "way
/// too long again" are the same complaint and share almost no words. So the
/// subject is the complaint with the fix removed, then slugged, which is what
/// `slug` already strips filler for.
pub fn subject_of(said: &str) -> String {
    let complaint = match said.split_once(',') {
        Some((head, _)) => head,
        None => said,
    };
    slug(complaint)
}
