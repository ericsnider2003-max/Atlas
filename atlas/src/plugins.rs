//! Add-ons: capabilities you or a friend add to Atlas, kept through every
//! update, and unable to do more than you allowed.
//!
//! This is Tier 1 of the plugin boundary in `docs/UPDATE_COURIER_SPEC.md` §5:
//! **declarative, no code.** An add-on is one file, `data/plugins/<id>/plugin.yaml`,
//! that gives Atlas new things to do *by composing what it already does* -- a
//! named sequence of ordinary commands with the phrases that start it. The
//! engine interprets it; nothing in it ever executes as code. So the largest
//! class of hole a plugin system has (running someone else's program) does
//! not exist here.
//!
//! What stops an add-on doing more than it should, in the order it is met:
//!
//! 1. **It declares what it needs** (`permissions`), in plain categories you
//!    can read. Every step is checked against that list when the file is
//!    read; a step that needs something undeclared makes the add-on refuse to
//!    load, rather than load and fail later.
//! 2. **You approve it** -- on the hub or with `atlas plugins approve` -- and
//!    the approval records the exact file (its SHA-256) and the permissions
//!    you granted. Nothing that arrives from anywhere is ever approved for
//!    you; there is no command an add-on (or a message) can send that
//!    approves an add-on.
//! 3. **Every step is checked again as it runs**: the file must still be the
//!    one you approved, the add-on must not be switched off, and the command
//!    the step turned out to be -- decided by the same parser as anything you
//!    say -- must fall in a permission you still grant. This is what makes
//!    taking a permission away immediate, and what catches a step whose text
//!    was changed by an earlier step's output (`{name}`).
//! 4. **It then goes through the ordinary approval gate** (`policy`), like
//!    anything you say. An add-on cannot make a consequential step skip the
//!    question by being part of a chain.
//!
//! Some commands are never available to an add-on at all (`NEVER`): the vault,
//! pairing, accounts, handing Atlas over, Atlas changing its own code or its
//! own standing instructions. And add-ons cannot call each other, or your own
//! saved sequences: a step is parsed as a command, never matched as a
//! trigger.

use crate::config::CommandsConfig;
use crate::flow::{Step, Workflow};
use crate::intent::{normalize, Parser};
use crate::store::Store;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The add-on format this build reads. An add-on states which it was written
/// for; one written for a newer Atlas is switched off with a reason, never
/// half-read.
pub const PLUGIN_API: u32 = 1;
/// The oldest format this build still reads.
pub const OLDEST_PLUGIN_API: u32 = 1;

/// Where add-ons live, under `data/`. In `upgrade::YOURS`.
pub const PLUGINS_DIR: &str = "plugins";
pub const MANIFEST_FILE: &str = "plugin.yaml";
/// Larger than any honest add-on; refused before it is parsed.
pub const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_FLOWS: usize = 32;
const MAX_STEPS: usize = 32;
const MAX_TRIGGERS: usize = 8;
const APPROVALS: &str = "plugin_approvals";

/// SHA-256 of an add-on file, as lowercase hex: what an approval records and
/// what every step re-checks.
fn fingerprint(bytes: &[u8]) -> String {
    crate::digest::sha256_hex(bytes)
}

/// The folder add-ons live in for this install.
pub fn plugins_dir() -> PathBuf {
    crate::roots::data_sub(PLUGINS_DIR)
}

// ------------------------------------------------------------------ permissions

/// One thing an add-on may be allowed to do, in words you approve.
#[derive(Debug)]
pub struct Permission {
    pub key: &'static str,
    /// Finishes the sentence "This add-on wants to ...".
    pub plain: &'static str,
    /// The commands (by their name in `commands.yaml`) this allows.
    pub intents: &'static [&'static str],
}

/// Every permission there is. A command in none of these and not in `NEVER`
/// is refused -- and a test fails, so a new command is decided on the day it
/// is added rather than left open.
pub const PERMISSIONS: &[Permission] = &[
    Permission {
        key: "basics",
        plain: "pause and resume, back up, and read what Atlas says about itself -- \
                what's outstanding, what it did, how it's running, what it can do",
        intents: &[
            "pause", "resume", "ready", "outstanding", "queued", "which_model", "how_am_i_doing",
            "model_trace", "machine_health", "self_check", "capabilities", "history", "recap",
            "act_alone", "knowledge_size", "why", "rehearse", "plain_change", "recommend", "back_up",
        ],
    },
    Permission {
        key: "read_notes",
        plain: "look things up in what you've written and what Atlas knows",
        intents: &["what_i_have", "brief_on"],
    },
    Permission {
        key: "write_notes",
        plain: "write notes, file them, and teach Atlas a document",
        intents: &["capture", "refile", "learn_knowledge"],
    },
    Permission {
        key: "calendar",
        plain: "read your calendar and add to it",
        intents: &["agenda", "schedule", "reminder", "booking"],
    },
    Permission {
        key: "desktop",
        plain: "open, close and arrange apps and windows, switch modes, and show panels",
        intents: &[
            "workspace_on", "workspace_off", "open_app", "close_app", "focus_app", "set_mode",
            "show_panel", "dismiss_panel",
        ],
    },
    Permission { key: "online", plain: "look things up on the internet", intents: &["research"] },
    Permission {
        key: "thinking",
        plain: "use the language model to think something through or explain it",
        intents: &["ask_the_room", "diagnose", "walk_through", "explain_code", "design_review"],
    },
    Permission {
        key: "making",
        plain: "write code, animations and 3-D scenes for you (checked before they're trusted)",
        // `scene3d` joined when round 10 merged this (6b7e938): a 3-D
        // scene is drawn the way an animation is -- a draft, checked.
        intents: &["build_it", "animate", "scene3d"],
    },
    Permission {
        key: "screen",
        plain: "see your screen, your camera and your clipboard, and watch your hands",
        intents: &["view_display", "whats_there", "whats_this", "use_clipboard", "gestures_on", "gestures_off"],
    },
    Permission {
        key: "files",
        plain: "read and convert your files, and rebuild the file index",
        intents: &["files", "rebuild_index"],
    },
    Permission {
        key: "read_messages",
        plain: "read your messages, your group chats and your inbox",
        intents: &["messages", "mail", "who_is_in"],
    },
    Permission {
        key: "send_messages",
        plain: "send messages as you (each still asks you first, as it would if you said it)",
        intents: &["message"],
    },
    Permission { key: "drafts", plain: "draft posts and check them -- never post them", intents: &["draft_post", "review_post"] },
];

/// Commands no add-on may ever use, whatever it asks for, and why.
pub const NEVER: &[(&str, &str)] = &[
    // Updates and feedback by voice (OPEN_GAPS 8.2, 8.14), decided when built.
    ("update_status", "it reads out this install's update record -- the owner's to ask"),
    ("update_install", "installing an update restarts Atlas; that's the person's decision, not an add-on's"),
    ("update_undo", "going back a version is only ever the person at this device's decision"),
    ("feedback_send", "feedback goes in the person's own words, read back to them and sent on their yes"),
    ("feedback_send_bare", "feedback goes in the person's own words, read back to them and sent on their yes"),
    ("feedback_list", "it reads out what friends told you"),
    ("feedback_reply", "an answer to a friend is yours to give"),
    ("phone_model_get", "a download of up to 1.8 GB onto your phone is yours to start"),
    ("phone_model_status", "it's a question about your phone, yours to ask"),
    ("undo", "it puts back whatever Atlas did last, which may not be the add-on's"),
    // Round 9's work log, decided when round 10 merged this (6b7e938).
    ("time_spent", "it reads out where your day went, app by app -- yours alone"),
    // Round 11's tools, decided when they were built.
    ("clip_history", "it reads back what you copied"),
    ("screen_text", "it reads whatever window is in front"),
    ("market_day", "it's yours to ask; an add-on has the market module itself"),
    ("waiting_for", "it reads your sent mail"),
    ("note_review", "it reads and settles your notes"),
    ("launch", "it opens programs by name"),
    ("trade_day", "it's your trading journal"),
    ("meeting_prep", "it reads your mail and notes about the people you meet"),
    ("snippet", "it types into whatever app is in front"),
    ("find_file", "it lists and opens your files"),
    ("pdf", "it writes files beside yours"),
    ("people", "it's what you keep about people"),
    ("feeds", "it fetches from the web as you"),
    ("receipt", "it reads the screen and what you spend"),
    ("habit", "it's your own record"),
    ("cards", "it changes your study schedule"),
    ("translate", "it sends your text through your model"),
    ("got_it_wrong", "it changes how Atlas behaves from now on"),
    ("apply_lesson", "it changes how Atlas behaves from now on"),
    ("mute_topic", "it would let an add-on silence what Atlas tells you"),
    ("address_as", "it changes who Atlas thinks it is talking to"),
    ("this_is_me", "it teaches Atlas who you are"),
    ("name_this", "it teaches Atlas what things and people are"),
    ("pair", "it decides which devices are yours"),
    ("accept_pairing", "it decides which devices are yours"),
    ("forget_peer", "it decides which devices are yours"),
    ("finish_setup", "it decides which devices are yours"),
    ("sync", "it moves your data between devices"),
    ("hand_over", "it gives Atlas to someone else"),
    ("take_it_back", "it takes Atlas back from someone"),
    ("unlock", "it opens the vault"),
    ("create_account", "it uses your accounts and passwords"),
    ("sign_in", "it uses your accounts and passwords"),
    ("travel_prep", "it changes how your accounts are secured"),
    ("name_group", "it changes the groups you're in"),
    ("leave_group", "it changes the groups you're in"),
    ("change_group", "it changes who is in your groups and what they may do"),
    ("friend", "it decides who can reach your Atlas"),
    ("work_on_yourself", "it changes Atlas's own code"),
    ("improve", "it changes Atlas's own code"),
    ("implement", "it changes Atlas's own code"),
    ("dictate", "it types into other apps as if it were you"),
    ("shakedown", "it drives checks across every capability, some of them visible"),
    // Eric's evening on the laptop (29 Sep 2026, 9101519), decided when the
    // commands' test caught them undecided.
    ("tidy_desktop", "it moves your files into folders"),
    ("use_mic", "it changes which microphone Atlas listens through -- one it can't hear you on silences it"),
    // The third chat's commands, decided when its line was merged (26 Sep
    // 2026). Every one acts as you or on other people, so none is opened.
    ("type_code", "it types a sign-in code into whatever is in front"),
    ("two_factor", "it turns two-factor on or off on your accounts"),
    ("call_notes_on", "it starts noting a call"),
    ("call_record_everyone", "it records other people"),
    ("call_they_agreed", "it says other people agreed to be recorded"),
    ("call_they_declined", "it answers for other people about recording"),
    ("call_couldnt_ask", "it answers for other people about recording"),
    ("call_no_answer", "it answers for other people about recording"),
    ("call_just_mine", "it changes what a call records"),
    ("call_status", "it reads what a call is recording"),
    ("call_what_it_does", "it's said to the people on your call"),
    ("call_notes_off", "it stops a call's notes"),
    ("delegate", "it writes and sends replies as you in other apps"),
    ("after_me", "it's your after-me arrangement, yours alone"),
    // The Atlas Project chat's 25j commands, decided when 25j was merged in
    // (26 Sep 2026). Held back the same way the third chat's were: each one
    // reads what's yours, acts as you, or changes Atlas. Opening any of them
    // to add-ons is a decision for Eric, not for a merge.
    ("read_document", "it opens and reads your files"),
    ("unzip", "it writes files beside yours"),
    ("drop_task", "it changes your task list"),
    ("suggestions", "it would let an add-on silence what Atlas tells you"),
    ("dangling", "it reads your notes"),
    ("overnight", "it reads out what Atlas did while you were away"),
    ("creator_advice", "it answers from what Atlas knows about your work"),
    ("money_advice", "it answers from what Atlas knows about your money"),
    ("teach_gesture", "it teaches Atlas what your hands mean"),
    ("languages", "it changes what Atlas listens for"),
    ("set_key", "it changes Atlas's keys"),
    ("edit_media", "it writes files beside yours"),
    ("move_big_files", "it moves folders between your drives"),
    ("pc_tune", "it closes your programs and changes what starts with Windows"),
    ("press_button", "it presses buttons in other apps as if it were you"),
    ("schedule_post", "it decides when your posts go out"),
    ("sort_mail", "it moves your mail"),
    ("later", "it's your own list"),
    ("goals", "it's your own goals"),
    ("keep_at_it", "it keeps changing Atlas's own code"),
    ("run_build", "it runs a program on your computer"),
    // 25k's, decided when 25k was merged in (26 Sep 2026).
    ("clock", "it's yours to ask; an add-on can read the time itself"),
    // Round 6's four, decided when they were merged onto the split (29 Sep
    // 2026), each held back for the same reason as its nearest neighbour:
    // photo edits write files as `edit_media` does; social reads your own
    // accounts and fetches as you as `feeds` does; the hunt reads your mail
    // for job alerts and keeps your list; wit changes how Atlas talks from
    // now on, as `got_it_wrong` does.
    ("edit_photo", "it writes files beside yours"),
    ("make_picture", "it writes into your Pictures folder and can download 6.5 GB"),
    ("self_test", "it copies your settings and state to test with"),
    ("operate", "it clicks and types in your apps"),
    ("social", "it reads your own accounts and fetches from the web as you"),
    ("opportunities", "it reads your mail for job alerts and keeps your own list"),
    ("wit", "it changes how Atlas behaves from now on"),
];

