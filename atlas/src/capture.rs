//! Catching a thought before it's gone, and filing it without you deciding
//! where.
//!
//! ## The idea
//!
//! Everyone's notes app is a graveyard. Not because people are disorganised,
//! but because capture and filing are the same action in every tool: to write
//! something down you must first decide where it goes, and that decision — at
//! the moment you have the thought — is exactly what stops you writing it
//! down. So the thought is lost, or it lands in one enormous untitled note
//! nobody reads.
//!
//! Splitting the two fixes it. **Capture is instant and costs nothing**: you
//! say a thing, it's saved, no questions. **Filing happens afterwards**, by
//! Atlas, from what the thought is actually about.
//!
//! The test of whether the filing worked is not whether the folders look tidy.
//! It's whether "where's that thing about the broker fees" finds it — which is
//! why what's stored is not a folder but a set of handles you might reach for
//! it by.

use serde::{Deserialize, Serialize};

/// One thing you said or wrote, saved without ceremony.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub id: u64,
    pub text: String,
    pub at: u64,
    /// What you were doing when you said it. Often the best clue about what
    /// it's for, and free to collect.
    pub while_in: Option<String>,
    /// What Atlas worked out about it.
    pub about: Vec<String>,
    pub kind: Kind,
    /// You corrected the filing.
    pub confirmed: bool,
    /// When it's for, if the note said a sure time ("call the bank Friday
    /// at 10") -- read by `when` against the local time it was said (round
    /// 11). Local seconds.
    #[serde(default)]
    pub due: Option<u64>,
    /// Looked at in a weekly review: kept as it is.
    #[serde(default)]
    pub reviewed: bool,
}

/// What sort of thing it is, which decides what happens to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Something to do.
    Task,
    /// Something to remember.
    Fact,
    /// A half-formed thought worth keeping.
    Idea,
    /// Something someone said.
    Quote,
    /// A question you want answered.
    Question,
    /// A decision you made, and why.
    Decision,
    /// Can't tell.
    Note,
}

impl Kind {
    pub fn plain(&self) -> &'static str {
        match self {
            Kind::Task => "to do",
            Kind::Fact => "worth remembering",
            Kind::Idea => "idea",
            Kind::Quote => "something said",
            Kind::Question => "question",
            Kind::Decision => "decision",
            Kind::Note => "note",
        }
    }
    /// Does it belong in the outstanding list rather than just filed?
    fn is_work(&self) -> bool {
        matches!(self, Kind::Task | Kind::Question)
    }
}

const TASKISH: &[&str] = &[
    "remind me", "need to", "have to", "must", "todo", "to do", "chase",
    "follow up", "book", "order", "call", "email", "send", "fix", "check",
];
const IDEAISH: &[&str] = &[
    "what if", "idea", "maybe we", "could we", "it'd be good", "would be good",
    "wonder if", "worth trying", "occurred to me",
];
const QUESTIONISH: &[&str] = &["how do", "why does", "what is", "is there", "can you find out"];
const DECISIONISH: &[&str] = &[
    "decided", "going with", "we'll use", "settled on", "sticking with", "not doing",
];

/// Work out what sort of thing it is.
pub fn kind_of(text: &str) -> Kind {
    let t = text.to_lowercase();
    if DECISIONISH.iter().any(|w| t.contains(w)) {
        return Kind::Decision;
    }
    if TASKISH.iter().any(|w| t.contains(w)) {
        return Kind::Task;
    }
    if IDEAISH.iter().any(|w| t.contains(w)) {
        return Kind::Idea;
    }
    if QUESTIONISH.iter().any(|w| t.contains(w)) || t.trim().ends_with('?') {
        return Kind::Question;
    }
    if text.contains('"') || t.contains("said") {
        return Kind::Quote;
    }
    Kind::Fact
}

