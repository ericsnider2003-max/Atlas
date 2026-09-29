//! Getting something to you when you are not at the desk.
//!
//! Three modules already sit next to this one and none of them do this job.
//! `interrupt` decides **whether** anything is worth saying. `presence` knows
//! whether you are there. `channel` splits disposable progress from a result
//! that has to stand alone. What was missing is the step between "this is
//! worth saying" and "you actually heard it": the **route**.
//!
//! Until now everything Atlas wanted to tell you either went to the speakers
//! or went to the log. Speakers reach an empty room; the log reaches nobody.
//! A disk filling up while you are away is exactly the case where the log is
//! the wrong answer and the speaker is too.
//!
//! ## The rules this enforces
//!
//! **Nothing is reported as delivered unless it was.** The failure this
//! codebase keeps producing is a component that returns success because
//! nothing objected. A notifier that is not installed must say so, not
//! quietly succeed. `Sent::Failed` exists so a caller cannot mistake one for
//! the other, and `Delivery::sent()` is deliberately not a bool.
//!
//! **Something that could not reach you is held, not dropped.** Held items
//! are delivered when you come back. An alert that evaporates because you
//! were out is worse than no alerting, because you will believe you were told.
//!
//! **Private things wait for a private moment.** `presence` already knows when
//! somebody else is at your desk. A notification is visible on a screen other
//! people can see, so discretion has to be checked here rather than assumed
//! upstream.
//!
//! ## Why an external command rather than a library
//!
//! Atlas runs on Windows, macOS and Linux, and each has its own notification
//! mechanism with no common Rust crate worth the dependency. The established
//! pattern in this codebase for "the OS can already do this" is an
//! `ExternalTool` configured per platform in `tools.yaml` — the same way
//! research reaches curl and the browser. Nothing new to install, and the
//! command is visible and editable rather than compiled in.

use crate::tools::{ExternalTool, Vars};
use serde::{Deserialize, Serialize};

/// How much this matters, which decides whether it may arrive at a bad moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Urgency {
    /// Can wait for a good moment.
    Routine,
    /// Should reach you now — the machine is about to stop working, something
    /// is failing, a deadline is passing.
    Urgent,
}

/// How this one should reach you.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// You are here and it is fine to talk.
    Speak,
    /// You are not here, or talking is not appropriate. Put it on the screen.
    Notify,
    /// You are not at the machine at all. Send it to your phone.
    Phone,
    /// Nothing works right now. Keep it and say it when you come back.
    Hold,
}

/// Whether it actually went.
///
/// Deliberately not a bool. "It didn't work" and "there was nothing to do"
/// are different outcomes, and collapsing them is how a component starts
/// reporting success for doing nothing.
#[derive(Debug, Clone, PartialEq)]
pub enum Sent {
    Spoken,
    Notified,
    Held,
    /// Tried and failed, with the reason kept so it can be fixed.
    Failed(String),
}

impl Sent {
    /// Did this actually reach the person?
    pub fn reached_you(&self) -> bool {
        matches!(self, Sent::Spoken | Sent::Notified)
    }
}

/// One thing waiting to be said.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub title: String,
    pub body: String,
    pub urgency: Urgency,
    /// True when this should not appear on a screen other people can see.
    #[serde(default)]
    pub private: bool,
    pub at: u64,
}

impl Note {
    pub fn new(title: &str, body: &str, urgency: Urgency, at: u64) -> Note {
        Note {
            title: title.into(),
            body: body.into(),
            urgency,
            private: false,
            at,
        }
    }

    pub fn private(mut self) -> Note {
        self.private = true;
        self
    }

