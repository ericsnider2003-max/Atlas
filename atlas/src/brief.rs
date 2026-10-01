//! The morning run.
//!
//! One pass, before you sit down, that reads what arrived overnight, puts it
//! against the day, drafts what can be drafted, and hands you a short list of
//! the things only you can decide.
//!
//! The shape is borrowed from how a good assistant actually reports: not
//! "you have 47 emails", which is an accusation, but "here is what I handled,
//! here are the two things I need you from". A count makes a long list sound
//! like a failure. A named next action makes it a morning.
//!
//! Three rules hold the whole thing up:
//!
//! * **Nothing sends.** Every reply this produces is a draft in the pending
//!   state. `Outcome::Sent` cannot be reached from here at all — sending is
//!   `mail`'s job, after you approve, and the separation is the point.
//! * **The brief is bounded.** A brief that grows with your inbox is one you
//!   stop reading in a bad week, which is exactly the week you needed it.
//!   `MAX_LINES` caps it and the overflow is counted, not printed.
//! * **Mail is one source, not the source.** The first version of this module
//!   took `items` that were only ever email, which meant a machine with no
//!   inbox had no brief at all — and this one has no inbox, so for its whole
//!   existence the brief was `run(&[], &[], &default())` and always said
//!   "Nothing needs you." An assistant that is useful offline has to be able
//!   to say what is waiting from what it already holds: a friend's note at the
//!   door, a job that failed overnight, a request it could not finish, a time
//!   someone proposed a week ago, a post waiting on your yes. `Source` names
//!   where an item came from and `gather` collects them. Mail keeps its place
//!   in the list for the day there is a reader; nothing else waits on it.

use crate::mail::{self, MailConfig, Provider};
use serde::{Deserialize, Serialize};

/// Where something ended up after the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    /// Atlas dealt with it. Nothing needed from you.
    Handled,
    /// A reply is written and waiting for your yes.
    Drafted,
    /// Only you can answer this.
    Yours,
    /// Read, filed, no action. Never shown individually.
    Ignored,
}

/// How urgent, in the only three grades that change behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Weight {
    Ignore,
    Info,
    Urgent,
}

/// Where something came from.
///
/// The point of naming it: the brief is not an inbox reader, and an item that
/// cannot say where it came from is one you cannot go and deal with. Only
/// `Mail` needs a network — the rest are things already on this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Source {
    /// Email. Was "the one source this tree has no reader for" — `imap.rs`
    /// landed in September, so the reader exists; whether this source is
    /// empty now depends on `mail.accounts`, which ships empty.
    Mail,
    /// A note or file a paired friend left at the door.
    Handoff,
    /// Something Atlas tried and could not finish.
    Blocked,
    /// Scheduled work that failed.
    Job,
    /// A time someone proposed and nobody has answered.
    Booking,
    /// A post written and waiting on your yes.
    Post,
    /// Atlas's own upkeep asking for a decision — a stale index, a guess it
    /// has been sitting on, a connection that stopped working.
    Upkeep,
    /// Your own day, from what's kept here (round 11): a promise due, a
    /// reply you're owed, someone you meant to be in touch with, a habit,
    /// the market's calendar. See `workday::day_items`.
    Day,
}

impl Source {
    /// For the spoken line, so an item reads as a thing rather than an id.
    pub fn plain(&self) -> &'static str {
        match self {
            Source::Mail => "email",
            Source::Handoff => "a handoff",
            Source::Blocked => "something I couldn't finish",
            Source::Job => "scheduled work",
            Source::Booking => "a proposed time",
            Source::Post => "a draft",
            Source::Upkeep => "upkeep",
            Source::Day => "your day",
        }
    }

    /// Does getting to this need the network up?
    ///
    /// Exists so a brief built while offline can say what is *actionable*
    /// rather than listing things you cannot do anything about until the
    /// router comes back.
    pub fn needs_network(&self) -> bool {
        matches!(self, Source::Mail)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    /// Where it came from. Defaults to `Mail` only because that is what every
    /// item was before there were any others.
    #[serde(default = "mail_source")]
    pub source: Source,
    /// Who it is from, in the form you would recognise.
    pub from: String,
    pub subject: String,
    pub weight: Weight,
    pub outcome: Outcome,
    /// The drafted reply, where there is one. Never sent from here.
    pub draft: Option<String>,
    /// Set when this collides with something already on the day.
    pub conflicts_with: Option<String>,
}