/// Said alone, these answer a question Atlas asked. An add-on may never be
/// started by one, or a "yes" meant for Atlas could start it instead.
const ANSWERS: &[&str] = &[
    "yes", "no", "yeah", "yep", "nope", "ok", "okay", "sure", "cancel", "stop", "go ahead", "do it",
    "dont", "not now", "never mind", "yes please", "no thanks", "thats fine", "that's fine",
];

pub fn permission(key: &str) -> Option<&'static Permission> {
    PERMISSIONS.iter().find(|p| p.key == key)
}

/// Which permission a command needs, or why an add-on may not use it at all.
fn permission_for(intent: &str) -> Result<&'static Permission, String> {
    if let Some((_, why)) = NEVER.iter().find(|(i, _)| *i == intent) {
        return Err(format!("add-ons may never use \"{intent}\": {why}"));
    }
    PERMISSIONS
        .iter()
        .find(|p| p.intents.contains(&intent))
        .ok_or_else(|| format!("\"{intent}\" has not been opened to add-ons"))
}

// ------------------------------------------------------------------ the file

#[derive(Deserialize)]
struct Header {
    plugin_api: u32,
}

/// What an add-on says about itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Which add-on format it was written for; see `PLUGIN_API`.
    pub plugin_api: u32,
    /// Its folder name under `data/plugins`: lowercase letters, digits, `-`.
    pub id: String,
    pub name: String,
    /// Who made it, as they say. Shown when you approve; not proof of anything.
    pub author: String,
    #[serde(default)]
    pub description: String,
    /// Keys from `PERMISSIONS`.
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub flows: Vec<PluginFlow>,
}

/// A named sequence of ordinary commands, and what you say to start it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginFlow {
    pub name: String,
    #[serde(default)]
    pub triggers: Vec<String>,
    /// When it runs by itself, if it does: "every 30 minutes", "every 2
    /// hours", or "daily at 08:00" (your local time). Approving the add-on is
    /// the consent for these runs -- the same rule a scheduled job follows --
    /// and every step is still checked as it runs.
    #[serde(default)]
    pub schedule: Option<String>,
    pub steps: Vec<PluginStep>,
}

/// When an add-on's sequence runs by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schedule {
    /// Every this many seconds.
    Every(u64),
    /// Once a day, at this minute of your local day.
    DailyAt(u32),
}

/// The shortest interval an add-on may run at. Anything faster is an add-on
/// polling, which is a background load you did not agree to.
pub const MIN_EVERY_MINUTES: u64 = 15;

/// Read "every 30 minutes", "every 2 hours", "daily at 08:00".
pub fn read_schedule(s: &str) -> Result<Schedule, String> {
    let t = s.trim().to_lowercase();
    let words: Vec<&str> = t.split_whitespace().collect();
    match words.as_slice() {
        ["every", n, unit] => {
            let n: u64 = n.parse().map_err(|_| format!("\"{s}\": {n} isn't a number"))?;
            let mins = match *unit {
                "minute" | "minutes" => n,
                "hour" | "hours" => n * 60,
                other => return Err(format!("\"{s}\": say minutes or hours, not {other}")),
            };
            if mins < MIN_EVERY_MINUTES {
                return Err(format!("\"{s}\": add-ons may run at most every {MIN_EVERY_MINUTES} minutes"));
            }
            Ok(Schedule::Every(mins * 60))
        }
        ["daily", "at", hm] => {
            let (h, m) = hm.split_once(':').ok_or_else(|| format!("\"{s}\": write the time as HH:MM"))?;
            let (h, m): (u32, u32) = (
                h.parse().map_err(|_| format!("\"{s}\": write the time as HH:MM"))?,
                m.parse().map_err(|_| format!("\"{s}\": write the time as HH:MM"))?,
            );
            if h > 23 || m > 59 {
                return Err(format!("\"{s}\": that isn't a time of day"));
            }
            Ok(Schedule::DailyAt(h * 60 + m))
        }
        _ => Err(format!("\"{s}\": write \"every N minutes\", \"every N hours\" or \"daily at HH:MM\"")),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginStep {
    /// A command, exactly as you would say it.
    pub command: String,
    /// Carry on if this step fails.
    #[serde(default)]
    pub optional: bool,
    /// Name this step's result, for `{name}` in a later step.
    #[serde(default)]
    pub produces: Option<String>,
}

fn id_ok(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 40
        && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !id.starts_with('-')
}

// ------------------------------------------------------------------ approvals

/// Your decision about one add-on.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Approval {
    /// The exact file you approved. Any change to it needs approving again.
    pub sha256: String,
    /// What you allowed. Starts as everything it asked for; you can take any
    /// of it away.
    pub granted: Vec<String>,
    pub approved_at: u64,
    /// Switched off by you, without forgetting the approval.
    #[serde(default)]
    pub disabled: bool,
    /// Steps you said needn't ask each time, exactly as the add-on wrote
    /// them. See `may_skip_question`.
    #[serde(default)]
    pub trusted_steps: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Approvals {
    pub plugins: BTreeMap<String, Approval>,
    /// Where an add-on came from, when it came from a paired Atlas: that
    /// device's name, as you paired it. Proven by the pairing, unlike the
    /// `author` the file claims for itself.
    #[serde(default)]
    pub sent_by: BTreeMap<String, String>,
}

impl Approvals {
    /// A missing or unreadable record means nothing is approved: an add-on
    /// that cannot prove you said yes does not run.
    pub fn load(store: &Store) -> Approvals {
        store.load(APPROVALS)
    }
    fn save(&self, store: &Store) -> Result<(), String> {
        store.save(APPROVALS, self).map_err(|e| format!("couldn't save your decision: {e}"))
    }
}

// ------------------------------------------------------------------ reading

/// Where an add-on stands.
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    /// Approved, unchanged, on.
    Active,
    /// Not approved yet. It does nothing until you look at it.
    Waiting,
    /// The file is not the one you approved. Off until you approve it again.
    Changed,
    /// You switched it off.
    Disabled,
    /// Written for a newer Atlas.
    NeedsNewerAtlas(u32),
    /// Written for an add-on format this Atlas no longer reads.
    TooOld(u32),
    /// Unreadable, or it asks for something it may not have.
    Broken(String),
}

impl Status {
    pub fn plain(&self) -> String {
        match self {
            Status::Active => "on".into(),
            Status::Waiting => "waiting for you to approve it -- it does nothing until then".into(),
            Status::Changed => "off -- the file changed since you approved it; look again and approve it if you still want it".into(),
            Status::Disabled => "switched off by you".into(),
            Status::NeedsNewerAtlas(v) => format!("off -- it was written for a newer Atlas (add-on format {v}; this one reads up to {PLUGIN_API})"),
            Status::TooOld(v) => format!("off -- it was written for add-on format {v}, which this Atlas no longer reads; it needs updating"),
            Status::Broken(why) => format!("off -- {why}"),
        }
    }
}

/// One add-on as found on disk, and what you have decided about it.
#[derive(Debug, Clone)]
pub struct Plugin {
    /// Its folder name.
    pub id: String,
    pub manifest: Option<Manifest>,
    /// Of the file as it is now.
    pub sha256: String,
    pub status: Status,
    /// What it may do now: what you granted, of what it still asks for.
    pub granted: Vec<String>,
    /// Things refused without switching the whole add-on off -- a trigger
    /// that would shadow one of Atlas's own commands, say. Shown with it.
    pub trouble: Vec<String>,
    /// The sequences it can run, with only the triggers that were allowed.
    pub flows: Vec<Workflow>,
    /// Sequences that run by themselves: (sequence name, when).
    pub schedules: Vec<(String, Schedule)>,
    /// Steps Atlas would normally ask about before doing, and whether you
    /// said it needn't: (step as written, trusted, why it can't be trusted).
    pub questions: Vec<StepQuestion>,
    /// The paired Atlas that sent it, if one did.
    pub sent_by: Option<String>,
}

/// A step that would ask you first, and what you decided about asking.
#[derive(Debug, Clone, PartialEq)]
pub struct StepQuestion {
    pub command: String,
    pub trusted: bool,
    /// Why "don't ask each time" isn't available for it, if it isn't.
    pub always_asks: Option<String>,
}

impl Plugin {
    pub fn name(&self) -> String {
        self.manifest.as_ref().map(|m| m.name.clone()).unwrap_or_else(|| self.id.clone())
    }
}

/// What reading the file found, before your decision is consulted.
enum Read {
    Ok(Checked, String),
    Off(Status, String),
}

/// Everything reading an add-on file found.
struct Checked {
    manifest: Manifest,
    trouble: Vec<String>,
    flows: Vec<Workflow>,
    schedules: Vec<(String, Schedule)>,
    /// Steps that would ask, as written, with why they can't be trusted.
    asks: Vec<(String, Option<String>)>,
}

fn read_file(dir: &Path, id: &str, commands: &CommandsConfig) -> Read {
    let path = dir.join(MANIFEST_FILE);
    let meta = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(_) => return Read::Off(Status::Broken(format!("there is no {MANIFEST_FILE} in its folder")), String::new()),
    };
    if meta.len() > MAX_MANIFEST_BYTES {
        return Read::Off(Status::Broken(format!("its file is larger than {} KB", MAX_MANIFEST_BYTES / 1024)), String::new());
    }
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => return Read::Off(Status::Broken(format!("couldn't read it: {e}")), String::new()),
    };
    let sha = fingerprint(&bytes);
    let Ok(text) = String::from_utf8(bytes) else {
        return Read::Off(Status::Broken("its file isn't text".into()), sha);
    };
    match parse_and_check(&text, Some(id), commands) {
        Ok(c) => Read::Ok(c, sha),
        Err(status) => Read::Off(status, sha),
    }
}

