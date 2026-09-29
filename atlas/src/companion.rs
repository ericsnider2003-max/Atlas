//! Atlas on your phone.
//!
//! An assistant that needs you to be at a particular laptop is a filing
//! cabinet with opinions. The point is that what you were doing is where you
//! are.
//!
//! ## Superseded — see `sync.rs`
//!
//! This module was built on the idea that the phone should be a window rather
//! than a real Atlas, to avoid two copies of your state drifting apart. That
//! was the wrong call: it made Atlas useless for the months a laptop is off,
//! which is most of the point of having it.
//!
//! `sync.rs` has the answer — sync *what happened* rather than *state*, and
//! two full copies merge cleanly however long they've been apart. What's left
//! here is still true and still used: the rules about what must never travel
//! to a device you might lose in a taxi.

use serde::{Deserialize, Serialize};

/// What can travel to the phone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Piece {
    /// The outstanding list.
    Outstanding,
    /// Projects and where each stands.
    Projects,
    /// Notes you've captured.
    Notes,
    /// What Atlas last said.
    LastBrief,
    /// Which accounts have recovery codes, and how many are left.
    CodeCounts,
    /// The conversation, so you can carry on where you left off.
    Thread,
}

impl Piece {
    /// Is it safe on a device you might lose?
    ///
    /// A phone gets left in a taxi. Everything that travels has to be
    /// survivable on a lost device, which rules out most of what's valuable
    /// on the laptop.
    pub fn safe_on_a_phone(&self) -> bool {
        !matches!(self, Piece::CodeCounts)
    }

    /// Readable only, or can you change it there?
    pub fn writable(&self) -> bool {
        matches!(self, Piece::Notes | Piece::Thread | Piece::Outstanding)
    }
}

/// Things that never leave the laptop.
pub fn never_travels() -> Vec<(&'static str, &'static str)> {
    vec![
        ("the vault", "a passphrase-locked file is only as safe as the device it's on, and a phone is the device you're most likely to lose"),
        ("your credentials", "same reason, and worse — they're the thing that opens everything else"),
        ("recovery codes", "they're on paper on purpose"),
        ("the index of your files", "it's a map of everything you have, which is worth more than most single files"),
    ]
}

/// Something you did on the phone that has to reach the laptop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pending {
    pub id: u64,
    /// What you did.
    pub what: String,
    pub piece: Piece,
    pub at: u64,
    /// Reached the laptop.
    pub landed: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct CompanionConfig {
    pub enabled: bool,
    /// What to mirror.
    pub mirror: Vec<String>,
    /// Hold things captured while the laptop is off, and send them later.
    pub queue_while_offline: bool,
    /// Days of captures the phone will hold before it starts warning.
    pub hold_days: u32,
    /// Never mirror anything that opens something else. Not configurable.
    #[serde(skip, default = "never")]
    pub mirrors_secrets: bool,
}

fn never() -> bool {
    false
}

impl Default for CompanionConfig {
    fn default() -> Self {
        CompanionConfig {
            enabled: false,
            mirror: vec![
                "outstanding".into(),
                "projects".into(),
                "notes".into(),
                "last_brief".into(),
                "thread".into(),
            ],
            queue_while_offline: true,
            // Long enough for a deployment.
            hold_days: 200,
            mirrors_secrets: false,
        }
    }
}

/// The phone's side.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Phone {
    /// Everything captured that hasn't reached the laptop.
    pub pending: Vec<Pending>,
    /// When the mirror was last refreshed.
    pub mirrored_at: Option<u64>,
    next_id: u64,
}

impl Phone {
    /// Put something in. Works with the laptop off, which is the point.
    pub fn capture(&mut self, what: &str, piece: Piece, now: u64) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.pending.push(Pending {
            id,
            what: what.into(),
            piece,
            at: now,
            landed: false,
        });
        id
    }

    pub fn waiting(&self) -> Vec<&Pending> {
        self.pending.iter().filter(|p| !p.landed).collect()
    }

    /// How stale the mirror is.
    pub fn mirror_age_days(&self, now: u64) -> Option<u64> {
        self.mirrored_at.map(|t| now.saturating_sub(t) / 86_400)
    }

    /// What the phone says about itself.
    ///
    /// Being honest about staleness matters more here than anywhere: acting on
    /// a six-month-old task list is worse than having none.
    pub fn state(&self, now: u64, cfg: &CompanionConfig) -> String {
        let waiting = self.waiting().len();
        match self.mirror_age_days(now) {
            // NOT "nothing from the laptop yet", which is what this said.
            //
            // `atlas mobile mirror` loads the `phone_mirror` record and nothing in
            // the tree ever saves it, so `mirrored_at` is always `None` --
            // on a phone that has never synced and on one that synced an
            // hour ago, identically. See `unbuilt()`.
            None => match unbuilt() {
                Some(why) => format!("I can't tell you. {why}"),
                None => "Nothing from the laptop yet.".into(),
            },
            Some(days) if days == 0 => {
                if waiting == 0 {
                    "Up to date.".into()
                } else {
                    format!("Up to date. {waiting} things waiting to go back.")
                }
            }
            Some(days) => {
                let mut s = format!("Last synced {days} days ago — treat this as a snapshot.");
                if waiting > 0 {
                    s.push_str(&format!(" {waiting} things waiting to go back."));
                }
                if days > cfg.hold_days as u64 {
                    s.push_str(" That's long enough that some of it will be wrong.");
                }
                s
            }
        }
    }
}

