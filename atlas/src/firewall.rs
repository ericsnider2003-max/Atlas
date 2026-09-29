//! The line between your own work and a business you share.
//!
//! ## The decision this implements
//!
//! Eric said twice that this must not be specified without him, and then
//! specified it: **block, notify, and pause the item** — and the boundary runs
//! **one way**. Personal never reaches a business space. Business freely
//! reaches his own Atlas.
//!
//! One-way because that is where the harm actually is. His files reaching a
//! customer is the thing that cannot be undone; a business's task data
//! reaching his own Atlas is not a problem at all. A boundary built to hold in
//! both directions is twice as much machinery guarding one real risk, and the
//! half that guards nothing is the half that will break something.
//!
//! ## Block, notify, pause — three things, not three names for one
//!
//! - **Block.** It does not cross. Not later, not partially, not as a
//!   summary. That is settled before anything else happens.
//! - **Notify.** He is told, through `notify`, which already knows how to
//!   reach him whether he is at the desk, away, or out. A silently blocking
//!   firewall is indistinguishable from a broken feature: he would never learn
//!   that a business task kept failing because it needed something it could
//!   never have.
//! - **Pause.** The item is held, not dropped. Blocking without holding means
//!   the work is simply lost and has to be noticed and redone; holding means
//!   he can look at what it was, and release that one item if it was fine.
//!
//! ## What is never written down here
//!
//! The held list records **what the thing was called and where it was going,
//! never what was in it**. A firewall that logs the contents of what it
//! blocked has copied that content across the boundary into its own log, and
//! the log is the one file nobody thinks of as sensitive.
//!
//! ## Default deny
//!
//! Anything whose origin is not positively known to be that business's own is
//! personal. There is no third state and no benefit of the doubt: a boundary
//! that is unsure and lets things through is not a boundary, and "I couldn't
//! tell" is exactly the case it exists for.

use crate::earned::Space;
use crate::error::Result;
use crate::notify::{Note, Urgency};
use crate::store::Store;
use serde::{Deserialize, Serialize};

/// Whether a thing may be seen in a business space.
#[derive(Debug, Clone, PartialEq)]
pub enum Crossing {
    /// It belongs to that business already, so there is no boundary to cross.
    Allowed,
    /// It does not, so it stops here. Held under this number.
    Stopped { held: u64, why: String },
}

impl Crossing {
    pub fn allowed(&self) -> bool {
        matches!(self, Crossing::Allowed)
    }
}

/// Something that was on its way out and did not go.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Held {
    pub id: u64,
    /// What it was called. Never what was in it.
    pub what: String,
    /// The business space it was heading for.
    pub into: String,
    pub when: u64,
    pub why: String,
    /// Eric looked at it and said it was fine. Releases this one item and
    /// nothing else — the rule is untouched.
    pub released: bool,
}

/// The boundary, and everything paused at it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Firewall {
    held: Vec<Held>,
    next_id: u64,
}

impl Firewall {
    pub fn load(store: &Store) -> Firewall {
        store.load("firewall")
    }

    pub fn save(&self, store: &Store) -> Result<()> {
        store.save("firewall", self)
    }

    /// May this be seen in that business space?
    ///
    /// `from` is where the thing originated, not where it happens to be
    /// sitting. A personal note copied into a business folder is still
    /// personal, and asking where it is rather than where it came from is how
    /// a boundary gets walked around by moving a file.
    pub fn check(&mut self, from: &Space, into: &str, what: &str, when: u64) -> Crossing {
        let Some(why) = self.would_stop(from, into) else {
            return Crossing::Allowed;
        };
        // An empty destination is a caller bug, not a crossing. Nothing has
        // gone anywhere, so nothing is held — the `0` says there is no id to
        // release.
        if into.trim().is_empty() {
            return Crossing::Stopped { held: 0, why };
        }
        self.next_id += 1;
        let id = self.next_id;
        self.held.push(Held {
            id,
            what: describe(what),
            into: into.to_string(),
            when,
            why: why.clone(),
            released: false,
        });
        Crossing::Stopped { held: id, why }
    }

