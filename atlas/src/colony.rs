//! The colony: Atlas and its crew drawn as a small top-down settlement.
//!
//! This is Eric's Easter egg (build plan: "Atlas Colony — Easter Egg Build
//! Plan", 16 Sep 2026). It is found by a long press on Atlas's mark in the
//! hub, and nowhere else: no menu item, no setting, no hint. Nobody has to
//! know it exists to use Atlas.
//!
//! Two layers, kept apart on purpose:
//!
//! - **What's real** comes from here. Every worker on the map is an errand
//!   the crew or a window job actually has in hand; the cafeteria holds the
//!   crew's free hands; the boardroom lights only while a council is really
//!   sitting, and its console shows the last real convening; a capsule
//!   goes down the tube each time work is really handed out or a message
//!   really arrives. Nothing here is invented.
//! - **Atmosphere** (weather, day and night, wandering between jobs, which
//!   prop a worker holds) is drawn by the page and claims nothing.
//!
//! The feed is read-only. The page's buttons (hold, carry on, call off, and
//! "tell Atlas") go through the same paths as saying those things to Atlas;
//! the colony is a shortcut, never a second control surface.
//!
//! Built 30 Sep 2026. Personal Atlas only: nothing trading-side is mapped.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Where the store keeps the last time the council sat.
pub const CONVENED_KEY: &str = "colony_convened";

/// How many finished errands and capsules the colony remembers.
const KEEP: usize = 24;

/// One building. The set is data, not a fixed drawing, so a new workspace
/// is a new entry here and takes the next free plot on the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Zone {
    /// Atlas's own office, north of the plaza: where it goes to focus.
    Office,
    /// Research: looking things up, reading, weighing.
    Lab,
    /// Business plans and outreach, assembled from research.
    Factory,
    /// Messages in and out; the hub of the tube.
    MailRoom,
    /// Photos, video, pictures, and anything printed or built to hand out.
    Printing,
    /// Code, upkeep, Atlas's own machinery, and work driven in a window.
    ComputerLab,
    /// Where free hands wait.
    Cafeteria,
    /// Where the council sits.
    Boardroom,
}

impl Zone {
    pub const ALL: [Zone; 8] = [
        Zone::Office,
        Zone::Lab,
        Zone::Factory,
        Zone::Printing,
        Zone::MailRoom,
        Zone::Cafeteria,
        Zone::ComputerLab,
        Zone::Boardroom,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Zone::Office => "office",
            Zone::Lab => "lab",
            Zone::Factory => "factory",
            Zone::MailRoom => "mail_room",
            Zone::Printing => "printing",
            Zone::ComputerLab => "computer_lab",
            Zone::Cafeteria => "cafeteria",
            Zone::Boardroom => "boardroom",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Zone::Office => "Atlas's office",
            Zone::Lab => "Laboratory",
            Zone::Factory => "Factory",
            Zone::MailRoom => "Mail room",
            Zone::Printing => "Printing room",
            Zone::ComputerLab => "Computer lab",
            Zone::Cafeteria => "Cafeteria",
            Zone::Boardroom => "Boardroom",
        }
    }

    pub fn what(self) -> &'static str {
        match self {
            Zone::Office => "Where Atlas works when it's focused on something for you.",
            Zone::Lab => "Research: looking things up, reading, weighing.",
            Zone::Factory => "Plans, drafts and outreach, built from what the lab found.",
            Zone::MailRoom => "Messages in and out. Every capsule starts here.",
            Zone::Printing => "Photos, video, pictures, and builds to hand out.",
            Zone::ComputerLab => "Code, upkeep, Atlas's own machinery, and work in a window.",
            Zone::Cafeteria => "Free hands, waiting for something to do.",
            Zone::Boardroom => "The council sits here when Atlas asks the room.",
        }
    }
}

