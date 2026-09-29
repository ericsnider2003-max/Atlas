//! More than one person using Atlas.
//!
//! The risk here is not technical, it is that one person's assistant quietly
//! knows another person's business. Memory, conversation history, approval
//! records, research notes, drafts, the outstanding list — all of it is
//! personal, and none of it should ever cross.
//!
//! So a profile is not a setting. It is a **separate state directory**, and
//! the isolation is enforced by construction: nothing shares a path, and
//! switching wipes what's in memory rather than trusting the next read.
//!
//! For your friends the right answer is usually simpler still — each runs
//! their own copy on their own machine. Profiles exist for the case where two
//! people share one computer, and for keeping a guest off your own data.

use crate::error::{AtlasError, Result};
use crate::store::{now, Store};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Full use of everything.
    Owner,
    /// Their own memory and workspace, but cannot reach the owner's data,
    /// cannot post as anyone, cannot apply file changes.
    Guest,
}

/// What a guest may never do, by the name `session::kind_of` gives it.
///
/// Everything here is an action that **acts as you** — that reaches outside
/// this machine, or changes Atlas itself, wearing your name. It is the right
/// list for a guest *profile*, where the other person has their own state
/// directory and their own empty everything.
pub const NEVER_AS_A_GUEST: &[&str] = &[
    // Other programs' tools (`mcp`): they act with the owner's files,
    // browser and apps (28 Sep 2026).
    "mcp_tool",
    // Installing or undoing an update on the owner's machine, and sending
    // feedback (or answering it) in the owner's name (OPEN_GAPS 8.2, 8.14).
    "updates",
    "feedback",
    // A 0.6-1.8 GB download onto the owner's phone.
    "phone_model",
    // Typing into your apps as you, and recording a call on your machine:
    // both speak or listen as you, and neither is a guest's to start.
    "delegate",
    "call_notes",
    // Two-factor: typing the owner's code (from their mail or texts) and
    // turning it on or off on their accounts. Theirs alone (Eric, B1).
    "type_code",
    "two_factor",
    // A long job on the owner's model and disk, like `build_it`.
    "keep_at_it",
    // The owner's goals and their list for later: reading them is reading
    // the owner, and changing them is theirs alone.
    "goals",
    "later",
    // The owner's mailbox.
    "sort_mail",
    // Posting in public as the owner.
    "schedule_post",
    // Acting in the owner's apps.
    "press_button",
    // Moving the owner's files between drives.
    "move_big_files",
    "edit_media",
    "set_key",
    "languages",
    "teach_gesture",
    "money_advice",
    "creator_advice",
    "overnight",
    "dangling",
    "suggestions",
    "drop_task",
    "unzip",
    "read_document",
    // Anything that speaks as you, to anyone outside this machine.
    "draft_post",
    "review_post",
    "mail",
    // A message goes to a person who knows you, as you, and cannot be
    // recalled once it is on their machine. The most impersonatable thing
    // Atlas can do.
    "message",
    // Anything that changes Atlas itself, or the machine's state.
    "work_on_yourself",
    // Commissioning code is substantial work on the owner's model and disk,
    // and its description can be handed to a worker online — not something a
    // stranger holding the laptop gets to set in motion.
    "build_it",
    // Working on the owner's projects, and applying a change to their files,
    // are theirs alone.
    "improve",
    "implement",
    // Finishing setup writes config: which monitor is which, which
    // microphone, which voice. Somebody else's answers to those questions
    // are answers about their desk, not yours.
    "finish_setup",
    // Teaching the album that a face is *the owner's*. A stranger doing this
    // makes Atlas think they are you, which is the worst single thing on
    // either list.
    "this_is_me",
    "back_up",
    "undo",
    "workspace_off",
    // Your accounts, and the keys to them.
    "unlock",
    "sign_in",
    "create_account",
    // Handing your Atlas to someone else's.
    "pair",
    "accept_pairing",
    "forget_peer",
    // Leaving a group chat is acting as you toward people who know you: it
    // announces to the group that you left, and cannot be recalled. Same
    // reasoning as "message" -- the most impersonatable things Atlas can do.
    "leave_group",
    // Changing who is in a group you own speaks for you to everyone in it,
    // the same weight as leaving one.
    "change_group",
    // Adding a friend lets someone reach your Atlas from now on.
    "friend",
];