/// A sentence that already contains the whole record.
///
/// Taken from watching someone dictate a task at the gym: "make a reel in the
/// Q4 project, call it the Claude tutorial, assign it to me, due Monday, add
/// text on screen". That isn't a note to sort later — it's a fully specified
/// item, said in one breath.
///
/// The distinction matters because it changes what capture is for. A note you
/// file later is a debt. An item that arrives complete is done.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Spoken {
    /// What to make.
    pub title: Option<String>,
    pub project: Option<String>,
    pub client: Option<String>,
    /// "a reel", "a post", "a doc".
    pub kind: Option<String>,
    pub assigned_to: Option<String>,
    /// As said — "Monday", "next week". Resolved later, never guessed at now.
    pub due_words: Option<String>,
    /// Anything after "in the script" or "note that".
    pub notes: Option<String>,
}

impl Spoken {
    /// Enough to make something, rather than to file something.
    pub fn is_a_whole_item(&self) -> bool {
        self.title.is_some() && (self.project.is_some() || self.kind.is_some())
    }

    /// What's missing that would be worth one question.
    ///
    /// One, not a form. Asking three questions about a sentence someone said
    /// while walking is how you teach them not to bother.
    pub fn worth_asking(&self) -> Option<&'static str> {
        if self.title.is_none() {
            return Some("what should I call it?");
        }
        if self.project.is_none() && self.client.is_none() {
            return Some("which project?");
        }
        None
    }
}

/// Read a dictated task.
pub fn read_spoken(said: &str, known_projects: &[String], known_people: &[String]) -> Spoken {
    let t = said.to_ascii_lowercase();
    let mut out = Spoken::default();

    for (lead, field) in [
        ("call it ", 0u8),
        ("called ", 0),
        ("name it ", 0),
    ] {
        if let Some(i) = t.find(lead) {
            let rest = &said[i + lead.len()..];
            let stop = rest
                .find([',', '.'])
                .unwrap_or(rest.len());
            let v = rest[..stop].trim().to_string();
            if !v.is_empty() {
                let _ = field;
                out.title = Some(v);
                break;
            }
        }
    }

    // A named project wins over a guessed one.
    for p in known_projects {
        if t.contains(&p.to_lowercase()) {
            out.project = Some(p.clone());
            break;
        }
    }
    for p in known_people {
        if t.contains(&format!("assign it to {}", p.to_lowercase()))
            || t.contains(&format!("assign to {}", p.to_lowercase()))
        {
            out.assigned_to = Some(p.clone());
            break;
        }
    }
    if out.assigned_to.is_none() && (t.contains("assign it to me") || t.contains("assign to me")) {
        out.assigned_to = Some("me".into());
    }

    for k in ["a reel", "a short", "a post", "a video", "a doc", "a document", "a note"] {
        if t.contains(k) {
            out.kind = Some(k.trim_start_matches("a ").to_string());
            break;
        }
    }

    // Kept as words. Resolving "Monday" needs today's date, and guessing at
    // it here is how something lands on the wrong Monday.
    for lead in ["due ", "by "] {
        if let Some(i) = t.find(lead) {
            let rest = &t[i + lead.len()..];
            let stop = rest.find([',', '.']).unwrap_or(rest.len());
            let v = rest[..stop].trim();
            if !v.is_empty() && v.len() < 24 {
                out.due_words = Some(v.to_string());
                break;
            }
        }
    }

    for lead in ["in the script ", "in the script,", "note that ", "make sure "] {
        if let Some(i) = t.find(lead) {
            let v = said[i + lead.len()..].trim();
            if !v.is_empty() {
                out.notes = Some(v.to_string());
                break;
            }
        }
    }
    out
}

/// What Atlas says back to a complete one.
///
/// Short, and it repeats the two things worth getting wrong — the name and
/// where it went — rather than reading the whole record back.
pub fn made(s: &Spoken) -> String {
    let title = s.title.clone().unwrap_or_else(|| "it".into());
    match (&s.project, &s.due_words) {
        (Some(p), Some(d)) => format!("Made \"{title}\" in {p}, due {d}."),
        (Some(p), None) => format!("Made \"{title}\" in {p}."),
        (None, Some(d)) => format!("Made \"{title}\", due {d}."),
        (None, None) => format!("Made \"{title}\"."),
    }
}