impl Item {
    /// The item as a person would say it.
    ///
    /// `"{from} — {subject}"` is right when `from` is a person and wrong when
    /// it is a mechanism: "me — fix the printer" and "a scheduled job — back
    /// up" are both worse than the subject alone. Mail keeps the old shape
    /// exactly, which is why this is a method rather than a change to `run`.
    pub fn headline(&self) -> String {
        match self.source {
            Source::Mail | Source::Handoff | Source::Booking => {
                format!("{} — {}", self.from, self.subject)
            }
            _ => self.subject.clone(),
        }
    }
}

/// Something already committed to today.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Commitment {
    pub id: String,
    pub what: String,
    /// Minutes from the start of the day. Kept relative so a brief written at
    /// 6am and read at 9am does not go stale.
    pub at_minute: u32,
    pub minutes: u32,
    /// Something that must happen before this, and has not.
    pub needs_prep: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct BriefConfig {
    pub enabled: bool,
    /// The earliest hour the day's brief will be given unasked. Local.
    ///
    /// This was `at_hour`, "the hour the run happens", and for one afternoon
    /// it was wired as exactly that: a daily brief at seven. That was wrong
    /// about the thing it was for. The brief is worth having when you *start*,
    /// and you do not start at the same time every day -- a run fixed to
    /// seven o'clock greets a night session at its fourth hour and misses the
    /// morning you began at ten.
    ///
    /// So arrival decides *whether* (`daily::arriving`), and this decides the
    /// earliest it may interrupt. The two answer different questions and both
    /// are real: nobody wants a list of outstanding work at four in the
    /// morning, and an earlier riser should not be waiting for seven.
    ///
    /// Asking always works, at any hour. This governs the unasked one only.
    pub not_before_hour: u8,
    /// Below this weight, items are counted and not listed.
    pub floor: Weight,
    /// Draft replies at all. Off until you have watched it read for a while.
    pub draft_replies: bool,
}

impl Default for BriefConfig {
    fn default() -> Self {
        BriefConfig {
            // On. It was off because the only source was an inbox this tree
            // cannot read, so a brief could only ever be empty; now it reads
            // handoffs, the backlog, scheduled work, bookings and drafts, all
            // of which are already on this machine. Nothing here sends, asks
            // or interrupts -- `run` is pure and the interruption gates are
            // `proactive`'s, unchanged.
            enabled: true,
            not_before_hour: 7,
            floor: Weight::Info,
            draft_replies: false,
        }
    }
}

fn mail_source() -> Source {
    Source::Mail
}

/// The most lines a brief may be. Past this it stops being read.
pub const MAX_LINES: usize = 12;

/// How many more of the things needing you the spoken brief names after the
/// first; the rest are counted.
pub const ALSO_NAMED: usize = 3;

/// Nothing in this module may send. Stated as a constant so the guard in
/// `tests/guards.rs` has something to check, and so deleting the separation
/// requires deleting something named.
pub const NEVER_SENDS: &str = "brief drafts; mail sends; approval is the boundary between them";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Brief {
    /// What to do first. The whole point of the run.
    pub start_with: Option<String>,
    /// Things needing you, in order.
    pub yours: Vec<Item>,
    /// Drafted and waiting.
    pub drafted: Vec<Item>,
    /// Dealt with. Named as a count, not a list.
    pub handled: usize,
    /// Below the floor. Counted so you can see it was not lost.
    pub ignored: usize,
    /// Clashes on the day, and prep that has not happened.
    pub conflicts: Vec<String>,
    /// The single thing holding up the most other things.
    pub blocking: Option<String>,
}