    /// Would this be stopped, and why — without holding anything.
    ///
    /// ## Why this is separate from `check`
    ///
    /// `check` is the enforcement path. It takes `&mut self`, allocates an
    /// id, pushes a `Held` and is meant to be called when something is
    /// actually crossing.
    ///
    /// `atlas shared check` called it, and then printed **"Held as {id}.
    /// Nothing has actually moved."** — followed by `wall.save(&store)`. The
    /// sentence is true about the file and false about the wall: every
    /// invocation of a subcommand named *check* created a real hold and wrote
    /// it to disk, so asking the same question three times left three entries
    /// in `atlas shared list`, each with an id you could "release".
    ///
    /// The two halves live in one function so they cannot disagree about what
    /// crosses: `check` is this plus the recording.
    ///
    /// `None` means it would go through.
    pub fn would_stop(&self, from: &Space, into: &str) -> Option<String> {
        let into = into.trim();
        if into.is_empty() {
            // Nowhere to go is not a crossing. Said rather than silently
            // allowed, because an empty space name is a caller bug and
            // treating it as "fine" would be the quietest possible hole.
            return Some(
                "there's no business space named, so I've not sent it anywhere".into(),
            );
        }
        if let Space::Business(owner) = from {
            if owner.trim().eq_ignore_ascii_case(into) {
                return None;
            }
        }
        // Everything else: personal, or another business's. Both stop.
        Some(match from {
            Space::Personal => format!("that's your own work, and {into} is a shared space"),
            Space::Business(other) => format!("that belongs to {}, not to {into}", other.trim()),
        })
    }

    /// What Atlas would say if this actually crossed.
    ///
    /// The notification text without the hold behind it, so a dry run can
    /// show the third leg of block/pause/notify without creating anything.
    pub fn would_say(&self, into: &str, what: &str, why: &str, when: u64) -> Note {
        note(&Held {
            id: 0,
            what: describe(what),
            into: into.trim().to_string(),
            when,
            why: why.to_string(),
            released: false,
        })
    }

    /// What is paused, newest first.
    pub fn waiting(&self) -> Vec<&Held> {
        let mut out: Vec<&Held> = self.held.iter().filter(|h| !h.released).collect();
        out.sort_by(|a, b| b.when.cmp(&a.when));
        out
    }

    pub fn get(&self, id: u64) -> Option<&Held> {
        self.held.iter().find(|h| h.id == id)
    }

    /// He looked at it and it was fine.
    ///
    /// Releases exactly this item. It does not widen the rule, teach the
    /// boundary anything, or make the next one of its kind pass — a firewall
    /// that learns from being overruled is one that eventually stops
    /// refusing.
    pub fn release(&mut self, id: u64) -> std::result::Result<String, String> {
        match self.held.iter_mut().find(|h| h.id == id) {
            Some(h) if h.released => Err(format!("{} has already gone.", h.what)),
            Some(h) => {
                h.released = true;
                Ok(format!("Alright — {} can go to {}.", h.what, h.into))
            }
            None => Err(format!("I don't have anything held under {id}.")),
        }
    }

    /// He looked at it and it should never have been going.
    pub fn forget(&mut self, id: u64) -> std::result::Result<String, String> {
        match self.held.iter().position(|h| h.id == id) {
            Some(i) => {
                let gone = self.held.remove(i);
                Ok(format!("Dropped — {} isn't going anywhere.", gone.what))
            }
            None => Err(format!("I don't have anything held under {id}.")),
        }
    }

    /// Said out loud.
    pub fn spoken(&self) -> String {
        let waiting = self.waiting();
        match waiting.len() {
            0 => "Nothing's waiting at the line between your work and anything shared.".into(),
            1 => format!(
                "One thing stopped at the line: {} was heading for {}. {}.",
                waiting[0].what, waiting[0].into, waiting[0].why
            ),
            n => format!(
                "{n} things stopped at the line between your work and what's shared. The most \
                 recent: {} heading for {}.",
                waiting[0].what, waiting[0].into
            ),
        }
    }
}

/// The notification for one blocked item.
///
/// Marked private, so on a screen somebody else can see it knocks rather than
/// naming the thing. A notification about a boundary that reads out the name
/// of what it stopped, in front of the person it was stopped from, has
/// defeated itself.
pub fn note(held: &Held) -> Note {
    Note::new(
        "Stopped at the line",
        &format!(
            "{} was on its way to {} and hasn't gone. {}. It's held — say \"release {}\" if \
             that was fine.",
            held.what, held.into, held.why, held.id
        ),
        Urgency::Routine,
        held.when,
    )
    .private()
}

/// What a thing is called, trimmed to a name and never a body.
///
/// A path keeps only its last part: the folders above a file are themselves
/// personal information, and "Tax/2025/settlement-letter.pdf" says a great
/// deal more than "settlement-letter.pdf".
fn describe(what: &str) -> String {
    let what = what.trim();
    let last = what
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(what)
        .trim()
        .to_string();
    if last.is_empty() {
        return "something with no name".into();
    }
    // Long names are usually the contents pasted in by mistake, which is the
    // one thing this file must never store.
    if last.chars().count() > 80 {
        let short: String = last.chars().take(60).collect();
        return format!("{short}…");
    }
    last
}