/// Which building an errand is worked in, by the crew's name for it.
///
/// Anything not named here is research-shaped (the lab), which is what an
/// unknown errand most often is. `tests/the_colony_is_real.rs` holds every
/// errand the daemon hands out to a building other than by that fallback.
pub fn zone_for(errand: &str) -> Zone {
    match errand {
        "council" => Zone::Boardroom,
        "research" | "search-check" | "read-file" | "screen-words" | "decide" | "fold" | "call-notes" => Zone::Lab,
        "draft" | "outreach" | "hunt" | "opportunities" => Zone::Factory,
        "mail" | "mail-sort" | "mail-sort-apply" | "unsubscribe" | "friend-knock" | "conversation-reply" | "post" => {
            Zone::MailRoom
        }
        "photo" | "edit-media" | "pictures" | "video" => Zone::Printing,
        "code" | "build" | "improve" | "housekeeping" | "backup" | "reclaim" | "security-change" | "model-piece"
        | "draft-model" | "mcp" | "move-files" | "sign-in" | "sign-up" | "teach-gesture" | "outlook-connect" => {
            Zone::ComputerLab
        }
        e if e.starts_with("social-") => Zone::Lab,
        _ => Zone::Lab,
    }
}

/// The errand's name as a person would say it.
pub fn plain(errand: &str) -> String {
    let s = match errand {
        "research" => "Looking something up",
        "search-check" => "Checking a search",
        "read-file" => "Reading a file",
        "screen-words" => "Reading the screen",
        "decide" => "Weighing a decision",
        "fold" => "Tidying what it learned",
        "call-notes" => "Writing up a call",
        "draft" => "Drafting",
        "outreach" => "Outreach",
        "mail" => "Handling mail",
        "mail-sort" => "Sorting mail",
        "mail-sort-apply" => "Filing mail",
        "unsubscribe" => "Unsubscribing",
        "friend-knock" => "Knocking on a friend's Atlas",
        "conversation-reply" => "Writing a reply",
        "post" => "Posting",
        "photo" => "Editing a photo",
        "edit-media" => "Editing media",
        "pictures" => "Looking at pictures",
        "code" => "Writing code",
        "build" => "Building",
        "improve" => "Improving Atlas",
        "housekeeping" => "Housekeeping",
        "backup" => "Backing up",
        "reclaim" => "Looking for space",
        "security-change" => "A security change",
        "model-piece" => "Fetching part of a model",
        "draft-model" => "Setting up a model",
        "mcp" => "Using an add-on",
        "move-files" => "Moving files",
        "sign-in" => "Signing in",
        "sign-up" => "Signing up",
        "teach-gesture" => "Learning a gesture",
        "outlook-connect" => "Connecting Outlook",
        "council" => "The council is sitting",
        other => return sentence_case(&other.replace(['-', '_'], " ")),
    };
    s.to_string()
}

fn sentence_case(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Where one worker stands, for the pose it's drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Standing {
    Working,
    /// Asked to hold and not yet at a safe point.
    Pausing,
    /// Held: nothing lost, nothing moving.
    Holding,
    /// Queued for a free hand.
    Waiting,
}

/// One worker on the map: an errand really in hand.
#[derive(Debug, Clone, Serialize)]
pub struct Worker {
    pub id: u64,
    /// "crew" or "window".
    pub kind: &'static str,
    pub what: String,
    pub task: String,
    pub zone: Zone,
    pub standing: Standing,
    pub since: u64,
    /// Can it hold at a safe point, or only finish or be called off?
    pub can_hold: bool,
}

/// The last time the council sat, for the boardroom's console.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Convened {
    pub at: u64,
    pub topic: String,
    /// The room's call, first sentence only.
    pub call: String,
}

/// One errand that finished, for the flag over its bench.
#[derive(Debug, Clone, Serialize)]
pub struct Finished {
    pub what: String,
    pub zone: Zone,
    /// `None` when it was called off rather than finishing either way.
    pub ok: Option<bool>,
    pub at: u64,
}

/// One capsule down the tube.
#[derive(Debug, Clone, Serialize)]
pub struct Capsule {
    pub at: u64,
    pub to: Zone,
}