/// Parse an add-on file and check everything that can be checked without
/// your decision. `Err` is why it cannot run at all.
fn parse_and_check(text: &str, folder: Option<&str>, commands: &CommandsConfig) -> Result<Checked, Status> {
    // The format number first, leniently, so a file for a newer Atlas is
    // told apart from a broken one.
    let header: Header = serde_yaml::from_str(text)
        .map_err(|e| Status::Broken(format!("it doesn't read as an add-on ({e})")))?;
    if header.plugin_api > PLUGIN_API {
        return Err(Status::NeedsNewerAtlas(header.plugin_api));
    }
    if header.plugin_api < OLDEST_PLUGIN_API {
        return Err(Status::TooOld(header.plugin_api));
    }
    let m: Manifest = serde_yaml::from_str(text)
        .map_err(|e| Status::Broken(format!("it doesn't read as an add-on ({e})")))?;

    let broken = |why: String| Err(Status::Broken(why));
    if !id_ok(&m.id) {
        // Quoted as written, never `{:?}`: that shows escapes like `\u{301}`
        // and reads as code on the Add-ons page (27 Sep 2026).
        return broken(format!("its id \u{201c}{}\u{201d} must be lowercase letters, digits and dashes", m.id));
    }
    if let Some(f) = folder {
        if f != m.id {
            return broken(format!("its id is \u{201c}{}\u{201d} but its folder is \u{201c}{f}\u{201d}", m.id));
        }
    }
    for (what, v) in [("name", &m.name), ("author", &m.author)] {
        if v.trim().is_empty() || v.chars().count() > 80 {
            return broken(format!("its {what} must be 1 to 80 characters"));
        }
    }
    let mut seen = Vec::new();
    for p in &m.permissions {
        if permission(p).is_none() {
            return broken(format!("it asks for a permission \"{p}\" that doesn't exist"));
        }
        if seen.contains(&p) {
            return broken(format!("it asks for \"{p}\" twice"));
        }
        seen.push(p);
    }
    if m.flows.len() > MAX_FLOWS {
        return broken(format!("it has more than {MAX_FLOWS} sequences"));
    }

    let parser = Parser::new(commands);
    let builtin: Vec<String> =
        commands.commands.iter().flat_map(|c| c.phrases.iter().map(|p| normalize(p))).collect();
    let mut trouble = Vec::new();
    let mut flows = Vec::new();
    let mut schedules = Vec::new();
    let mut asks: Vec<(String, Option<String>)> = Vec::new();
    for f in &m.flows {
        if f.name.trim().is_empty() {
            return broken("a sequence has no name".into());
        }
        if f.steps.is_empty() || f.steps.len() > MAX_STEPS {
            return broken(format!("\"{}\" must have 1 to {MAX_STEPS} steps", f.name));
        }
        if f.triggers.len() > MAX_TRIGGERS {
            return broken(format!("\"{}\" has more than {MAX_TRIGGERS} ways to start it", f.name));
        }
        for s in &f.steps {
            // Checked as written. A `{name}` placeholder stays in the text,
            // so this sees the command it starts with; what it becomes once
            // filled in is checked again as it runs (`may_run`).
            match parser.parse_named(&s.command).1 {
                None => return broken(format!("\"{}\" isn't a command Atlas knows", s.command)),
                Some(intent) => match permission_for(&intent) {
                    Err(why) => return broken(format!("\"{}\": {why}", s.command)),
                    Ok(p) if !m.permissions.iter().any(|k| k == p.key) => {
                        return broken(format!(
                            "\"{}\" needs permission \"{}\" ({}), which it doesn't ask for",
                            s.command, p.key, p.plain
                        ))
                    }
                    Ok(_) => {}
                },
            }
            if let Some(n) = &s.produces {
                if n.is_empty() || !n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    return broken(format!("\"{n}\" isn't a usable name for a step's result"));
                }
            }
            // Would Atlas ask before this step? By its own baseline rules --
            // what you taught it since can only make it ask less.
            let (intent, name) = parser.parse_named(&s.command);
            if crate::policy::classify(&intent).needs_consent() && !asks.iter().any(|(c, _)| c == &s.command) {
                asks.push((s.command.clone(), why_always_asks(&s.command, name.as_deref())));
            }
        }
        if let Some(when) = &f.schedule {
            match read_schedule(when) {
                Ok(sch) => schedules.push((f.name.clone(), sch)),
                Err(why) => return broken(why),
            }
        }
        let mut triggers = Vec::new();
        for t in &f.triggers {
            match trigger_problem(t, &builtin, &parser) {
                Some(why) => trouble.push(format!("\"{t}\" won't start \"{}\": {why}", f.name)),
                None => triggers.push(normalize(t)),
            }
        }
        if triggers.is_empty() && f.schedule.is_none() {
            trouble.push(format!("\"{}\" has no way to start it that Atlas can allow", f.name));
        }
        flows.push(Workflow {
            name: f.name.clone(),
            triggers,
            steps: f
                .steps
                .iter()
                .map(|s| Step {
                    command: s.command.clone(),
                    on_fail: if s.optional { crate::flow::OnFail::Continue } else { crate::flow::OnFail::Stop },
                    produces: s.produces.clone(),
                })
                .collect(),
        });
    }
    Ok(Checked { manifest: m, trouble, flows, schedules, asks })
}

/// Commands an add-on's step asks about every time, whatever you said
/// before, and why.
pub const ALWAYS_ASKS: &[(&str, &str)] = &[("message", "it speaks to other people as you")];

/// Why a step can never be "don't ask each time", if it can't.
pub fn why_always_asks(command: &str, intent: Option<&str>) -> Option<String> {
    if command.contains('{') {
        return Some("part of it is filled in from an earlier step, so it isn't the same thing each time".into());
    }
    let intent = intent?;
    ALWAYS_ASKS.iter().find(|(i, _)| *i == intent).map(|(_, why)| (*why).to_string())
}

/// Why a phrase may not start an add-on, if it may not.
fn trigger_problem(t: &str, builtin: &[String], parser: &Parser) -> Option<String> {
    let n = normalize(t);
    if n.split_whitespace().count() < 2 {
        return Some("it must be at least two words".into());
    }
    // Whatever Atlas would take as a yes or a no to its own question -- the
    // same reading the daemon uses -- can never start an add-on.
    if crate::session::is_yes(&n) || crate::session::is_no(&n) || ANSWERS.iter().any(|a| normalize(a) == n) {
        return Some("it's how you answer a question Atlas asked".into());
    }
    if builtin.contains(&n) {
        return Some("Atlas already has a command said exactly that way".into());
    }
    if let Some(intent) = parser.parse_named(&n).1 {
        if NEVER.iter().any(|(i, _)| *i == intent) {
            return Some(format!("it reads as \"{intent}\", which add-ons may never stand in for"));
        }
    }
    None
}

/// Every add-on in `dir`, with your decisions applied, sorted by id.
pub fn scan(dir: &Path, commands: &CommandsConfig, approvals: &Approvals) -> Vec<Plugin> {
    let mut out: Vec<Plugin> = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return out };
    let mut folders: Vec<PathBuf> =
        entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    folders.sort();
    for folder in folders {
        let id = folder.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if !id_ok(&id) {
            out.push(Plugin {
                id: id.clone(),
                manifest: None,
                sha256: String::new(),
                status: Status::Broken("its folder name must be lowercase letters, digits and dashes".into()),
                granted: vec![],
                trouble: vec![],
                flows: vec![],
                schedules: vec![],
                questions: vec![],
                sent_by: None,
            });
            continue;
        }
        let plugin = match read_file(&folder, &id, commands) {
            Read::Off(status, sha) => Plugin {
                sent_by: approvals.sent_by.get(&id).cloned(),
                id,
                manifest: None,
                sha256: sha,
                status,
                granted: vec![],
                trouble: vec![],
                flows: vec![],
                schedules: vec![],
                questions: vec![],
            },
            Read::Ok(c, sha) => {
                let m = c.manifest;
                let approval = approvals.plugins.get(&id);
                let (status, granted) = match approval {
                    None => (Status::Waiting, vec![]),
                    Some(a) if a.sha256 != sha => (Status::Changed, vec![]),
                    Some(a) if a.disabled => (Status::Disabled, vec![]),
                    Some(a) => (
                        Status::Active,
                        a.granted.iter().filter(|g| m.permissions.contains(g)).cloned().collect(),
                    ),
                };
                let trusted = |cmd: &str| {
                    approval.is_some_and(|a| a.sha256 == sha && a.trusted_steps.iter().any(|t| t == cmd))
                };
                let questions = c
                    .asks
                    .into_iter()
                    .map(|(command, always_asks)| StepQuestion { trusted: trusted(&command), command, always_asks })
                    .collect();
                Plugin {
                    sent_by: approvals.sent_by.get(&id).cloned(),
                    id,
                    manifest: Some(m),
                    sha256: sha,
                    status,
                    granted,
                    trouble: c.trouble,
                    flows: c.flows,
                    schedules: c.schedules,
                    questions,
                }
            }
        };
        out.push(plugin);
    }

    // Two add-ons claiming the same words: neither gets them. Guessing which
    // one you meant is how the wrong one runs.
    let mut owners: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for p in out.iter().filter(|p| p.status == Status::Active) {
        for f in &p.flows {
            for t in &f.triggers {
                owners.entry(t.clone()).or_default().push(p.id.clone());
            }
        }
    }
    for (t, ids) in owners.iter().filter(|(_, ids)| ids.len() > 1) {
        for p in out.iter_mut().filter(|p| ids.contains(&p.id)) {
            for f in &mut p.flows {
                f.triggers.retain(|x| x != t);
            }
            p.trouble.push(format!("\"{t}\" is claimed by more than one add-on ({}), so it starts none", ids.join(", ")));
        }
    }
    out
}