impl Brief {
    pub fn is_empty(&self) -> bool {
        self.yours.is_empty() && self.drafted.is_empty() && self.conflicts.is_empty()
    }

    /// Lines it would print. Used to keep it under `MAX_LINES`.
    pub fn lines(&self) -> usize {
        1 + self.yours.len() + self.drafted.len() + self.conflicts.len()
    }
}

/// Run the morning pass.
///
/// Pure: takes what arrived and what is committed, returns the brief. No IO,
/// no clock, no side effects — which is what makes the whole thing testable
/// without an inbox.
pub fn run(items: &[Item], day: &[Commitment], cfg: &BriefConfig) -> Brief {
    let mut b = Brief {
        start_with: None,
        yours: Vec::new(),
        drafted: Vec::new(),
        handled: 0,
        ignored: 0,
        conflicts: Vec::new(),
        blocking: None,
    };
    if !cfg.enabled {
        return b;
    }

    for it in items {
        if it.weight < cfg.floor {
            b.ignored += 1;
            continue;
        }
        match it.outcome {
            Outcome::Handled => b.handled += 1,
            Outcome::Ignored => b.ignored += 1,
            Outcome::Drafted => {
                // A draft with nothing drafted is a claim, not a draft.
                //
                // `draft_replies` asks whether *the brief* may compose a
                // reply, which its own config comment says plainly: "off
                // until you have watched it read for a while". It was being
                // used here as "may a draft be shown", and those are not the
                // same question — a post `publish` already wrote and is
                // holding for your yes is not the brief drafting anything,
                // and hiding it meant the morning greeting could not offer
                // the one thing it exists to offer. So the setting governs
                // mail, which is the only source the brief would ever write
                // for; a draft that arrived already written is reported.
                let brief_would_write_it = it.source == Source::Mail;
                let show = it.draft.is_some() && (cfg.draft_replies || !brief_would_write_it);
                if show {
                    b.drafted.push(it.clone());
                } else {
                    let mut promoted = it.clone();
                    promoted.outcome = Outcome::Yours;
                    promoted.draft = None;
                    b.yours.push(promoted);
                }
            }
            Outcome::Yours => b.yours.push(it.clone()),
        }
    }

    // Urgent first, then by arrival, so the order is defensible rather than
    // whatever the inbox happened to hand over.
    b.yours.sort_by(|x, y| y.weight.cmp(&x.weight).then(x.id.cmp(&y.id)));
    b.drafted.sort_by(|x, y| y.weight.cmp(&x.weight).then(x.id.cmp(&y.id)));

    // The day: overlaps, and prep that has not happened.
    for (i, c) in day.iter().enumerate() {
        for other in day.iter().skip(i + 1) {
            let (a_end, b_end) = (c.at_minute + c.minutes, other.at_minute + other.minutes);
            if c.at_minute < b_end && other.at_minute < a_end {
                b.conflicts.push(format!("{} overlaps {}", c.what, other.what));
            }
        }
        if let Some(p) = &c.needs_prep {
            b.conflicts.push(format!("{} needs {p} first", c.what));
        }
    }

    // What is holding up the most. Only claimed when something actually is —
    // an invented bottleneck is worse than none.
    b.blocking = most_blocking(items);

    b.start_with = b
        .yours
        .first()
        .map(|i| i.headline())
        .or_else(|| b.conflicts.first().cloned())
        .or_else(|| b.drafted.first().map(|i| format!("approve the reply to {}", i.from)));

    b
}