    /// What actually appears on screen.
    ///
    /// The answer to a problem Atlas cannot sense its way out of: it has no
    /// idea who else can see your monitor, and trying to guess produces the
    /// coffee-shop failure — everyone is always nearby, so a "hold it when
    /// people are around" rule holds everything forever.
    ///
    /// So a private note knocks rather than discloses. You are told there is
    /// something and what kind of thing it is; the content waits until you
    /// ask. That is correct at your desk, on a train, and in a coffee shop
    /// alike, without knowing anything about the room.
    pub fn shown(&self) -> (String, String) {
        if self.private {
            (self.title.clone(), "Ask me when you're ready.".into())
        } else {
            (self.title.clone(), self.body.clone())
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct NotifyConfig {
    pub enabled: bool,
    /// The OS command that puts something on screen. `{title}` and `{body}`
    /// are substituted. Absent means Atlas cannot notify on this machine, and
    /// it will say so rather than pretending.
    pub tool: Option<ExternalTool>,
    /// Drop held items older than this rather than reading out yesterday's
    /// news when you sit down. Urgent ones are kept regardless.
    pub stale_after_secs: u64,
    /// Never hold more than this. A queue that grows without limit becomes a
    /// wall of text nobody reads, which is the same as losing it.
    pub max_held: usize,
}

impl Default for NotifyConfig {
    fn default() -> Self {
        NotifyConfig {
            enabled: true,
            tool: None,
            // Six hours. Long enough to cover a working day away, short
            // enough that a routine note does not greet you the next morning.
            stale_after_secs: 6 * 3600,
            max_held: 20,
        }
    }
}

/// Where a note should go, given what Atlas can see right now.
///
/// `speakers_work` is separate from presence on purpose: being at the desk
/// with the volume off, or with no audio device, is common and is not the
/// same as being away.
pub fn route(
    presence: crate::presence::Presence,
    speakers_work: bool,
    can_notify: bool,
    can_phone: bool,
    long_gone: bool,
) -> Route {
    // This used to hold a private note whenever `presence` reported somebody
    // else in the room. That was wrong twice over, and both ways matter:
    //
    // 1. **It could never fire.** `presence::Sensor` is not wired to anything.
    //    Nothing produces a `Look`, so `Stranger` and `NotAlone` never occur
    //    and the branch was dead. Privacy handling built on a signal that does
    //    not exist is worse than none, because it reads as handled.
    //
    // 2. **It is the wrong question.** Even with a working camera, "is anyone
    //    nearby" does not answer "can anyone read my screen". In a coffee shop
    //    the answer is always yes, so the rule would hold everything forever
    //    and Atlas would go silent exactly where you most need it. And Atlas
    //    cannot tell you from anyone else — there is no face recognition here,
    //    and a camera cannot distinguish you from a photograph anyway.
    //
    // So discretion is no longer a routing decision. A private note is always
    // delivered; what changes is how much of it appears. See `Note::shown`.
    // The note itself is deliberately not a parameter here any more -- routing
    // does not depend on what is in it, and a parameter nothing reads is a
    // standing invitation to believe it is being considered.
    if presence.worth_speaking() && speakers_work {
        return Route::Speak;
    }
    // The phone comes before the screen only when you have been gone long
    // enough that you are probably not coming straight back. A push for
    // something you would have seen on your own monitor in two minutes is how
    // people learn to silence an app.
    if long_gone && can_phone {
        return Route::Phone;
    }
    if can_notify {
        return Route::Notify;
    }
    // A screen you are not in front of is not delivery. If Atlas is drawing a
    // window into an empty room, the phone is the better answer even for
    // something recent.
    if can_phone {
        return Route::Phone;
    }
    Route::Hold
}

/// Everything waiting, and what has been tried.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Outbox {
    pub held: Vec<Note>,
}

/// The record name, and it is not `"outbox"` — see [`Outbox::load`].
const RECORD: &str = "held_notes";

impl Outbox {
    /// Read what is still waiting from disk.
    ///
    /// Held notes have to survive a restart. Without this the outbox was
    /// rebuilt empty on every start, so anything held while you were away was
    /// silently gone the moment Atlas restarted — and both `NO_NOTIFIER` and
    /// `doctor` were telling you "nothing is lost". Claiming durability
    /// without storing anything is exactly the shape of bug this module was
    /// written in response to.
    ///
    /// ## And it was that bug again, by a second route, until 17 Sep
    ///
    /// This record was called `"outbox"`. So is [`crate::outbox::Outbox`],
    /// which is a different type holding `replies: Vec<PendingReply>` — the
    /// client and brand replies Atlas has drafted and not sent. **Both wrote
    /// `data/state/outbox.json`.**
    ///
    /// The sequence needed no unusual conditions. Atlas drafts a reply and
    /// saves `{"replies":[…]}`; on the next tick `Daemon::persist` saves
    /// this type over it as `{"held":[…]}`. Neither struct's field is
    /// `#[serde(default)]`, so the other side's next `load` fails both the
    /// envelope parse and the bare fallback, `Store::preserve` renames the
    /// file aside as `outbox.corrupt.<ts>.json.bak`, and the caller is handed
    /// `Default::default()`. Every unsent draft destroyed within one tick;
    /// every held notification destroyed the first time a draft was saved.
    ///
    /// Both modules carried a comment boasting that exactly this loss had
    /// been fixed. It had been fixed once, for the case they were each
    /// looking at.
    ///
    /// **Nothing recovers the data already lost.** Whatever is in an existing
    /// `outbox.json` is whichever type wrote last, so the other list is gone
    /// and was gone before this was found. Renaming this record leaves
    /// `outbox.json` to the module actually called `outbox`, which is the
    /// one whose name means what the file says.
    ///
    /// The structural half is `tests/one_name_one_record.rs`: the store's key
    /// space is a namespace like any other, and nothing was guarding it.
    pub fn load(store: &crate::store::Store) -> Outbox {
        store.load(RECORD)
    }

    pub fn save(&self, store: &crate::store::Store) -> crate::error::Result<()> {
        store.save(RECORD, self)
    }

    /// Keep it for later.
    ///
    /// Oldest routine notes are dropped first when full. An urgent note is
    /// never dropped to make room for a routine one — if that ever has to
    /// happen, the routine one goes.
    pub fn hold(&mut self, note: Note, cfg: &NotifyConfig) {
        self.held.push(note);
        while self.held.len() > cfg.max_held {
            let victim = self
                .held
                .iter()
                .enumerate()
                .filter(|(_, n)| n.urgency == Urgency::Routine)
                .min_by_key(|(_, n)| n.at)
                .map(|(i, _)| i)
                // Everything held is urgent. Drop the oldest, and it is worth
                // knowing that this is the case where something urgent is
                // being lost.
                .or_else(|| {
                    self.held
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, n)| n.at)
                        .map(|(i, _)| i)
                });
            match victim {
                Some(i) => {
                    self.held.remove(i);
                }
                None => break,
            }
        }
    }

