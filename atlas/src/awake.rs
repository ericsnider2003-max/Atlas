//! Staying up long enough to finish, and no longer.
//!
//! The problem is real and has a bad default answer. You go to bed, the laptop
//! sleeps, and whatever Atlas was doing stops halfway — so the obvious fix is
//! to stop the machine sleeping. That's wrong: a laptop that never sleeps is
//! a laptop with a flat battery and a hot lid in a bag.
//!
//! Windows has the right mechanism for this and almost nothing uses it
//! properly. You can tell the system "don't sleep while I'm doing this", scoped
//! to a piece of work rather than to the program, and release it the moment
//! you're done. Screens still turn off. The machine still sleeps the instant
//! the work ends.
//!
//! So: **Atlas keeps the machine awake only while something is actually
//! running, and only for work worth it.**

use serde::{Deserialize, Serialize};

/// Why the machine is being kept up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Because {
    /// A job you asked for.
    YouAskedForIt,
    /// Overnight work you approved.
    OvernightWork,
    /// Finishing something that would be worse half-done.
    MidwayThrough,
    /// Nothing. Let it sleep.
    Nothing,
}

impl Because {
    /// Worth holding a laptop awake for?
    ///
    /// Indexing is not. Rendering something you asked for is.
    pub fn worth_it(&self) -> bool {
        !matches!(self, Because::Nothing)
    }
}

/// What Atlas asks the system for.
///
/// Named after what it maps to so the mapping is checkable: on Windows this
/// is `SetThreadExecutionState` with `ES_SYSTEM_REQUIRED | ES_CONTINUOUS`,
/// and releasing it is the same call with `ES_CONTINUOUS` alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Hold {
    /// Keep the system up. Screen may still turn off.
    SystemOnly,
    /// Let go.
    Release,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AwakeConfig {
    pub enabled: bool,
    /// Longest Atlas will hold the machine up for one thing, in minutes.
    ///
    /// A cap rather than a promise: something that thinks it needs four hours
    /// is usually stuck.
    pub longest_hold_mins: u32,
    /// Don't hold it up below this battery percentage, whatever's running.
    pub give_up_below_battery: u32,
}

impl Default for AwakeConfig {
    fn default() -> Self {
        AwakeConfig {
            enabled: true,
            longest_hold_mins: 90,
            // Below this you want the battery for tomorrow morning, not for
            // finishing a render.
            give_up_below_battery: 25,
        }
    }
}

/// What the machine is doing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Power {
    pub on_battery: bool,
    pub battery_pct: u32,
    /// The lid is shut.
    pub lid_closed: bool,
    /// What the machine does when the lid shuts, which is a setting you own.
    ///
    /// Treating a closed lid as "asleep" was wrong: plenty of people set it
    /// to do nothing and run with the laptop shut on a stand, and stopping
    /// their work every time they close it would be maddening.
    pub lid_action: LidAction,
    /// External displays are attached and awake.
    pub external_display: bool,
}

/// What closing the lid actually does on this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LidAction {
    Sleep,
    Hibernate,
    /// Screen off, nothing else.
    ScreenOff,
    /// Genuinely nothing.
    Nothing,
    /// Not read yet.
    Unknown,
}

impl LidAction {
    /// Does closing the lid actually stop the machine?
    pub fn stops_the_machine(&self) -> bool {
        matches!(self, LidAction::Sleep | LidAction::Hibernate)
    }
}

/// What the machine is actually doing, as opposed to what it looks like.
///
/// A locked session with the screens off looks identical to a sleeping laptop
/// from across the room, and they are completely different: one is still
/// working and one isn't.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Running {
    /// Awake, you're here.
    Awake,
    /// Awake and working, but the session is locked and the screens are off.
    /// Everything carries on.
    LockedButUp,
    /// Suspended. Nothing runs.
    Asleep,
}

impl Running {
    pub fn work_continues(&self) -> bool {
        !matches!(self, Running::Asleep)
    }
}

/// Work out which of the three it is.
///
/// The one that catches people: locked with the displays off is not sleep,
/// and if you treat it as sleep you stop working every night for no reason.
pub fn running_state(power: &Power, session_locked: bool, displays_off: bool) -> Running {
    if power.lid_closed && power.lid_action.stops_the_machine() {
        return Running::Asleep;
    }
    if session_locked || displays_off {
        return Running::LockedButUp;
    }
    Running::Awake
}