/// The one thing several others are waiting on, if there is one.
fn most_blocking(items: &[Item]) -> Option<String> {
    use std::collections::BTreeMap;
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for it in items {
        if let Some(c) = &it.conflicts_with {
            *counts.entry(c.as_str()).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .filter(|(_, n)| *n >= 2)
        .max_by_key(|(_, n)| *n)
        .map(|(k, n)| format!("{k} is holding up {n} other things"))
}

/// The brief, spoken. Leads with what to do, never with a count.
pub fn spoken(b: &Brief) -> String {
    if b.is_empty() {
        return "Nothing needs you. I'll get on with the rest.".into();
    }
    let mut out = Vec::new();
    if let Some(s) = &b.start_with {
        out.push(format!("Start with {s}."));
    }
    // 30 Sep 2026: the rest of what needs you was never said -- a brief with
    // six things in it spoke one ("Start with ...") and a count. The next few
    // are named, and the remainder counted.
    let rest: Vec<String> = b
        .yours
        .iter()
        .map(|i| i.headline())
        .filter(|h| b.start_with.as_deref() != Some(h.as_str()))
        .collect();
    if !rest.is_empty() {
        let named: Vec<&str> = rest.iter().take(ALSO_NAMED).map(|s| s.trim_end_matches('.')).collect();
        let more = rest.len().saturating_sub(ALSO_NAMED);
        let tail = if more > 0 { format!(", and {more} more on the hub") } else { String::new() };
        out.push(format!("Also: {}{tail}.", named.join("; ")));
    }
    if let Some(bl) = &b.blocking {
        out.push(format!("{bl}."));
    }
    for c in b.conflicts.iter().take(3) {
        out.push(format!("{c}."));
    }
    if !b.drafted.is_empty() {
        out.push(format!(
            "{} repl{} written and waiting on your yes.",
            b.drafted.len(),
            if b.drafted.len() == 1 { "y" } else { "ies" }
        ));
    }
    if b.handled > 0 {
        out.push(format!("I handled {}.", b.handled));
    }
    // Truncated rather than allowed to sprawl.
    out.truncate(MAX_LINES);
    out.join(" ")
}

// ---------------------------------------------------------------------------
// The parts that were already written, finally called.
//
// The first version of this module reimplemented triage, drafting and timing
// inside itself, and the wiring guard caught it: seven modules came off the
// unwired list while nothing in the running program actually reached them.
// That is precisely the drift the ratchet exists to stop, so the duplication
// was removed rather than the entries re-added.
// ---------------------------------------------------------------------------

/// What `mail` would do with this category, so the brief and the mail rules
/// cannot disagree about what "urgent" means.
fn action_for_item(category: &str, from: &str, cfg: &MailConfig) -> mail::Action {
    mail::action_for(category, Provider::from_address(from), cfg)
}

/// Run the mail rules over the raw categories and only then build the brief.
///
/// The weight on an item is not decided here — it comes from whether `mail`
/// considers its action reversible. Something Atlas can undo is information;
/// something it cannot is yours.
pub fn from_mail(
    raw: &[(String, String, String)],
    day: &[Commitment],
    mcfg: &MailConfig,
    cfg: &BriefConfig,
) -> Brief {
    let items: Vec<Item> = raw
        .iter()
        .map(|(id, from, category)| {
            let action = action_for_item(category, from, mcfg);
            let (weight, outcome) = if action.reversible() {
                (Weight::Info, Outcome::Handled)
            } else {
                (Weight::Urgent, Outcome::Yours)
            };
            Item {
                id: id.clone(),
                source: Source::Mail,
                from: from.clone(),
                subject: category.clone(),
                weight,
                outcome,
                draft: None,
                conflicts_with: None,
            }
        })
        .collect();
    run(&items, day, cfg)
}

/// Run a drafted reply past `draft`'s critique before it is ever shown.
///
/// A draft the brief is proud of and `draft` would have rejected is worse
/// than no draft, because you approve it on the brief's word.
pub fn vet_draft(text: &str, dcfg: &crate::draft::DraftConfig) -> Option<String> {
    let notes = crate::draft::critique(text, None, dcfg);
    crate::draft::revision_brief(&notes)
}

/// How long the morning run is allowed to take.
///
/// Bounded through `timebox` rather than by a number here, so a run that
/// stalls on one slow mailbox stops rather than eating the morning it was
/// supposed to save.
pub fn budget(now: u64) -> crate::timebox::Box_ {
    crate::timebox::Box_::start("the morning run", crate::timebox::Size::Long, now)
}

/// The brief as a chain, so a run that dies halfway says where it stopped.
///
/// Every step is marked reversible and `goes_out: false` — the morning run
/// reads and drafts, and `chain`'s own check is what enforces that rather
/// than a promise in this file.
pub fn as_chain() -> crate::chain::Chain {
    let step = |what: &str, needs: Option<&str>, produces: Option<&str>| crate::chain::Step {
        what: what.into(),
        app: "atlas".into(),
        reversible: true,
        goes_out: false,
        needs: needs.map(|x| x.into()),
        produces: produces.map(|x| x.into()),
    };
    crate::chain::Chain::new(
        "the morning run",
        vec![
            step("read what arrived", None, Some("mail")),
            step("sort it by what mail would do", Some("mail"), Some("triage")),
            step("put it against the day", Some("triage"), Some("clashes")),
            step("write what can be written", Some("triage"), Some("drafts")),
        ],
    )
}

/// Is the run due? Answered by `routine` rather than by comparing hours here,
/// so "every weekday morning" means the same thing everywhere in Atlas.
pub fn due(w: &crate::routine::Watcher, hour: u32, weekday: u32) -> bool {
    w.due_now(hour, weekday).iter().any(|r| r.name.contains("brief"))
}

/// Turn the one thing that needs you into a question `answering` can settle
/// by voice, by typing, or by a nod — the brief should not invent a fourth
/// way of saying yes.
pub fn ask(b: &Brief, t: u64) -> Option<crate::answering::Question> {
    let s = b.start_with.as_ref()?;
    let asked = format!("Start with {s}?");
    Some(crate::answering::Question::new(&asked, false, t))
}

// ---------------------------------------------------------------------------
// The half that was missing: where the items come from.
//
// Everything above decides what to do with a list of things that need you.
// Nothing ever built that list from anything but mail, and this tree has no
// mail reader — so `run` was called with two empty slices and a `BriefConfig`
// that could not be turned on, and the morning brief was a function that
// always returned the same empty answer.
//
// These are the sources that already exist on this machine, offline. Each one
// converts itself, so a new source is a new function rather than a change to
// `run`, and each is testable without the daemon.
// ---------------------------------------------------------------------------

/// How long a handoff may sit at the door before it is more than information.
pub const HANDOFF_WAITING_URGENT_SECS: u64 = 48 * 3600;

/// A friend's note or file, waiting at the door.
///
/// Always `Yours`: only you can decide to keep it or bin it, and Atlas
/// deliberately never reads one as an instruction — see
/// `household::Received`'s own note on that.
pub fn from_handoffs(inbox: &crate::household::Inbox, now: u64) -> Vec<Item> {
    inbox
        .items
        .iter()
        .map(|r| Item {
            id: format!("handoff:{}", r.id),
            source: Source::Handoff,
            from: r.from.clone(),
            // The note itself, trimmed to a subject. Shown, never obeyed.
            subject: first_words(&r.what, 12),
            // A thing a person sent you that has sat for two days has stopped
            // being news and started being something you are ignoring.
            weight: if now.saturating_sub(r.at) >= HANDOFF_WAITING_URGENT_SECS {
                Weight::Urgent
            } else {
                Weight::Info
            },
            outcome: Outcome::Yours,
            draft: None,
            conflicts_with: None,
        })
        .collect()
}

/// Requests Atlas could not finish, and what stopped each one.
///
/// `conflicts_with` carries the blocker rather than a related item, which is
/// what makes `most_blocking` say something true offline: "no connection is
/// holding up 3 other things" is exactly the sentence it was written to
/// produce, and until now nothing ever gave it the input.
pub fn from_backlog(b: &crate::backlog::Backlog) -> Vec<Item> {
    b.outstanding()
        .iter()
        .map(|i| Item {
            id: format!("blocked:{}", i.id),
            source: Source::Blocked,
            from: "me".into(),
            subject: i.request.clone(),
            // Something waiting on *you* is urgent; something waiting on the
            // world clears itself and is information until it doesn't.
            weight: if i.blocker.self_clearing() { Weight::Info } else { Weight::Urgent },
            outcome: Outcome::Yours,
            draft: None,
            conflicts_with: Some(i.blocker.explain()),
        })
        .collect()
}

/// Scheduled work: what failed, and what is still to come.
///
/// Returns both halves because they are two different things to a brief — a
/// job that failed needs you, a job that is due is the shape of your day.
pub fn from_jobs(s: &crate::scheduler::Scheduler, now: u64) -> (Vec<Item>, Vec<Commitment>) {
    let mut items = Vec::new();
    let mut day = Vec::new();
    for j in &s.jobs {
        match j.state {
            crate::scheduler::JobState::Failed => items.push(Item {
                id: format!("job:{}", j.id),
                source: Source::Job,
                from: "a scheduled job".into(),
                subject: j.command.clone(),
                // Atlas tried and it broke. That is not information.
                weight: Weight::Urgent,
                outcome: Outcome::Yours,
                draft: None,
                conflicts_with: j.last_result.clone(),
            }),
            crate::scheduler::JobState::AwaitingApproval => items.push(Item {
                id: format!("job:{}", j.id),
                source: Source::Job,
                from: "a scheduled job".into(),
                subject: j.command.clone(),
                weight: Weight::Info,
                outcome: Outcome::Yours,
                draft: None,
                conflicts_with: None,
            }),
            crate::scheduler::JobState::Pending => {
                // Only today's. A job due in a fortnight is not the shape of
                // this morning, and a day full of them is the list you stop
                // reading.
                if j.due >= now && j.due < now + 86_400 {
                    day.push(Commitment {
                        id: format!("job:{}", j.id),
                        what: j.command.clone(),
                        // Minutes from now rather than from midnight: the
                        // brief is read within an hour of being written, and
                        // this keeps it right without a timezone.
                        at_minute: (j.due.saturating_sub(now) / 60) as u32,
                        minutes: 1,
                        needs_prep: None,
                    });
                }
            }
            // Done and Cancelled are counted by `handled`, not listed.
            _ => {}
        }
    }
    (items, day)
}

/// Scheduled work that ran, and worked, since the brief last looked.
///
/// This is the "here is what I handled" half. Without it the brief is only
/// ever a list of demands, which is the accusation the module header is
/// written against.
pub fn handled_since(s: &crate::scheduler::Scheduler, since: u64) -> usize {
    s.jobs
        .iter()
        .filter(|j| j.state == crate::scheduler::JobState::Done && j.due >= since)
        .count()
}

/// Times someone proposed that nobody has answered.
pub fn from_bookings(proposals: &[crate::booking::Proposal], now: u64) -> Vec<Item> {
    crate::booking::going_stale(proposals, now)
        .iter()
        .map(|p| Item {
            id: format!("booking:{}", p.id),
            source: Source::Booking,
            from: p.from.clone(),
            subject: first_words(&p.their_words, 12),
            // `going_stale` already means "within two days and unanswered",
            // and silence becomes a no by default. That is not information.
            weight: Weight::Urgent,
            outcome: Outcome::Yours,
            draft: None,
            conflicts_with: None,
        })
        .collect()
}

/// Posts written and waiting on your yes.
///
/// `Outcome::Drafted` is exactly this: written, not sent. The module's first
/// rule — nothing sends — is `publish`'s rule too, so the two agree by
/// construction rather than by promise.
pub fn from_posts(p: &crate::publish::Publisher) -> Vec<Item> {
    p.pending()
        .iter()
        .filter(|post| post.state == crate::publish::PostState::AwaitingApproval)
        .map(|post| Item {
            id: format!("post:{}", post.id),
            source: Source::Post,
            // `Channel::name()` has existed all along and this did not call
            // it. On an `Email { to, subject }` the debug format put
            // `email { to: "…", subject: "…" }` into the morning brief.
            from: post.channel.name(),
            subject: first_words(&post.body, 12),
            weight: Weight::Info,
            outcome: Outcome::Drafted,
            draft: Some(post.body.clone()),
            conflicts_with: None,
        })
        .collect()
}

/// Atlas's own upkeep, where it needs a decision rather than a tidy-up.
///
/// Deliberately narrow. Housekeeping that Atlas can just do belongs in the
/// hourly sweep and never in a brief; what belongs here is the small set of
/// things it has decided it should not do without you.
pub fn from_upkeep(asking: &[String]) -> Vec<Item> {
    asking
        .iter()
        .enumerate()
        .map(|(n, what)| Item {
            id: format!("upkeep:{n}"),
            source: Source::Upkeep,
            from: "upkeep".into(),
            subject: what.clone(),
            weight: Weight::Info,
            outcome: Outcome::Yours,
            draft: None,
            conflicts_with: None,
        })
        .collect()
}

/// Everything the brief can read on this machine, with nothing plugged in.
///
/// Borrowed rather than owned, and every field a plain reference, so this
/// stays a view over what the daemon already holds instead of a second copy
/// that can disagree with it.
pub struct Sources<'a> {
    pub inbox: &'a crate::household::Inbox,
    pub backlog: &'a crate::backlog::Backlog,
    pub scheduler: &'a crate::scheduler::Scheduler,
    pub proposals: &'a [crate::booking::Proposal],
    pub publisher: &'a crate::publish::Publisher,
    /// Upkeep questions Atlas has decided it should not answer alone.
    pub upkeep: &'a [String],
    /// Mail, for the day there is a reader. Empty everywhere today, and named
    /// rather than omitted so that adding one is a change at one call site.
    pub mail: &'a [Item],
    /// Is the network up?
    ///
    /// A brief that leads with an email you cannot open is worse than one
    /// that leads with the second thing. Offline, sources that need a network
    /// are left out rather than listed and unactionable — they have not gone
    /// away, and they come back the moment the router does.
    pub online: bool,
}

/// Collect what needs you, from what is already here.
///
/// Pure, like `run`: it reads the collections it is handed and returns two
/// lists. `now` is passed rather than read so the whole thing stays testable
/// without a clock.
pub fn gather(s: &Sources, now: u64) -> (Vec<Item>, Vec<Commitment>) {
    let (job_items, day) = from_jobs(s.scheduler, now);
    let mut items = Vec::new();
    items.extend(from_handoffs(s.inbox, now));
    items.extend(from_backlog(s.backlog));
    items.extend(job_items);
    items.extend(from_bookings(s.proposals, now));
    items.extend(from_posts(s.publisher));
    items.extend(from_upkeep(s.upkeep));
    items.extend(s.mail.iter().cloned());
    if !s.online {
        items.retain(|i| !i.source.needs_network());
    }
    (items, day)
}

/// The brief, built from this machine rather than from an inbox.
///
/// `since` is when the brief last looked, which is what makes "I handled 4"
/// mean something rather than counting every job that ever ran.
pub fn from_here(s: &Sources, cfg: &BriefConfig, now: u64, since: u64) -> Brief {
    let (items, day) = gather(s, now);
    let mut b = run(&items, &day, cfg);
    if cfg.enabled {
        b.handled += handled_since(s.scheduler, since);
    }
    b
}

/// First `n` words, so a note or a post body reads as a subject line.
///
/// Trimmed rather than truncated mid-word, and a newline ends it — a subject
/// that runs to three lines is the body again.
fn first_words(text: &str, n: usize) -> String {
    let first_line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    let words: Vec<&str> = first_line.split_whitespace().take(n).collect();
    let joined = words.join(" ");
    if first_line.split_whitespace().count() > n {
        format!("{joined}…")
    } else {
        joined
    }
}