/// The add-ons as last read, and what they were read from (27 Sep 2026).
///
/// `Registry::load` reads every add-on's file, hashes it and parses it, and
/// the daemon called it on every turn you spoke and every tick -- disk work
/// on the path between you speaking and Atlas answering, for a folder that
/// almost never changes. Now it is read again only when a file in it, the
/// folder itself, or your approvals have a different size or modified time.
#[derive(Debug, Clone, Default)]
pub struct Kept {
    stamp: u64,
    reg: Option<Registry>,
}

/// Sizes and modified times of everything `Registry::load` reads: the
/// add-ons folder, each add-on's folder and files, and the approvals record.
/// Looked at, never read.
fn stamp(dir: &Path, store: &Store) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let note = |p: &Path, h: &mut std::collections::hash_map::DefaultHasher| match std::fs::metadata(p) {
        Ok(m) => {
            p.hash(h);
            m.len().hash(h);
            m.modified().ok().hash(h);
        }
        Err(_) => 0u8.hash(h),
    };
    note(dir, &mut h);
    note(&store.root().join(format!("{APPROVALS}.json")), &mut h);
    if let Ok(rd) = std::fs::read_dir(dir) {
        let mut folders: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        folders.sort();
        for f in folders {
            note(&f, &mut h);
            if let Ok(inner) = std::fs::read_dir(&f) {
                let mut files: Vec<PathBuf> = inner.flatten().map(|e| e.path()).collect();
                files.sort();
                for file in files {
                    note(&file, &mut h);
                }
            }
        }
    }
    h.finish()
}

/// The add-ons that are on, ready to be matched against what you say.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    pub plugins: Vec<Plugin>,
}

impl Registry {
    pub fn load(dir: &Path, commands: &CommandsConfig, store: &Store) -> Registry {
        Registry { plugins: scan(dir, commands, &Approvals::load(store)) }
    }

    /// The registry, read again only when something it was read from has
    /// changed (`stamp`). See [`Kept`].
    pub fn load_kept<'k>(kept: &'k mut Kept, dir: &Path, commands: &CommandsConfig, store: &Store) -> &'k Registry {
        let now = stamp(dir, store);
        if kept.reg.is_none() || kept.stamp != now {
            kept.reg = Some(Registry::load(dir, commands, store));
            kept.stamp = now;
        }
        kept.reg.get_or_insert_with(Registry::default)
    }

    /// The add-on sequence started by exactly this sentence, if any.
    ///
    /// Exact, not "contains": your own saved sequences match loosely because
    /// you wrote them; an add-on only answers to the words it declared.
    pub fn match_trigger(&self, said: &str) -> Option<(String, Workflow)> {
        let n = normalize(said);
        self.plugins.iter().filter(|p| p.status == Status::Active).find_map(|p| {
            p.flows.iter().find(|f| f.triggers.contains(&n)).map(|f| (p.id.clone(), f.clone()))
        })
    }
}

// ------------------------------------------------------------------ as it runs

/// May this add-on run this step, now? Called before every step of an
/// add-on's sequence, with the command the step turned out to be.
///
/// Reads your decisions and the file fresh each time, so taking a permission
/// away or switching the add-on off stops it at its very next step, and a
/// file swapped after it was approved stops it at once.
///
/// `as_written` is the command the step's text names *before* any `{name}`
/// was filled in. A result from an earlier step -- a web page, a message --
/// may fill in what a step acts on, never change which command it is: "open
/// {x}" stays an `open`, whatever `x` turned out to say.
pub fn may_run(
    store: &Store,
    dir: &Path,
    id: &str,
    intent: Option<&str>,
    as_written: Option<&str>,
) -> Result<(), String> {
    let approvals = Approvals::load(store);
    let Some(a) = approvals.plugins.get(id) else {
        return Err(format!("the add-on {id} isn't approved"));
    };
    if a.disabled {
        return Err(format!("you switched the add-on {id} off"));
    }
    let file = dir.join(id).join(MANIFEST_FILE);
    let now = std::fs::read(&file).map(|b| fingerprint(&b)).unwrap_or_default();
    if now != a.sha256 {
        return Err(format!("the add-on {id} changed since you approved it"));
    }
    if as_written.is_some() && as_written != intent {
        return Err(format!(
            "an earlier step's result changed what a step of the add-on {id} does (it was written as \"{}\" and became \"{}\")",
            as_written.unwrap_or("?"),
            intent.unwrap_or("something Atlas doesn't know")
        ));
    }
    let Some(intent) = intent else {
        return Err(format!(
            "the add-on {id} asked for something that isn't a command Atlas knows, and add-ons may only use commands"
        ));
    };
    let p = permission_for(intent).map_err(|why| format!("the add-on {id} tried something {why}"))?;
    if !a.granted.iter().any(|g| g == p.key) {
        return Err(format!(
            "the add-on {id} wanted to {}, which you haven't allowed it to do",
            p.plain
        ));
    }
    Ok(())
}

// ------------------------------------------------------------------ your decisions

/// Approve an add-on, exactly as you saw it.
///
/// `seen_sha256` is the file you were shown. If it changed between looking
/// and pressing approve, this refuses rather than approving something you
/// did not see.
pub fn approve(
    store: &Store,
    dir: &Path,
    commands: &CommandsConfig,
    id: &str,
    seen_sha256: &str,
) -> Result<String, String> {
    let approvals = Approvals::load(store);
    let found = scan(dir, commands, &approvals).into_iter().find(|p| p.id == id);
    let Some(p) = found else { return Err(format!("there's no add-on called {id}")) };
    if p.sha256 != seen_sha256 {
        return Err(format!("{} changed since you looked at it. Look again, then approve.", p.name()));
    }
    let Some(m) = &p.manifest else { return Err(format!("{} can't be approved: {}", p.name(), p.status.plain())) };
    let mut approvals = approvals;
    // "Don't ask each time" you gave an earlier version carries over only
    // for a step that is still there word for word and still trustable --
    // you are approving the new file as shown, and those steps are in it.
    let kept: Vec<String> = approvals
        .plugins
        .get(id)
        .map(|a| a.trusted_steps.clone())
        .unwrap_or_default()
        .into_iter()
        .filter(|t| p.questions.iter().any(|q| &q.command == t && q.always_asks.is_none()))
        .collect();
    approvals.plugins.insert(
        id.to_string(),
        Approval {
            sha256: p.sha256.clone(),
            granted: m.permissions.clone(),
            approved_at: crate::store::now(),
            disabled: false,
            trusted_steps: kept,
        },
    );
    approvals.save(store)?;
    Ok(format!("{} is on. You can take any of what it may do away on the Add-ons page.", m.name))
}

/// Take one permission away from an add-on.
pub fn revoke(store: &Store, id: &str, key: &str) -> Result<String, String> {
    let mut approvals = Approvals::load(store);
    let Some(a) = approvals.plugins.get_mut(id) else { return Err(format!("{id} isn't approved")) };
    let before = a.granted.len();
    a.granted.retain(|g| g != key);
    if a.granted.len() == before {
        return Ok(format!("{id} wasn't allowed that anyway."));
    }
    approvals.save(store)?;
    let what = permission(key).map(|p| p.plain).unwrap_or(key);
    Ok(format!("{id} may no longer {what}. Anything it tries that needs it now stops."))
}

/// Switch an add-on off or back on, keeping your approval.
pub fn set_off(store: &Store, id: &str, off: bool) -> Result<String, String> {
    let mut approvals = Approvals::load(store);
    let Some(a) = approvals.plugins.get_mut(id) else { return Err(format!("{id} isn't approved")) };
    a.disabled = off;
    approvals.save(store)?;
    Ok(if off { format!("{id} is off.") } else { format!("{id} is back on.") })
}

/// Copy an add-on file into the add-ons folder. It arrives waiting for your
/// approval -- adding is never approving.
pub fn add_from(source: &Path, dir: &Path, commands: &CommandsConfig) -> Result<String, String> {
    let file = if source.is_dir() { source.join(MANIFEST_FILE) } else { source.to_path_buf() };
    let len = std::fs::metadata(&file).map_err(|e| format!("couldn't read {}: {e}", file.display()))?.len();
    if len > MAX_MANIFEST_BYTES {
        return Err(format!("{} is larger than an add-on can be", file.display()));
    }
    let text = std::fs::read_to_string(&file).map_err(|e| format!("couldn't read {}: {e}", file.display()))?;
    let c = parse_and_check(&text, None, commands).map_err(|s| format!("not added: {}", s.plain()))?;
    let (m, trouble) = (c.manifest, c.trouble);
    let dest = dir.join(&m.id);
    std::fs::create_dir_all(&dest).map_err(|e| format!("couldn't make {}: {e}", dest.display()))?;
    std::fs::write(dest.join(MANIFEST_FILE), &text).map_err(|e| format!("couldn't write it: {e}"))?;
    let mut said = format!(
        "Added {} by {}. It does nothing until you approve it -- the hub's Add-ons page shows what it wants to do.",
        m.name, m.author
    );
    for t in trouble {
        said.push_str(&format!("\n  note: {t}"));
    }
    Ok(said)
}

// ------------------------------------------------------------------ asking less

/// May this step of an add-on's run skip Atlas's usual "go ahead?"
///
/// Only when you said so for exactly this step, as written, on the file you
/// approved -- and never for a step whose text was filled in from an earlier
/// result, or one in `ALWAYS_ASKS`. The point of approving an add-on is that
/// you decided once; asking the same question every morning is how a
/// careful assistant becomes one you stop reading.
pub fn may_skip_question(
    store: &Store,
    dir: &Path,
    id: &str,
    as_written: &str,
    as_run: &str,
    intent: Option<&str>,
) -> bool {
    if as_written != as_run || why_always_asks(as_written, intent).is_some() {
        return false;
    }
    let approvals = Approvals::load(store);
    let Some(a) = approvals.plugins.get(id) else { return false };
    let now = std::fs::read(dir.join(id).join(MANIFEST_FILE)).map(|b| fingerprint(&b)).unwrap_or_default();
    !a.disabled && a.sha256 == now && a.trusted_steps.iter().any(|t| t == as_written)
}