/// The handles you might reach for it by.
///
/// Not a folder. You won't remember which folder — you'll remember a word from
/// it, roughly when it was, or what you were doing. So all three are stored,
/// and any of them finds it.
pub fn handles(text: &str, while_in: Option<&str>, known_projects: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let lower = text.to_lowercase();

    // A project you're already working on, mentioned by name.
    for p in known_projects {
        if lower.contains(&p.to_lowercase()) {
            out.push(p.clone());
        }
    }

    // Proper nouns and unusual words — the things you'd actually search for.
    for w in text.split(|c: char| !c.is_alphanumeric() && c != '-') {
        let clean = w.trim();
        let is_number = !clean.is_empty() && clean.chars().all(|c| c.is_ascii_digit());
        // A number is a handle at any length — "40" is exactly the sort of
        // thing you'd search for, and the length rule was throwing it away.
        if clean.len() < 4 && !(is_number && clean.len() >= 2) {
            continue;
        }
        let is_proper = clean.chars().next().map(|c| c.is_uppercase()).unwrap_or(false);
        let is_number = is_number || clean.chars().any(|c| c.is_ascii_digit());
        if (is_proper || is_number) && !out.iter().any(|o| o.eq_ignore_ascii_case(clean)) {
            out.push(clean.to_string());
        }
    }

    // What you were doing. Often the best clue and free to collect.
    if let Some(app) = while_in {
        out.push(format!("while in {app}"));
    }
    out.truncate(8);
    out
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct CaptureConfig {
    pub enabled: bool,
    /// Ask nothing at capture time. This is the whole point.
    pub never_ask_on_capture: bool,
    /// Turn tasks into outstanding items automatically.
    pub tasks_become_work: bool,
    /// Projects Atlas knows about, for handles.
    pub projects: Vec<String>,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        CaptureConfig {
            enabled: true,
            // Not configurable in spirit: asking where it goes is the thing
            // that stops you writing it down.
            never_ask_on_capture: true,
            tasks_become_work: true,
            projects: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Notebook {
    pub notes: Vec<Note>,
    next_id: u64,
}

impl Notebook {
    /// Where the notebook lives between runs.
    ///
    /// It did not live anywhere until 17 September 2026. `Notebook` had
    /// derived `Serialize` and `Deserialize` since it was written, the daemon
    /// built one with `Notebook::default()` at startup, captured into it, said
    /// "got it" — and nothing ever wrote it down. Every thought caught before
    /// it was gone was gone at the next restart.
    ///
    /// Nothing failed, which is the part worth remembering. `capture.rs` had
    /// its own tests and they all passed, because they exercised the notebook
    /// directly and never asked where it went. The daemon branch that joins
    /// the capability to the disk was the untested one, and this is the defect
    /// that was sitting in it.
    pub const FILE: &str = "notebook";

    pub fn load(store: &crate::store::Store) -> Notebook {
        store.load(Self::FILE)
    }

    pub fn save(&self, store: &crate::store::Store) -> crate::error::Result<()> {
        store.save(Self::FILE, self)
    }

    /// Save it. No questions, no folder, no title.
    pub fn capture(&mut self, text: &str, while_in: Option<&str>, at: u64, cfg: &CaptureConfig) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.notes.push(Note {
            id,
            text: text.trim().to_string(),
            at,
            while_in: while_in.map(str::to_string),
            about: handles(text, while_in, &cfg.projects),
            kind: kind_of(text),
            confirmed: false,
            due: None,
            reviewed: false,
        });
        id
    }

    /// Give a note the time it named, when it named one surely. `local_now`
    /// is local seconds. Returns the due time set.
    pub fn date_it(&mut self, id: u64, local_now: u64) -> Option<u64> {
        let n = self.notes.iter_mut().find(|n| n.id == id)?;
        let p = crate::when::parse(&n.text, local_now).filter(|p| p.sure && p.day_said && p.start >= local_now.saturating_sub(86_400))?;
        n.due = Some(p.start);
        n.due
    }

    /// Notes due between `from` and `to` (local seconds), soonest first.
    pub fn due_between(&self, from: u64, to: u64) -> Vec<&Note> {
        let mut v: Vec<&Note> = self.notes.iter().filter(|n| n.due.map(|d| d >= from && d < to).unwrap_or(false)).collect();
        v.sort_by_key(|n| n.due);
        v
    }

    /// The weekly review (GTD's "review" step): everything captured and not
    /// yet looked at since, oldest first, numbered for "keep 2", "drop 3",
    /// "3 is a task". Nothing is changed by looking.
    pub fn to_review(&self) -> Vec<&Note> {
        let mut v: Vec<&Note> = self.notes.iter().filter(|n| !n.reviewed && !n.confirmed).collect();
        v.sort_by_key(|n| n.at);
        v
    }

    /// Settle one item of a review: `keep` marks it looked at; `drop`
    /// removes it (the only way a note is ever removed -- by you).
    pub fn settle(&mut self, id: u64, keep: bool) -> bool {
        if keep {
            match self.notes.iter_mut().find(|n| n.id == id) {
                Some(n) => {
                    n.reviewed = true;
                    true
                }
                None => false,
            }
        } else {
            let before = self.notes.len();
            self.notes.retain(|n| n.id != id);
            self.notes.len() != before
        }
    }

    /// What Atlas says back. Almost nothing — a confirmation you have to read
    /// is a cost at exactly the wrong moment.
    pub fn acknowledge(&self, id: u64) -> String {
        match self.notes.iter().find(|n| n.id == id) {
            Some(n) if n.kind.is_work() => "Got it — on the list.".to_string(),
            Some(_) => "Got it.".into(),
            None => String::new(),
        }
    }

    /// "Where's that thing about the broker fees?"
    ///
    /// Matched on any handle, any word, or roughly when — because you'll
    /// remember one of those and not which.
    pub fn find(&self, asked: &str, now: u64) -> Vec<&Note> {
        let t = asked.to_lowercase();
        let words: Vec<&str> = t
            .split_whitespace()
            .filter(|w| {
                w.len() > 2
                    && !["that", "thing", "about", "where", "what", "note", "the", "from",
                         "last", "this", "you", "was", "one"]
                        .contains(w)
            })
            .collect();

        let when = when_they_mean(&t, now);

        let mut scored: Vec<(usize, &Note)> = self
            .notes
            .iter()
            .map(|n| {
                let hay = format!("{} {}", n.text, n.about.join(" ")).to_lowercase();
                let mut score = words.iter().filter(|w| hay.contains(**w)).count() * 2;
                // A handle match is worth more than a word buried in the text.
                score += n
                    .about
                    .iter()
                    .filter(|a| words.iter().any(|w| a.to_lowercase().contains(*w)))
                    .count();
                if let Some((from, to)) = when {
                    if n.at >= from && n.at <= to {
                        score += 3;
                    }
                }
                (score, n)
            })
            .filter(|(s, _)| *s > 0)
            .collect();

        scored.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.at.cmp(&a.1.at)));
        scored.into_iter().take(5).map(|(_, n)| n).collect()
    }

    /// Everything of one kind, for "what have I got to do".
    pub fn of_kind(&self, kind: Kind) -> Vec<&Note> {
        self.notes.iter().filter(|n| n.kind == kind).collect()
    }

    /// You said it was filed wrong. That correction is worth more than the
    /// original guess.
    pub fn correct(&mut self, id: u64, kind: Option<Kind>, add_handle: Option<&str>) -> bool {
        match self.notes.iter_mut().find(|n| n.id == id) {
            Some(n) => {
                if let Some(k) = kind {
                    n.kind = k;
                }
                if let Some(h) = add_handle {
                    n.about.push(h.to_string());
                }
                n.confirmed = true;
                true
            }
            None => false,
        }
    }

    /// Notes that never got looked at or acted on.
    ///
    /// Worth surfacing once, not nagging about: the point of frictionless
    /// capture is that some of it is rubbish, and that's fine.
    pub fn never_revisited(&self, now: u64, older_than_days: u64) -> Vec<&Note> {
        self.notes
            .iter()
            .filter(|n| !n.confirmed && now.saturating_sub(n.at) > older_than_days * 86_400)
            .filter(|n| n.kind == Kind::Idea)
            .collect()
    }
}