/// Should Atlas hold the machine awake right now?
pub fn decide(
    why: Because,
    power: &Power,
    held_for_mins: u32,
    cfg: &AwakeConfig,
) -> (Hold, String) {
    if !cfg.enabled || !why.worth_it() {
        return (Hold::Release, String::new());
    }
    // A closed lid only matters if closing it is meant to stop the machine.
    // On a laptop set to do nothing — shut, on a stand, driving monitors —
    // stopping every time you close it would be maddening.
    if power.lid_closed && power.lid_action.stops_the_machine() && !power.external_display {
        return (
            Hold::Release,
            "the lid's shut and this machine sleeps when it is, so I've stopped rather than              keeping it running in a bag"
                .into(),
        );
    }
    if power.on_battery && power.battery_pct < cfg.give_up_below_battery {
        return (
            Hold::Release,
            format!(
                "{}% battery — I've let it sleep. You'll want that in the morning more than I \
                 want to finish this",
                power.battery_pct
            ),
        );
    }
    if held_for_mins >= cfg.longest_hold_mins {
        return (
            Hold::Release,
            format!(
                "I've kept this awake {held_for_mins} minutes, which is longer than anything \
                 here should take. Something's stuck, so I've stopped holding it"
            ),
        );
    }
    (Hold::SystemOnly, String::new())
}

/// What happens to work that was interrupted.
///
/// The important half. Preventing sleep is a nice-to-have; **coming back
/// correctly after sleeping anyway is not optional**, because the machine will
/// sleep — you'll shut the lid, the battery will go, Windows will update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnWaking {
    /// Pick up where it stopped.
    Resume,
    /// Start again — it wasn't safe to resume partway.
    StartOver,
    /// Don't touch it. Tell them.
    AskFirst,
}

/// What Atlas found when it checked, rather than what it assumes.
///
/// The difference between asking you and working it out. Each of these is a
/// cheap check — does the file exist, did the message send, is the lock still
/// held — and together they answer the question without a conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Checked {
    /// The step it was on had already finished by the time it stopped.
    pub last_step_completed: bool,
    /// Whatever it had changed is still as it left it.
    pub world_unchanged: bool,
    /// The thing it was working on still exists.
    pub subject_still_there: bool,
    /// It had sent, posted or paid something.
    pub already_sent_something: bool,
    /// How long the check took. Kept because this has to be fast.
    pub took_ms: u32,
}

/// The longest Atlas will spend deciding.
///
/// You may want your brief the second you sit down, and a system that spends
/// twenty seconds working out what it was doing has answered the wrong
/// question.
pub const BUDGET_MS: u32 = 1500;

/// What to do with a job that was running when the machine stopped.
///
/// Decided from what's actually true rather than from rules of thumb, and
/// decided quickly. The only thing that reaches you is a job it genuinely
/// can't judge — and even then it says what it found rather than asking an
/// open question.
pub fn on_waking_checked(c: &Checked, was_reversible: bool, minutes_asleep: u64) -> OnWaking {
    // Something already left the machine. Repeating it is the one failure
    // worth stopping for, and no amount of checking makes a second email
    // un-sent.
    if c.already_sent_something && !c.last_step_completed {
        return OnWaking::AskFirst;
    }
    // The check itself was too slow to trust, so don't act on it.
    if c.took_ms > BUDGET_MS {
        return OnWaking::AskFirst;
    }
    // What it was working on has gone. Starting again would recreate it,
    // which may be exactly what you didn't want.
    if !c.subject_still_there {
        return OnWaking::AskFirst;
    }
    // Something else changed it while the machine was down.
    if !c.world_unchanged {
        return if was_reversible {
            OnWaking::StartOver
        } else {
            OnWaking::AskFirst
        };
    }
    // Everything is as it left it. Length of sleep stops mattering once the
    // world has been checked — that rule existed only because nothing was
    // being checked.
    let _ = minutes_asleep;
    if c.last_step_completed || was_reversible {
        OnWaking::Resume
    } else {
        OnWaking::StartOver
    }
}

/// What Atlas says when it comes back, having checked.
///
/// One line, and it leads with the decision rather than the reasoning —
/// you're sitting down to get on with something, not to read a report.
pub fn woke_checked(what: &str, decision: OnWaking, c: &Checked) -> String {
    match decision {
        OnWaking::Resume => format!("Picking {what} back up — nothing moved while it was off."),
        OnWaking::StartOver => {
            format!("Started {what} again; something had changed underneath it.")
        }
        OnWaking::AskFirst if c.already_sent_something => format!(
            "{what} stopped after it had already sent something. I've left it — carrying on \
             might send it twice."
        ),
        OnWaking::AskFirst if !c.subject_still_there => {
            format!("{what} stopped and what it was working on has gone. Left it alone.")
        }
        OnWaking::AskFirst => format!("{what} stopped and I couldn't tell where. Left it alone."),
    }
}

/// The honest summary of what this can and can't do.
pub const WHAT_THIS_DOES: &str =
    "I can stop the machine sleeping while something is actually running, and I let go the moment \
     it's done — screens still turn off, and it still sleeps when nothing's happening. What I \
     can't do is stop you shutting the lid, and I won't try: the important part isn't preventing \
     sleep, it's coming back correctly afterwards.";