/// "Don't ask me about this step each time." For a step of an approved add-on
/// that would otherwise ask, and that can be trusted at all.
pub fn trust_step(store: &Store, dir: &Path, commands: &CommandsConfig, id: &str, step: &str) -> Result<String, String> {
    let mut approvals = Approvals::load(store);
    let found = scan(dir, commands, &approvals).into_iter().find(|p| p.id == id);
    let Some(p) = found else { return Err(format!("there's no add-on called {id}")) };
    if p.status != Status::Active {
        return Err(format!("{} isn't on, so there's nothing to stop asking about", p.name()));
    }
    let Some(q) = p.questions.iter().find(|q| q.command == step) else {
        return Err(format!("\"{step}\" isn't a step of {} that asks first", p.name()));
    };
    if let Some(why) = &q.always_asks {
        return Err(format!("I'll keep asking before \"{step}\": {why}."));
    }
    let Some(a) = approvals.plugins.get_mut(id) else {
        return Err(format!("\"{id}\" isn't approved any more, so I'll ask before \"{step}\"."));
    };
    if !a.trusted_steps.iter().any(|t| t == step) {
        a.trusted_steps.push(step.to_string());
    }
    approvals.save(store)?;
    Ok(format!("I won't ask before \"{step}\" when {} runs it. The Add-ons page puts the question back.", p.name()))
}

/// Put the question back.
pub fn untrust_step(store: &Store, id: &str, step: &str) -> Result<String, String> {
    let mut approvals = Approvals::load(store);
    let Some(a) = approvals.plugins.get_mut(id) else { return Err(format!("{id} isn't approved")) };
    a.trusted_steps.retain(|t| t != step);
    approvals.save(store)?;
    Ok(format!("I'll ask before \"{step}\" again."))
}

// ------------------------------------------------------------------ removing

/// Remove an add-on: its folder goes to Atlas's trash (so it can be put back
/// for the usual 30 days), and your approval of it is forgotten, so putting
/// the same file back later means approving it again.
pub fn remove(store: &Store, dir: &Path, id: &str, trash: &crate::safety::Trash) -> Result<String, String> {
    if !id_ok(id) {
        return Err(format!("{id:?} isn't an add-on id"));
    }
    let folder = dir.join(id);
    if !folder.is_dir() {
        return Err(format!("there's no add-on called {id}"));
    }
    trash.take(&folder, "add-on removed by you").map_err(|e| format!("couldn't move it to the trash: {e}"))?;
    let mut approvals = Approvals::load(store);
    approvals.plugins.remove(id);
    approvals.sent_by.remove(id);
    approvals.save(store)?;
    Ok(format!("Removed {id}. It's in Atlas's trash for now if you want it back."))
}

// ------------------------------------------------------------------ shared by friends

/// The file name an add-on travels under between Atlases.
pub const SENT_SUFFIX: &str = ".atlas-addon.yaml";
/// What a handed-over add-on's covering line starts with when it was shared
/// in a group: `addon-share:<group name>`. Anything else is a private share.
pub const SHARED_IN: &str = "addon-share:";
const OFFERS: &str = "plugin_offers";
/// More than anyone would sift through; the oldest goes first.
const MAX_OFFERS: usize = 50;

/// An add-on somebody shared with you, waiting on your choice.
///
/// Nothing on this shelf is installed, approved or run. Taking one is your
/// decision alone, made seeing exactly what it would be allowed to do --
/// which is how a friend's add-on spreads by people choosing it, and never by
/// arriving.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Offered {
    pub offer: u64,
    /// The paired Atlas that sent it -- proven by the pairing.
    pub from: String,
    /// The group it was shared in, or `None` when sent to you alone.
    pub in_group: Option<String>,
    pub text: String,
    pub sha256: String,
    pub id: String,
    pub name: String,
    /// What the file says about who wrote it -- not proven by anything.
    pub author: String,
    pub description: String,
    pub permissions: Vec<String>,
    pub at: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Offers {
    pub items: Vec<Offered>,
    next: u64,
}

impl Offers {
    pub fn load(store: &Store) -> Offers {
        store.load(OFFERS)
    }
    fn save(&self, store: &Store) -> Result<(), String> {
        store.save(OFFERS, self).map_err(|e| format!("couldn't save it: {e}"))
    }
}

/// What happened to a file a paired Atlas handed over.
#[derive(Debug, Clone, PartialEq)]
pub enum Offer {
    /// Not an add-on; it goes wherever handed-over files go.
    NotAnAddOn,
    /// On your shelf, for you to take or leave.
    Shelved(String),
    /// It called itself an add-on and isn't a usable one.
    Refused(String),
}

/// A file from a paired Atlas that may be an add-on: checked like any add-on,
/// then put on the shelf of things offered to you. `covering` is the line it
/// came with (`SHARED_IN` + group name for a group share).
pub fn offered(
    store: &Store,
    commands: &CommandsConfig,
    from_peer: &str,
    file_name: &str,
    bytes: &[u8],
    covering: &str,
    now: u64,
) -> Offer {
    if !file_name.ends_with(SENT_SUFFIX) {
        return Offer::NotAnAddOn;
    }
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Offer::Refused(format!("{from_peer} sent an add-on larger than an add-on can be"));
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Offer::Refused(format!("{from_peer} sent an add-on that isn't text"));
    };
    let c = match parse_and_check(text, None, commands) {
        Ok(c) => c,
        Err(s) => return Offer::Refused(format!("{from_peer} sent an add-on I can't use: {}", s.plain())),
    };
    let in_group = covering.strip_prefix(SHARED_IN).map(|g| g.trim().to_string()).filter(|g| !g.is_empty());
    let sha = fingerprint(bytes);
    let mut offers = Offers::load(store);
    if offers.items.iter().any(|o| o.sha256 == sha) {
        // The same file again (a re-share, or a second friend): nothing new
        // to decide, and not a second thing to say.
        return Offer::Shelved(String::new());
    }
    offers.next += 1;
    let m = &c.manifest;
    offers.items.push(Offered {
        offer: offers.next,
        from: from_peer.to_string(),
        in_group: in_group.clone(),
        text: text.to_string(),
        sha256: sha,
        id: m.id.clone(),
        name: m.name.clone(),
        author: m.author.clone(),
        description: m.description.clone(),
        permissions: m.permissions.clone(),
        at: now,
    });
    if offers.items.len() > MAX_OFFERS {
        offers.items.remove(0);
    }
    if let Err(e) = offers.save(store) {
        return Offer::Refused(format!("{from_peer} shared an add-on and I couldn't keep it: {e}"));
    }
    Offer::Shelved(match in_group {
        Some(g) => format!("{from_peer} shared the add-on \"{}\" in {g}. It's on your Add-ons page if you want it.", m.name),
        None => format!("{from_peer} sent you the add-on \"{}\". It's on your Add-ons page if you want it.", m.name),
    })
}

/// Take an add-on off the shelf: it goes into your add-ons **and is approved
/// in the same step**, for exactly the file and the permissions you were
/// shown (`seen_sha256`) -- one decision, not two.
pub fn take_offer(store: &Store, dir: &Path, commands: &CommandsConfig, offer: u64, seen_sha256: &str) -> Result<String, String> {
    let mut offers = Offers::load(store);
    let o = offers.items.iter().find(|o| o.offer == offer).cloned().ok_or("that offer isn't there any more")?;
    if o.sha256 != seen_sha256 {
        return Err("that isn't the add-on you were shown".into());
    }
    parse_and_check(&o.text, None, commands).map_err(|s| format!("I can't use it: {}", s.plain()))?;
    let path = dir.join(&o.id).join(MANIFEST_FILE);
    if let Ok(mine) = std::fs::read_to_string(&path) {
        if fingerprint(mine.as_bytes()) != o.sha256 {
            return Err(format!(
                "you already have a different add-on called {}. Remove yours first if you want theirs instead.",
                o.id
            ));
        }
    }
    std::fs::create_dir_all(dir.join(&o.id)).and_then(|_| std::fs::write(&path, &o.text)).map_err(|e| format!("couldn't save it: {e}"))?;
    let said = approve(store, dir, commands, &o.id, &o.sha256)?;
    let mut approvals = Approvals::load(store);
    approvals.sent_by.insert(o.id.clone(), o.from.clone());
    approvals.save(store)?;
    offers.items.retain(|x| x.offer != offer);
    offers.save(store)?;
    Ok(said)
}

/// Leave it: off the shelf, nothing kept.
pub fn decline_offer(store: &Store, offer: u64) -> Result<String, String> {
    let mut offers = Offers::load(store);
    let before = offers.items.len();
    offers.items.retain(|x| x.offer != offer);
    if offers.items.len() == before {
        return Err("that offer isn't there any more".into());
    }
    offers.save(store)?;
    Ok("Left it.".into())
}

/// An add-on of yours, ready to hand to someone: (file name to send under,
/// its bytes, its name).
pub fn to_share(dir: &Path, id: &str) -> Result<(String, Vec<u8>, String), String> {
    if !id_ok(id) {
        return Err(format!("{id:?} isn't an add-on id"));
    }
    let bytes = std::fs::read(dir.join(id).join(MANIFEST_FILE)).map_err(|_| format!("there's no add-on called {id}"))?;
    let name = std::str::from_utf8(&bytes)
        .ok()
        .and_then(|t| serde_yaml::from_str::<Manifest>(t).ok())
        .map(|m| m.name)
        .unwrap_or_else(|| id.to_string());
    Ok((format!("{id}{SENT_SUFFIX}"), bytes, name))
}

// ------------------------------------------------------------------ running by itself

const SCHEDULE_RUNS: &str = "plugin_schedule_runs";

/// When each scheduled add-on sequence last ran (or was first seen), keyed
/// `id/sequence name`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScheduleRuns {
    pub last: BTreeMap<String, u64>,
}

impl ScheduleRuns {
    pub fn load(store: &Store) -> ScheduleRuns {
        store.load(SCHEDULE_RUNS)
    }
    pub fn save(&self, store: &Store) -> crate::error::Result<()> {
        store.save(SCHEDULE_RUNS, self)
    }
    /// It ran (or started) now.
    pub fn ran(&mut self, key: &str, now: u64) {
        self.last.insert(key.to_string(), now);
    }
}

/// Is a schedule due, given when it last ran? `None` last run means it has
/// just been seen for the first time: never due on sight -- an add-on you
/// approve at 10:00 with "daily at 08:00" first runs tomorrow at 08:00, not
/// the moment you approve it.
pub fn is_due(sch: Schedule, last: u64, now: u64, offset_mins: i16) -> bool {
    match sch {
        Schedule::Every(secs) => now >= last.saturating_add(secs),
        Schedule::DailyAt(minute) => {
            let local = |t: u64| (t as i64 + offset_mins as i64 * 60).max(0) as u64;
            let (day_now, day_last) = (local(now) / 86_400, local(last) / 86_400);
            let minute_now = (local(now) % 86_400 / 60) as u32;
            let minute_last = (local(last) % 86_400 / 60) as u32;
            // A later day, past the time -- or the same day, having last run
            // (or been first seen) before the time and now past it.
            (day_now > day_last && minute_now >= minute)
                || (day_now == day_last && minute_last < minute && minute_now >= minute)
        }
    }
}

