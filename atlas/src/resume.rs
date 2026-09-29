//! What Atlas was doing when it stopped, and what it does about it when it
//! starts again.
//!
//! Before this, a restart lost everything in hand: a window being worked for
//! you, research half done, a council mid-debate. Nothing said so; you'd find
//! out by the answer never coming. Now each piece of work you asked for is
//! written down as it's handed to the crew and crossed off when it ends, and
//! a window being worked is written down whenever it changes. What's still
//! written down at start-up is what was cut off.
//!
//! What happens to each depends on what redoing it would do:
//!
//! - **Again** — work whose only effect is an answer or a proposal (research,
//!   the council, building or improving code that lands in a queue for your
//!   go-ahead). Redone once, from your own words, and said so. A piece of
//!   work that was already redone once after a restart isn't redone a second
//!   time: if it was what brought Atlas down, doing it again would bring it
//!   down again.
//! - **Ask** — work that reaches outside the machine or acts as you (mail,
//!   unsubscribing, outreach, signing in to Outlook), or that depended on a
//!   moment that has passed (a look at your screen, a call's write-up). Named,
//!   never redone on its own.
//! - Chores Atlas starts itself (backups, housekeeping, the search check)
//!   aren't recorded at all: they come round again on their own.

use serde::{Deserialize, Serialize};

/// A piece of work you asked for, as it was handed to the crew.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Unfinished {
    /// The errand's kind (`crew` label).
    pub label: String,
    /// What it was about, as `which_errand` names it.
    #[serde(default)]
    pub topic: Option<String>,
    /// Your words that started it — what's said again to redo it.
    pub asked: String,
    pub started: u64,
    /// How many times it has already been redone after a restart.
    #[serde(default)]
    pub redone: u32,
}

/// What to do with a kind of work that was cut off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum After {
    Again,
    Ask,
    /// Not recorded: Atlas's own chores, and window replies (the window job
    /// itself is restored, and writes its next reply afresh).
    Skip,
}

pub fn what_to_do_with(label: &str) -> After {
    match label {
        "research" | "council" | "build" | "improve" => After::Again,
        "mail" | "unsubscribe" | "outreach" | "outlook-connect" | "pictures" | "call-notes"
        // Acting as you on a site, or a code that has expired by now.
        | "code" | "sign-in" | "security-change" | "sign-up"
        | "mail-sort" | "mail-sort-apply" | "post" | "move-files" | "edit-media" | "photo" | "decide" | "teach-gesture"
        // Another program's tool: it may have acted; never redone unasked.
        | "mcp" => After::Ask,
        _ => After::Skip,
    }
}

/// Where the list lives in the store.
pub const RECORD: &str = "unfinished_errands";

/// Split what was cut off into what's redone now and what's only named.
pub fn sort(left: Vec<Unfinished>) -> (Vec<Unfinished>, Vec<Unfinished>) {
    let mut again = Vec::new();
    let mut ask = Vec::new();
    for u in left {
        match what_to_do_with(&u.label) {
            After::Again if u.redone == 0 && !u.asked.trim().is_empty() => again.push(u),
            After::Skip => {}
            _ => ask.push(u),
        }
    }
    (again, ask)
}

/// The sentence said at start-up, if anything was cut off.
pub fn said(again: &[Unfinished], ask: &[Unfinished], describe: impl Fn(&Unfinished) -> String) -> Option<String> {
    let mut parts = Vec::new();
    if !again.is_empty() {
        let names: Vec<String> = again.iter().map(&describe).collect();
        parts.push(format!("I stopped in the middle of {}, so I've started {} again.", names.join(" and "), if again.len() == 1 { "it" } else { "them" }));
    }
    if !ask.is_empty() {
        let names: Vec<String> = ask.iter().map(&describe).collect();
        parts.push(format!(
            "{} {} cut off when I stopped; I haven't redone {} on my own — ask again if you still want {}.",
            names.join(" and "),
            if ask.len() == 1 { "was" } else { "were" },
            if ask.len() == 1 { "it" } else { "them" },
            if ask.len() == 1 { "it" } else { "them" },
        ));
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// A window being worked for you, as it's kept across a restart. The
/// window's handle survives Atlas restarting as long as the window itself
/// stays open; if it's gone, the job is dropped and said so.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedWindow {
    pub id: u64,
    pub job: crate::delegate::Delegation,
    pub win: u64,
    #[serde(default)]
    pub after_mine: Option<String>,
    pub started: u64,
    #[serde(default)]
    pub held: bool,
}

/// Where window jobs live in the store.
pub const WINDOWS: &str = "working_for_you";