/// "Last week", "yesterday", "in March" — turned into a window.
fn when_they_mean(asked: &str, now: u64) -> Option<(u64, u64)> {
    const DAY: u64 = 86_400;
    let (from, to) = if asked.contains("today") {
        (now.saturating_sub(DAY), now)
    } else if asked.contains("yesterday") {
        (now.saturating_sub(2 * DAY), now.saturating_sub(DAY))
    } else if asked.contains("last week") {
        (now.saturating_sub(14 * DAY), now.saturating_sub(7 * DAY))
    } else if asked.contains("this week") {
        (now.saturating_sub(7 * DAY), now)
    } else if asked.contains("last month") {
        (now.saturating_sub(60 * DAY), now.saturating_sub(30 * DAY))
    } else {
        return None;
    };
    Some((from, to))
}

/// A review, said: numbered, grouped by what each note is, with the one
/// suggestion that fits each kind. `local_day_start` for "due today".
pub fn review_said(notes: &[&Note]) -> String {
    if notes.is_empty() {
        return "Nothing to review: every note has been looked at.".into();
    }
    let mut out = vec![format!("{} note{} to look at:", notes.len(), if notes.len() == 1 { "" } else { "s" })];
    for (i, n) in notes.iter().enumerate().take(20) {
        let hint = match n.kind {
            Kind::Task => "do it, date it, or drop it",
            Kind::Question => "find out, or drop it",
            Kind::Idea => "keep it for later, or drop it",
            Kind::Decision => "keep it as the record",
            _ => "keep or drop",
        };
        let flat: String = n.text.split_whitespace().collect::<Vec<_>>().join(" ");
        let short: String = flat.chars().take(70).collect();
        out.push(format!("  {}. [{}] {short} -- {hint}", i + 1, n.kind.plain()));
    }
    if notes.len() > 20 {
        out.push(format!("  … and {} more; say \"review my notes\" again after these.", notes.len() - 20));
    }
    out.push("Say \"keep 1\", \"drop 2\", or \"keep all\".".into());
    out.join("\n")
}