impl Registry {
    /// The add-on sequences due to run by themselves now, as (add-on id,
    /// sequence, schedule key), and records the ones seen for the first time.
    /// Only for add-ons that are on. Starting one is recorded with
    /// `ScheduleRuns::ran`, so one that couldn't start yet stays due.
    pub fn due(&self, runs: &mut ScheduleRuns, now: u64, offset_mins: i16) -> Vec<(String, Workflow, String)> {
        let mut out = Vec::new();
        for p in self.plugins.iter().filter(|p| p.status == Status::Active) {
            for (name, sch) in &p.schedules {
                let key = format!("{}/{name}", p.id);
                let Some(&last) = runs.last.get(&key) else {
                    runs.last.insert(key, now);
                    continue;
                };
                if is_due(*sch, last, now, offset_mins) {
                    if let Some(f) = p.flows.iter().find(|f| &f.name == name) {
                        out.push((p.id.clone(), f.clone(), key));
                    }
                }
            }
        }
        // Forget schedules whose add-on or sequence is gone, so re-adding one
        // starts fresh rather than firing on a stale record.
        runs.last.retain(|k, _| {
            let (id, name) = k.split_once('/').unwrap_or((k, ""));
            self.plugins.iter().any(|p| p.id == id && p.schedules.iter().any(|(n, _)| n == name))
        });
        out
    }
}

// ------------------------------------------------------------------ your other devices

const ANNOUNCED: &str = "plugin_sync_announced";

/// The prefix sync events about add-ons carry in their subject.
pub const SYNC_PREFIX: &str = "addon:";

/// What this device has already told your other devices about its add-ons,
/// so only changes travel -- and what it heard from them, so it does not
/// echo it straight back.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct Announced {
    /// id -> fingerprint of the file as announced.
    files: BTreeMap<String, String>,
    /// id -> the approval as announced, as JSON.
    approvals: BTreeMap<String, String>,
}

/// One change to carry to your other devices: (subject, field, value) for a
/// `sync::What::Changed`.
pub type SyncChange = (String, String, String);

/// What changed about add-ons since this device last told your others.
///
/// Worked out by comparing, rather than recorded where each change is made:
/// an add-on changes through the hub, the terminal, or a friend sending one,
/// some of them in another process -- comparing at sync time catches all of
/// them and cannot miss the one nobody remembered to record.
pub fn changes_to_carry(store: &Store, dir: &Path) -> Vec<SyncChange> {
    let mut told: Announced = store.load(ANNOUNCED);
    let approvals = Approvals::load(store);
    let mut out = Vec::new();
    let mut here: BTreeMap<String, (String, String)> = BTreeMap::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let id = e.file_name().to_string_lossy().to_string();
            if !id_ok(&id) {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(e.path().join(MANIFEST_FILE)) {
                if text.len() as u64 <= MAX_MANIFEST_BYTES {
                    here.insert(id, (fingerprint(text.as_bytes()), text));
                }
            }
        }
    }
    for (id, (sha, text)) in &here {
        if told.files.get(id) != Some(sha) {
            out.push((format!("{SYNC_PREFIX}{id}"), "file".into(), text.clone()));
            told.files.insert(id.clone(), sha.clone());
        }
    }
    for id in told.files.keys().cloned().collect::<Vec<_>>() {
        if !here.contains_key(&id) {
            out.push((format!("{SYNC_PREFIX}{id}"), "removed".into(), String::new()));
            told.files.remove(&id);
            told.approvals.remove(&id);
        }
    }
    for (id, a) in &approvals.plugins {
        let json = serde_json::to_string(a).unwrap_or_default();
        if here.contains_key(id) && told.approvals.get(id) != Some(&json) {
            out.push((format!("{SYNC_PREFIX}{id}"), "approval".into(), json.clone()));
            told.approvals.insert(id.clone(), json);
        }
    }
    if !out.is_empty() {
        let _ = store.save(ANNOUNCED, &told);
    }
    out
}

/// Take in one add-on change from your other device.
///
/// `sealed` says whether it arrived in a bundle sealed with your household
/// key. A file may arrive either way -- it lands switched off, which is what
/// any new add-on does. An *approval* is only taken from a sealed bundle:
/// an unsealed bundle in a shared folder could have been written by anyone
/// with the folder, and an approval is permission to act.
pub fn take_synced(
    store: &Store,
    dir: &Path,
    commands: &CommandsConfig,
    subject: &str,
    field: &str,
    value: &str,
    sealed: bool,
    from_device: &str,
) -> Option<String> {
    let id = subject.strip_prefix(SYNC_PREFIX)?;
    if !id_ok(id) {
        return None;
    }
    let mut told: Announced = store.load(ANNOUNCED);
    let said = match field {
        "file" => {
            let c = parse_and_check(value, Some(id), commands).ok()?;
            let path = dir.join(id).join(MANIFEST_FILE);
            if std::fs::read_to_string(&path).ok().as_deref() == Some(value) {
                None
            } else {
                std::fs::create_dir_all(dir.join(id)).ok()?;
                std::fs::write(&path, value).ok()?;
                told.files.insert(id.to_string(), fingerprint(value.as_bytes()));
                Some(format!("The add-on \"{}\" came over from {from_device}.", c.manifest.name))
            }
        }
        "approval" if sealed => {
            let a: Approval = serde_json::from_str(value).ok()?;
            let mut approvals = Approvals::load(store);
            if approvals.plugins.get(id) == Some(&a) {
                return None;
            }
            approvals.plugins.insert(id.to_string(), a);
            approvals.save(store).ok()?;
            told.approvals.insert(id.to_string(), value.to_string());
            None
        }
        "removed" => {
            let folder = dir.join(id);
            if !folder.exists() {
                return None;
            }
            // Your other device removed it; this one switches it off and
            // forgets the approval, and leaves the file for you to delete --
            // a sync that deletes files is one bad bundle away from deleting
            // the wrong ones.
            let mut approvals = Approvals::load(store);
            approvals.plugins.remove(id);
            let _ = approvals.save(store);
            told.files.remove(id);
            told.approvals.remove(id);
            Some(format!("You removed the add-on {id} on {from_device}, so it's off here too."))
        }
        _ => None,
    };
    let _ = store.save(ANNOUNCED, &told);
    said
}

/// One press of a button on the hub's Add-ons page.
#[allow(clippy::too_many_arguments)]
pub fn hub_action(
    store: &Store,
    dir: &Path,
    commands: &CommandsConfig,
    trash: &crate::safety::Trash,
    what: &str,
    id: &str,
    key: &str,
    sha: &str,
) -> Result<String, String> {
    match what {
        "remove" => remove(store, dir, id, trash),
        "approve" => approve(store, dir, commands, id, sha),
        "revoke" => revoke(store, id, key),
        "off" => set_off(store, id, true),
        "on" => set_off(store, id, false),
        // `key` carries the step, for these two.
        // `id` carries the offer number and `sha` what you were shown, for these two.
        "take" => take_offer(store, dir, commands, id.parse().map_err(|_| "no such offer".to_string())?, sha),
        "decline" => decline_offer(store, id.parse().map_err(|_| "no such offer".to_string())?),
        "trust" => trust_step(store, dir, commands, id, key),
        "share" | "recommend" => Err("Sharing talks to other people's Atlases, so it needs Atlas itself running. Start it and press it again.".into()),
        "untrust" => untrust_step(store, id, key),
        other => Err(format!("unknown add-on action {other:?}")),
    }
}

// ------------------------------------------------------------------ tests

#[cfg(test)]
mod tests {
    use super::*;

    fn commands() -> CommandsConfig {
        serde_yaml::from_str(include_str!("../config/commands.yaml")).unwrap()
    }