    /// What is worth saying now that you are back.
    ///
    /// Stale routine notes are dropped; urgent ones are kept however old,
    /// because "your disk filled up four hours ago" is still true.
    /// What would be handed over, without removing it.
    ///
    /// Separate from `collect` because of a window that would otherwise lose
    /// things: the summary is built at the start of a turn, but a turn can end
    /// early — while paused, or when `addressing` judges the words were not
    /// meant for Atlas. Draining at that point and only then failing to say
    /// anything means the notes are gone. Nothing is removed until it has
    /// actually been put in front of you.
    pub fn ready(&self, now: u64, cfg: &NotifyConfig) -> Vec<Note> {
        self.held
            .iter()
            .filter(|n| {
                !(n.urgency == Urgency::Routine && now.saturating_sub(n.at) > cfg.stale_after_secs)
            })
            .cloned()
            .collect()
    }

    pub fn collect(&mut self, now: u64, cfg: &NotifyConfig) -> Vec<Note> {
        let stale = |n: &Note| {
            n.urgency == Urgency::Routine && now.saturating_sub(n.at) > cfg.stale_after_secs
        };
        let worth_saying: Vec<Note> = self.held.drain(..).filter(|n| !stale(n)).collect();
        worth_saying
    }

    pub fn waiting(&self) -> usize {
        self.held.len()
    }
}