/// What Atlas will not read out, or write into, for somebody who is not you.
///
/// # Why this is a second list rather than more names on the first
///
/// A guest *profile* and a handed-over *install* look like the same
/// situation and are not, and the difference is exactly what this list is
/// made of.
///
/// A guest profile has its own state directory (`state_dir`), so their
/// outstanding list is *their* outstanding list and it starts empty. Reading
/// it back to them is the feature. Blocking it would leave them with an
/// assistant that refuses to tell them what they themselves asked it to
/// remember five minutes ago.
///
/// A handover has no such directory. Your friend is holding your laptop with
/// *your* state live underneath — your brief, your notes, your history, the
/// index of everything you have ever written down. `NEVER_AS_A_GUEST` does
/// not cover any of it, because every name on that list is about acting as
/// you, and none of it is about reading you. So for as long as the handover
/// was checked against that list alone, "hand over" stopped your friend
/// posting as you and let them ask what was on your mind today.
///
/// Hence two lists, both consulted by `handover::refuses` and only the first
/// by `Role::may`. They do not drift, because the test that keeps them
/// honest does not compare them to each other: it takes every name
/// `session::kind_of` can produce and requires each one to be classified
/// exactly once, here or in `NEVER_AS_A_GUEST` or in the test's own written
/// list of things a stranger may freely do. A new intent does not quietly
/// land on the permissive side; it fails the build until somebody says which
/// it is.
/// What only the owner may even ask, from any profile. A guest profile has
/// its own memory, so most of `THE_OWNERS_OWN` is fine there — they'd be
/// reading their own. This is the exception: Eric, 25 Sep 2026, on the
/// envelope, "no one else can ask Atlas." Not what the answer would be; that
/// the question is his.
pub const ONLY_YOU_MAY_ASK: &[&str] = &["after_me"];

pub const THE_OWNERS_OWN: &[&str] = &[
    // What you arranged if something happens to you. Said only when you ask.
    "after_me",
    // Reading back what Atlas holds for you.
    "messages",
    "outstanding",
    "queued",
    // How one of your other machines is getting on. The reply is a list of
    // what your server could not do -- your infrastructure, read out to
    // whoever is holding the laptop. Same reasoning as "outstanding" and
    // "queued" directly above, which are the local version of the same thing.
    "brief_on",
    "ready",
    "history",
    // Reading this session's turns back is your conversation read out to
    // whoever is holding the laptop -- the same as "history" beside it.
    "recap",
    "why",
    "what_i_have",
    // How much Atlas has folded into its knowledge store is a read of your
    // own accumulated knowledge and its size -- yours, the same as
    // "what_i_have" beside it. A guest should not hear the shape of what
    // you've had it learn.
    "knowledge_size",
    "model_trace",
    "how_am_i_doing",
    // Where your time went is which apps and windows you were in, for how
    // long -- your working day read out to whoever holds the laptop. Yours.
    "time_spent",
    // Round 11: your clipboard, your mail's promises, your notes, people,
    // habits, cards, receipts, snippets, feeds, files and trading journal --
    // all yours. A guest opening an app by name goes through open_app.
    "clip_history",
    "screen_text",
    "waiting_for",
    "note_review",
    "launch",
    "trade_day",
    "meeting_prep",
    "snippet",
    "find_file",
    "pdf",
    "people",
    "feeds",
    // The opportunities found for you, what you look for and skip (29 Sep
    // 2026); and how Atlas talks to you -- someone holding the laptop
    // doesn't get to turn your wit up or read your leads.
    "opportunities",
    "wit",
    "receipt",
    "habit",
    "cards",
    // The autonomy ledger read out is your own track record and what Atlas is
    // trusted to do as you -- yours, the same as "how_am_i_doing" above it. A
    // guest should not hear where Atlas will act on your accounts unasked.
    "act_alone",
    "travel_prep",
    // Panels, which are a window onto the same things.
    "show_panel",
    // Your calendar. Reading it back is your schedule read out to whoever is
    // holding the laptop; adding to it writes into your days. Both are yours,
    // the same as "outstanding" (read) and "capture" (write) above.
    "agenda",
    "schedule",
    // Writing into what Atlas remembers, or teaching it.
    "capture",
    // Teaching Atlas a whole document writes into your knowledge base, the
    // same as "capture" one line above -- a guest should not seed your memory.
    "learn_knowledge",
    // Correcting how a captured note was filed -- changing its kind, or adding
    // a handle you can find it by -- rewrites your own stored notes, the same
    // as "capture" above and "got_it_wrong" below. A guest should not re-file
    // what you wrote down.
    "refile",
    // A meeting proposal read back, logged, or answered is your calendar and
    // your commitments -- same as "agenda" and "schedule" above.
    "booking",
    // Your group chats: who is in one is your contacts read out, and naming one
    // writes into your social state. Both are yours (leaving one is blocked
    // outright, up on NEVER_AS_A_GUEST).
    "who_is_in",
    "name_group",
    // What Atlas tells *you* about, afterwards. A guest silencing your
    // backups notices nothing; you find out weeks later.
    "mute_topic",
    "got_it_wrong",
    "apply_lesson",
    "rebuild_index",
    "name_this",
    "address_as",
    // Your files, and your other machines.
    "files",
    "sync",
];