    fn scratch(tag: &str) -> (PathBuf, Store) {
        let d = std::env::temp_dir().join(format!("atlas-plugins-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("plugins")).unwrap();
        std::fs::create_dir_all(d.join("state")).unwrap();
        let store = Store::new(d.join("state"));
        (d.join("plugins"), store)
    }

    const GOOD: &str = "\
plugin_api: 1
id: morning-markets
name: Morning markets
author: Eric
permissions: [desktop, online]
flows:
  - name: morning markets
    triggers: [\"morning markets\", \"markets please\"]
    steps:
      - command: open tradingview
      - command: research eurusd news
        produces: news
";

    fn put(dir: &Path, id: &str, text: &str) {
        std::fs::create_dir_all(dir.join(id)).unwrap();
        std::fs::write(dir.join(id).join(MANIFEST_FILE), text).unwrap();
    }

    fn only(dir: &Path, store: &Store) -> Plugin {
        scan(dir, &commands(), &Approvals::load(store)).remove(0)
    }

    #[test]
    fn every_command_is_decided_for_add_ons() {
        let cmds = commands();
        let mut undecided = Vec::new();
        for c in &cmds.commands {
            let in_perm = PERMISSIONS.iter().filter(|p| p.intents.contains(&c.intent.as_str())).count();
            let never = NEVER.iter().filter(|(i, _)| *i == c.intent).count();
            if in_perm + never != 1 {
                undecided.push(format!("{} (in {in_perm} permissions, {never} never)", c.intent));
            }
        }
        assert!(undecided.is_empty(), "every command must be in exactly one permission or in NEVER: {undecided:?}");
        // And nothing names a command that doesn't exist.
        for p in PERMISSIONS {
            for i in p.intents {
                assert!(cmds.commands.iter().any(|c| c.intent == *i), "{} names {i}, which isn't a command", p.key);
            }
        }
        for (i, _) in NEVER {
            assert!(cmds.commands.iter().any(|c| c.intent == *i), "NEVER names {i}, which isn't a command");
        }
    }

    #[test]
    fn an_add_on_does_nothing_until_approved_and_runs_only_as_approved() {
        let (dir, store) = scratch("approve");
        put(&dir, "morning-markets", GOOD);
        let p = only(&dir, &store);
        assert_eq!(p.status, Status::Waiting);
        assert!(Registry { plugins: vec![p.clone()] }.match_trigger("morning markets").is_none());
        assert!(may_run(&store, &dir, "morning-markets", Some("open_app"), None).is_err());

        approve(&store, &dir, &commands(), "morning-markets", &p.sha256).unwrap();
        let reg = Registry::load(&dir, &commands(), &store);
        let (id, flow) = reg.match_trigger("Morning markets!").expect("exact trigger starts it");
        assert_eq!(id, "morning-markets");
        assert_eq!(flow.steps.len(), 2);
        assert!(reg.match_trigger("morning markets and more").is_none(), "only the exact words start it");
        assert!(may_run(&store, &dir, "morning-markets", Some("open_app"), None).is_ok());
        assert!(may_run(&store, &dir, "morning-markets", Some("research"), None).is_ok());
    }

    #[test]
    fn the_kept_registry_notices_an_approval_and_a_new_add_on() {
        // 27 Sep 2026: kept between turns rather than read and hashed every
        // time, so what changes it must still reach it.
        let (dir, store) = scratch("kept");
        let mut kept = Kept::default();
        assert!(Registry::load_kept(&mut kept, &dir, &commands(), &store).plugins.is_empty());
        put(&dir, "morning-markets", GOOD);
        let p = Registry::load_kept(&mut kept, &dir, &commands(), &store).plugins[0].clone();
        assert_eq!(p.status, Status::Waiting, "a new add-on was not seen");
        approve(&store, &dir, &commands(), "morning-markets", &p.sha256).unwrap();
        assert!(
            Registry::load_kept(&mut kept, &dir, &commands(), &store).match_trigger("morning markets").is_some(),
            "an approval was not seen"
        );
    }

    #[test]
    fn an_earlier_result_may_fill_in_a_step_but_never_change_which_command_it_is() {
        let (dir, store) = scratch("placeholder");
        put(&dir, "morning-markets", GOOD);
        let p = only(&dir, &store);
        approve(&store, &dir, &commands(), "morning-markets", &p.sha256).unwrap();
        // "open {x}" filled in with anything is still an open.
        assert!(may_run(&store, &dir, "morning-markets", Some("open_app"), Some("open_app")).is_ok());
        // "{x}" that turned into a different command -- even one it's allowed -- stops.
        let e = may_run(&store, &dir, "morning-markets", Some("research"), Some("open_app")).unwrap_err();
        assert!(e.contains("changed what a step"), "{e}");
    }

    #[test]
    fn a_permission_it_did_not_get_stops_the_step() {
        let (dir, store) = scratch("scope");
        put(&dir, "morning-markets", GOOD);
        let p = only(&dir, &store);
        approve(&store, &dir, &commands(), "morning-markets", &p.sha256).unwrap();
        // A step whose text became something else (say, through {news}).
        let e = may_run(&store, &dir, "morning-markets", Some("message"), None).unwrap_err();
        assert!(e.contains("haven't allowed"), "{e}");
        let e = may_run(&store, &dir, "morning-markets", Some("unlock"), None).unwrap_err();
        assert!(e.contains("never"), "{e}");
        let e = may_run(&store, &dir, "morning-markets", None, None).unwrap_err();
        assert!(e.contains("isn't a command"), "{e}");
    }

    #[test]
    fn taking_a_permission_away_is_immediate() {
        let (dir, store) = scratch("revoke");
        put(&dir, "morning-markets", GOOD);
        let p = only(&dir, &store);
        approve(&store, &dir, &commands(), "morning-markets", &p.sha256).unwrap();
        revoke(&store, "morning-markets", "online").unwrap();
        assert!(may_run(&store, &dir, "morning-markets", Some("research"), None).is_err());
        assert!(may_run(&store, &dir, "morning-markets", Some("open_app"), None).is_ok());
        set_off(&store, "morning-markets", true).unwrap();
        assert!(may_run(&store, &dir, "morning-markets", Some("open_app"), None).is_err());
        assert_eq!(only(&dir, &store).status, Status::Disabled);
        set_off(&store, "morning-markets", false).unwrap();
        assert_eq!(only(&dir, &store).status, Status::Active);
        assert_eq!(only(&dir, &store).granted, vec!["desktop".to_string()]);
    }

    #[test]
    fn a_file_swapped_after_approval_stops_at_once_and_needs_approving_again() {
        let (dir, store) = scratch("swap");
        put(&dir, "morning-markets", GOOD);
        let p = only(&dir, &store);
        approve(&store, &dir, &commands(), "morning-markets", &p.sha256).unwrap();
        put(&dir, "morning-markets", &GOOD.replace("open tradingview", "open notepad"));
        assert!(may_run(&store, &dir, "morning-markets", Some("open_app"), None).is_err());
        assert_eq!(only(&dir, &store).status, Status::Changed);
        assert!(Registry::load(&dir, &commands(), &store).match_trigger("morning markets").is_none());
    }

    #[test]
    fn approving_refuses_a_file_that_changed_since_you_looked() {
        let (dir, store) = scratch("toctou");
        put(&dir, "morning-markets", GOOD);
        let seen = only(&dir, &store).sha256;
        put(&dir, "morning-markets", &GOOD.replace("[desktop, online]", "[desktop, online, send_messages]"));
        assert!(approve(&store, &dir, &commands(), "morning-markets", &seen).is_err());
        assert_eq!(only(&dir, &store).status, Status::Waiting);
    }

    #[test]
    fn a_step_needing_an_undeclared_or_forbidden_permission_is_refused_on_reading() {
        let (dir, store) = scratch("undeclared");
        put(&dir, "morning-markets", &GOOD.replace("[desktop, online]", "[desktop]"));
        assert!(matches!(only(&dir, &store).status, Status::Broken(ref w) if w.contains("online")));
        put(&dir, "morning-markets", &GOOD.replace("research eurusd news", "unlock hunter2"));
        assert!(matches!(only(&dir, &store).status, Status::Broken(ref w) if w.contains("never")));
        put(&dir, "morning-markets", &GOOD.replace("research eurusd news", "do a little dance"));
        assert!(matches!(only(&dir, &store).status, Status::Broken(ref w) if w.contains("isn't a command")));
    }

    #[test]
    fn unknown_fields_ids_and_formats_are_refused_visibly() {
        let (dir, store) = scratch("strict");
        put(&dir, "morning-markets", &format!("{GOOD}run_this: rm -rf /\n"));
        assert!(matches!(only(&dir, &store).status, Status::Broken(_)));
        put(&dir, "morning-markets", &GOOD.replace("plugin_api: 1", "plugin_api: 9"));
        assert_eq!(only(&dir, &store).status, Status::NeedsNewerAtlas(9));
        put(&dir, "morning-markets", &GOOD.replace("plugin_api: 1", "plugin_api: 0"));
        assert_eq!(only(&dir, &store).status, Status::TooOld(0));
        put(&dir, "morning-markets", &GOOD.replace("id: morning-markets", "id: something-else"));
        assert!(matches!(only(&dir, &store).status, Status::Broken(ref w) if w.contains("folder")));
        put(&dir, "morning-markets", &GOOD.replace("permissions: [desktop, online]", "permissions: [desktop, online, root]"));
        assert!(matches!(only(&dir, &store).status, Status::Broken(_)));
        std::fs::write(dir.join("morning-markets").join(MANIFEST_FILE), vec![b'#'; MAX_MANIFEST_BYTES as usize + 1]).unwrap();
        assert!(matches!(only(&dir, &store).status, Status::Broken(ref w) if w.contains("larger")));
    }

    #[test]
    fn a_trigger_cannot_stand_in_for_an_answer_a_command_or_something_forbidden() {
        let (dir, store) = scratch("triggers");
        let t = GOOD.replace(
            "[\"morning markets\", \"markets please\"]",
            "[\"yes\", \"go ahead\", \"back up\", \"unlock the vault\", \"markets\", \"morning markets\"]",
        );
        put(&dir, "morning-markets", &t);
        let p = only(&dir, &store);
        assert_eq!(p.status, Status::Waiting, "trouble with triggers doesn't switch the whole add-on off");
        assert_eq!(p.flows[0].triggers, vec!["morning markets".to_string()]);
        assert_eq!(p.trouble.len(), 5, "{:?}", p.trouble);
    }

    #[test]
    fn two_add_ons_claiming_the_same_words_start_neither() {
        let (dir, store) = scratch("collide");
        put(&dir, "morning-markets", GOOD);
        let other = GOOD.replace("id: morning-markets", "id: copycat").replace("\"markets please\"", "\"copycat please\"");
        put(&dir, "copycat", &other);
        for p in scan(&dir, &commands(), &Approvals::default()) {
            approve(&store, &dir, &commands(), &p.id, &p.sha256).unwrap();
        }
        let reg = Registry::load(&dir, &commands(), &store);
        assert!(reg.match_trigger("morning markets").is_none());
        assert_eq!(reg.match_trigger("markets please").map(|x| x.0), Some("morning-markets".to_string()));
        assert_eq!(reg.match_trigger("copycat please").map(|x| x.0), Some("copycat".to_string()));
        assert!(reg.plugins.iter().all(|p| p.trouble.iter().any(|t| t.contains("more than one"))));
    }

    #[test]
    fn adding_is_never_approving() {
        let (dir, store) = scratch("add");
        let src = dir.parent().unwrap().join("incoming.yaml");
        std::fs::write(&src, GOOD).unwrap();
        let said = add_from(&src, &dir, &commands()).unwrap();
        assert!(said.contains("does nothing until you approve"));
        assert_eq!(only(&dir, &store).status, Status::Waiting);
        std::fs::write(&src, GOOD.replace("id: morning-markets", "id: ../../escape")).unwrap();
        assert!(add_from(&src, &dir, &commands()).is_err());
    }

    #[test]
    fn an_unreadable_approvals_record_approves_nothing() {
        let (dir, store) = scratch("corrupt");
        put(&dir, "morning-markets", GOOD);
        let p = only(&dir, &store);
        approve(&store, &dir, &commands(), "morning-markets", &p.sha256).unwrap();
        let rec = std::fs::read_dir(dir.parent().unwrap().join("state"))
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .find(|p| p.to_string_lossy().contains(APPROVALS))
            .expect("the approvals record");
        std::fs::write(&rec, "{ not json").unwrap();
        assert_eq!(only(&dir, &store).status, Status::Waiting);
        assert!(may_run(&store, &dir, "morning-markets", Some("open_app"), None).is_err());
    }

    // ---------------- asking less ----------------

    const ASKS: &str = "\
plugin_api: 1
id: wind-down
name: Wind down
author: a friend
permissions: [basics, desktop, send_messages]
flows:
  - name: wind down
    triggers: [\"wind things down\"]
    steps:
      - command: close chrome
      - command: what can you do
        produces: x
      - command: close {x}
      - command: message sam goodnight
";

    fn approved(tag: &str, text: &str, id: &str) -> (PathBuf, Store, Plugin) {
        let (dir, store) = scratch(tag);
        put(&dir, id, text);
        let p = only(&dir, &store);
        approve(&store, &dir, &commands(), id, &p.sha256).unwrap();
        let p = only(&dir, &store);
        (dir, store, p)
    }

    #[test]
    fn the_steps_that_would_ask_are_known_and_only_safe_ones_can_stop_asking() {
        let (dir, store, p) = approved("questions", ASKS, "wind-down");
        let q = |c: &str| p.questions.iter().find(|q| q.command == c).cloned();
        assert!(q("close chrome").is_some_and(|q| q.always_asks.is_none()), "{:?}", p.questions);
        assert!(q("close {x}").is_some_and(|q| q.always_asks.is_some()), "a filled-in step can't be trusted");
        assert!(q("what can you do").is_none(), "a step that never asks isn't listed");
        assert!(trust_step(&store, &dir, &commands(), "wind-down", "close {x}").is_err());
        if let Some(m) = q("message sam goodnight") {
            assert!(m.always_asks.is_some(), "a message always asks");
            assert!(trust_step(&store, &dir, &commands(), "wind-down", "message sam goodnight").is_err());
        }
        trust_step(&store, &dir, &commands(), "wind-down", "close chrome").unwrap();
        assert!(may_skip_question(&store, &dir, "wind-down", "close chrome", "close chrome", Some("close_app")));
        // Filled in from an earlier result: still asks, even if the text matches something trusted.
        assert!(!may_skip_question(&store, &dir, "wind-down", "close {x}", "close chrome", Some("close_app")));
        untrust_step(&store, "wind-down", "close chrome").unwrap();
        assert!(!may_skip_question(&store, &dir, "wind-down", "close chrome", "close chrome", Some("close_app")));
    }

    #[test]
    fn trust_carries_to_a_new_version_only_for_steps_still_there_word_for_word() {
        let (dir, store, _) = approved("carry", ASKS, "wind-down");
        trust_step(&store, &dir, &commands(), "wind-down", "close chrome").unwrap();
        // A changed file: nothing is skipped until it's approved again.
        put(&dir, "wind-down", &ASKS.replace("what can you do", "whats outstanding"));
        assert!(!may_skip_question(&store, &dir, "wind-down", "close chrome", "close chrome", Some("close_app")));
        let p = only(&dir, &store);
        approve(&store, &dir, &commands(), "wind-down", &p.sha256).unwrap();
        assert!(may_skip_question(&store, &dir, "wind-down", "close chrome", "close chrome", Some("close_app")));
        // A version where the step changed: the old trust doesn't apply to the new words.
        put(&dir, "wind-down", &ASKS.replace("close chrome", "close notepad"));
        let p = only(&dir, &store);
        approve(&store, &dir, &commands(), "wind-down", &p.sha256).unwrap();
        assert!(Approvals::load(&store).plugins["wind-down"].trusted_steps.is_empty());
    }

    // ---------------- removing ----------------

    #[test]
    fn removing_moves_it_to_the_trash_and_forgets_the_approval() {
        let (dir, store, _) = approved("remove", GOOD, "morning-markets");
        let trash = crate::safety::Trash::new(crate::safety::TrashConfig {
            dir: dir.parent().unwrap().join("trash").to_string_lossy().into_owned(),
            keep_days: 30,
        });
        remove(&store, &dir, "morning-markets", &trash).unwrap();
        assert!(!dir.join("morning-markets").exists());
        assert!(trash.ledger().iter().any(|d| d.original.ends_with("morning-markets")));
        assert!(Approvals::load(&store).plugins.is_empty());
        assert!(remove(&store, &dir, "../state", &trash).is_err());
    }

    // ---------------- from a friend ----------------

    #[test]
    fn a_shared_add_on_waits_on_the_shelf_until_you_choose_it() {
        let (dir, store) = scratch("offered");
        let name = format!("morning-markets{SENT_SUFFIX}");
        assert_eq!(offered(&store, &commands(), "Sam", "notes.yaml", GOOD.as_bytes(), "", 1), Offer::NotAnAddOn);
        let said = offered(&store, &commands(), "Sam", &name, GOOD.as_bytes(), &format!("{SHARED_IN}Friends"), 1);
        assert!(matches!(&said, Offer::Shelved(m) if m.contains("in Friends")), "{said:?}");
        // Nothing installed, nothing approved.
        assert!(scan(&dir, &commands(), &Approvals::load(&store)).is_empty());
        // The same file again is not a second offer.
        assert_eq!(offered(&store, &commands(), "Maya", &name, GOOD.as_bytes(), "", 2), Offer::Shelved(String::new()));
        let shelf = Offers::load(&store).items;
        assert_eq!(shelf.len(), 1);
        assert_eq!(shelf[0].in_group.as_deref(), Some("Friends"));
        assert_eq!(shelf[0].permissions, vec!["desktop".to_string(), "online".to_string()]);

        // Taking it is one decision: added and approved, exactly as shown.
        assert!(take_offer(&store, &dir, &commands(), shelf[0].offer, "not-what-you-saw").is_err());
        take_offer(&store, &dir, &commands(), shelf[0].offer, &shelf[0].sha256).unwrap();
        let p = only(&dir, &store);
        assert_eq!(p.status, Status::Active);
        assert_eq!(p.sent_by.as_deref(), Some("Sam"));
        assert!(Offers::load(&store).items.is_empty());
    }

    #[test]
    fn a_shared_add_on_never_replaces_yours_and_a_bad_one_never_reaches_the_shelf() {
        let (dir, store) = scratch("offered-clash");
        put(&dir, "morning-markets", &GOOD.replace("Morning markets", "Mine"));
        let name = format!("morning-markets{SENT_SUFFIX}");
        offered(&store, &commands(), "Sam", &name, GOOD.as_bytes(), "", 1);
        let o = Offers::load(&store).items[0].clone();
        assert!(take_offer(&store, &dir, &commands(), o.offer, &o.sha256).is_err());
        assert!(std::fs::read_to_string(dir.join("morning-markets").join(MANIFEST_FILE)).unwrap().contains("Mine"));
        decline_offer(&store, o.offer).unwrap();
        assert!(Offers::load(&store).items.is_empty());

        let bad = GOOD.replace("research eurusd news", "unlock hunter2").replace("morning-markets", "bad-one");
        assert!(matches!(offered(&store, &commands(), "Sam", "bad-one.atlas-addon.yaml", bad.as_bytes(), "", 2), Offer::Refused(_)));
        assert!(Offers::load(&store).items.is_empty());
    }

    // ---------------- running by itself ----------------

    #[test]
    fn schedules_are_read_strictly() {
        assert_eq!(read_schedule("every 30 minutes"), Ok(Schedule::Every(1800)));
        assert_eq!(read_schedule("Every 2 hours"), Ok(Schedule::Every(7200)));
        assert_eq!(read_schedule("daily at 08:05"), Ok(Schedule::DailyAt(485)));
        assert!(read_schedule("every 5 minutes").is_err(), "faster than the floor");
        assert!(read_schedule("daily at 25:00").is_err());
        assert!(read_schedule("whenever").is_err());
    }

    #[test]
    fn a_daily_sequence_runs_once_a_day_at_its_time_and_never_on_sight() {
        let day = 86_400;
        let at = |d: u64, h: u64, m: u64| d * day + h * 3600 + m * 60;
        let s = Schedule::DailyAt(8 * 60);
        // Seen at 10:00: not today.
        assert!(!is_due(s, at(1, 10, 0), at(1, 11, 0), 0));
        assert!(!is_due(s, at(1, 10, 0), at(2, 7, 59), 0));
        assert!(is_due(s, at(1, 10, 0), at(2, 8, 0), 0));
        // Ran at 08:00: not again until tomorrow.
        assert!(!is_due(s, at(2, 8, 0), at(2, 23, 0), 0));
        // Seen at 07:00: today at 08:00.
        assert!(is_due(s, at(3, 7, 0), at(3, 8, 1), 0));
        // Local time: 08:00 at UTC-5 is 13:00 UTC.
        assert!(!is_due(s, at(4, 0, 0), at(4, 8, 0), -300));
        assert!(is_due(s, at(4, 0, 0), at(4, 13, 0), -300));
        let every = Schedule::Every(1800);
        assert!(!is_due(every, 1000, 2000, 0));
        assert!(is_due(every, 1000, 2800, 0));
    }

    #[test]
    fn a_scheduled_sequence_is_due_only_when_on_and_is_first_seen_not_fired() {
        let text = GOOD.replace("    triggers: [\"morning markets\", \"markets please\"]\n", "    schedule: every 30 minutes\n");
        let (dir, store) = scratch("sched");
        put(&dir, "morning-markets", &text);
        let mut runs = ScheduleRuns::default();
        let reg = Registry::load(&dir, &commands(), &store);
        assert!(reg.plugins[0].trouble.is_empty(), "a scheduled sequence needs no words: {:?}", reg.plugins[0].trouble);
        assert!(reg.due(&mut runs, 1_000, 0).is_empty(), "not approved");
        approve(&store, &dir, &commands(), "morning-markets", &reg.plugins[0].sha256).unwrap();
        let reg = Registry::load(&dir, &commands(), &store);
        assert!(reg.due(&mut runs, 1_000, 0).is_empty(), "first sight records, doesn't fire");
        assert!(reg.due(&mut runs, 2_000, 0).is_empty());
        let due = reg.due(&mut runs, 2_900, 0);
        assert_eq!(due.len(), 1);
        runs.ran(&due[0].2, 2_900);
        assert!(reg.due(&mut runs, 3_000, 0).is_empty());
        set_off(&store, "morning-markets", true).unwrap();
        let reg = Registry::load(&dir, &commands(), &store);
        assert!(reg.due(&mut runs, 9_000, 0).is_empty(), "switched off means off");
    }

    // ---------------- your other devices ----------------

    #[test]
    fn add_ons_follow_you_but_an_approval_only_from_a_sealed_bundle() {
        let (laptop, laptop_store, _) = approved("sync-laptop", GOOD, "morning-markets");
        let (phone, phone_store) = scratch("sync-phone");
        let changes = changes_to_carry(&laptop_store, &laptop);
        assert!(changes.iter().any(|(_, f, _)| f == "file"));
        assert!(changes.iter().any(|(_, f, _)| f == "approval"));
        assert!(changes_to_carry(&laptop_store, &laptop).is_empty(), "only changes travel");

        // Unsealed: the file arrives, switched off; the approval does not.
        for (sub, field, val) in &changes {
            take_synced(&phone_store, &phone, &commands(), sub, field, val, false, "laptop");
        }
        assert_eq!(only(&phone, &phone_store).status, Status::Waiting);
        // Sealed: the approval comes too.
        for (sub, field, val) in &changes {
            take_synced(&phone_store, &phone, &commands(), sub, field, val, true, "laptop");
        }
        assert_eq!(only(&phone, &phone_store).status, Status::Active);
        // And what arrived is not sent straight back.
        assert!(changes_to_carry(&phone_store, &phone).is_empty(), "echoed back what it was just told");

        // Removed on the laptop: off on the phone, file left for you.
        let trash = crate::safety::Trash::new(crate::safety::TrashConfig {
            dir: laptop.parent().unwrap().join("trash").to_string_lossy().into_owned(),
            keep_days: 30,
        });
        remove(&laptop_store, &laptop, "morning-markets", &trash).unwrap();
        for (sub, field, val) in changes_to_carry(&laptop_store, &laptop) {
            take_synced(&phone_store, &phone, &commands(), &sub, &field, &val, true, "laptop");
        }
        assert_eq!(only(&phone, &phone_store).status, Status::Waiting);
    }

    #[test]
    fn a_synced_file_is_checked_like_any_other() {
        let (phone, phone_store) = scratch("sync-bad");
        let bad = GOOD.replace("research eurusd news", "unlock hunter2");
        assert!(take_synced(&phone_store, &phone, &commands(), "addon:morning-markets", "file", &bad, true, "laptop").is_none());
        assert!(!phone.join("morning-markets").exists());
        assert!(take_synced(&phone_store, &phone, &commands(), "addon:../x", "file", GOOD, true, "laptop").is_none());
    }

}