/// What Atlas says when it finds them.
pub fn found(notes: &[&Note]) -> String {
    match notes.split_first() {
        None => "I can't find that one.".into(),
        Some((first, rest)) => {
            let mut s = first.text.clone();
            if let Some(app) = &first.while_in {
                s.push_str(&format!(" — you said that while you were in {app}."));
            }
            if !rest.is_empty() {
                s.push_str(&format!(" {} others like it.", rest.len()));
            }
            s
        }
    }
}

/// Ideas saved a month ago or more and never come back to, not named before.
/// At most three. Eric, 25 Sep 2026 (F1): yes, name them — once each.
pub fn ideas_to_name<'a>(book: &'a Notebook, now: u64, named: &[u64]) -> Vec<&'a Note> {
    book.never_revisited(now, 30).into_iter().filter(|n| !named.contains(&n.id)).take(3).collect()
}

/// The line for the brief.
pub fn ideas_line(ideas: &[&Note], now: u64) -> Option<String> {
    if ideas.is_empty() {
        return None;
    }
    let each: Vec<String> = ideas
        .iter()
        .map(|n| {
            let weeks = now.saturating_sub(n.at) / (7 * 86_400);
            let short: String = n.text.chars().take(60).collect();
            format!("\"{short}\" ({weeks} weeks ago)")
        })
        .collect();
    Some(format!("ideas you saved and haven't come back to: {} — worth a look, or I'll leave them be", each.join(", ")))
}