/// One line for a person, summarising what was missed.
///
/// Leads with the count and then the urgent ones by name. A list of twenty
/// titles is not something anyone reads on returning to their desk.
pub fn spoken(notes: &[Note]) -> String {
    if notes.is_empty() {
        return String::new();
    }
    let urgent: Vec<&Note> = notes.iter().filter(|n| n.urgency == Urgency::Urgent).collect();
    if urgent.is_empty() {
        return format!(
            "{} thing{} happened while you were away, nothing urgent.",
            notes.len(),
            if notes.len() == 1 { "" } else { "s" }
        );
    }
    let named: Vec<String> = urgent.iter().take(2).map(|n| n.title.clone()).collect();
    format!(
        "While you were away: {}{}.",
        named.join(", "),
        if notes.len() > named.len() {
            format!(" and {} other{}", notes.len() - named.len(), if notes.len() - named.len() == 1 { "" } else { "s" })
        } else {
            String::new()
        }
    )
}

/// Put something on the screen through the operating system.
///
/// Returns the tool's own failure verbatim. A notifier that is missing,
/// blocked by policy, or simply not configured is a real thing to fix, and
/// summarising it as "couldn't notify" throws away the one detail that would
/// tell you which.
pub fn show(note: &Note, cfg: &NotifyConfig, vars: &Vars) -> Result<(), String> {
    if !cfg.enabled {
        return Err("notifications are switched off in your settings".into());
    }
    let Some(tool) = cfg.tool.as_ref() else {
        // No external notifier configured, which is the shipped default now
        // that PowerShell is gone. Atlas draws its own panel instead.
        //
        // A private note knocks: the title, and nothing else, because Atlas
        // has no idea who else can see the screen and the safe version is the
        // one that does not need to know.
        // A Windows notification first: it waits in the Action Center if you
        // weren't looking. A private note shows its title only, as below.
        if cfg!(windows) {
            let body = if note.private { "Ask me when you're ready.".to_string() } else { note.body.clone() };
            if crate::toast::show(&note.title, &body).is_ok() {
                return Ok(());
            }
        }
        if !crate::window::can_open() {
            return Err("there's no display here to put a window on".into());
        }
        let panel = if note.private {
            crate::window::Contents::knock(&note.title)
        } else {
            crate::window::Contents::new(
                crate::window::Panel::Urgent,
                &note.title,
                vec![note.body.clone()],
            )
            .because("Shown rather than said.")
        };
        return crate::window::open(&panel);
    };
    let (title, body) = note.shown();
    let mut v = vars.clone();
    v.insert("title".into(), title);
    v.insert("body".into(), body);
    tool.run(&v, None).map(|_| ()).map_err(|e| e.to_string())
}

/// Can this machine notify at all?
///
/// Checked rather than assumed, and reported honestly by `doctor`. A machine
/// with no notifier still works — everything falls back to being held and
/// spoken when you return — but you should know that is what is happening.
pub fn can_notify(cfg: &NotifyConfig) -> bool {
    // `cfg.tool.is_some()` was the whole test, and it stopped being the right
    // one when `show` learned to draw Atlas's own window: with `tool` unset
    // it no longer needs an external notifier at all. `doctor` was updated to
    // match ("on-screen alerts in Atlas's own window"); this was not.
    //
    // The shipped config leaves `tool` unset **deliberately** -- tools.yaml
    // says so in as many words -- so on a default install this returned
    // false, `route` never returned `Route::Notify`, and every on-screen
    // alert was silently pushed to the phone or held instead. `doctor`
    // meanwhile reported notifications healthy. Two declarations of one fact,
    // disagreeing, with the wrong one governing.
    cfg.enabled && (cfg.tool.is_some() || crate::window::can_open())
}

/// Said plainly when there is no way to reach you off the desk.
/// What Atlas does when no on-screen notifier is configured.
///
/// Deliberately not PowerShell. A toast raised through PowerShell flashes a
/// console window and puts a shell in the path of every alert, which is a
/// poor trade for a popup.
pub const NO_NOTIFIER: &str =
    "I can't put anything on your screen on this machine — no notification command is set up. \
     Nothing is lost: anything that happens while you're away is held, and I'll tell you the \
     moment you're back. But it won't reach you before then.";