/// What the colony remembers between ticks. Everything here is written by
/// the daemon as it happens; the page only reads it.
#[derive(Debug, Default)]
pub struct Log {
    finished: VecDeque<Finished>,
    capsules: VecDeque<Capsule>,
    pub convened: Option<Convened>,
}

impl Log {
    /// Work was handed out: a capsule leaves the mail room for its building.
    pub fn dispatched(&mut self, errand: &str, t: u64) {
        push(&mut self.capsules, Capsule { at: t, to: zone_for(errand) });
    }

    /// Something arrived from outside: a capsule lands in the mail room.
    pub fn arrived(&mut self, t: u64) {
        push(&mut self.capsules, Capsule { at: t, to: Zone::MailRoom });
    }

    /// An errand ended.
    pub fn finished(&mut self, errand: &str, ok: Option<bool>, t: u64) {
        push(&mut self.finished, Finished { what: plain(errand), zone: zone_for(errand), ok, at: t });
    }

    /// The council sat. Returns what to keep, so the caller can save it.
    pub fn sat(&mut self, topic: &str, said: &str, t: u64) -> Convened {
        let c = Convened { at: t, topic: topic.trim().to_string(), call: first_sentence(said) };
        self.convened = Some(c.clone());
        c
    }

    pub fn recent_finished(&self) -> impl Iterator<Item = &Finished> {
        self.finished.iter()
    }

    pub fn recent_capsules(&self) -> impl Iterator<Item = &Capsule> {
        self.capsules.iter()
    }
}

fn push<T>(q: &mut VecDeque<T>, v: T) {
    q.push_back(v);
    while q.len() > KEEP {
        q.pop_front();
    }
}

fn first_sentence(s: &str) -> String {
    let s = s.trim();
    let end = s
        .char_indices()
        .find(|(i, c)| matches!(c, '.' | '!' | '?') && s[i + 1..].starts_with(' '))
        .map(|(i, _)| i + 1)
        .unwrap_or(s.len());
    let one = &s[..end];
    if one.chars().count() > 160 {
        let cut: String = one.chars().take(157).collect();
        format!("{}…", cut.trim_end())
    } else {
        one.to_string()
    }
}

/// What Atlas itself is doing.
#[derive(Debug, Clone, Serialize, Default)]
pub struct AtlasNow {
    /// At its desk on something for you, rather than out walking the colony.
    pub busy: bool,
    pub doing: String,
}

/// Everything the page draws, as of now.
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub now: u64,
    pub atlas: AtlasNow,
    /// How many hands the crew has in all.
    pub hands: usize,
    pub workers: Vec<Worker>,
    pub council_sitting: bool,
    pub council_topic: Option<String>,
    pub convened: Option<Convened>,
    pub finished: Vec<Finished>,
    pub capsules: Vec<Capsule>,
    pub zones: Vec<ZoneInfo>,
    pub paused: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ZoneInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub what: &'static str,
}

pub fn zones() -> Vec<ZoneInfo> {
    Zone::ALL.iter().map(|z| ZoneInfo { id: z.id(), name: z.name(), what: z.what() }).collect()
}

/// The hidden page. One file, everything inline: no fonts, pictures or
/// scripts from anywhere, so it draws the same with no network at all.
pub fn page() -> String {
    PAGE.to_string()
}

const PAGE: &str = include_str!("colony_page.html");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_zone_has_words() {
        for z in Zone::ALL {
            assert!(!z.name().is_empty() && !z.what().is_empty());
        }
    }

    #[test]
    fn the_log_keeps_only_the_recent() {
        let mut l = Log::default();
        for i in 0..100 {
            l.dispatched("research", i);
        }
        assert_eq!(l.recent_capsules().count(), KEEP);
        assert_eq!(l.recent_capsules().next().unwrap().at, 100 - KEEP as u64);
    }

    #[test]
    fn the_call_is_one_sentence() {
        assert_eq!(first_sentence("Build it. The sceptic disagrees."), "Build it.");
        assert_eq!(first_sentence("v1.2 is fine"), "v1.2 is fine");
    }
}