/// Why the mirror's state is unknown rather than empty, while it is.
///
/// `None` the moment something writes the `phone_mirror` record. Until then
/// this is the honest answer, and
/// `tests/the_mirror_knows_it_was_never_filled.rs` fails if the note and the
/// wiring disagree.
///
/// ## What is missing, and what is not
///
/// Everything in this module is built: `capture` takes something with the
/// laptop off, `waiting` lists what has not landed, `merge` decides what
/// happens when the laptop comes back, and `state` turns an age into a
/// sentence with the right warning attached. What does not exist is the
/// **other end** -- a phone-side client that captures into this type and a
/// sync that writes it -- so `Phone::capture` has no production caller and
/// the record has no writer.
///
/// That is a real gap and this says so, rather than a `mirrored_at` of
/// `None` reading as a fact about a phone.
pub fn unbuilt() -> Option<&'static str> {
    Some(
        "nothing writes the phone mirror yet -- there is no phone-side client to \
         capture into it and no sync to save it -- so the answer is the same \
         whatever your phone has actually got.",
    )
}

/// What happens when the laptop comes back.
#[derive(Debug, Clone, PartialEq)]
pub enum Merge {
    /// Nothing to do.
    Nothing,
    /// Take it all in — none of it clashes.
    TakeItAll { count: usize },
    /// Some of it needs you.
    NeedsYou { clean: usize, clashes: Vec<String> },
}

/// Bring the phone's captures back.
///
/// The rule that avoids the hard problem: **notes and captures always merge,
/// because adding a note can't conflict with anything.** Only edits to
/// something that also changed on the laptop need a decision, and those are
/// rare because you're one person.
pub fn merge(pending: &[Pending], changed_on_laptop: &[String]) -> Merge {
    if pending.is_empty() {
        return Merge::Nothing;
    }
    let mut clean = 0;
    let mut clashes = Vec::new();

    for p in pending {
        // Anything additive just lands.
        if matches!(p.piece, Piece::Notes | Piece::Thread) {
            clean += 1;
            continue;
        }
        if changed_on_laptop.iter().any(|c| c.eq_ignore_ascii_case(&p.what)) {
            clashes.push(p.what.clone());
        } else {
            clean += 1;
        }
    }

    if clashes.is_empty() {
        Merge::TakeItAll { count: clean }
    } else {
        Merge::NeedsYou { clean, clashes }
    }
}

/// What Atlas says when it comes back to a pile of captures.
pub fn on_return(m: &Merge, away_days: u64) -> String {
    match m {
        Merge::Nothing => String::new(),
        Merge::TakeItAll { count } => format!(
            "{count} things from your phone while I was off. Taken in, in the order you \
             said them."
        ),
        Merge::NeedsYou { clean, clashes } => format!(
            "{clean} things from your phone taken in. {} clashed with something here — {}. \
             Which version?",
            clashes.len(),
            clashes.join(", ")
        ),
    }
    .to_string()
        + &if away_days > 30 {
            format!(" You were gone {away_days} days, so I'd treat anything I was mid-way through as stale.")
        } else {
            String::new()
        }
}

/// How the phone reaches the laptop, without a server.
///
/// Three ways, in order of how little they cost you.
pub fn how_they_talk() -> Vec<(&'static str, &'static str, bool)> {
    vec![
        (
            "the same network",
            "when both are on your wifi, the phone talks to the laptop directly — nothing in \
             between, nothing stored anywhere",
            true,
        ),
        (
            "your cloud folder",
            "the phone writes an encrypted file, the laptop picks it up next time it's on. Works \
             when they're never on together, which is your case",
            true,
        ),
        (
            "a shared note",
            "crudest option and it works everywhere — the phone appends, Atlas reads it and \
             clears it",
            true,
        ),
    ]
}

/// The honest limit of a phone client.
pub const WHAT_IT_CANNOT_DO: &str =
    "The phone can't reach your laptop's files or windows, and it thinks with a smaller model. \
     Everything else is the same Atlas. The vault and your credentials stay on the laptop — not \
     because the phone is less trusted, but because it's the device you're most likely to lose.";

/// Why not just run all of it on the phone.
pub const WHY_NOT_TWO_ATLASES: &str =
    "This was my earlier reasoning and it was wrong. Two copies of your *state* do drift — but \
     syncing what happened rather than what is means they don't, however long they've been \
     apart. See sync.rs.";