// ---------------------------------------------------------------------------
// Speaking out loud, or not.
//
// The rule set here is Eric's, and it solves the problem the presence check
// could not: Atlas cannot tell who else is in the room, and it never will
// reliably — there is no face recognition, and "somebody is nearby" is always
// true in a coffee shop. So the question is changed from *who is around* to
// *can this be heard by anyone but you*, which Atlas can actually answer.
//
// Headphones are the whole trick. If they are connected, speaking is private
// wherever you are, so a busy room stops mattering. If they are not, Atlas
// falls back to showing rather than saying.
// ---------------------------------------------------------------------------

/// How to deliver something audibly, if at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Say {
    /// Out loud, through the named device. You are somewhere it is fine to be
    /// overheard.
    Aloud(String),
    /// Through headphones. Private regardless of where you are.
    InYourEar(String),
    /// Not aloud at all — show it instead, and knock rather than disclose.
    Silent(Quiet),
}

/// Why Atlas is not speaking. Kept because the reasons want different
/// handling, and because "it went quiet" with no explanation is the thing
/// that makes an assistant feel broken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quiet {
    /// You are on a call. Talking over it is the one unforgivable interruption.
    OnACall,
    /// You said you are somewhere public and there are no headphones on.
    InPublic,
    /// Nothing can play sound at all.
    NoOutput,
}

impl Quiet {
    pub fn plain(&self) -> &'static str {
        match self {
            Quiet::OnACall => "you're on a call",
            Quiet::InPublic => "you're out and not wearing headphones",
            Quiet::NoOutput => "there's nothing here that can play sound",
        }
    }
}

/// Decide how to say something, from what Atlas can actually observe.
///
/// Every input is a real signal, not an inference about the room:
///
/// * `outputs` — the audio devices actually enumerated right now, which is
///   how Atlas knows headphones are connected.
/// * `on_a_call` — whether a call application currently has the foreground.
///   Observed, not guessed.
/// * `in_public` — a mode you set. Deliberately explicit: there is no honest
///   way to sense this, and a wrong guess either broadcasts your business to
///   a café or silences Atlas at your own desk.
///
/// The ordering matters. Headphones beat `in_public`, because that is exactly
/// the case they solve. A call beats everything, because talking over one is
/// worse than being late.
pub fn how_to_say(
    outputs: Option<&[crate::audio::Device]>,
    on_a_call: bool,
    in_public: bool,
) -> Say {
    if on_a_call {
        return Say::Silent(Quiet::OnACall);
    }
    // `None` means Atlas has never probed the audio devices, which is not the
    // same as finding none. Treating "I haven't looked" as "there is nothing
    // here that can play sound" would silence Atlas on every machine that has
    // not run voice mode yet -- an absence of a finding read as a finding,
    // which is the mistake this whole codebase keeps making.
    let Some(outputs) = outputs else {
        return Say::Aloud("the system default".into());
    };
    let headphones = outputs
        .iter()
        .find(|d| d.kind == crate::audio::Kind::Output && (d.bluetooth || !d.builtin));
    if let Some(h) = headphones {
        return Say::InYourEar(h.name.clone());
    }
    if in_public {
        return Say::Silent(Quiet::InPublic);
    }
    match outputs.iter().find(|d| d.kind == crate::audio::Kind::Output) {
        Some(d) => Say::Aloud(d.name.clone()),
        None => Say::Silent(Quiet::NoOutput),
    }
}

/// Applications whose foreground presence means you are on a call.
///
/// Named rather than guessed. A browser tab in a meeting is the gap here and
/// is not solved by this list — window titles are the only way to catch it,
/// and they are checked too.
pub const CALL_APPS: &[&str] = &[
    "zoom", "teams", "discord", "slack", "meet", "webex", "facetime", "skype", "whereby",
];

/// Is a call app in the foreground right now?
pub fn on_a_call(active: Option<&crate::platform::ActiveWindow>) -> bool {
    let Some(w) = active else { return false };
    let hay = format!("{} {}", w.process.to_lowercase(), w.title.to_lowercase());
    // A meeting held in a browser tab shows up in the title rather than the
    // process name, so both are read.
    CALL_APPS.iter().any(|a| hay.contains(a))
        || hay.contains("meeting")
        || hay.contains(" call")
}