impl Role {
    /// Things a guest is never allowed to do, whatever they say.
    ///
    /// Every name here must be one `session::kind_of` actually produces, or
    /// the restriction is decoration. Four of the original seven were not:
    /// `publish`, `send_email`, `promote_changes` and `switch_profile` are
    /// not in `kind_of`'s closed set, so a guest profile that read as blocked
    /// from publishing and emailing was blocked from neither. The intents
    /// that do those things are named `draft_post`, `review_post`, `mail` and
    /// `work_on_yourself`.
    ///
    /// `tests/profiles.rs` now checks this list against `kind_of` directly,
    /// so a name that matches nothing fails the build rather than reading as
    /// a protection.
    pub fn may(&self, action: &str) -> bool {
        match self {
            Role::Owner => true,
            Role::Guest => !NEVER_AS_A_GUEST.contains(&action) && !ONLY_YOU_MAY_ASK.contains(&action),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    /// Slug used as the folder name.
    pub id: String,
    pub name: String,
    pub role: Role,
    pub created: u64,
    pub last_used: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Profiles {
    pub profiles: Vec<Profile>,
    pub active: Option<String>,
}

pub fn slug(name: &str) -> String {
    let s: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    s.trim_matches('-').to_string()
}

impl Profiles {
    /// The profile registry lives above the per-profile directories, so it is
    /// readable when no profile is active.
    pub fn load(root: &Path) -> Profiles {
        Store::new(root).load("profiles")
    }
    pub fn save(&self, root: &Path) -> Result<()> {
        Store::new(root).save("profiles", self)
    }

    /// Where one profile's state lives. Nothing outside this path belongs to
    /// them, and nothing inside it belongs to anyone else.
    pub fn state_dir(root: &Path, id: &str) -> PathBuf {
        root.join("profiles").join(id)
    }

    /// The active person's directory, when there is more than one person.
    ///
    /// `None` on a single-person install — which is the normal case and must
    /// stay the normal case. Nobody who never runs `atlas profiles add`
    /// should find their notes moved into a subfolder, so the default is the
    /// install's own state and the subdirectory only appears once a second
    /// person exists.
    pub fn active_state_dir(&self, install_root: &Path) -> Option<PathBuf> {
        if self.profiles.len() < 2 {
            return None;
        }
        let id = self.active.as_deref()?;
        Some(Self::state_dir(install_root, id))
    }

    pub fn add(&mut self, name: &str, role: Role) -> Result<Profile> {
        let id = slug(name);
        if id.is_empty() {
            return Err(AtlasError::Config("a profile needs a name".into()));
        }
        if self.profiles.iter().any(|p| p.id == id) {
            return Err(AtlasError::Config(format!("there's already a profile called {name}")));
        }
        let p = Profile {
            id: id.clone(),
            name: name.to_string(),
            role,
            created: now(),
            last_used: 0,
        };
        self.profiles.push(p.clone());
        if self.active.is_none() {
            self.active = Some(id);
        }
        Ok(p)
    }

    pub fn get(&self, id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    pub fn current(&self) -> Option<&Profile> {
        self.active.as_ref().and_then(|a| self.get(a))
    }

    pub fn role(&self) -> Role {
        self.current().map(|p| p.role).unwrap_or(Role::Owner)
    }

    /// Match spoken text to a profile: "switch to Sam", "I'm Sam".
    pub fn match_name(&self, said: &str) -> Option<&Profile> {
        let t = said.to_lowercase();
        self.profiles
            .iter()
            .filter(|p| t.contains(&p.name.to_lowercase()))
            .max_by_key(|p| p.name.len())
    }

    /// Switch. The caller must have satisfied identity first — this returns
    /// what to forget, and forgetting is not optional.
    pub fn switch(&mut self, id: &str, t: u64) -> Result<Switch> {
        let from = self.active.clone();
        if self.get(id).is_none() {
            return Err(AtlasError::Config(format!("no profile called {id}")));
        }
        if from.as_deref() == Some(id) {
            return Err(AtlasError::Config("already on that profile".into()));
        }
        if let Some(p) = self.profiles.iter_mut().find(|p| p.id == id) {
            p.last_used = t;
        }
        self.active = Some(id.to_string());
        Ok(Switch { from, to: id.to_string() })
    }

    pub fn remove(&mut self, id: &str) -> Result<()> {
        if self.active.as_deref() == Some(id) {
            return Err(AtlasError::Config("switch away before removing a profile".into()));
        }
        let before = self.profiles.len();
        self.profiles.retain(|p| p.id != id);
        if self.profiles.len() == before {
            return Err(AtlasError::Config(format!("no profile called {id}")));
        }
        Ok(())
    }

    pub fn summary(&self) -> String {
        match self.profiles.len() {
            0 => "No profiles yet.".into(),
            1 => format!("Just you: {}.", self.profiles[0].name),
            n => format!(
                "{n} profiles. You're on {}.",
                self.current().map(|p| p.name.clone()).unwrap_or_else(|| "none".into())
            ),
        }
    }
}

/// What a switch requires the caller to do.
#[derive(Debug, Clone, PartialEq)]
pub struct Switch {
    pub from: Option<String>,
    pub to: String,
}

impl Switch {
    /// Everything that must be dropped from memory before the new profile is
    /// used. Returning this rather than a boolean makes it hard to forget one.
    pub fn must_forget(&self) -> &'static [&'static str] {
        &[
            "thread",
            "session",
            "memory",
            "backlog",
            "publisher",
            "journal",
            "referents",
            "voiceprint",
            "identity grace",
        ]
    }

    pub fn say(&self, name: &str) -> String {
        // This used to end "Nothing from the previous profile carries over."
        // It was false: every profile shared one state directory, and the
        // nine subsystems `must_forget` names were printed and never dropped.
        //
        // They are separate directories now, and the switch is applied when
        // Atlas next starts -- because the running daemon clones its store
        // into thirteen subsystems at construction, and a switch that moved
        // one of them would leave twelve reading the previous person. A
        // switch that has not taken yet is visible; a switch that half-took
        // is not.
        format!(
            "{name} is who I'll be for next time. Nothing carries over -- but I'm \
             still on the previous profile until I'm restarted, so stop me and \
             start me again before handing this over."
        )
    }
}

/// The directory the running Atlas should open, read from the registry.
///
/// A free function because `roots::store()` needs it before any `Profiles` is
/// loaded, and loading one needs a `Store` — the registry deliberately lives
/// *above* the per-person directories so this is not circular.
pub fn active_dir(install_root: &Path) -> Option<PathBuf> {
    Profiles::load(install_root).active_state_dir(install_root)
}

/// Are two profiles genuinely separated on disk? Named with the
/// `_for_test` suffix `bug_sweep.rs`'s dead-capability sweep already
/// exempts -- this exists to be called from a test, never from runtime
/// code, and a capability whose only legitimate caller is a test is not
/// the same thing as one nothing calls at all.
pub fn isolated_for_test(root: &Path, a: &str, b: &str) -> bool {
    let pa = Profiles::state_dir(root, a);
    let pb = Profiles::state_dir(root, b);
    pa != pb && !pa.starts_with(&pb) && !pb.starts_with(&pa)
}
