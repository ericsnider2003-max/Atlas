//! The always-on core.
//!
//! This is what turns Atlas from a command you type into something that runs
//! all day: wake word, spoken turn, spoken reply, follow-up without needing
//! the wake word again, plus a background tick that runs scheduled work and
//! decides whether to speak first.
//!
//! Voice in, voice out. Typing is a debugging affordance, not the interface.

use crate::awareness::{Awareness, Signals};
use crate::brain::{self, Brain, Llm};
use crate::config::Config;
use crate::error::Result;
use crate::index::Index;
use crate::intent::{Intent, Parser};
use crate::memory::Memory;
use crate::platform::Platform;
use crate::policy::Decision;
use crate::proactive::{Offer, Proactive};
use crate::scheduler::Scheduler;
use crate::session::{is_no, is_yes, kind_of, Pending, Session};
use crate::addressing::{assess, respond, Response as Addressed, Situation};
use crate::attention::{hear, Attention, Heard};
use crate::backlog::{Backlog, Blocker, Conditions};
use crate::connectivity::{need_of, Connectivity, Reach};
use crate::delivery;
use crate::input::{Keyboard, Tier, Tiers};
use crate::activity::{Journal, Kind as Act};
use crate::anticipate::{Anticipator, Moment};
use crate::flow::Library;
use crate::health::{assess as assess_machine, summary as health_summary, HealthConfig, Readings, Reporter};
use crate::lanes::{lane_for, LaneConfig, Queue};
use crate::modes::Modes;
use crate::persona::Persona;
use crate::safety::{back_up, due_for_backup, prune_backups, BackupConfig, Trash};
use crate::thread::{Thread, ThreadConfig};
use crate::watch::Watcher;
use crate::crew::{self, Crew};
use crate::watching;
use crate::publish::{Channel, Publisher};
use crate::references::{resolve, Resolution};
use crate::grants::Permissions;
use crate::references::Referents;
use crate::log::Log;
use crate::perf::{Power, Throttle};
use crate::probe::Probe;
use crate::store::Store;
use crate::workspace;

// --- the `impl Daemon` split by what it does (29 Sep 2026, docs/refactor-plan-daemon-split.md).
// Each child is `impl<'a> Daemon<'a>` and nothing else; the struct, `new`
// and the free functions stay here.
mod late;
mod conversation;
mod running;
mod on_itself;
mod model;
mod errands;
mod helping;
mod messages;
mod hands;
mod away;
mod tick;
mod inbox;
mod making;
mod reading;
mod execute;

/// What Atlas is allowed to do on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Autonomy {
    /// You are here. Gated actions can ask and get an answer.
    Supervised,
    /// You are not here. Nothing that needs consent may run — it parks until
    /// you are back. This is the whole safety story for unattended operation:
    /// autonomy does not mean permission, it means the safe subset runs
    /// without you and the rest waits.
    Unattended,
}

pub trait Ears {
    fn wait_for_wake(&self) -> Result<()>;
    /// One bounded attempt at hearing the wake word. Returning quickly is what
    /// lets the daemon stay responsive to typed input and scheduled work
    /// instead of blocking forever on the microphone.
    fn wake_once(&self) -> Result<bool> {
        self.wait_for_wake().map(|_| true)
    }
    fn listen(&self) -> Result<String>;
    /// Listen for as long as `held` says the push-to-talk key is down (H1).
    /// Ok(None) is a key let go before anything was said.
    fn listen_while(&self, held: &dyn Fn() -> bool) -> Result<Option<String>> {
        let _ = held;
        self.listen().map(Some)
    }
    /// Listen for a follow-up with no wake word. Ok(None) means silence.
    fn listen_briefly(&self, secs: u32) -> Result<Option<String>>;
    /// How the last `listen()` divided between waiting for you to finish
    /// (which is not Atlas being slow) and turning it into words (which is).
    ///
    /// `None` means this implementation cannot tell them apart — reported as
    /// absent rather than guessed, because a guessed split is worse than a
    /// missing one when the whole point is deciding what to speed up.
    fn last_listen_split_ms(&self) -> Option<(u32, u32)> {
        None
    }

    /// A voice embedding for whatever `listen()` last recorded, if this
    /// machine can produce one.
    ///
    /// Defaulting to `None` is the point: an implementation that cannot tell
    /// voices apart says so, and `voiceid` treats that as `NotEnrolled` and
    /// proceeds. Voice-lock unavailable must never be mistaken for voice-lock
    /// on, and defaulting the other way would let a missing encoder silently
    /// look like a passing check.
    fn voiceprint(&self) -> Option<Vec<f32>> {
        None
    }

    /// The microphone's work, for its own thread (`micthread`): the wake
    /// word and cutting in by voice run there, so the loop -- and the hub it
    /// answers -- never waits on a recording. `None` (test stand-ins that
    /// can't be sent to a thread) keeps the wake word on the loop, as before.
    fn mic_work(&self) -> Option<Box<dyn crate::micthread::MicWork>> {
        None
    }
}

pub trait Mouth {
    fn speak(&self, text: &str) -> Result<()>;
    /// A whole reply, before `speak` is given its sentences one by one: a
    /// voice that can make the next sentence while this one plays (Kokoro,
    /// `kokoro::Ahead`) starts on it. Nothing by default.
    fn prepare(&self, _reply: &str) {}
    /// More of a reply already `prepare`d (the model still writing it): made
    /// after what is already queued, not instead of it.
    fn prepare_more(&self, _more: &str) {}
    /// Milliseconds synthesising, then milliseconds playing. `None` when the
    /// implementation cannot separate them.
    fn last_speak_split_ms(&self) -> Option<(u32, u32)> {
        None
    }
    /// This voice, owned, for the thread that plays a reply
    /// (`speakthread`), so the loop -- and the hub it answers -- never waits
    /// on a sentence being said. `None` (test stand-ins that can't be sent
    /// to a thread) says each sentence on the loop, as before.
    fn speak_work(&self) -> Option<Box<dyn crate::speakthread::SpeakWork>> {
        None
    }
}

/// How something reached Atlas.
///
/// `Directed` covers the wake word, push-to-talk, and anything typed into
/// Atlas's own prompt. `OpenMic` is ambient audio nobody aimed at Atlas, and
/// is the only case the addressing check should second-guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrival {
    Directed,
    OpenMic,
}


/// Whether an errand's ending is something to say, or a chore that only
/// earns a mention when `long_work`'s own "was this worth interrupting for"
/// rule says so.
#[derive(Clone, Copy, PartialEq)]
enum SpeakPolicy {
    /// A quiet, fast, successful chore stays quiet — the backup and the
    /// housekeeping pass. `long_work.to_report` decides, same as any other
    /// watched job.
    ViaWatcher,
    /// This is the answer to something you asked for, not a chore running
    /// behind your back. Reported every time it settles — success,
    /// failure, fast or slow — because staying quiet about it would mean
    /// the thing you asked for just never came back.
    Always,
}

/// What each kind of crew errand costs, and how soon it's wanted — decided
/// in one place so a new crew citizen can't forget to say.
///
/// Something you asked for (`SpeakPolicy::Always`) is `Now`; a chore Atlas
/// runs on its own is `Later`. Work whose result depends only on its name and
/// topic is keyed, so asking twice runs it once.
/// tools.yaml with its paths resolved against this install. Run once, at
/// start; `Daemon::tools_cfg` hands out the result.
fn resolve_tools(tools: Option<&crate::voice::ToolsConfig>, store: &Store) -> crate::voice::ToolsConfig {
    let mut t = tools.cloned().unwrap_or_default();
    // `work_dir` used to ship as a bare relative default ("data/tmp") —
    // the same shape of bug `backup.dir` and `research.notes_dir` had.
    // Scratch space isn't in `upgrade::YOURS` (it's cleared per turn,
    // nothing to preserve), but two installs — or two tests — sharing
    // a working directory would still overwrite each other's frames
    // and clips mid-run, which is the same isolation failure either
    // way. The default is now empty, meaning "this install's scratch",
    // so an unresolved value cannot masquerade as a usable path.
    if t.work_dir.trim().is_empty() {
        t.work_dir = store.data_dir().join("tmp").to_string_lossy().into_owned();
    } else {
        let d = std::path::PathBuf::from(&t.work_dir);
        if d.is_relative() {
            t.work_dir = store.install_root().join(d).to_string_lossy().into_owned();
        }
    }
    t
}

/// How often a tick with nothing to say still saves.
pub const PERSIST_SWEEP_SECS: u64 = 60;

/// The tick's nap while the crew has something it could start.
const CREW_NAP_MS: u64 = 100;

/// A hub request slower than this, from arriving to answered, gets a log line
/// of its own (`Daemon::hub_timing`).
const SLOW_HUB_MS: u64 = 150;

/// A pass of `tick` slower than this is written to the log.
const SLOW_TICK_MS: u64 = 200;

/// Hub requests counted between the once-a-minute summary lines.
#[derive(Debug, Clone, Default)]
struct HubTimes {
    since: u64,
    count: u64,
    total_ms: u64,
    slowest_ms: u64,
}

fn crew_job(name: &str, topic: Option<&str>, speak: &SpeakPolicy) -> crew::Job {
    use crew::Needs::*;
    let needs = match name {
        // A compiler, the whole council's model calls, the self-improvement
        // loop: each saturates every core on its own.
        "build" | "improve" | "council" => TheWholeMachine,
        // Disk and network: copying files, talking to a mail server.
        "backup" | "mail" | "unsubscribe" | "outlook-connect" | "outreach" | "reclaim" | "mcp" | "draft-model" => MostlyWaiting,
        // Hub buttons that wait on the network (`hubjobs`), and the knock on
        // a friend's Atlas that `friend_upkeep` used to make inside the tick.
        "hub-send" | "addon-share" | "friend-knock" | "phone-code" => MostlyWaiting,
        // "reclaim" is the Status page's disk survey: a walk of the disk.
        _ => OneCore,
    };
    let urgency = if *speak == SpeakPolicy::Always { crew::Urgency::Now } else { crew::Urgency::Later };
    let job = crew::Job::new(name).needs(needs).urgency(urgency);
    match name {
        // Each of these sends or changes something per ask; two asks are two
        // pieces of work even when they look alike.
        "outreach" | "unsubscribe" | "hub-send" | "addon-share" | "friend-knock" | "phone-code" | "mcp" => job,
        _ => job.keyed(match topic {
            Some(t) => format!("{name}:{}", t.trim().to_lowercase()),
            None => name.to_string(),
        }),
    }
}

/// What the daemon does with a hub errand's answer once the crew has it
/// (`hubjobs`, `Daemon::hub_errand`), by the crew's id for the errand. Kept
/// here rather than carried through the crew, because some of it (a friend
/// kept to try again, a code's server) isn't text.
pub(crate) enum HubAfter {
    /// A document sent: logged on the item when it went.
    Sent { job: u64, doc: u64, to: String },
    /// A new friend knocked on from the hub.
    FriendAdd { job: u64, keep: crate::friends::Pending },
    /// A friend who wasn't reachable, knocked on again by `friend_upkeep`.
    FriendRetry { keep: crate::friends::Pending },
    /// An add-on shared: the line in the group's chat.
    AddonShare { job: u64, name: String, group: Option<(String, String)> },
    /// A phone's code: its server, once it's up.
    PhoneCode { job: u64, slot: std::sync::Arc<std::sync::Mutex<Option<crate::phoneadd::Showing>>>, stop: std::sync::Arc<std::sync::atomic::AtomicBool> },
}

/// A new friend, ready to knock on (`Daemon::prepare_friend`).
pub(crate) struct FriendKnock {
    pub identity: crate::peerkey::Identity,
    pub keep: crate::friends::Pending,
    pub socks: Option<u16>,
}

/// What a knock on a new friend's Atlas came to, in words.
pub(crate) fn knock_said(keep: &crate::friends::Pending, outcome: &crate::friends::Knock) -> String {
    use crate::friends::Knock;
    match outcome {
        Knock::Taken => format!("You and {} are friends now -- you can message each other.", keep.name),
        Knock::Refused => format!(
            "{}'s Atlas turned that link down -- it's been used already or it's run out. Ask them for a new one.",
            keep.link.name
        ),
        Knock::Unreachable(_) => format!(
            "Added {}. I can't reach their Atlas right now, so I'll keep trying for the next week -- \
             it finishes by itself once it's online.",
            keep.name
        ),
    }
}

/// A knock's outcome as the crew carries it back.
pub(crate) fn knock_tag(k: &crate::friends::Knock) -> &'static str {
    match k {
        crate::friends::Knock::Taken => "taken",
        crate::friends::Knock::Refused => "refused",
        crate::friends::Knock::Unreachable(_) => "unreachable",
    }
}

fn knock_of(tag: &str) -> crate::friends::Knock {
    match tag {
        "taken" => crate::friends::Knock::Taken,
        "refused" => crate::friends::Knock::Refused,
        _ => crate::friends::Knock::Unreachable(String::new()),
    }
}

/// An add-on ready to share (`Daemon::prepare_addon_share`).
pub(crate) struct AddonShare {
    pub name: String,
    file_name: String,
    bytes: Vec<u8>,
    covering: String,
    people: Vec<String>,
    /// The group it's shared in: (id, name).
    pub group: Option<(String, String)>,
    pairings: crate::kin::Pairings,
    link: crate::kin::PeerLink,
}

impl AddonShare {
    /// Hand it to each person. Waits on the network; no daemon state is
    /// touched. Returns who it reached and who it didn't.
    pub(crate) fn send(&self) -> std::result::Result<(Vec<String>, Vec<String>), String> {
        let mut reached = Vec::new();
        let mut missed = Vec::new();
        // A folder of its own, so two shares at once can't share a file.
        let dir = std::env::temp_dir().join(format!(
            "atlas-share-{}-{}",
            std::process::id(),
            crate::server::new_token().map(|t| t[..12.min(t.len())].to_string()).unwrap_or_default()
        ));
        let named = dir.join(&self.file_name);
        if let Err(e) = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&named, &self.bytes)) {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(format!("I couldn't get it ready to send: {e}"));
        }
        for who in &self.people {
            let Some(contact) = self.pairings.contacts.iter().find(|c| crate::kin::same_name(&c.name, who)) else {
                missed.push(who.clone());
                continue;
            };
            let h = crate::household::share_with_friend(&self.covering, "me");
            // Sent under the add-on name, so the far side knows what it is.
            match self.link.hand_note(&contact.name, &h, Some(&named)) {
                Ok(()) => reached.push(who.clone()),
                Err(_) => missed.push(who.clone()),
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
        Ok((reached, missed))
    }
}

/// What sharing an add-on came to, in words.
pub(crate) fn addon_share_said(name: &str, group: Option<&(String, String)>, reached: &[String], missed: &[String]) -> String {
    let mut said = match (group, reached.is_empty()) {
        (_, true) => format!("I couldn't reach anyone to share \"{name}\" with yet."),
        (Some((_, g)), false) => format!("Shared \"{name}\" in {g} with {}.", reached.join(", ")),
        (None, false) => format!("Sent \"{name}\" to {}.", reached.join(", ")),
    };
    if !missed.is_empty() && !reached.is_empty() {
        said.push_str(&format!(" Couldn't reach {} just now.", missed.join(", ")));
    }
    said
}

/// Free memory and power, for the crew's admission rules.
fn crew_room() -> crew::Room {
    let r = crate::health::read_machine();
    let free_mb = (r.ram_total_gb > 0.0).then(|| ((r.ram_total_gb - r.ram_used_gb).max(0.0) * 1024.0) as u64);
    crew::Room { free_mb, on_battery: r.on_battery, battery_percent: r.battery_percent }
}

/// One seat's model call, carried out of the crew errand so
/// `take_crew_news` can record all five the same way the synchronous
/// version always did — `tests/a_room_that_disagrees.rs`'s
/// `every_seat_is_one_recorded_model_call` holds that count.
#[derive(serde::Serialize, serde::Deserialize)]
struct SeatCall {
    took_ms: u64,
    prompt_chars: usize,
    reply_chars: usize,
    failed: Option<String>,
    /// How the answer turned out, when it can be told without a model:
    /// good, or bad and why. Written to the grades beside the call log.
    #[serde(default)]
    grade: Option<(bool, String)>,
    /// The words, carried back in memory so they can be kept if the call is
    /// graded (`trace::keep_example`). Never written on their own.
    #[serde(default)]
    words: Option<crate::trace::Words>,
}

/// What a council errand found, in enough detail for `take_crew_news` to
/// do everything the synchronous version did: record each seat's call,
/// and — when the room genuinely split with no seat to break it — file a
/// `NeedsYourDecision` backlog entry rather than just say so and forget.
#[derive(serde::Serialize, serde::Deserialize)]
struct CouncilOutcome {
    calls: Vec<SeatCall>,
    /// What actually gets spoken.
    text: String,
    /// The room answered but could not agree — this is a real outcome, not
    /// a failure, but it means the choice really is yours.
    needs_guidance: bool,
    /// Too few seats were reachable to trust anything — a technical
    /// problem, not a finding.
    technical_failure: bool,
}

/// What an `improve` errand built, carried back as JSON so the tick thread
/// can file it into the project's queue (a crew errand cannot touch
/// `self.workshop`). The whole result of the errand is this envelope.
#[derive(serde::Serialize, serde::Deserialize)]
struct ImproveOutcome {
    project: String,
    title: String,
    what: String,
    files: Vec<crate::workshop::FileEdit>,
    verified: bool,
    note: String,
    /// The human line to speak once it is filed.
    summary: String,
    /// What the files it writes looked like when the work started, so the
    /// queued change can tell later if the code moved on under it.
    #[serde(default)]
    bases: Vec<(String, String)>,
}

/// What a crew errand was for, so its ending can be reported once
/// `take_crew_news` sees it — the crew itself only knows the errand ran and
/// what it returned, not what that means to the rest of Atlas.
struct CrewLink {
    watch_id: u64,
    /// Prefix for the journal line this errand's result gets recorded
    /// under, e.g. "backed up".
    label: &'static str,
    /// What the errand was actually asked about, if anything — needed to
    /// file a backlog entry under the right name on failure. `research`
    /// needs this; the housekeeping chores don't.
    topic: Option<String>,
    speak: SpeakPolicy,
}

pub struct Daemon<'a> {
    pub cfg: &'a Config,
    pub plat: &'a dyn Platform,
    pub llm: Option<std::sync::Arc<dyn Llm>>,
    pub parser: Parser,
    pub session: Session,
    pub memory: Memory,
    pub scheduler: Scheduler,
    pub index: Index,
    /// The file index as last read from or written to disk: how many
    /// entries and when it was scanned (`Index::written_as`). `persist`
    /// writes the index only when that has moved (28 Sep 2026): it used to
    /// turn the whole index into text after every turn -- 30 MB for 68,000
    /// files -- only for the store to find the bytes unchanged.
    index_on_disk: Option<(usize, u64)>,
    /// The index as it is read from disk at start, off this thread
    /// (`index::Loading`, 28 Sep 2026). `settle_index` takes it in.
    index_load: crate::index::Loading,
    pub awareness: Awareness,
    pub proactive: Proactive,
    /// Whether each connection is still working. Shown on the hub's
    /// Connections page.
    pub connections: crate::integrations::Board,
    /// How long recent turns took, and where the time went.
    ///
    /// Filled by `turn()` on every turn. An empty window is not the same
    /// fact as a fast one — see `no_slow_turns_and_no_turns_are_different`
    /// in `tests/timing_wired.rs`.
    pub timing: crate::timing::Recent,
    /// The accounts Atlas knows about, and what protects each one.
    ///
    /// `accounts.rs` could audit a slice of these from the day it was written
    /// and nothing ever held one, so the hub's page listed nothing on a
    /// machine with plenty of accounts.
    pub accounts: crate::accounts::Book,
    /// Your enrolled voice, if Atlas has been taught it.
    pub voice_id: crate::voiceid::VoiceId,
    /// What the last spoken turn sounded like. `NotEnrolled` on anything that
    /// did not arrive through a microphone, which is the honest default: a
    /// typed line has no voice to judge.
    pub last_verdict: crate::voiceid::Verdict,
    /// What this machine can actually run. Measured once at start.
    ///
    /// The knobs that matter for staying out of your way are `concurrency`
    /// and `keep_model_warm`: how many things Atlas does at once, and whether
    /// it holds a model in memory between turns. Both were previously fixed
    /// regardless of the machine.
    pub fit: crate::fit::Plan,
    /// The machine the plan was made for, and when it was last re-measured.
    fit_measured: (crate::fit::Machine, u64),
    /// Recovery-code sets you've told Atlas about, and ways back into the
    /// vault. Empty is a real answer — "you have none" is what the gap
    /// reports are for.
    /// The gestures you have taught it.
    pub gestures: crate::handshape::Vocabulary,
    pub code_sets: Vec<crate::codes::Set>,
    pub vault_recovery: Vec<crate::recovery::Setup>,
    /// What the camera last saw, and when it last looked.
    ///
    /// `presence::Sensor` has existed since it was written with nothing ever
    /// calling `observe` — the whole face-and-hands loop was built and never
    /// closed. `None` means never looked, which is not the same as an empty
    /// room and must never be reported as one.
    pub eyes: crate::presence::Sensor,
    /// Named `last_sighting`, not `last_seen`: `last_seen` on this struct
    /// already means when you were last observed, as a timestamp. Fourth
    /// name collision caught in this codebase.
    pub last_sighting: Option<crate::gaze::Sighting>,
    /// When Atlas last looked. The camera opens for a reason, so this only
    /// paces a reason that already exists — it never causes a look.
    pub looked_at: u64,
    /// You told Atlas to watch, until you tell it to stop.
    pub watching_me: bool,
    /// Typing what you say, when it is on.
    ///
    /// `Some` means every utterance goes to `dictate::Dictation` instead of
    /// the parser — see the gate near the top of `turn_from`. Held on the
    /// daemon rather than passed around because dictation spans turns: the
    /// window it started in, what was typed last (so "scratch that" can take
    /// it back), and when you last spoke all have to survive to the next
    /// thing you say.
    pub dictation: Option<crate::dictate::Dictation>,
    /// Steering mode ends at this time unless a hand renews it. A mode you can
    /// leave on by accident is a camera left watching an empty room.
    pub steering_until: Option<u64>,
    /// Which panel the hands are pointed at.
    pub steering_at: usize,
    /// Hand tracking, running on its own thread.
    ///
    /// `None` when it isn't running. Held here so `watch_hands(false)` can
    /// stop it and wait — a tracking thread that outlived the request would
    /// keep moving the pointer after you said stop.
    pub hands: Option<crate::handloop::Tracking>,
    /// Smoothed position and predicted motion, so a slow detector still
    /// feels immediate.
    /// The seeing models, opened the first time Atlas is asked to look.
    ///
    /// Not at start-up: four models take the better part of a second to load,
    /// and most days nobody asks Atlas what a thing is. Held open afterwards,
    /// because the load is the expensive half and the run is not.
    pub looking: Option<crate::vision::Looking>,
    /// Everything Atlas has been shown and told the name of.
    ///
    /// This is the part that makes seeing open-ended. The models know a fixed
    /// list of names somebody else chose; the album is the list that grows
    /// because Eric added to it.
    pub album: crate::vision::Album,
    pub track: crate::handtrack::Track,
    /// What detection actually costs on this machine.
    pub pace: crate::handtrack::Pace,
    /// Where the hand was last, so a drag can be told from a hold.
    pub steering_hand: crate::gaze::Steering,
    /// The window currently being carried, and where it started.
    pub carrying: Option<crate::platform::WindowId>,
    pub carried_from: Option<crate::platform::PixelRect>,
    /// What the hand is over right now, outlined on the desktop.
    ///
    /// Eric was explicit: he doesn't want to watch himself, he wants to see
    /// what he's selecting. So there is no camera preview anywhere — the only
    /// feedback is a ring drawn around the actual thing.
    pub pointing_at: Option<crate::platform::PixelRect>,
    /// Things you handed over from wherever you were standing.
    pub tray: crate::tray::Tray,
    /// How often Atlas has turned out to be right, per kind of work.
    ///
    /// The answer to "how do we make it more confident" that is not "raise the
    /// bar until it acts less": a kind of work earns its own rope from its own
    /// record, and says exactly what would earn it more.
    pub earned: crate::earned::Record,
    /// How Atlas touches each app, and what it has learned about which way
    /// works where.
    ///
    /// RECOVERED IN THE 17 SEP MERGE, along with the three `record` calls and
    /// the `save`. The reading side is wired -- every window Atlas reads
    /// records whether the accessibility backend could actually read it --
    /// because reading names what was selected and never acts on your
    /// accounts, so it needs no ruling. The acting side waits for `delegate`.
    pub backends: crate::backends::Router,

    /// Subsystems whose last save failed, and why. Empty is the healthy
    /// state. Read by `persist_trouble`, and public so `doctor` and the hub
    /// can report the standing state rather than a remembered one.
    pub persist_failures: Vec<(&'static str, String)>,
    /// Whether the person has already been told about the current run of
    /// failures. Stops a full disk producing one sentence per tick.
    persist_told: bool,
    /// Whether `shut_down` has already run.
    ///
    /// `Drop` persists as a backstop for the paths that do not go through
    /// `shut_down` -- the CLI, the tests, a panic unwinding out of `run`.
    /// Without this flag an orderly exit would write all sixteen files twice
    /// and report the same disk failure twice, which reads like two separate
    /// faults.
    stopped: bool,
    /// Consecutive failures to refresh the instance lock. See `tick`.
    missed_beats: u32,
    /// Your settings as they stand now, once one has changed while Atlas was
    /// running. `None` until then, and `cfg.tools` is still the truth.
    tools_live: Option<crate::voice::ToolsConfig>,
    /// Call notes: the call being noted, if any (`callnotes`).
    pub call_notes: crate::callnotes::Notes,
    /// Calls being written up in the background, and when a call was last
    /// looked for.
    last_call_look: u64,
    /// Working a window for you ("finish this until I'm back"): the job,
    /// the window, what was on screen after Atlas last wrote, and when it
    /// last looked.
    /// Windows being worked for you, each an errand of its own.
    pub working_for_you: Vec<WorkingForYou>,
    next_window_job: u64,
    /// The folder whose `tools.yaml` and `settings.yaml` are watched for
    /// changes, and what they held last time they were read. Only a daemon
    /// told where its settings live (`watch_settings`) watches anything.
    settings_watch: Option<(std::path::PathBuf, u64)>,
    /// The settings files' sizes and modified times at the last look
    /// (`settings_stamp`): the contents are only read and hashed again when
    /// this changes.
    settings_stamp: u64,
    /// What you have reached for in the palette lately.
    pub palette: crate::palette::Recent,
    /// The dashboard arrangement, yours, read once at start.
    ///
    /// Named `dashboard`, not `layout`: `layout_prefs::Layout` is the window
    /// arrangement on your monitors and already owns that word here. Two
    /// fields called `layout` on one struct is the third naming collision in
    /// this codebase and the compiler only caught it because they happened to
    /// be different types.
    pub dashboard: crate::dash::Layout,
    /// Whether the dashboard is in arranging mode. Reading is the default.
    pub arranging: bool,
    /// The hub, served by this Atlas rather than by a separate settings-only
    /// mode. Its connections are read on threads of their own
    /// (`server::HubDoor`); the loop answers every request waiting each time
    /// it looks, which is every 50ms while idle (27 Sep 2026 -- it used to
    /// take one connection per pass of the loop, and a click is several).
    pub hub_server: Option<crate::server::HubDoor>,
    /// Everything Atlas can search: research notes, and anything else on the
    /// shelf. Rebuilt from disk at start and after anything writes a note.
    pub library: crate::recall::Library,
    /// Meaning vectors already made, keyed by note content.
    ///
    /// The library above is derived — rebuilt from the notes folder with
    /// every `embedding: None` — so the vectors live here, where a rebuild
    /// cannot lose them, and `reload_library` rehydrates unchanged notes for
    /// free instead of re-running the encoder over everything at startup.
    pub meaning: crate::meaning::Remembered,
    /// Search gets measured again (`recall::measure`) once every note has
    /// been re-read by a new meaning model.
    search_check_due: bool,
    /// What a finished sandbox change would put on the machine.
    ///
    /// Held between the build and the landing because `sandbox::plan` calls
    /// itself the preview and nothing ever turned a preview into a landing.
    pub pending_landing: Vec<crate::sandbox::Change>,
    /// The piece of work Atlas is doing on itself, if any.
    ///
    /// Held rather than rebuilt: `Session::new` on every turn meant the five
    /// stages could never be entered, because `stage` was always `Thought` and
    /// `thought` always `None`. A state machine with no state answers with its
    /// first state forever.
    pub selfwork: Option<crate::selfwork::Session>,
    /// Corrections you have made, and the edits they earned.
    ///
    /// The scoreboard is `repeat_rate`: how often Atlas repeats a mistake it
    /// had already written down a fix for. Nothing else in `revise` matters
    /// if that number is not falling.
    pub mending: crate::revise::Mending,
    /// A correction waiting on you to say what right looks like. "That's
    /// wrong" is a signal to ask, not a lesson to file.
    pending_correction: Option<String>,
    /// The words of this turn, as you actually said them.
    ///
    /// Exists because the phrase parser hands an intent the text *after* the
    /// phrase it matched: "that was too long, keep it to one line" arrives as
    /// `GotItWrong("keep it to one line")`, which is the fix with the
    /// complaint thrown away. A correction needs both — the complaint is what
    /// makes two corrections match as the same one.
    last_said: String,
    /// An edit that has been proposed and not yet answered.
    pending_edit: Option<crate::revise::Edit>,
    /// When the brief last looked, so "I handled 4" counts what happened
    /// since rather than everything that ever ran.
    last_brief: u64,
    /// When the day's run was last given.
    ///
    /// A timestamp rather than a day number, and the change is the point.
    /// "Has today's run gone out" needs a day, and a day is not a thing this
    /// can decide on its own: which day a moment belongs to depends on when
    /// *you* stop, which `daily::Rhythm` works out and `daily::day_of_with`
    /// applies. Keeping the moment and asking `daily` about it means the
    /// answer follows your hours instead of the calendar's.
    last_brief_at: u64,
    /// The last turn that was yours. Atlas's own work does not count.
    ///
    /// What tells working through the night from starting a day: the gap
    /// since this. See `daily::arriving`.
    last_turn_of_yours: u64,
    /// Every model call this install has made: who asked, how long it took,
    /// whether it worked. Never what was said.
    pub trace: crate::trace::Trace,
    /// The master index of the notes folder: what Atlas knows it has, without
    /// opening any of it. (`index` on this struct is the *file* index, a
    /// different thing entirely.)
    ///
    /// Deliberately *not* rebuilt from disk on every read. The whole value of
    /// `contents::drift` is that this is a written-down claim which can turn
    /// out to be wrong; an index regenerated on demand agrees with the folder
    /// by construction and can never drift, which is the same as not having
    /// the check at all.
    pub contents: crate::contents::Contents,
    /// Anything Atlas could not get to you at the time it happened.
    pub outbox: crate::notify::Outbox,
    /// Audio devices as last enumerated. `None` means never looked.
    pub audio_devices: Option<Vec<crate::audio::Device>>,
    /// Where a message from another Atlas would arrive, if one is open.
    /// `None` for the overwhelming majority of installs, which will never
    /// have another Atlas to hear from -- see `with_signal_listener`.
    signal_listener: Option<crate::server::SignalListener>,
    /// A running tally of where answers came from -- named capability,
    /// cached report, or an actual model call. Kept because the useful
    /// question is not whether tiering runs, it is whether it is doing
    /// anything: a daemon where everything ends up at Think means the
    /// short-circuit is not paying for itself.
    pub tier_mix: crate::tier::Mix,
    /// Initiative. `proactive` decides whether Atlas may speak; this decides
    /// whether it has anything worth saying that you did not ask for.
    pub nudger: crate::nudge::Nudger,
    pub store: Store,
    pub autonomy: Autonomy,
    /// One record of everything Atlas did, across every area, so "what did
    /// you do" and "undo that" have something to answer from.
    pub history: crate::undo::History,
    /// Decisions worth being able to explain afterwards. A `why::Record`
    /// rather than a bare `Vec` so `Intent::Why` has something to read: the
    /// record trims itself and every turn's routing choice is written to it
    /// through `note_full`, which was the missing half -- the read path
    /// existed and the list it read was never filled.
    pub decisions: crate::why::Record,
    /// Said once per run at most: an unfamiliar voice is told there is a way
    /// to hand the machine over on purpose. A sensor gets one sentence, not a
    /// running commentary.
    pub offered_handover: bool,
    /// Somewhere a passphrase can be typed, when there is one.
    ///
    /// `None` -- the default, and what every test gets -- means Atlas has no
    /// screen of its own to put a prompt on, and says so rather than
    /// pretending to have asked. The binary installs `typed::Console`.
    ///
    /// Held rather than constructed on demand for the reason `typed.rs`
    /// gives: a daemon that read stdin inside `turn` would make the tests
    /// hang instead of fail.
    asks_quietly: Option<Box<dyn crate::typed::AsksQuietly>>,
    /// The passphrase-locked store. Sealed until you open it.
    pub vault: crate::vault::Vault,
    /// How many times each undelivered message has been tried, and when.
    ///
    /// Not persisted, deliberately: it is about the network rather than the
    /// conversation, and a retry count that survived a restart would make
    /// Atlas back off from a peer for a quarter of an hour on the strength of
    /// attempts made before the machine was even switched on.
    pub tries: crate::courier::Tries,
    /// Conversations with the people you work with.
    ///
    /// Held rather than loaded per turn because the ordering counter lives on
    /// it: a copy read fresh each time would keep re-reading the same counter
    /// and two messages written in one session could tie.
    pub chats: crate::chat::Chats,
    /// Which sites Atlas may sign you into.
    pub access: crate::signin::Access,
    /// Things you said, filed afterwards.
    pub notebook: crate::capture::Notebook,
    /// Round 11's tools: what they keep, loaded on first use (`workday`).
    pub workday: crate::workday::Kit,
    /// The one gate everything unprompted passes through.
    pub gate: crate::interrupt::Gate,
    /// Everything outstanding, as items with properties rather than as a
    /// list — so the same things can be a list, a board or a calendar.
    pub workspace: Vec<crate::workspace_view::Item>,
    /// How you've arranged the hub.
    pub layout: crate::layout_prefs::Layout,
    /// When the list was last looked at, so the day turning can be noticed.
    pub last_seen: u64,
    /// How many days each open thing has been carried.
    pub carried: Vec<(String, u32)>,
    /// Times other people have asked for, waiting on you.
    pub proposals: Vec<crate::booking::Proposal>,
    /// Your own calendar — events, offline, and the bridge to the phone's.
    pub calendar: crate::calendar::Calendar,
    /// Every day closed so far, kept so `daily::still_keep` has something to
    /// prune. `daily::close()` was purely computational until now -- built
    /// fresh from live workspace items each rollover and hand straight to the
    /// brief opener, with nothing archived. Persisted the same way `mending`
    /// is: one JSON file in the store, loaded whole, saved whole.
    pub daily_history: Vec<crate::daily::Closed>,
    /// When you were active, so the day's rollover is worked out rather than
    /// set.
    pub rhythm: crate::daily::Rhythm,
    /// Things you said you weren't doing, kept so they're findable.
    pub dropped: Vec<crate::daily::Dropped>,
    /// Requests Atlas was given and had no way to carry out, kept so that
    /// "what am I missing?" (`Intent::Recommend`) can turn a real ask into a
    /// concrete suggestion. This is the source `Observations` needed and never
    /// had -- only `unsupported_requests` is filled here; the timings and
    /// missing-tool signals are measured fresh each time the report is asked.
    pub wants_seen: crate::wants::Observations,
    /// Work that stopped when the machine did, with what was found on
    /// checking it.
    pub interrupted: Vec<(String, crate::awake::Checked, bool)>,
    /// What Atlas has noticed about itself — routes that fail, answers you
    /// keep correcting. The stage before a diagnosis.
    pub signals: Vec<crate::selfaudit::Signal>,
    /// What was said while the current job ran. Progress is disposable; the
    /// result has to stand without it.
    pub run: crate::channel::Run,
    /// Typed, linked memory. Kinds decide decay and precedence.
    pub facts: crate::facts::Book,
    /// Everything Atlas knows, merged rather than accumulated.
    pub known: Vec<crate::consolidate::Claim>,
    /// A line for each thing let go to make room, so "I knew something
    /// about that" can be said instead of nothing (H10).
    pub stones: Vec<crate::consolidate::Tombstone>,
    /// Your own words — names, projects, jargon — handed to the speech model
    /// as hints (H11). Kept where the voice reads it.
    pub vocab: crate::improve::Vocabulary,
    /// Old conversation is being summarised off the turn (H12).
    folding: bool,
    /// Awake, locked but working, or asleep (H13a).
    running: crate::awake::Running,
    /// Something about how well Atlas is hearing you, said on the next tick.
    heard_note: Option<String>,
    /// The connection lines last written to the log, so each is written once.
    connections_logged: Vec<String>,
    /// The typing box, started hidden and shown on its key.
    typebox: Option<crate::typebox::Standby>,
    /// The turn being answered was typed, not said: "hands-free only"
    /// (Sound & voice) keeps its reply on screen.
    pub(crate) turn_was_typed: bool,
    /// Things said that reached nothing, and how many were said at all.
    ///
    /// Counted at dispatch because every route into an action passes through
    /// that one function — counting at the parser would miss anything reaching
    /// the daemon another way.
    pub unknown_count: u32,
    pub utterance_count: u32,
    pub last_unknown: String,

    /// How long to keep listening after Atlas speaks, before requiring the
    /// wake word again. This is what makes it a conversation.
    pub followup_secs: u32,
    /// The offer awaiting a yes/no.
    pending_offer: Option<Offer>,
    /// Set when `wanted::read` came back genuinely unclear and Atlas asked
    /// which you wanted. Holds the original words, so whichever way you
    /// answer, Atlas is responding to what you actually said, not to the
    /// answer to the meta-question.
    pending_wanted: Option<String>,
    /// The scheduled job whose approval question is outstanding.
    pending_job: Option<u64>,
    /// The backlog item Atlas last offered to do ("Want me to do it now?").
    /// Holds the item id so the yes/no that follows can run the original
    /// request or, on a no, `backlog::dismiss` it for good rather than raise
    /// it again on the next quiet tick.
    pending_backlog: Option<u64>,
    /// Which input tier is live.
    pub tiers: Tiers,
    pub power: Power,
    pub log: Log,
    pub probe: Probe,
    /// Things Atlas could not do, offered back when they become possible.
    pub backlog: Backlog,
    /// Patterns noticed about how you work — rhythm, taste, what you
    /// correct, what you say no to. Built from what happens, never asked
    /// for directly.
    pub person: crate::person::Person,
    /// Posts and emails waiting to go out.
    pub publisher: Publisher,
    pub workshop: crate::workshop::Workshop,
    /// Paused / running.
    pub attention: Attention,
    /// Which apps Atlas may touch.
    pub permissions: Permissions,
    /// What "it" and "that" currently refer to.
    pub referents: Referents,
    pub connectivity: Connectivity,
    /// Heavyweight helper processes, and the budget they run under.
    ///
    /// Was a bare `Supervisor`, which returns orders and owns nothing — so
    /// `acquire` had no callers, `running` was empty forever, and every
    /// tick reaped nothing. `Helpers` holds the child processes, so an
    /// eviction is a kill.
    pub helpers: crate::lifecycle::Helpers,
    /// Whether this Atlas starts its own model server when it needs one
    /// (`keep_model_server`). Off unless a door asks for it
    /// (`starting_the_model_server`): a test's Atlas must never start a
    /// 3 GB server because the machine running the tests happens to have a
    /// model in `models/`.
    starts_model_server: bool,
    /// When to look again for a model, while there's none (`keep_model_server`).
    model_look_at: u64,
    /// What was typed or said on the hub's Talk page, waiting for its turn:
    /// (words, said aloud). Answered on the next tick rather than inside the
    /// request, so sending never hangs the page while the model thinks.
    pub(crate) talk_queue: Vec<(String, bool)>,
    /// A turn whose model call is running on a worker thread, so the daemon
    /// loop -- the hub, the ticks -- carries on while the model thinks
    /// (27 Sep 2026: the whole hub hung for as long as a Talk reply took).
    pending_turn: Option<PendingTurn>,
    /// The last id given to a `pending_turn`, so a caller can tell the turn
    /// it started from one that was already waiting (28 Sep 2026: a voice
    /// turn took over a Talk page turn still thinking).
    pending_seq: u64,
    /// What the model was also asked to do in this turn's reply and won't
    /// be, named at the end of the reply (`Brain::converse_noting`).
    also_asked: Vec<String>,
    /// Set while a worker's answer is being finished: a tool that reads
    /// something back may have its result put into words by the model
    /// (`RephraseAsk`).
    rephrase_ok: bool,
    /// What `run_command` found for the model to put into words.
    rephrase_ask: Option<RephraseAsk>,
    /// The time of the turn being acted on, while `run_command` acts on it
    /// (`now_acting`).
    acting_at: Option<u64>,
    /// Whether a model server not started by this Atlas was answering, and
    /// when that was found (`keep_model_server`).
    model_server_seen: std::sync::Arc<std::sync::Mutex<Option<(std::time::Instant, bool)>>>,
    model_probe_busy: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// What warming the model's prompt came to (`warm_the_model`), for the
    /// log: written by that thread, logged by the next tick.
    model_warmed: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    /// When this Atlas last tried to start one.
    model_start_tried: Option<std::time::Instant>,
    /// Turns may hand their model call to a worker (`pending_turn`): set by
    /// the Talk page's queue and the voice loop around a turn.
    defer_turns: bool,
    /// Set for the one `run_command` at the end of such a turn.
    may_defer: bool,
    /// The decision a worker brought back, for `run_command` to finish with.
    decided_already: Option<brain::Decision>,
    /// How long that worker's model call took.
    decided_in_ms: Option<u64>,
    /// What the model has written so far for the turn in flight -- the words
    /// as they arrive, for the Talk page to show in place of "thinking…".
    pub talk_partial: std::sync::Arc<std::sync::Mutex<String>>,
    /// When the question Atlas is waiting on was first seen, and which one:
    /// a question left unanswered for ten minutes is dropped rather than
    /// taking whatever is said next as its answer.
    pending_stamp: Option<(u64, String)>,
    /// When Atlas last finished speaking to you. A follow-up soon after is
    /// the conversation carrying on, whoever it mentions.
    last_spoke_at: u64,
    /// Every command, as a tool the model can call (`intent::ToolBook`).
    tool_book: crate::intent::ToolBook,
    /// Other programs' tools (`mcp`): the servers in `tools.yaml`, started
    /// on their own threads the first time a conversation needs tools.
    mcp: crate::mcp::McpHub,
    /// This turn's model was asked over the chat path, which stops at a
    /// sentence's end by itself.
    by_chat: bool,
    /// The last thing said about the model server not starting, so the log
    /// gets it once rather than every turn.
    model_server_trouble: Option<String>,
    /// Slow work that runs off the tick thread instead of blocking it.
    /// `crew::Crew::settle` is called once a tick and never waits — see
    /// `crew.rs`. Not persisted: nothing survives a restart mid-errand
    /// anyway (the thread dies with the process), and what each errand was
    /// doing lives in `long_work` instead, which is.
    pub crew: Crew,
    /// tools.yaml, resolved once. See `tools_cfg`.
    tools_resolved: std::sync::Arc<crate::voice::ToolsConfig>,
    /// When the end-of-tick save last ran. See `persist_after`.
    last_persist: u64,
    /// What long-running crew work is worth telling you about, and when.
    /// Named `long_work` rather than `watcher` because that name is already
    /// taken by `watch::Watcher`, the unrelated uptime prober.
    pub long_work: watching::Watcher,
    /// Maps a crew errand's id to what it was for, so `take_crew_news` can
    /// update the right `long_work` job when it settles.
    crew_links: std::collections::HashMap<u64, CrewLink>,
    /// "Which one?" asked about errands and not yet answered: the verb, the
    /// ids it was asked about, and when. The next line answers it.
    errand_question: Option<(crate::which_errand::Verb, Vec<u64>, u64)>,
    /// The last errand pick, so "no, the backup" can swap it.
    last_errand_pick: Option<(crate::which_errand::Verb, Vec<u64>, u64)>,
    /// Errands held by a whole-Atlas "pause", released by the resume that
    /// ends it — and only those, so one you paused on its own stays paused.
    held_by_pause: Vec<u64>,
    /// Last time the housekeeping sweep ran.
    last_tidy: u64,
    /// What the last index check found, waiting to be raised.
    ///
    /// Held rather than raised on the spot because the check runs whether or
    /// not Atlas is allowed to speak this tick, and the drift does not stop
    /// being true just because the moment was wrong.
    index_drifted: Option<crate::contents::Drift>,
    /// Last time the notes index was checked against the folder.
    ///
    /// Its own clock rather than the tidy sweep's, because the tidy sweep
    /// runs quiet housekeeping and this one may speak. Sharing a timer would
    /// have meant the two could not be moved independently later without
    /// changing how often Atlas interrupts.
    last_index_check: u64,
    /// Posts already warned about, so one broken post isn't mentioned hourly.
    warned_posts: Vec<u64>,
    /// What Atlas was cut off mid-way through saying.
    pub unsaid: Option<String>,
    /// What Atlas has done, for "what happened while I was away".
    pub journal: Journal,
    /// Said at the first tick if the activity log fails its check on
    /// loading: an altered record of what Atlas did is something you hear,
    /// not something a log file keeps to itself.
    journal_warning: Option<String>,
    /// Work you asked for that's with the crew, by crew id, written down so
    /// a restart can say what it cut off (`resume`).
    unfinished: std::collections::BTreeMap<u64, crate::resume::Unfinished>,
    /// Set while a cut-off piece of work is being redone, so its record
    /// says so.
    redoing: Option<u32>,
    /// What a restart found cut off, dealt with on the first tick.
    left_over: Vec<crate::resume::Unfinished>,
    /// Window jobs saved before the restart, restored on the first tick.
    left_windows: Vec<crate::resume::SavedWindow>,
    /// The window jobs as last saved, so they're only written when they change.
    saved_windows: String,
    /// When you last spoke, so the away-brief knows what to cover.
    last_present: u64,
    /// Where the time went (`worklog`), kept on this machine only.
    pub worklog: crate::worklog::WorkLog,
    /// Since when the keyboard and mouse have been silent past `away_after`.
    input_away: Option<u64>,
    /// A break the keyboard and mouse saw, not yet welcomed: (left, back).
    back_from: Option<(u64, u64)>,
    /// The last tick the work log saw, so a sleeping laptop's gap is seen.
    last_beat: u64,
    /// When the run loop last finished work of its own (a turn, a tick). A
    /// long answer or render stalls the ticks; that gap is Atlas busy with
    /// you, not the lid shut, so the break check measures from here.
    worked_until: u64,
    /// The last animation drawn, its spec, and when -- what "make it faster"
    /// or "make it red" refers to for the next hour (`motion::refine`).
    last_animation: Option<(std::path::PathBuf, crate::motion::MotionSpec, u64)>,
    /// Silence longer than this counts as having been away.
    pub away_after: u64,
    /// An away-brief waiting to be prepended to the next reply.
    pending_brief: Option<String>,
    /// The list behind an *offered* away-brief ("updates on X when you want
    /// them"), kept so that saying yes gets `returning::full_brief` of the
    /// things that actually happened while you were gone, not a fresh read
    /// of a journal that has moved on since.
    pending_brief_detail: Option<Vec<crate::returning::Happened>>,
    /// What kind of moment the last turn was, so the model gets the right
    /// instructions next time.
    pub last_register: crate::register::Register,
    /// The clipboard, as last read. Set by the platform layer when you ask —
    /// never watched in the background.
    pub clipboard_text: Option<String>,
    /// A clipboard question waiting to go to the model.
    pub pending_clipboard: Option<String>,
    /// An answer to put back on the clipboard, so you can paste it where you
    /// were. Set when a clipboard request is answered and `reply_to_clipboard`
    /// is on; the platform layer takes it after the turn and writes it out —
    /// the mirror of how `clipboard_text` is set on the way in.
    pub clipboard_writeback: Option<String>,
    /// The behaviour view of the change currently staged by a self-fix, so you
    /// can ask "what will that change do?" and hear what will be *different*
    /// rather than read a diff. Set when a candidate is staged, cleared when it
    /// lands or is thrown away.
    pub pending_change_effect: Option<crate::plainchange::Effect>,
    /// Calendar reminders already spoken, by (event id, occurrence start), so a
    /// reminder fires once per occurrence and a daily standup reminds each day
    /// without repeating within a day. Pruned of past occurrences on each tick.
    pub reminded: std::collections::BTreeSet<(u64, u64)>,
    /// Text a hub page shows once and that must never be in an address: a
    /// friend link, an invitation code, a recovery key (`hubjobs::Flash`).
    /// Gone once shown, or after five minutes. Replaced `said_about_sync`,
    /// which was never cleared, so a code from last week sat on the page.
    pub flash_once: Vec<(crate::hub::Page, crate::hubjobs::Flash, u64)>,
    /// Hub buttons whose work is running on the crew (`hubjobs`), shared
    /// with the threads doing it.
    pub hub_jobs: std::sync::Arc<crate::hubjobs::Jobs>,
    /// What to do when each hub errand ends, by the crew's id for it.
    pub(crate) hub_after: std::collections::HashMap<u64, HubAfter>,
    /// The hub's one-shot things for the vault, Sync and "Free up space"
    /// sections: a recovery key waiting to be shown once, the passphrase
    /// forms' one-time marks, and a survey in hand (`hubvault`).
    pub shown_once: crate::hubvault::ShownOnce,
    /// The full walk-through of the last rehearsal, for "show me the detail".
    pub last_rehearsal: Option<String>,
    /// A panel that should be on screen.
    pub wants_panel: Option<crate::panel::Panel>,
    /// When the current panel was asked for, so the waking one can leave by
    /// itself after `panels.waking_secs` (`panel::faded`).
    panel_shown_at: u64,
    /// When the updates folder was last looked at, and what was last said
    /// about it, so a waiting update is mentioned once rather than daily.
    last_update_check: (u64, String),
    /// Sleep held off while the night's work runs (`inhibit`); dropped the
    /// moment it ends.
    sleep_hold: Option<crate::inhibit::Held>,
    /// A window job waiting on your yes before it goes on.
    pending_window_confirm: Option<u64>,
    /// A panel waiting on your yes, when there's only one screen.
    pub pending_panel: Option<crate::panel::Panel>,
    /// A sign-in in Atlas's browser waiting for a two-factor code (the site).
    signing_in_waiting: Option<String>,
    /// Where a code being looked for in your email goes, and for which site.
    code_wanted: Option<(crate::twofactor::Target, Option<String>)>,
    /// A security change read back and waiting for your yes.
    pending_security: Option<crate::confirmed::Asked>,
    /// A security change waiting for the code its sign-in asked for.
    after_code: Option<crate::confirmed::Asked>,
    /// A sign-up waiting on you (a robot check or a code).
    enrolling: Option<crate::enrol::Enrolment>,
    /// A sign-in you've asked to be checked on first, waiting for your yes.
    pending_signin: Option<(String, String)>,
    /// The last "wrong tokens at the hub" said, so it's said once per run.
    guesses_said: Option<String>,
    /// Hub requests since the last summary line (`hub_timing`).
    hub_times: HubTimes,
    /// Things you do the same way every time (Eric, E4).
    pub routines: crate::routine::Watcher,
    /// A routine asked about, waiting for your yes.
    pending_routine: Option<String>,
    /// A routine due now that asks first ("Your usual morning setup?").
    pending_routine_run: Option<Vec<String>>,
    /// What sorting your mailbox would do, rehearsed and waiting for "go".
    pub mail_plans: Vec<crate::mail::SortPlan>,
    pending_mail_sort: bool,
    /// A post whose approval question is waiting on your answer.
    pending_post_approval: Option<u64>,
    /// An approved-in-principle post waiting for when to go.
    pending_post_when: Option<u64>,
    /// Posts being sent, or waiting to retry: (post, not before). A slow
    /// send isn't started twice, and a retry waits five minutes rather than
    /// going round every tick.
    posting: Vec<(u64, u64)>,
    /// A button that can't be undone, waiting for your yes: (window, name, app).
    pending_press: Option<(u64, String, String)>,
    /// A storage plan shown and waiting for your yes.
    pending_storage: Option<crate::tune::StoragePlan>,
    /// An undo asked about ("Undo X?"), waiting for your yes.
    pending_undo: Option<u64>,
    /// A dropped task you asked about: put back on the list on a yes (H8).
    pending_bring_back: Option<String>,
    /// A decision being worked over several turns (H9), kept across restarts.
    pub deciding: Option<crate::decide::Decision>,
    /// Waiting on your answer to a move, or on "want the whole working?".
    pending_decision: Option<Option<crate::decide::Move>>,
    /// An edited video waiting for "keep it?": (original, working copy, result).
    pending_media_keep: Option<(String, String, String)>,
    /// A file the virus scan couldn't check, and what you asked to do with it
    /// ("read" or "unzip"). Opened only on your yes (H3).
    pending_unscanned: Option<(String, String)>,
    /// Push-to-talk and the typing-box key (H1), watched from anywhere in
    /// Windows. Set by the program before `run`; `None` in tests and where
    /// there are no global keys.
    pub hotkeys: Option<crate::hotkeys::Hotkeys>,
    /// "Get rid of the original?", waiting: the original's path.
    pending_media_original: Option<String>,
    /// Correcting as you type: the watcher thread and what it shares.
    typing_thread: Option<std::thread::JoinHandle<()>>,
    /// The microphone's own thread (`micthread`): the wake word, and
    /// watching for your voice while Atlas speaks. Started by `run`.
    mic: Option<crate::micthread::MicThread>,
    /// `Ears::mic_work` was asked, so a `None` isn't asked again every pass.
    mic_asked: bool,
    /// Heard while the loop was napping, for the next pass.
    mic_heard: Option<crate::micthread::Heard>,
    /// What you said by voice over the last reply (not a stop or a pause),
    /// to be answered next (`speakthread::Said::words`).
    cut_in_by_voice: Option<String>,
    typing_stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    typing_busy: std::sync::Arc<std::sync::Mutex<Vec<u64>>>,
    typing_said: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    /// What Atlas is doing, and the thinking behind it.
    pub mind: crate::mind::Mind,
    /// One conversation across days, rather than a session that resets.
    pub thread: Thread,
    /// Atlas's manner. Shapes everything it says.
    pub persona: Persona,
    /// Did the last turn go wrong? Feeds `register::Moment::after_a_failure`
    /// on the NEXT turn, which is the turn where being short with Atlas
    /// actually shows up.
    /// The append-only log of what happened here, for carrying to your other
    /// devices.
    ///
    /// `sync.rs` syncs *what you decided*, not raw state -- which is why it
    /// can merge without the copies fighting. It had no log to append to and
    /// nothing appending, so the merge, the bundle and the conflict rule were
    /// all exercised only by their own tests.
    synclog: crate::sync::Log,
    /// A bound listener for a direct same-network sync, if one could be bound.
    /// Lazily opened on the first sync pass and polled each pass, so a phone on
    /// the same wifi is caught up straight across the socket rather than only
    /// through the folder. `None` when the port was taken or binding failed —
    /// the folder still carries everything, so this is an accelerator, never a
    /// dependency.
    sync_server: Option<crate::transport::Server>,
    /// The highest sequence number taken in from each other device.
    ///
    /// `sync::already_seen` is built around exactly this and had nothing
    /// keeping it: without it, reading the same folder twice counts the same
    /// events again, which is not a rare case -- the folder is read every
    /// time you say "sync".
    seen_up_to: Vec<(String, u64)>,
    /// The night's work, while a night is running.
    ///
    /// `overnight.rs` was a complete state machine -- window, give-up rule,
    /// budget, morning brief -- with no caller. It kept nothing itself, so
    /// the session lives here and is saved with everything else: a night
    /// that forgot itself at the first restart would report an empty
    /// morning.
    overnight: Option<crate::overnight::Session>,
    /// Last tick that ran a step of the night.
    last_overnight: u64,
    /// When the sync folder was last checked for still syncing
    /// (`cloudsync::still_syncing`), so it runs every `check_every_hours`.
    last_sync_check: u64,
    /// Set when a night ends, cleared once the brief has been said.
    morning_brief: Option<String>,
    /// Where you were while the last stretch of unattended work happened.
    ///
    /// Kept so the write-up can say "overnight" or "while you were out"
    /// rather than assuming the first. A night's work reported as having
    /// happened overnight, when it happened on a Tuesday afternoon you spent
    /// at the office, is a small lie that makes you trust the rest less.
    worked_while: Option<crate::daily::Whereabouts>,
    pub last_turn_failed: bool,
    /// The register this turn was briefed with, so the shaping step uses the
    /// same reading rather than taking a second one.
    pub this_turn_register: Option<crate::register::Register>,
    /// The sentence cap this turn was briefed with, so the trimming uses the
    /// same number rather than computing a second one.
    pub this_turn_cap: Option<usize>,
    /// The question Atlas asked, when this turn is the answer to it.
    ///
    /// Held for exactly one turn and taken by `context`, so the model is told
    /// what was asked before it reads the reply to it.
    pub answering: Option<String>,
    /// Named workspace modes.
    pub modes: Modes,
    /// Machine monitoring, and the restraint that stops it nagging.
    pub health: Reporter,
    /// Other machines watched from the outside.
    pub watcher: Watcher,
    /// Your standing watches (`tools.automations`), and what they remember
    /// across restarts — a "for 10 minutes" timer at minute 8 survives one.
    automations: crate::automation::Engine,
    /// Work prepared before you ask for it.
    pub anticipator: Anticipator,
    /// Nothing removed is gone immediately.
    pub trash: Trash,
    /// Two lanes, so Atlas being busy never makes you wait.
    pub queue: Queue,
    /// Named sequences you can trigger by name.
    pub flows: Library,
    /// The workflow in flight, if one is.
    ///
    /// This is what makes a saved flow a RUN rather than a handful of queue
    /// pushes: it holds its place, carries each step's output to the next
    /// (`{name}` substitution), honours `on_fail`, and can pause mid-chain
    /// for a yes. All of that machinery existed in `flow::Run` with tests
    /// and no caller — triggering a flow flattened its steps into the queue
    /// and threw `on_fail` and `produces` away, so a step three that used
    /// `{name}` from step one ran with the literal braces in the command.
    ///
    /// In-memory only, deliberately: a flow interrupted by a restart is a
    /// flow whose middle steps may or may not have happened, and resuming
    /// one on a guess is worse than saying it stopped.
    current_flow: Option<crate::flow::Run>,
    /// Where add-ons are read from (`plugins`). This install's
    /// `data/plugins`; a field so a test can point it somewhere of its own.
    pub plugins_dir: std::path::PathBuf,
    /// The add-ons as last read (`plugins::Kept`), so a turn doesn't read
    /// and hash every add-on's file.
    plugins_kept: crate::plugins::Kept,
    /// Where this install's pairings and its own key live
    /// (`kin::where_pairings_live`); a field so a test can run two Atlases
    /// in one process, each with its own.
    pub peer_dir: std::path::PathBuf,
    /// The one address friend links give for this Atlas, when set, instead of
    /// asking the router (`portmap`). For running two Atlases on one machine
    /// (and in tests), where the answer is simply `127.0.0.1`.
    pub friend_host: Option<String>,
    /// Atlas's own Tor (`onion`), once started: how friends reach this Atlas
    /// and how it reaches theirs.
    tor: Option<crate::onion::Tor>,
    /// Which Tor to run and with what extra settings, instead of the one
    /// beside Atlas: `Some(None)` runs none at all. For tests (a private Tor
    /// network, or none) and for two Atlases on one machine.
    pub tor_instead: Option<Option<(std::path::PathBuf, Vec<String>)>>,
    /// The `tor` program Atlas last started, and whether its lines are
    /// Atlas's own (no `kin.tor_extra`): only then may Atlas switch it to
    /// bridges by itself.
    tor_binary: Option<(std::path::PathBuf, bool)>,
    /// Connections to friends through Tor, kept open between messages (gap AN).
    tor_connections: std::sync::Arc<crate::kin::TorConnections>,
    /// Feedback read back to you and waiting on your yes ("send it?"): the
    /// exact thing that will go, so the yes sends what was read out.
    pub(crate) feedback_draft: Option<crate::feedback::Feedback>,
    /// When each paired Atlas was last heard from (Partners' online/offline).
    pub(crate) reached: std::sync::Arc<crate::kin::Reached>,
    /// Voices being downloaded from the Sound page (`voicepick`).
    pub(crate) voice_downloads: std::sync::Arc<crate::voicepick::Downloads>,
    /// The code the Your phone page is showing, if any (`phoneadd`).
    pub(crate) phone_code: Option<crate::phoneadd::Showing>,
    /// The release key this Atlas trusts, which an update it signs from the
    /// hub must be signed with: `release::RELEASE_PUBLIC_KEY`, built in.
    pub(crate) release_anchor: [u8; 32],
    /// Where to look for downloads (a build to sign, the phone apps), instead
    /// of Downloads and the Desktop.
    pub(crate) builds_folder: Option<std::path::PathBuf>,
    /// iPhones heard by the code's server, not yet kept.
    pub(crate) phones_heard: std::sync::Arc<std::sync::Mutex<Vec<crate::phoneadd::Device>>>,
    /// When each paired Atlas was last tried for an introduction or a group
    /// list, so one that's offline is retried every few minutes rather than
    /// every tick.
    peer_tries: std::collections::BTreeMap<String, u64>,
    /// The `mind` work id narrating the flow in flight. Zero when none —
    /// `Mind` never issues zero.
    flow_mind: u64,
    last_backup: u64,
    /// When the current unbroken stretch of work started.
    ///
    /// `person.hours_before_saying` is a threshold on this, and nothing was
    /// counting it until 19 Sep 2026. Reset by a real break rather than by a
    /// pause: five minutes away from the keyboard is not a break, and
    /// treating it as one would mean the threshold was never reached.
    working_since: Option<u64>,
}

/// Which kind of Tor bridge got through on this network last (`onion::BRIDGE_KINDS`),
/// or empty for straight through.
const TOR_BRIDGES: &str = "tor_bridges";

impl<'a> Daemon<'a> {
    pub fn new(
        cfg: &'a Config,
        plat: &'a dyn Platform,
        llm: Option<std::sync::Arc<dyn Llm>>,
        store: Store,
        proactive: Proactive,
    ) -> Self {
        let store_for_load = store.clone();
        let store_for_load2 = store.clone();
        let store_for_reached = store.clone();
        let tools_resolved = std::sync::Arc::new(resolve_tools(cfg.tools.as_ref(), &store));
        // The clock everything is shown on: your `time_zone` if you chose one,
        // this machine's otherwise (`localclock`, `tz::home`).
        {
            let set = cfg.tools.as_ref().map(|t| t.time_zone.trim().to_string()).unwrap_or_default();
            crate::localclock::set_home_zone((!set.is_empty() && !set.eq_ignore_ascii_case("automatic")).then(|| crate::tz::home(&set)));
        }
        let logs_dir = store.logs_dir();
        // The crew's worker count, derived from this machine rather than a
        // fixed number. `fit::measure` knows the real cores and memory, so the
        // count is right on a big desktop and safe on a small laptop; the
        // config's `background_slots`, if set, is honoured only as a ceiling
        // and never as a floor (see `Machine::background_workers`).
        //
        // RECOVERED IN THE 17 SEP MERGE. This was `unwrap_or(2)` on the
        // improvements side -- the hardcoded number this replaced on 16 Sep --
        // and taking their `daemon.rs` whole put it back. `dead_methods`
        // caught it by reporting `fit::background_workers` as an ORPHAN:
        // public, uncalled, and untested, which is what a wired function
        // becomes when its only caller is overwritten.
        // Your `retention:` section, read here because both of the things it
        // governs are built in this initialiser. Until 18 Sep 2026 neither
        // was: the log rotated at a hardcoded 2MB while the file said 4, and
        // the conversation kept a hardcoded 40 turns while `session_turns`
        // sat unread beside it. The first is the worse of the two -- a person
        // raising `logs_mb` to keep more history got no more history, and
        // `retention::plan` gives logs no age limit, so this cap is the only
        // thing that decides what survives.
        let retention = cfg
            .tools
            .as_ref()
            .map(|t| t.retention.clone())
            .unwrap_or_default();
        let configured_cap = cfg.tools.as_ref().map(|t| t.lanes.background_slots);
        let here = crate::fit::measure();
        let background_slots = here.background_workers(configured_cap).max(1);
        // The measured plan's `keep_model_warm`, acted on: every model request
        // carries a keep-alive from it (`brain::with_keep_alive`). It was
        // computed here and printed by `doctor` as "stays loaded" while the
        // model server unloaded the weights after five idle minutes anyway.
        crate::brain::set_keep_warm(crate::fit::plan_for(&here).keep_model_warm);
        // The crew's rules as numbers: thinking hands from the machine, a
        // core left free, and your memory margin and battery floor.
        let crew_cfg = cfg.tools.as_ref().map(|t| t.crew.clone()).unwrap_or_default();
        let crew_limits = crew::Limits {
            slots: background_slots,
            cores: Some(here.cpu_cores.max(1) as usize),
            waiting_hands: crew::WAITING_HANDS,
            keep_free_mb: crew_cfg.keep_free_mb,
            battery_floor_percent: crew_cfg.battery_floor_percent,
        };
        let memory = Memory::load(&store);
        let scheduler = Scheduler::load(&store);
        // Read on its own thread (`index::Loading`): tens of megabytes on a
        // full disk, and it used to hold up the hub's first page. Empty until
        // it arrives; `settle_index` takes it in.
        let index_load = crate::index::Loading::start(&store);
        let index = Index::default();
        let journal_at_start = Journal::load(&store);
        // Loaded before the store is moved into the struct, like the four
        // above it.
        let chats_at_start = crate::chat::Chats::load(&store);
        // Same reason, and it was missing for longer: a defaulted notebook
        // starts every run empty, so "catch a thought before it's gone" lost
        // the thought at the next restart while saying it had kept it.
        let notebook_at_start = crate::capture::Notebook::load(&store);
        let known = store.load("known");
        // Calendar reminders already spoken, and times other people proposed
        // that are waiting on you — both persisted so a restart doesn't repeat
        // a reminder or lose a proposal mid-decision.
        let reminded = store.load("reminded");
        let proposals = store.load("proposals");
        let mut d = Daemon {
            parser: Parser::new(&cfg.commands),
            cfg,
            plat,
            llm,
            session: Session::holding(retention.session_turns),
            memory,
            scheduler,
            // "(0, 0)": the empty index isn't written over the one on disk
            // while that one is still being read.
            index_on_disk: Some(index.written_as()),
            index,
            index_load,
            awareness: Awareness::default(),
            proactive,
            connections: crate::integrations::dependencies(),
            timing: crate::timing::Recent::default(),
            accounts: crate::accounts::Book::load(&store),
            gestures: crate::handshape::Vocabulary::load(&store),
            code_sets: store.load::<Vec<crate::codes::Set>>("codes"),
            vault_recovery: store.load::<Vec<crate::recovery::Setup>>("recovery"),
            voice_id: crate::voiceid::VoiceId::load(&store),
            last_verdict: crate::voiceid::Verdict::NotEnrolled,
            fit: crate::fit::plan_as_set(&crate::fit::measure(), &cfg.tools.as_ref().map(|t| t.fit.clone()).unwrap_or_default()),
            fit_measured: (crate::fit::measure(), 0),
            eyes: crate::presence::Sensor::new(
                cfg.tools.as_ref().map(|t| t.presence.clone()).unwrap_or_default(),
            ),
            last_sighting: None,
            looked_at: 0,
            watching_me: false,
            dictation: None,
            steering_until: None,
            steering_at: 0,
            hands: None,
            looking: None,
            album: crate::vision::Album::load(&store_for_load2),
            track: crate::handtrack::Track::default(),
            pace: crate::handtrack::Pace::default(),
            steering_hand: crate::gaze::Steering::default(),
            carrying: None,
            carried_from: None,
            pointing_at: None,
            tray: crate::tray::Tray::load(&store),
            earned: crate::earned::Record::load(&store),
            backends: crate::backends::Router::load(&store),
            mcp: crate::mcp::McpHub::new(
                &cfg.tools.as_ref().map(|t| t.mcp.clone()).unwrap_or_default(),
                &store.load::<Vec<String>>(crate::mcp::SWITCHED_OFF),
                &cfg.tools.as_ref().map(|t| t.vars.clone()).unwrap_or_default(),
            ),
            persist_failures: Vec::new(),
            persist_told: false,
            stopped: false,
            missed_beats: 0,
            tools_live: None,
            call_notes: crate::callnotes::Notes::new(
                cfg.tools.as_ref().map(|t| t.call_notes.clone()).unwrap_or_default(),
                store.data_dir().join("calls"),
            ),
            last_call_look: 0,
            working_for_you: Vec::new(),
            next_window_job: WINDOW_JOB_IDS,
            settings_watch: None,
            settings_stamp: 0,
            palette: crate::palette::Recent::load(&store),
            dashboard: crate::dash::Layout::load(&store),
            arranging: false,
            hub_server: None,
            library: crate::recall::Library::default(),
            // Loaded before `reload_library` runs below, so the rebuilt
            // library rehydrates vectors made in an earlier session rather
            // than starting every note over as unembedded.
            meaning: crate::meaning::Remembered::load(&store_for_load),
            search_check_due: store_for_load.load::<Vec<crate::recall::SearchCheck>>("search_checks").is_empty(),
            contents: crate::contents::Contents::default(),
            trace: crate::trace::Trace::default(),
            last_brief: 0,
            // Loaded rather than zeroed: a restart would otherwise give you
            // the day's run a second time, and a brief said twice is the
            // failure `morning_brief` already documents.
            last_brief_at: store_for_load.load::<u64>("last_brief_at"),
            // Loaded too, and for the opposite reason. Zeroed, a restart
            // looks like an infinite gap, so every restart would read as you
            // arriving -- and restarts happen in the middle of the night you
            // are working through.
            last_turn_of_yours: store_for_load.load::<u64>("last_turn_of_yours"),
            mending: store_for_load.load("mending"),
            selfwork: store_for_load.load("selfwork"),
            pending_landing: Vec::new(),
            pending_correction: None,
            last_said: String::new(),
            pending_edit: None,
            outbox: crate::notify::Outbox::load(&store_for_load2),
            audio_devices: None,
            signal_listener: None,
            sync_server: None,
            tier_mix: crate::tier::Mix::default(),
            nudger: {
                // Your goals, kept across a restart (F4): the nudges toward
                // them had nothing to nudge about while this list started
                // empty every time.
                let mut n = crate::nudge::Nudger::new(crate::nudge::NudgeConfig::default());
                n.goals = store_for_load.load(crate::nudge::GOALS);
                n
            },
            store,
            // Kept across a restart: "undo" the morning after is still undo.
            history: store_for_load.load("undo_history"),
            decisions: crate::why::Record::default(),
            offered_handover: false,
            asks_quietly: None,
            // From the install's own state, not the active profile's --
            // see `vault::Vault::FILE`. Loaded rather than defaulted:
            // a `default()` vault has no salt and no check value, so it
            // held nothing across a restart and its passphrase was set
            // afresh by the first unlock of every run.
            vault: crate::vault::Vault::load(&crate::roots::install_state()),
            chats: chats_at_start,
            tries: crate::courier::Tries::default(),
            // Loaded rather than defaulted, for the same reason the vault is:
            // a `default()` `Access` holds no grants, so every restart threw
            // away whatever you had given Atlas and `may_start` could only
            // ever answer "I don't have access to that".
            access: crate::signin::Access::load(&store_for_load),
            notebook: notebook_at_start,
            workday: Default::default(),
            gate: crate::interrupt::Gate::default(),
            workspace: Vec::new(),
            layout: crate::layout_prefs::Layout::default_layout(),
            last_seen: 0,
            carried: Vec::new(),
            proposals,
            calendar: crate::calendar::Calendar::load(&store_for_load),
            daily_history: store_for_load.load("daily_history"),
            rhythm: store_for_load2.load("rhythm"),
            dropped: store_for_load.load("dropped"),
            wants_seen: store_for_load.load("wants_seen"),
            interrupted: Vec::new(),
            signals: Vec::new(),
            run: crate::channel::Run::default(),
            facts: crate::facts::Book::load(&store_for_load),
            known,
            stones: store_for_load.load("known_stones"),
            vocab: store_for_load.load("vocabulary"),
            folding: false,
            running: crate::awake::Running::Awake,
            heard_note: None,
            connections_logged: Vec::new(),
            typebox: None,
            turn_was_typed: false,
            unknown_count: 0,
            utterance_count: 0,
            last_unknown: String::new(),
            autonomy: Autonomy::Supervised,
            followup_secs: 6,
            pending_offer: None,
            pending_wanted: None,
            pending_job: None,
            pending_backlog: None,
            tiers: Tiers::default(),
            power: Power::default(),
            log: Log::new(logs_dir, retention.logs_mb * 1024 * 1024),
            probe: Probe::default(),
            backlog: Backlog::load(&store_for_load),
            person: crate::person::Person::load(&store_for_load),
            publisher: Publisher::load(&store_for_load),
            workshop: crate::workshop::Workshop::load(&store_for_load),
            attention: Attention::default(),
            // Loaded, then `new_session` is run just below so `Session` and
            // spent `Once` grants are dropped and only `Always` survives a
            // restart. Was `Permissions::default()` — a field constructed
            // empty and referenced nowhere, so the whole app-permission gate
            // was dead: an unknown app was never asked about, and an
            // "always allow X" could not have been remembered because
            // nothing was ever written or read.
            permissions: store_for_load.load("permissions"),
            referents: Referents::default(),
            connectivity: Connectivity::default(),
            // From the config, not `default()`. `ToolsConfig.lifecycle`
            // was parsed out of tools.yaml and thrown away: `budget_mb`,
            // `idle_timeout_secs` and `keep_warm_secs` never reached the
            // running Supervisor.
            starts_model_server: false,
            model_look_at: 0,
            talk_queue: Vec::new(),
            pending_turn: None,
            pending_seq: 0,
            also_asked: Vec::new(),
            rephrase_ok: false,
            rephrase_ask: None,
            acting_at: None,
            model_server_seen: Default::default(),
            model_probe_busy: Default::default(),
            model_warmed: Default::default(),
            model_start_tried: None,
            defer_turns: false,
            may_defer: false,
            decided_already: None,
            decided_in_ms: None,
            talk_partial: Default::default(),
            pending_stamp: None,
            last_spoke_at: 0,
            tool_book: crate::intent::ToolBook::new(&cfg.commands),
            by_chat: false,
            model_server_trouble: None,
            helpers: crate::lifecycle::Helpers::new(
                cfg.tools.as_ref().map(|t| t.lifecycle.clone()).unwrap_or_default(),
            ),
            crew: Crew::with_limits(crew_limits).with_room(Box::new(crew_room)),
            last_persist: 0,
            tools_resolved,
            long_work: watching::Watcher::load(&store_for_load),
            crew_links: std::collections::HashMap::new(),
            errand_question: None,
            last_errand_pick: None,
            held_by_pause: Vec::new(),
            last_tidy: 0,
            last_index_check: 0,
            index_drifted: None,
            warned_posts: Vec::new(),
            unsaid: None,
            unfinished: Default::default(),
            redoing: None,
            left_over: store_for_load2.load(crate::resume::RECORD),
            left_windows: store_for_load2.load(crate::resume::WINDOWS),
            saved_windows: String::new(),
            journal_warning: {
                // Against its own heads and every backup's.
                let j = &journal_at_start;
                let backup = cfg
                    .tools
                    .as_ref()
                    .map(|t| t.backup.clone())
                    .unwrap_or_default()
                    .resolved(&store_for_load2.install_root());
                let (sealed, from) = crate::activity::check_with_backups(j, store_for_load2.root(), &backup);
                match sealed {
                    crate::activity::Sealed::Broken { .. } => Some(crate::activity::said_with_backups(&sealed, from)),
                    _ => None,
                }
            },
            // Read once (28 Sep 2026): it was read twice at every start,
            // once for the check above and again for this.
            journal: journal_at_start,
            last_present: crate::store::now(),
            worklog: store_for_load2.load("worklog"),
            input_away: None,
            back_from: None,
            last_beat: 0,
            worked_until: 0,
            last_animation: None,
            away_after: 900,
            pending_brief: None,
            pending_brief_detail: None,
            last_register: crate::register::Register::Working,
            clipboard_text: None,
            clipboard_writeback: None,
            pending_change_effect: None,
            reminded,
            flash_once: Vec::new(),
            hub_jobs: Default::default(),
            hub_after: std::collections::HashMap::new(),
            shown_once: Default::default(),
            pending_clipboard: None,
            last_rehearsal: None,
            wants_panel: None,
            panel_shown_at: 0,
            last_update_check: (0, String::new()),
            sleep_hold: None,
            pending_window_confirm: None,
            pending_panel: None,
            signing_in_waiting: None,
            code_wanted: None,
            pending_security: None,
            after_code: None,
            enrolling: None,
            pending_signin: None,
            guesses_said: None,
            hub_times: HubTimes::default(),
            routines: store_for_load.load("routines"),
            pending_routine: None,
            pending_routine_run: None,
            mail_plans: Vec::new(),
            pending_mail_sort: false,
            pending_post_approval: None,
            pending_post_when: None,
            posting: Vec::new(),
            pending_press: None,
            pending_storage: None,
            pending_undo: None,
            pending_media_keep: None,
            pending_unscanned: None,
            pending_bring_back: None,
            deciding: store_for_load.load::<Option<crate::decide::Decision>>("deciding"),
            pending_decision: None,
            hotkeys: None,
            pending_media_original: None,
            typing_thread: None,
            mic: None,
            mic_asked: false,
            mic_heard: None,
            cut_in_by_voice: None,
            typing_stop: Default::default(),
            typing_busy: Default::default(),
            typing_said: Default::default(),
            mind: Default::default(),
            seen_up_to: store_for_load.load("sync_seen"),
            synclog: {
                // Loaded, then named. A log restored from disk keeps its
                // events; a fresh one gets the device name from your config
                // so that a bundle says which machine it came from rather
                // than arriving anonymous.
                let mut l: crate::sync::Log = store_for_load.load("synclog");
                if l.device.trim().is_empty() {
                    let named = cfg
                        .tools
                        .as_ref()
                        .map(|t| t.sync.name.clone())
                        .unwrap_or_default();
                    l = crate::sync::Log::new(if named.trim().is_empty() {
                        "this device"
                    } else {
                        named.trim()
                    });
                }
                l
            },
            overnight: store_for_load.load("overnight"),
            last_overnight: 0,
            last_sync_check: 0,
            morning_brief: None,
            worked_while: None,
            last_turn_failed: false,
            this_turn_register: None,
            this_turn_cap: None,
            answering: None,
            thread: Thread::load(&store_for_load2),
            // The configured one, not `Persona::default()`.
            //
            // This was `Persona::default()`, which meant the whole `persona:`
            // block in `config/tools.yaml` -- name, address, tone,
            // max_spoken_sentences, wit, argues, converses -- decided
            // nothing. Setting `address: "Eric"` there changed no behaviour
            // anywhere. `voice.rs` carries the field and nothing read it.
            persona: cfg.tools.as_ref().map(|t| t.persona.clone()).unwrap_or_default(),
            modes: Modes::load(&store_for_load2),
            health: Reporter::default(),
            watcher: Watcher::load(&store_for_load2),
            automations: crate::automation::Engine::new(Vec::new(), store_for_load2.load("automations"), 0),
            anticipator: Anticipator::load(&store_for_load2),
            // Was `data/state/trash` — a fourth trash location nobody
            // decided on, invisible to `atlas reclaim`, and it ignored
            // `tools.trash` entirely. Now the one the config names,
            // resolved against this install.
            trash: Trash::new(
                cfg.tools
                    .as_ref()
                    .map(|t| t.trash.clone())
                    .unwrap_or_default()
                    .resolved(&store_for_load2.install_root()),
            ),
            queue: Queue::load(&store_for_load2),
            flows: Library::load(&store_for_load2),
            current_flow: None,
            plugins_dir: crate::plugins::plugins_dir(),
            plugins_kept: crate::plugins::Kept::default(),
            peer_dir: crate::kin::where_pairings_live(),
            friend_host: None,
            tor: None,
            tor_instead: None,
            tor_binary: None,
            tor_connections: Default::default(),
            feedback_draft: None,
            reached: std::sync::Arc::new(crate::kin::Reached::load(&store_for_reached)),
            voice_downloads: std::sync::Arc::default(),
            phone_code: None,
            release_anchor: crate::release::RELEASE_PUBLIC_KEY,
            builds_folder: None,
            phones_heard: std::sync::Arc::default(),
            peer_tries: std::collections::BTreeMap::new(),
            flow_mind: 0,
            last_backup: 0,
            working_since: None,
        };
        // Built here rather than at the two call sites in main.rs, so every
        // path that constructs a Daemon gets a populated library. Loading it
        // only after a research run would have meant notes written in an
        // earlier session were invisible until another one happened — wired,
        // and still not working.
        d.reload_library();
        // The master index, and only the master index. `contents::boot`
        // asserts one thing loaded; reading the notes themselves here would
        // be the eager load the whole module exists to avoid.
        d.load_index();
        // The record has to outlive the process or it answers "what has the
        // model been doing" with "since you started me, nothing much".
        d.trace = crate::trace::open(&d.trace_path());
        // A fresh session drops everything but the "always allow X" grants —
        // a permission you gave for one sitting must not silently outlive it.
        // This is the whole reason `Span::Session` and `Span::Once` exist as
        // separate from `Always`, and until now nothing pruned them.
        d.permissions.new_session();
        d
    }

    // ---------- one spoken turn ----------

    /// How this reached Atlas.
    ///
    /// The distinction the addressing check needed and never got. Saying the
    /// wake word, pressing push-to-talk, or typing into Atlas's own prompt are
    /// all unambiguous: nobody types into Atlas to talk to the person next to
    /// them. Only genuinely ambient audio is worth second-guessing.
    ///
    /// This existed as a field on `addressing::Situation` -- `after_wake_word`
    /// -- and `turn` built that struct with `..Default::default()`, so it was
    /// always false. The wake word was detected upstream and thrown away
    /// before the one thing that needed to know about it, which is why saying
    /// "Atlas" and then "hello" got silence: addressing scored a one-word
    /// greeting as a fragment, called it overheard, and returned nothing.
    pub fn turn(&mut self, said: &str, t: u64) -> String {
        // The parser needs your project names to tell "fix the parser in
        // Homelab" (project work) from "change the volume" (not).
        let names: Vec<String> = self.workshop.projects.iter().map(|p| p.name.clone()).collect();
        self.parser.know_projects(names);
        // The people and habits you have, and the list a number would
        // refer to -- so "open 2" and "I called Sam" read right.
        self.workday_known(t);
        self.turn_from(said, t, Arrival::Directed)
    }

    /// Handle something you said, knowing how it arrived.
    pub fn turn_from(&mut self, said: &str, t: u64, how: Arrival) -> String {
        self.last_said = said.to_string();
        // A question Atlas asked long ago is not what this answers.
        self.expire_stale_question(t);
        // The names "go to", "close" and "how's" may take.
        self.parser.know_names(self.names_for_the_parser());
        // Started now, not waited for: a line the parser settles doesn't
        // need it, and one that does waits for it to finish loading
        // (`models::WaitsForServer`).
        self.keep_model_server(t);
        // You did something, and *when*. This is what the gap in
        // `daily::whereabouts` and `daily::arriving` is measured from, so it
        // decides whether Atlas may get on with an hour of work and whether
        // this is the turn that earns the day's brief.
        //
        // Recorded here rather than in `execute`, where `rhythm.saw` is,
        // because `execute` has no time of its own and reaches for `clock()`.
        // That is right in production and wrong everywhere the time is
        // supplied -- a test driving a night at one in the morning had its
        // gap measured against the real wall clock, came out as zero, and
        // Atlas concluded you were sitting there. Found by the night refusing
        // to run at all.
        self.last_turn_of_yours = t;
        let _ = self.store.save("last_turn_of_yours", &self.last_turn_of_yours);
        // Old conversation folded on every path, not only the ones that
        // reach the end of a turn (H12).
        self.fold_if_due(t);
        // Your words, for the speech model's hints (H11). Saved only when a
        // word it could hint with changed, not on every turn.
        // Which project this was about (H13h), so "what have I been on"
        // and the gone-quiet check have something real to go on.
        if let Some(p) = crate::person::project_named(said, &self.person.projects) {
            self.person.touched(&p, t);
            let _ = self.person.save(&self.store);
        }
        let before = self.vocab.hints(crate::improve::HINTS_GIVEN);
        self.vocab.learn(said);
        if self.vocab.hints(crate::improve::HINTS_GIVEN) != before {
            let _ = self.store.save("vocabulary", &self.vocab);
        }
        // Away is either a silence from you, or — where the machine says how
        // long since the keyboard and mouse were touched — a break it saw.
        // Without the second, typing for an hour without speaking read as an
        // hour away, and earned a "welcome back" at the desk you never left.
        let (away_since, gone) = match self.back_from.take() {
            Some((left, back)) => (left, back.saturating_sub(left)),
            None => (self.last_present, t.saturating_sub(self.last_present)),
        };
        let was_away = gone > self.away_after;
        self.last_present = t;
        if was_away {
            // Anything that could not reach you while you were out is said
            // now. This is the half that makes holding honest -- without it,
            // "held" is a nicer word for dropped.
            let cfg = self.notify_cfg();
            // Peeked, not drained. This turn may still end early — while
            // paused, or when `addressing` decides the words were not meant
            // for Atlas — and draining here would lose them for good.
            let waiting = self.outbox.ready(t, &cfg);

            // The structured list `returning.rs` was designed around.
            //
            // This path used to build a pre-formatted string here --
            // `Journal::brief` for what Atlas did, `notify::spoken` for what
            // could not reach you, concatenated -- which is why
            // `returning::welcome` and `returning::full_brief` sat complete,
            // tested and unreachable for as long as they did: they take a
            // `[Happened]`, and there was nowhere in the running program
            // that had one. Assembling the list first, and letting
            // `returning` decide what to say about it, is what actually
            // wires them in. The two decisions that used to live inline
            // below -- whether a quiet overnight still earns a line, and
            // whether to lead with the greeting -- are `welcome`'s own, and
            // are no longer duplicated here.
            let happened = self.happened_while_away(away_since, &waiting);

            // Your hour, on your clock: the `time_zone` you chose, or this
            // machine's own (`tz::home`). Both chats fixed this separately
            // ("Good morning" arrived at midnight in Pacific time).
            let hour = self.home_zone().hour(t as i64);
            let rcfg = self.returning_cfg();
            let welcome = crate::returning::welcome(gone, &happened, hour, &rcfg);
            // What you were in the middle of: the cue that shortens getting
            // back into it (Trafton & Monk 2007: an explicit reminder of
            // where you were beats none; a subtle one does no better).
            let cue = if self.worklog_cfg().enabled && self.worklog_cfg().resume_cue {
                self.worklog.last_context(away_since).filter(|c| c.until + 600 >= away_since).map(|c| c.cue())
            } else {
                None
            };

            // The brief goes on screen too, so you can read it rather than
            // having to catch it. It does not steal focus -- only an urgent
            // panel does that. What is on the panel is now the same list
            // `welcome` reasoned about, rather than a separate assembly
            // that could drift from what was said out loud.
            let lines: Vec<String> = happened.iter().map(|h| h.what.clone()).collect();
            self.show_panel(crate::window::Panel::Brief, "While you were away", lines);

            // `returning::welcome` leans on the greeting to carry "you have
            // just come back" -- with a name set, "Morning, Eric. Disk is
            // nearly full." says it on its own. But the default address is
            // deliberately nothing at all ("a system that calls you 'sir'
            // uninvited is doing a bit"), and without a greeting a bare
            // "Disk is nearly full." is indistinguishable from something
            // happening right now. That distinction is the entire point of a
            // returning brief: not what happened, but that it happened while
            // you were not looking. The framing is restored here rather than
            // inside `returning.rs` because being on the return path is the
            // daemon's knowledge, not the phrasing layer's.
            let unaddressed =
                rcfg.how_to_address() == crate::returning::Address::None && !happened.is_empty();
            let frame = |s: &String| {
                if unaddressed {
                    format!("While you were away: {s}")
                } else {
                    s.clone()
                }
            };

            self.pending_brief = match &welcome {
                crate::returning::Welcome::Straight(s) => Some(frame(s)),
                crate::returning::Welcome::Offer(s) => Some(frame(s)),
                // `returning` calls a sub-`away_after` absence no absence at
                // all and says nothing, which is right about the journal and
                // wrong about the outbox: something held back *because* you
                // were not there is owed either way, and `away_after` is
                // configurable low enough for the two to genuinely disagree
                // (anything under fifteen minutes reads as `Gone::Moment`).
                // Held things are still said; everything else stays quiet.
                crate::returning::Welcome::Nothing if !waiting.is_empty() => {
                    Some(crate::notify::spoken(&waiting))
                }
                crate::returning::Welcome::Nothing => None,
            };
            if let Some(cue) = cue {
                self.pending_brief = Some(match self.pending_brief.take() {
                    Some(b) => format!("{b} {cue}"),
                    None => cue,
                });
            }

            // An offer is only an offer if "yes" reaches something. Held so
            // the next turn's bare yes runs `full_brief` against this exact
            // list, rather than against whatever has happened since.
            self.pending_brief_detail = match welcome {
                crate::returning::Welcome::Offer(_) => Some(happened),
                _ => None,
            };
        }
        self.awareness.heard_you(t);
        let said = said.trim();
        if said.is_empty() {
            return String::new();
        }

        // Pause and resume are heard before anything else, so they work even
        // mid-task and mid-sentence.
        match hear(said) {
            // "stop everything" / "halt" / "emergency stop" / "drop
            // everything". THIS WAS NOT HANDLED, and it is the one phrase
            // Atlas teaches you by name: `firstrun.rs:181` ends setup with
            // 'Say "what can you do" any time, "stop everything" if I get
            // something wrong.'
            //
            // `attention::hear` recognised it and returned `Heard::Panic`;
            // this match had no arm for it, so it fell through `_ => {}` and
            // carried on to ordinary parsing, where it means nothing.
            // `Attention::halt` -- which empties the queues, abandons the
            // work and sets `halted` so a later resume does not silently
            // restart it -- had **zero callers in src/**. Built, tested,
            // taught to the user, and wired to nothing.
            //
            // A check that is performed and whose result is discarded is
            // worse than no check: it reads as covered.
            // `attention::halt` is the capability built for this and is what
            // `was_halted()` reads so a later resume does not silently
            // restart abandoned work. And it now reaches the crew too: the
            // note that used to sit here said the daemon "does not keep the
            // ids of what is running" -- it does, `self.crew_links` is
            // iterated for exactly that answer in `Intent::Queued` -- and
            // `ask_everyone_to_stop` needs no ids at all. So "stop
            // everything" finally stops an errand already in flight, not
            // just Atlas asking and starting things. The endings still
            // arrive through `settle`, reported rather than swallowed.
            Some(Heard::Panic) => {
                // A model turn still thinking is stopped too: its answer
                // would otherwise run its tool when it came back (28 Sep 2026).
                self.drop_pending_turn(t, "Stopped before I answered -- nothing was done.");
                let asked = self.crew.ask_everyone_to_stop();
                let halted = self.attention.halt(t);
                // `halt`'s own doc says "queues emptied, work abandoned" —
                // and nothing ever emptied this queue, so a later resume
                // quietly restarted the exact work you panicked about.
                // Keyed to `was_halted`, the flag that says a panic
                // happened, not to this arm's position in the file.
                if self.attention.was_halted() {
                    self.queue.tasks.retain(|task| {
                        matches!(
                            task.state,
                            crate::lanes::TaskState::Done | crate::lanes::TaskState::Failed
                        )
                    });
                }
                return if asked > 0 {
                    format!(
                        "{halted} Asked {asked} running errand{} to stop too.",
                        if asked == 1 { "" } else { "s" }
                    )
                } else {
                    halted
                };
            }
            Some(Heard::Pause) => {
                // What was mid-flight is recorded, so resume can name it.
                // `suspended` had no writer: `release()` returned an empty
                // vec at every call site since the day it was written.
                let running: Vec<u64> = self
                    .queue
                    .tasks
                    .iter()
                    .filter(|x| x.state == crate::lanes::TaskState::Running)
                    .map(|x| x.id)
                    .collect();
                for id in running {
                    self.attention.suspend(id);
                }
                self.drop_pending_turn(t, "Paused before I answered -- nothing was done.");
                // "Pause" is total: the errands hold too, at their safe
                // points, and nothing they have done is lost.
                let msg = self.attention.pause(self.current_work(), t);
                let held = self.hold_every_errand();
                return if held > 0 && msg != "Already paused." {
                    format!(
                        "{msg} Holding {held} errand{} where {} — nothing lost.",
                        if held == 1 { "" } else { "s" },
                        if held == 1 { "it is" } else { "they are" }
                    )
                } else {
                    msg
                };
            }
            // "I'm ready" only means resume if something was paused.
            // Otherwise it's you arriving at the desk, which is a different
            // thing entirely.
            Some(Heard::Resume) if self.attention.is_paused() => {
                let m = self.attention.resume(t);
                for id in std::mem::take(&mut self.held_by_pause) {
                    self.crew.resume(id);
                }
                // What was suspended at the pause, named on the way back —
                // the return value both call sites used to discard.
                let held = self.attention.release();
                let names: Vec<String> = held
                    .iter()
                    .filter_map(|id| {
                        self.queue.tasks.iter().find(|x| x.id == *id).map(|x| x.command.clone())
                    })
                    .collect();
                let m = if names.is_empty() {
                    m
                } else {
                    format!("{m} Still in hand: {}.", names.join(", "))
                };
                // If a reply was cut off mid-sentence, "carry on" finishes
                // it — that is what the parking in `say_interruptibly` was
                // for, and until now nothing ever asked for the remainder.
                return match self.finish_saying() {
                    Some(rest) => format!("{m} {rest}"),
                    None => m,
                };
            }
            // "Carry on" with nothing paused still finishes an interrupted
            // reply — stopping Atlas mid-answer does not pause the world,
            // so the remainder must be reachable outside a pause too.
            Some(Heard::Resume) if self.unsaid.is_some() => {
                if let Some(rest) = self.finish_saying() {
                    return rest;
                }
            }
            Some(Heard::Status) => return self.attention.status(t),
            _ => {}
        }
        // "Stop" with several errands going: which one. Above the pause gate
        // so an errand can be picked back up while Atlas itself is paused.
        if let Some(reply) = self.errand_control(said, t) {
            self.thread.append(said, &reply, None, t);
            self.persist();
            return reply;
        }

        // While paused, nothing else gets through — asked of `allows`,
        // the predicate written for exactly this, rather than a second
        // inline reading of the same rule. (Resume and Status returned in
        // the match above; `allows` is what keeps this line and that match
        // telling the same story.)
        if !self.attention.allows(hear(said)) {
            return String::new();
        }

        // While dictating, what you say is content, not commands.
        //
        // This sits above the addressing check deliberately. "Was that meant
        // for Atlas?" is the right question for a wake-word turn and the
        // wrong one here: you started dictation, so everything until you stop
        // it is meant to be typed. Below the pause check, because stopping
        // has to work from inside any mode.
        if self.dictation.is_some() {
            return self.dictated(said, t);
        }

        // Was that even meant for Atlas? Abandoning work because someone
        // walked in is worse than missing one instruction.
        let situation = Situation {
            awaiting_answer: self.session.is_waiting(),
            working: !self.scheduler.due(t).is_empty() || self.queue_is_busy(),
            after_wake_word: how == Arrival::Directed,
            // Atlas spoke to you within the last half minute: this is the
            // conversation carrying on, even when it's about "him" or "them".
            just_spoke: how == Arrival::OpenMic
                && self.last_spoke_at != 0
                && t.saturating_sub(self.last_spoke_at) <= crate::addressing::STILL_TALKING_SECS,
            ..Default::default()
        };
        let judged = assess(said, &situation);
        match respond(&judged, &situation) {
            // Silence is never the answer to something you deliberately sent.
            // Even when Atlas genuinely cannot tell, saying so beats saying
            // nothing -- an assistant that ignores you is indistinguishable
            // from one that has crashed.
            Addressed::Ignore if how == Arrival::Directed => {}
            Addressed::Ignore => return String::new(),
            Addressed::Ask(q) => {
                self.session.ask(&q);
                return q;
            }
            Addressed::Act => {}
        }

        // Some things are not an assistant's to have a go at. A message that
        // reads as a crisis is caught here -- after "was that meant for me?"
        // says yes, and before any mode, saved flow, parser or model can treat
        // it as a command. `person::beyond_me` is the deliberately narrow test
        // for exactly this ("most difficult conversations are just difficult
        // conversations"), and it reached nothing: a line like "I can't go on"
        // fell through to ordinary parsing, where the best case was a blank
        // `Unknown` and the worse case was a phrase-match on the wrong thing.
        // Redirecting to a person -- while still offering to take things off
        // your plate -- is the one right move, and it wins over everything
        // else Atlas might otherwise do with the words.
        if crate::person::beyond_me(said) {
            return crate::person::NOT_A_THERAPIST.to_string();
        }

        // A hard day that is not a crisis. Caught here for the same reason
        // `beyond_me` is -- before any mode, flow, parser or model can treat
        // "rough day, honestly" as a command and search the disk for a file
        // called "rough". The right move is not to redirect and not to
        // perform concern: it is to offer to take real work off your plate,
        // and the things it offers are the outstanding backlog items, the
        // concrete work Atlas could actually pick up rather than an empty
        // gesture. With nothing outstanding it says little and does not
        // pretend otherwise -- which is exactly what `hard_day([])` is for.
        if crate::person::having_a_hard_time(said) {
            let can_take_on: Vec<String> = self
                .backlog
                .outstanding()
                .iter()
                .map(|i| i.request.trim().to_string())
                .collect();
            let reply = crate::person::hard_day(&can_take_on);
            self.thread.append(said, &reply, Some("hard day".to_string()), t);
            self.persist();
            return reply;
        }

        // A named mode wins before anything else parses it as a command.
        if let Some(m) = self.modes.match_trigger(said).map(|m| m.name.clone()) {
            if self.modes.active().map(|a| a.name != m).unwrap_or(true) {
                if let Some(tr) = self.modes.enter(&m, &[]) {
                    self.thread.append(said, &tr.say, Some(format!("{m} mode")), t);
                    self.persist();
                    return tr.say;
                }
            }
        }

        // A saved sequence you named earlier.
        //
        // Run as a `flow::Run`, not flattened into the queue. The flatten
        // shape discarded `on_fail` and `produces` entirely — an optional
        // step stopped the chain, a retrying one never retried, and a step
        // using `{name}` from an earlier one ran with the braces still in
        // it. The run also gives the mind something true to say: this is
        // the first place `Mind` has ever been handed work, which is why
        // "what are you doing?" could only ever answer "nothing".
        if let Some(w) = self.flows.match_trigger(said).map(|w| w.name.clone()) {
            if let Some(f) = self.flows.get(&w).cloned() {
                return self.start_flow(f, None, Some(said), t);
            }
        }

        // An add-on's sequence (`plugins`). After your own, so one you saved
        // always wins over one an add-on declared -- and never while Atlas is
        // waiting on an answer from you, so nothing you say to answer it can
        // start an add-on instead.
        if matches!(self.session.pending, Pending::Nothing) {
            // Kept between turns, read again only when the folder changes.
            let found = crate::plugins::Registry::load_kept(&mut self.plugins_kept, &self.plugins_dir, &self.cfg.commands, &self.store)
                .match_trigger(said);
            if let Some((id, f)) = found {
                return self.start_flow(f, Some(id), Some(said), t);
            }
        }

        // Being spoken to like a person, answered like one.
        //
        // This sits above reference resolution, which is where it has to be:
        // "you there" contains a pronoun, so the resolver claimed it and asked
        // "Which one?". It is also above the policy gate, because greeting
        // Atlas is not an action that needs approving -- below it, "hello"
        // came back as "I didn't catch that. Go ahead?", which treats a
        // greeting as a failed command.
        //
        // Checked against what was said rather than a parsed intent: the
        // phrase parser matches loosely enough that half of these never reach
        // `Intent::Unknown`. The word cap and exact-match lists in
        // `social_reply` are what stop a real instruction being swallowed as a
        // pleasantry.
        if let Some(reply) = crate::persona::social_reply(said, crate::localclock::hour_here(t) as u8) {
            self.thread.append(said, &reply, None, t);
            self.persist();
            return reply;
        }

        // "Argue the other side" -- above reference resolution for the same
        // reason: "argue the other side" names "the other" and the resolver
        // claimed it and asked "Which one?". Only an explicit request
        // (`otherside::is_asked_for`) reaches it, and never for a guest.
        if !self.handover().stance.handed_over() {
            if let Some(reply) = self.other_side(said) {
                self.thread.append(said, &reply, None, t);
                self.persist();
                return reply;
            }
        }

        // "Make it faster" just after an animation: "it" is that animation.
        // Above reference resolution, which would otherwise ask "Which one?"
        // about a referent that is plain from what just happened.
        if !self.handover().stance.handed_over() {
            if let Some(reply) = self.refine_animation(said, t) {
                self.thread.append(said, &reply, None, t);
                self.persist();
                return reply;
            }
        }

        // "close it" -> "close chrome", or a question if there is no referent.
        //
        // Only when the phrase alone doesn't already say what to do. "undo
        // that" contains a pronoun but is complete on its own, and asking
        // "which one?" about it would be absurd.
        // "Complete" means more than "it parsed". "close it" parses fine and
        // then tries to close an app called "it", so an intent whose argument
        // is itself a pronoun still needs resolving.
        let parsed = self.parser.parse(said);
        let already_clear = !matches!(parsed, Intent::Unknown(_))
            && !argument_of(&parsed)
                .map(|a| crate::references::has_pronoun(&a))
                .unwrap_or(false);
        // An answer to a question Atlas asked in its own words ("change it
        // to …", a decision's next move) is that answer, pronouns and all.
        let answering = matches!(self.session.pending, Pending::Clarification(_))
            && (self.pending_post_approval.is_some() || self.pending_post_when.is_some() || self.pending_decision.is_some());
        let said_owned;
        let said = if already_clear || answering {
            said
        } else {
            match resolve(said, &self.referents) {
            Resolution::Unchanged(t) => {
                said_owned = t;
                said_owned.as_str()
            }
            Resolution::Resolved { text, .. } => {
                said_owned = text;
                said_owned.as_str()
            }
            // Not understood as a command either way: "which one?" would
            // be a question about nothing ("I'm so tired of this" got "Which
            // one?", 26 Sep 2026). Taken as said.
            Resolution::Ambiguous(_) if matches!(parsed, Intent::Unknown(_)) => said,
            Resolution::Ambiguous(q) => {
                self.session.ask(&q);
                return q;
            }
            }
        };

        // A parked approval takes precedence: the next thing you say is an
        // answer, not a new command.
        if let Pending::Approval(intent, description) = self.session.pending.clone() {
            let kind = kind_of(&intent).to_string();
            // A question you were asked is not a question anyone else may
            // answer. Atlas asks "go ahead?", you hand the laptop over, and
            // the next person says "yes" -- so the answer is dropped along
            // with the question.
            //
            // Ahead of `record_approval` on purpose: a yes recorded here
            // becomes a standing grant, and a standing grant is how the
            // *next* one of these gets waved through without being asked at
            // all. A stranger's yes must not teach Atlas anything about what
            // you approve of.
            if let Some(refusal) = self.handed_over_refusal(&intent) {
                self.session.pending = Pending::Nothing;
                self.pending_job = None;
                return refusal;
            }
            // A permission question is answered with a breadth, not just a
            // yes: "yes, always" records an `Always` grant, "just this
            // session" a `Session` one, a bare "yes" a `Once`. Recorded
            // BEFORE the action runs, so the gate's re-check finds the grant
            // and does not ask the same question again. Any app action the
            // grants gate parked comes back through here.
            if let Some((app, action)) = app_action_of(&intent) {
                if let Some(span) = crate::grants::span_from_answer(said) {
                    self.permissions.grant(&app, Some(&action), span, t);
                    let _ = self.store.save("permissions", &self.permissions);
                    self.session.pending = Pending::Nothing;
                    self.memory.record_approval(&kind, true, None);
                    let reply = self.execute(&intent);
                    self.session.record(said, &intent, &reply);
                    self.persist();
                    return reply;
                }
                // Not an affirmative answer — fall through to the ordinary
                // no-handling below, which learns from the refusal.
            }
            let yes = is_yes(said);
            self.session.pending = Pending::Nothing;
            // A yes or a no answers it. Anything else is something new: the
            // question is dropped and what you said is taken as itself. It
            // used to be "Left it alone." -- a no -- and the new request was
            // lost with the question (27 Sep 2026).
            if yes || is_no(said) {
                self.memory.record_approval(&kind, yes, None);
                let reply = if yes {
                    let r = self.execute(&intent);
                    // If this was a scheduled job asking, close it out too.
                    if let Some(jid) = self.pending_job.take() {
                        self.scheduler.approve(jid);
                        self.scheduler.complete(jid, t, &r, !r.starts_with("error"));
                    }
                    r
                } else {
                    self.pending_job = None;
                    // Worth learning once, per its own doc comment -- not a rule
                    // inferred from a single no, just a pattern that starts
                    // counting. `notice` is the same dedup-by-text used for every
                    // other trait, so saying no to the same kind of thing twice
                    // is what makes it `confident()`, not this one refusal alone.
                    let (what, k) = crate::person::learn_from_refusal(&description);
                    self.person.notice(&what, k, t);
                    "Left it alone.".into()
                };
                self.session.record(said, &intent, &reply);
                self.persist();
                return reply;
            }
            self.pending_job = None;
        }

        // A workflow paused mid-chain for your yes.
        //
        // Checked before the offer path: a flow awaiting approval and a
        // pending offer are never on the table at once, but if they ever
        // were, the flow asked most recently and the answer belongs to it.
        if let Pending::Clarification(_) = self.session.pending.clone() {
            let awaiting = self
                .current_flow
                .as_ref()
                .is_some_and(|r| r.state == crate::flow::RunState::AwaitingApproval);
            if awaiting {
                self.session.pending = Pending::Nothing;
                // "Always": a yes, and for an add-on's step, a standing one --
                // you decided once, so you are not asked again every run.
                let always = crate::session::is_always(said);
                let mut trusted_note = String::new();
                if always {
                    if let Some(run) = self.current_flow.as_ref() {
                        if let (Some(id), Some(step)) = (run.plugin.clone(), run.current().map(|s| s.command.clone())) {
                            trusted_note = match crate::plugins::trust_step(
                                &self.store,
                                &self.plugins_dir,
                                &self.cfg.commands,
                                &id,
                                &step,
                            ) {
                                Ok(_) => format!("I won't ask about \"{step}\" again. "),
                                Err(why) => format!("{why} "),
                            };
                        }
                    }
                }
                if is_yes(said) || always {
                    // Approval brings the job back to the front, wherever an
                    // intervening request pushed it.
                    self.mind.promote(self.flow_mind);
                    // The yes covers THE STEP IT WAS ASKED ABOUT. Handing it
                    // back to the policy gate would classify the same step
                    // into the same question, forever.
                    self.approved_flow_step(t);
                    let reply =
                        self.drive_flow(t).unwrap_or_else(|| "Carrying on.".into());
                    let reply = format!("{trusted_note}{reply}");
                    self.persist();
                    return reply;
                }
                if let Some(run) = self.current_flow.as_mut() {
                    // Refused mid-chain: the rest is abandoned rather than
                    // half-done, which is `deny`'s whole contract.
                    run.deny();
                }
                let reply = self
                    .drive_flow(t)
                    .unwrap_or_else(|| "Alright, leaving the rest.".into());
                self.persist();
                return reply;
            }
        }

        // A pending offer works the same way.
        let mut offer_dropped = false;
        if let Pending::Clarification(q) = self.session.pending.clone() {
            if let Some(offer) = self.pending_offer.clone() {
                let yes = is_yes(said);
                self.session.pending = Pending::Nothing;
                self.pending_offer = None;
                if yes {
                    self.proactive.record_response(&offer.kind, yes, &mut self.memory);
                    let reply = self.run_command(&offer.command, t);
                    self.persist();
                    return reply;
                }
                if is_no(said) {
                    self.proactive.record_response(&offer.kind, false, &mut self.memory);
                    self.persist();
                    return "Alright.".into();
                }
                // Neither: the offer is dropped unanswered and this is taken
                // as a new request ("want me to look it up?" -- "what about
                // Tuesday?"), rather than read as a no and lost.
                offer_dropped = true;
            }
            // A decision being worked (H9): the answer goes to its move.
            if let Some(waiting) = self.pending_decision.take() {
                self.session.pending = Pending::Nothing;
                let l = said.trim().to_lowercase();
                if ["stop", "cancel", "never mind", "nevermind", "forget it", "not now", "later"].iter().any(|w| l == *w || l.starts_with(&format!("{w} "))) {
                    return "Alright, I've put the decision aside. \"Back to the decision\" picks it up where we were.".into();
                }
                return match waiting {
                    None if is_yes(said) => self.deciding.as_ref().and_then(|d| d.lean().ok()).map(|l| l.written()).unwrap_or_else(|| "There's no working to show yet.".into()),
                    None => "Alright.".into(),
                    Some(m) => {
                        if let Some(d) = self.deciding.as_mut() {
                            d.take_answer(m, said);
                        }
                        self.say_the_decision()
                    }
                };
            }
            // A dropped task: back on the list on a yes (H8).
            if let Some(title) = self.pending_bring_back.take() {
                self.session.pending = Pending::Nothing;
                if !is_yes(said) {
                    return "Alright, it stays dropped.".into();
                }
                return self.bring_back(&title, t);
            }
            // A file the scan couldn't check: open it anyway only on a yes (H3).
            if let Some((what, path)) = self.pending_unscanned.take() {
                self.session.pending = Pending::Nothing;
                if !is_yes(said) {
                    return "Alright, I've left it unopened.".into();
                }
                // Off the loop like the first ask (`file_work_off_the_loop`).
                let job = if what == "unzip" { FileJob::Unzip } else { FileJob::Read };
                return self.file_work_off_the_loop(job, &path, true);
            }
            // An edited video: keep it, and then the original (G8).
            if let Some((original, copy, result)) = self.pending_media_keep.take() {
                self.session.pending = Pending::Nothing;
                let _ = std::fs::remove_file(&copy);
                if !is_yes(said) {
                    let _ = std::fs::remove_file(&result);
                    return "Alright — I've thrown the edit away. Your original is untouched.".into();
                }
                // Removing an original is the consequential kind of media
                // step whoever does it (`media_decision`), so it's asked.
                let d = crate::categories::media_decision(crate::categories::MediaOp::Export, false, true);
                if d.needs_consent() {
                    let q = format!("Kept. Do you want me to get rid of the original ({original})?");
                    self.session.ask(&q);
                    self.pending_media_original = Some(original);
                    return q;
                }
                return "Kept.".into();
            }
            if let Some(original) = self.pending_media_original.take() {
                self.session.pending = Pending::Nothing;
                if !is_yes(said) {
                    return "Alright, the original stays.".into();
                }
                return match self.trash.take(std::path::Path::new(&original), "replaced by the edited version, on your say-so") {
                    Ok(_) => "Done — the original is in my trash, so \"undo\" brings it back while it's there.".into(),
                    Err(e) => format!("I couldn't remove the original: {e}. It's still where it was."),
                };
            }
            // "Undo X?" — yes carries it out (G6).
            if let Some(id) = self.pending_undo.take() {
                self.session.pending = Pending::Nothing;
                if !is_yes(said) {
                    return "Left as it is.".into();
                }
                return self.carry_out_undo(id);
            }
            // Moving big folders to another drive (G5).
            if let Some(plan) = self.pending_storage.take() {
                self.session.pending = Pending::Nothing;
                if !is_yes(said) {
                    return "Alright, nothing moved.".into();
                }
                return self.carry_out_storage_plan(plan, t);
            }
            // A button that can't be undone (G4).
            if let Some((win, name, app)) = self.pending_press.take() {
                self.session.pending = Pending::Nothing;
                if !is_yes(said) {
                    return "Alright, I didn't press it.".into();
                }
                return self.press_now(crate::platform::WindowId(win), &name, &app);
            }
            // A post's approval: yes, then when (G2). The approval is only
            // given once there's a time, so a yes never posts on the spot.
            if let Some(id) = self.pending_post_approval.take() {
                self.session.pending = Pending::Nothing;
                // "Change it to …": your edit of the words, asked about again,
                // and learned from (H13g).
                let l = said.to_lowercase();
                if let Some(new) = ["change it to ", "make it read ", "make it ", "change the words to "]
                    .iter()
                    .find_map(|p| l.find(p).map(|i| said[i + p.len()..].trim().trim_matches('"').to_string()))
                    .filter(|n| !n.is_empty())
                {
                    let before = self.publisher.get(id).map(|p| p.body.clone()).unwrap_or_default();
                    self.learned_from_your_edit(&before, &new, t);
                    if self.publisher.edit(id, &new) {
                        if let Some(q) = self.publisher.request_approval(id) {
                            let _ = self.publisher.save(&self.store);
                            self.session.ask(&q);
                            self.pending_post_approval = Some(id);
                            return q;
                        }
                    }
                    return "I couldn't change that one.".into();
                }
                if !is_yes(said) {
                    return "Alright — it stays a draft.".into();
                }
                self.pending_post_when = Some(id);
                let q = "When should it go? Right now, or a time like at 6pm or tomorrow at 9.".to_string();
                self.session.ask(&q);
                return q;
            }
            if let Some(id) = self.pending_post_when.take() {
                self.session.pending = Pending::Nothing;
                return self.schedule_post_at(id, said, t);
            }
            // "Go" on a mailbox rehearsal.
            if self.pending_mail_sort {
                self.pending_mail_sort = false;
                self.session.pending = Pending::Nothing;
                let go = is_yes(said) || said.trim().eq_ignore_ascii_case("go");
                return if go {
                    self.apply_mail_plan(None, t)
                } else {
                    self.mail_plans.clear();
                    "Alright, I've left your mailbox as it is.".into()
                };
            }
            // A routine asked about, or one that asks before running.
            if let Some(reply) = self.answer_about_routine(said, t) {
                return reply;
            }
            // A security change read back: your yes makes it, anything
            // else doesn't. `confirmed::answer` is stricter than `is_yes`
            // on purpose: a mumble is a no here.
            if let Some(asked) = self.pending_security.take() {
                self.session.pending = Pending::Nothing;
                return match crate::confirmed::answer(said, &asked) {
                    crate::confirmed::Step::Go { asked } => self.make_security_change(asked, t),
                    crate::confirmed::Step::Dropped => "Left alone.".into(),
                    _ => "I needed a yes or a no on that one, so I've left it alone. Ask me again if you want it.".into(),
                };
            }
            // "Sign you in? You've asked me to check first."
            if let Some((site, account)) = self.pending_signin.take() {
                self.session.pending = Pending::Nothing;
                if !is_yes(said) {
                    return "Alright.".into();
                }
                return self.start_sign_in(&site, &account, t);
            }
            // A window job that asked before going on: yes carries it on,
            // no stops it.
            if let Some(id) = self.pending_window_confirm.take() {
                self.session.pending = Pending::Nothing;
                let yes = is_yes(said);
                if let Some(i) = self.working_for_you.iter().position(|w| w.id == id) {
                    let app = self.working_for_you[i].job.app.clone();
                    if yes {
                        let w = &mut self.working_for_you[i];
                        w.job.confirmed();
                        w.held = false;
                        w.looked = 0;
                        return format!("Carrying on in {app}.");
                    }
                    self.working_for_you[i].job.refused();
                    self.working_for_you.remove(i);
                    return format!("Alright, I've stopped working {app}.");
                }
                return "That one's already finished.".into();
            }
            // "Show it on your only screen?" — asked when a panel would sit
            // over what you're working on, and until now left with nothing
            // to answer it.
            if let Some(panel) = self.pending_panel.take() {
                self.session.pending = Pending::Nothing;
                if !is_yes(said) {
                    return "Alright.".into();
                }
                let monitors = self.plat.monitors().unwrap_or_default();
                let cfg = self.panel_cfg();
                if crate::panel::place_anyway(panel, &monitors, &cfg).is_none() {
                    return "I can't find a screen to put it on.".into();
                }
                self.wants_panel = Some(panel);
                self.draw_panel(panel);
                let content = match panel {
                    crate::panel::Panel::Tasks => self.backlog.summary(),
                    crate::panel::Panel::Mind => self.mind_summary(),
                    _ => String::new(),
                };
                return crate::panel::narration(panel, &content, &cfg).unwrap_or_else(|| "It's up.".into());
            }
            // "What should I have done instead?" -- the answer is the fix,
            // and it belongs to the correction that asked, not to whatever
            // the parser would make of it on its own. Checked before the
            // other pending questions because a correction is the only one of
            // them whose answer is deliberately free text.
            if self.pending_correction.is_some() {
                self.session.pending = Pending::Nothing;
                let now = crate::store::now();
                let reply = self.correction_wanted(said, now);
                self.persist();
                return reply;
            }
            // Same shape, for "do you want me to think about it with you,
            // or just listen?" -- resolved against what was originally
            // said, never against the answer to the meta-question itself.
            // Something that isn't an answer to it -- a new question, a
            // command -- is taken as itself, not as a confused answer
            // ("look into Rome" after "…or just listen?" got "Not sure which
            // you meant", 26 Sep 2026).
            let answered_it = crate::wanted::answer_to_ask(said).is_some();
            if let (Some(original), true) = (self.pending_wanted.clone(), answered_it) {
                self.session.pending = Pending::Nothing;
                self.pending_wanted = None;
                return match crate::wanted::answer_to_ask(said) {
                    Some(crate::wanted::Wanted::Hearing) => {
                        format!("{} {}", crate::wanted::heard(&original), crate::wanted::then_offer())
                    }
                    // Not `run_command` -- the original words would parse as
                    // `Intent::Unknown` again, `wanted::read` would read the
                    // same way a second time, and this would ask the same
                    // question forever. A note lookup is the one thing that
                    // can genuinely answer it; short of that, say so plainly
                    // rather than loop.
                    Some(_) => self.from_notes(&original, t).unwrap_or_else(|| {
                        "I don't have anything specific on that, but go ahead and tell me more."
                            .into()
                    }),
                    None => "Not sure which you meant -- I'll leave it there.".into(),
                };
            }
            if self.pending_wanted.take().is_some() {
                self.session.pending = Pending::Nothing;
            }
            // A backlog offer -- "Earlier you asked me to X but Y. Want me to
            // do it now?" -- and this is the yes or no. A yes runs the
            // original request; a no is the answer `backlog::dismiss` was
            // written for: the item stays on the record but is never raised
            // again. Without this a no only cleared the question, so
            // `next_offer` raised the same task on the next quiet tick and
            // "no" meant "ask me again later" forever. Anything that is
            // neither drops the offer and is handled from scratch.
            if let Some(id) = self.pending_backlog {
                if is_yes(said) {
                    self.pending_backlog = None;
                    self.session.pending = Pending::Nothing;
                    let request = self
                        .backlog
                        .items
                        .iter()
                        .find(|i| i.id == id)
                        .map(|i| i.request.clone());
                    if let Some(request) = request {
                        let reply = self.run_command(&request, t);
                        self.backlog.complete(id);
                        let _ = self.backlog.save(&self.store);
                        self.persist();
                        return reply;
                    }
                } else if is_no(said) {
                    self.pending_backlog = None;
                    self.session.pending = Pending::Nothing;
                    self.backlog.dismiss(id);
                    let _ = self.backlog.save(&self.store);
                    self.persist();
                    return "Alright, I'll leave that one off your list.".into();
                } else {
                    // The reply is about something else. The offer has already
                    // been raised; drop it rather than act on an ambiguous word.
                    self.pending_backlog = None;
                }
            }
            // Atlas asked something and this is the answer. Carry the
            // question into the turn.
            //
            // This was `let _ = q;` -- the question was **discarded**,
            // `pending` was cleared, and the answer fell through to
            // `run_command` to be parsed from scratch by a parser that had no
            // idea a question had been asked. So Atlas would ask "which
            // browser did you mean?", hear "the second one", and start again
            // from nothing.
            //
            // `ask` is the action the schema tells the model to use whenever
            // a request is ambiguous, so this is not a rare path: it is the
            // one the prompt actively steers toward, and it could not
            // complete. Two turns that read as one exchange to you were two
            // unrelated events to Atlas, which is most of what makes a
            // conversation feel like it is not happening.
            //
            // Cleared before running, so a question that somehow asks itself
            // again cannot loop.
            self.session.pending = Pending::Nothing;
            if !q.trim().is_empty() && !offer_dropped {
                self.answering = Some(q);
            }
        }

        // "Updates on the index and the backup when you want them." — and
        // then you said yes.
        //
        // This sits below every other pending answer on purpose: an offered
        // brief is the weakest claim on a bare yes there is, and anything
        // genuinely awaiting one has already returned above. It is also
        // deliberately *not* routed through `Pending::Clarification` like the
        // others: that machinery parks a question and treats your next
        // utterance as its answer, which is precisely what "when you're
        // ready" promised not to do — the offer rides along on a reply you
        // asked for, and you may well have come back to do something else.
        //
        // `pending_brief.is_none()` is what proves the offer was actually
        // delivered. Both are set on the same turn, and without this check a
        // "yes" that happened to be the first thing said after an absence
        // would answer an offer you had not heard yet, leaving the offer
        // itself queued to surface later against nothing.
        if self.pending_brief.is_none() {
            if let Some(happened) = self.pending_brief_detail.take() {
                if is_yes(said) {
                    return crate::returning::full_brief(&happened);
                }
                if is_no(said) {
                    return "Alright.".into();
                }
                // Anything else means you came back to do something specific,
                // which is the case the offer exists for. The list is dropped
                // rather than held: by the time you next say a bare yes it
                // will be about something else entirely, and answering it
                // with a stale brief is worse than not offering.
            }
        }

        // A bare yes/no with nothing pending is an acknowledgement, not a
        // command. Without this it parses as Unknown, gets gated, and parks a
        // question — which then eats your next real instruction as its answer.
        //
        // Unless one of round 11's tools just asked ("probably $7.75 -- keep
        // it?"): then the yes or no is its answer (`workday::read_first`).
        if (is_yes(said) || is_no(said)) && !matches!(self.parser.parse_named(said).0, Intent::Receipt(_) | Intent::TradeDay(_) | Intent::Cards(_)) {
            return "Nothing to confirm.".into();
        }

        // Naming a tool as the instrument of the task IS the permission for
        // it — "use Excel to build that sheet" grants Excel for this task, so
        // the gate does not then stop to ask about the very app you just told
        // it to use. `grant_in_instruction` is refusal-aware ("don't use
        // Discord" grants nothing) and only fires on a deliberate lead, not a
        // passing mention.
        if let Some((app, span)) = crate::grants::grant_in_instruction(said, &self.known_app_names()) {
            self.permissions.grant(&app, None, span, t);
        }

        // The Talk page and the voice loop let this turn's model call run on
        // a worker (`pending_turn`); nothing else does.
        self.may_defer = self.defer_turns;
        let reply = self.run_command(said, t);
        self.may_defer = false;
        reply
    }

    /// Everything Atlas can answer from what it already holds, before any
    /// model: reminders (B3), what you've said you want (B2), a correction or
    /// fact you've stated, your notes, and the rest. `None` when none of it does.
    fn answer_locally(&mut self, raw: &str, t: u64) -> Option<String> {
        self.remind_help(raw, t)
            .or_else(|| self.spot_opportunity(raw))
            .or_else(|| self.learn_stated(raw))
            .or_else(|| self.from_notes(raw, t))
            .or_else(|| self.ways_in_help(raw))
            .or_else(|| self.decision_help(raw))
            .or_else(|| self.knew_once_help(raw))
            .or_else(|| self.wanted_check(raw))
    }

    /// The part of `answer_locally` that is a real answer from what Atlas
    /// holds, asked before the model: a reminder, a fact you stated or asked
    /// about, your notes. Not the "think it through with you, or just
    /// listen?" check or the want-weighing, which are for when there's no
    /// model to hold a conversation.
    ///
    /// Since 27 Sep 2026 only the things that ARE answers go here: a reminder
    /// set, a correction you opened with, a fact asked for by its exact slot
    /// ("what's the wifi password"), and the decision and ways-in helpers when
    /// the sentence opens with them. Your notes, the fact book's looser
    /// matches and what Atlas once knew go to the model as hints
    /// (`notes_as_hints`) instead of answering on their own -- a note that
    /// shared one word with "what should I eat" was the whole reply.
    fn answer_before_the_model(&mut self, raw: &str, t: u64) -> Option<String> {
        self.remind_help(raw, t)
            .or_else(|| self.learn_stated(raw))
            .or_else(|| self.exact_fact(raw, t))
            .or_else(|| if opens_with_ways(raw) { self.ways_in_help(raw) } else { None })
            .or_else(|| if opens_with_deciding(raw) { self.decision_help(raw) } else { None })
    }

    /// A fact asked for by its exact slot: "what kind of car do I have".
    fn exact_fact(&self, question: &str, now: u64) -> Option<String> {
        if self.handover().stance.handed_over() {
            return None;
        }
        self.facts.slot_answer(question, now).map(|f| f.answer(now))
    }

    /// The date and time, what's running, what setup still lacks, and what
    /// Atlas knows about itself for this question: the model was told none
    /// of it before 27 Sep 2026, so it couldn't say what day it was, what
    /// Atlas can do, or where anything is.
    fn about_now(&self, said: &str, t: u64) -> String {
        let off = crate::localclock::offset_secs();
        let (year, _, _) = crate::hubpages::ymd(crate::localclock::day(t, off));
        let root = self.store.install_root();
        let missing: Vec<&str> = crate::getpieces::setup_pieces()
            .iter()
            .filter(|p| !crate::getpieces::have(p, &root))
            .map(|p| p.name)
            .collect();
        let research = self.tools_ref().is_some_and(|tc| tc.research.enabled);
        format!(
            "Now: {} ({year}).\nRunning here: the language model{}; looking things up on the web is {}.\n{}{}",
            crate::localclock::spoken_now(t, off),
            if missing.is_empty() { String::new() } else { format!("; setup hasn't fetched yet: {}", missing.join(", ")) },
            if research { "on" } else { "off (Settings can turn it on)" },
            // Only for a question about Atlas: headed "answer only from these
            // lines", it made a small model say "not sure" to everything else
            // (27 Sep 2026).
            if crate::capability::is_about_atlas(said) { crate::capability::about_atlas(said, 6) } else { String::new() },
            "",
        )
    }

    fn run_command(&mut self, said: &str, _t: u64) -> String {
        // Named modes and flows are already checked above this call, so the
        // RunNamed branch here rarely fires first -- what this adds is the
        // FromReport branch and the running Mix, which is the only way to
        // notice a daemon that is quietly spending a real model call on
        // questions its own reports already answered.
        //
        // reports is empty until something actually populates a cache of
        // prior work Atlas can answer from -- that cache does not exist yet,
        // so FromReport cannot fire in practice today. Tracked honestly
        // rather than faked: worth_saying() already says so once there is
        // enough history to judge by.
        let named: Vec<String> = self
            .modes
            .modes
            .iter()
            .map(|m| m.name.clone())
            .chain(self.flows.workflows.iter().map(|w| w.name.clone()))
            .collect();
        // Finishing a turn whose model call ran on a worker (`pending_turn`):
        // everything before the call already happened when it started.
        let resumed = self.decided_already.is_some();
        if !resumed {
            self.also_asked.clear();
        }
        let tier = crate::tier::tier_for(said, &named, &[], _t);
        if !resumed {
            self.tier_mix.note(&tier);
        }

        // Read before `context` takes it: the conversation path says it too.
        let answering = self.answering.clone();
        let ctx = format!("{}{}", self.context(), self.about_now(said, _t));

        // Atlas's own answers before the model's (Eric, 27 Sep 2026: "Atlas
        // did not seem that smart"). Reminders, facts you've told it, your
        // notes and the rest were asked only when there was NO model -- with
        // one, every unrecognised sentence went straight to the model, so
        // "remind me in 20 minutes" got a "sure" and nothing was set.
        let mut local = if !resumed
            && self.llm.is_some()
            && matches!(self.parser.parse(said), Intent::Unknown(_))
            && !self.handover().stance.handed_over()
        {
            self.answer_before_the_model(said, _t)
        } else {
            None
        };

        // What kind of moment this is, worked out BEFORE the model is asked.
        //
        // `register::read` was called further down, after `run_command` had
        // already produced the reply, so it could only trim what came back --
        // a `Chatting` register let eight sentences through from a model that
        // had been told to produce one short one. Reading it here is what
        // lets the register choose the *instructions* rather than just the
        // scissors.
        //
        // `after_a_failure` keys off the PREVIOUS turn rather than this one,
        // which is the only thing it can do here and is also the more
        // truthful reading: somebody is short with Atlas because the last
        // thing went wrong, not because this one is about to.
        let register = crate::register::read(
            said,
            &crate::register::Moment {
                recent: self.thread.recent.iter().rev().take(3).map(|e| e.said.clone()).collect(),
                after_a_failure: self.last_turn_failed,
                busy: self.queue_is_busy(),
            },
        );
        // One length, decided once, used for BOTH the briefing and the
        // trimming.
        //
        // Three caps stack here: `persona.max_spoken_sentences` from the
        // config, `modes::sentences_for(verbosity)` from the active mode, and
        // `register.length()`. The shaping step further down set
        // `persona.max_spoken_sentences = mode_cap.min(register.length())`,
        // overwriting the config value -- so the number the model was told
        // ("At most {n} sentences", rendered by `Persona::system_prompt`) and
        // the number it was cut at were computed from different things.
        //
        // Told eight and cut at three is the same failure as being told one
        // and allowed eight, which is the thing this whole pass is about. So
        // the cap is resolved here, written onto the persona that goes to the
        // model, and the shaping step reuses it rather than recomputing.
        //
        // `min` of all three: a mode that asks for brevity beats a chatty
        // register, and the config ceiling beats both.
        let mut persona = self.persona_now();
        // Only a mode somebody actually turned on gets to cap this. See
        // `Modes::verbosity_if_set`.
        // `.map(|v| f(v))` rather than `.map(f)`, and not as a style choice:
        // every reachability guard in this tree finds a call by looking for
        // `name(` (`tests/common/mod.rs`'s `called_names`), so a function
        // passed as a VALUE is invisible to all of them. Written the tidy
        // way, this made `modes::sentences_for` -- called right here -- count
        // as reached only by tests.
        let mode_cap = self
            .modes
            .verbosity_if_set()
            .map(|v| crate::modes::sentences_for(v))
            .unwrap_or(usize::MAX);
        persona.max_spoken_sentences =
            persona.max_spoken_sentences.min(mode_cap).min(register.length()).max(1);
        self.this_turn_cap = Some(persona.max_spoken_sentences);
        // Timed here rather than inside `decide`, because what matters is how
        // long *you* waited, which includes reaching the model and parsing
        // what came back -- not the part of it the model spent thinking.
        self.this_turn_register = Some(register);
        let started = std::time::Instant::now();
        // "What do you know about black holes": a notes question the notes
        // can't answer, so the model answers it rather than "Nothing in my
        // notes on black holes" (27 Sep 2026).
        let beyond_the_notes = !resumed && self.llm.is_some() && self.notes_have_nothing_on(said);
        if !resumed {
            self.by_chat = false;
        }
        let decision = match self.decided_already.take() {
            Some(d) => d,
            None => match self.llm.clone() {
                _ if local.is_some() => brain::Decision {
                    intent: Intent::Unknown(said.to_string()),
                    say: String::new(),
                    model: brain::Reached::NotNeeded,
                },
                Some(llm) => {
                    let needs_model = beyond_the_notes || matches!(self.parser.parse(said), Intent::Unknown(_));
                    // Built only when the model will read it: a command the
                    // phrases settle needs none of it.
                    let mut turn = if needs_model {
                        self.conversation_turn(said, _t, register, &persona, answering.as_deref(), &ctx)
                    } else {
                        brain::Turn { said: said.to_string(), one_prompt: ctx.clone(), ..Default::default() }
                    };
                    turn.skip_phrases = beyond_the_notes;
                    self.by_chat = needs_model && llm.native_chat();
                    // The Talk page and the voice loop don't wait here: the
                    // call runs on a worker and the turn is finished when it
                    // comes back (`finish_pending_turn`).
                    if std::mem::take(&mut self.may_defer) && needs_model && self.pending_turn.is_none() {
                        self.start_pending_turn(said, _t, llm, turn, persona.clone(), register);
                        self.this_turn_register = None;
                        self.this_turn_cap = None;
                        return String::new();
                    }
                    // The words so far are the Talk page's to show -- unless a
                    // Talk turn is still thinking, whose words they are
                    // (28 Sep 2026: a voice turn answered meanwhile cleared
                    // and wrote over them).
                    let partial = self.pending_turn.is_none().then(|| self.talk_partial.clone());
                    if let Some(Ok(mut p)) = partial.as_ref().map(|p| p.lock()) {
                        p.clear();
                    }
                    let mut also = Vec::new();
                    let d = Brain { llm: &*llm, fallback: &self.parser, voice: Some((&persona, register)) }.converse_noting(
                        &turn,
                        &mut |piece| {
                            if let Some(Ok(mut p)) = partial.as_ref().map(|p| p.lock()) {
                                p.push_str(piece);
                            }
                            true
                        },
                        &mut also,
                    );
                    self.also_asked = also;
                    d
                }
                None => {
                    let i = self.parser.parse(said);
                    let say = brain::default_say(&i);
                    brain::Decision { intent: i, say, model: brain::Reached::NotNeeded }
                }
            },
        };
        let took_ms = self.decided_in_ms.take().unwrap_or(started.elapsed().as_millis() as u64);
        // Only when the model was actually asked. `Reached::NotNeeded` means
        // the phrase parser settled it and nothing was sent, and recording
        // those would make the log say Atlas asks the model about everything
        // -- which is the opposite of what it is for.
        if decision.model != brain::Reached::NotNeeded {
            let failed = match decision.model {
                brain::Reached::No => Some(decision.say.clone()),
                _ => None,
            };
            self.record_model_call(
                "brain",
                took_ms,
                ctx.len() + said.len(),
                decision.say.len(),
                failed,
            );
        }
        // Record what the answer's dependencies actually did, at the moment
        // they did it. This is the only place the model's reachability is
        // observed, so if it is not written down here it is not written down
        // at all.
        match decision.model {
            brain::Reached::Yes => {
                if let Some(i) = self.connections.get_mut(crate::integrations::MODEL) {
                    i.worked(_t);
                }
            }
            brain::Reached::No => {
                if let Some(i) = self.connections.get_mut(crate::integrations::MODEL) {
                    i.failed(_t, "did not answer");
                }
            }
            // The parser handled it. That says nothing about the model either
            // way, and writing down a success it never earned would make the
            // board lie in the direction that matters least but still lie.
            brain::Reached::NotNeeded => {}
        }
        let mut intent = decision.intent.clone();

        // Write down where this turn's answer came from, so "why did you do
        // that?" / "why is that?" can be answered from a real record rather
        // than the empty list it used to read. Whether the model was reached
        // is the invisible choice the person cannot see and would ask about
        // ("why was that slow?"), and the tier carries the reason nothing
        // cheaper served. `decision.model` is what ACTUALLY happened, so the
        // record cannot claim the model was used on a turn the phrase parser
        // settled -- the tier alone is a pre-answer heuristic and would.
        //
        // Skipped for the questions that are themselves about the record --
        // `Intent::Why` and `Intent::History` -- because recording their own
        // routing would make a bare "why" surface the routing of the "why"
        // and answer itself.
        if !matches!(intent, Intent::Why(_) | Intent::History(_)) {
            let d = match decision.model {
                brain::Reached::Yes => crate::why::Decision {
                    at: _t,
                    what: "went to the model".into(),
                    because: match &tier {
                        crate::tier::Tier::Think { why } => why.clone(),
                        _ => "the request needed working out beyond what I had".into(),
                    },
                    instead_of: Some("answering from something I'd already worked out".into()),
                    set_by: None,
                },
                brain::Reached::No => crate::why::Decision {
                    at: _t,
                    what: "asked the model and got nothing back".into(),
                    because: "it did not answer, so I fell back to what I could do without it"
                        .into(),
                    instead_of: None,
                    set_by: None,
                },
                brain::Reached::NotNeeded => match &tier {
                    crate::tier::Tier::RunNamed(name) => crate::why::Decision {
                        at: _t,
                        what: format!("ran \"{name}\" without asking the model"),
                        because: "you named something I can run, so nothing had to be worked out"
                            .into(),
                        instead_of: Some("sending it to the model".into()),
                        set_by: None,
                    },
                    crate::tier::Tier::FromReport { report, as_of } => crate::why::Decision {
                        at: _t,
                        what: format!("answered from \"{report}\" rather than looking again"),
                        because: format!("it already covers this -- worked out {as_of}"),
                        instead_of: Some("sending it to the model".into()),
                        set_by: None,
                    },
                    crate::tier::Tier::Think { .. } => crate::why::Decision {
                        at: _t,
                        what: "answered without the model".into(),
                        because: "the phrasing settled it without anything having to be worked out"
                            .into(),
                        instead_of: Some("sending it to the model".into()),
                        set_by: None,
                    },
                },
            };
            self.decisions.note_full(d);
        }

        // "Summarise this" and "reply to this" are both clipboard commands,
        // and the difference between them is the whole instruction. Carry the
        // sentence through rather than falling back to a generic default.
        if let Intent::UseClipboard(arg) = &intent {
            if arg.trim().is_empty() {
                intent = Intent::UseClipboard(said.to_string());
            }
        }

        // Handed over, and this is one of the things Atlas will not do for
        // somebody who is not you.
        //
        // Above everything below it, and that position is the point. Below
        // the policy gate, a standing grant -- recorded when the machine was
        // yours -- returns `Ok` and the action runs. Below the notes lookup,
        // an unrecognised sentence has already been answered out of your
        // research notes, which is the leak this is for.
        if let Some(refusal) = self.handed_over_refusal(&intent) {
            self.log.info(&format!("refused while handed over: {}", kind_of(&intent)));
            return refusal;
        }

        // An unrecognised line is searched against Atlas's own notes before
        // anything else happens to it.
        //
        // This sits *above* the policy gate on purpose. An Unknown intent is
        // classified as needing approval, so it never reaches `execute` — it
        // becomes "I didn't catch that. Go ahead?" and waits. Reading back
        // something Atlas already wrote down changes nothing and risks
        // nothing, so asking permission for it is noise, and putting the
        // lookup below the gate meant it never ran at all.
        // Being spoken to like a person, answered like one — and above the
        // policy gate, because greeting Atlas is not an action that needs
        // approving. Below the gate it came back as "I didn't catch that. Go
        // ahead?", which treats hello as a failed command.
        let from_notes = match &intent {
            // Not while somebody else has it. `Unknown` is not on either
            // refusal list and must not be -- ordinary conversation lands
            // here, and refusing all of it would leave a guest with an Atlas
            // that says nothing. But *this* branch answers out of your
            // research notes and your own written-down wants, which is the
            // owner's material arriving through a door with no name on it.
            Intent::Unknown(raw) if !self.handover().stance.handed_over() => {
                // Reminders (B3) and stated-want weighing (B2) come first,
                // then a correction/declaration is learned before the notes
                // lookup ("actually my car is a Toyota" updates the fact).
                // All of it is gated by the handed_over() check on this arm.
                let answered = match local.take() {
                    Some(a) => Some(a),
                    None => self.answer_locally(raw, _t),
                };
                // Nothing here could answer it, and it read as a request
                // rather than a stray word or a mishear: Atlas was asked for
                // something it has no way to do. Written down so the next
                // "what am I missing?" can name it -- `recommend()` turns each
                // unsupported request into a concrete suggestion, and this is
                // the source it was built for. Recorded before the approval
                // gate, which for an unknown only ever asks or parks; whether
                // it does either does not change that the ask was made.
                if answered.is_none() && raw.split_whitespace().count() >= 2 {
                    self.wants_seen.asked_for_something_missing(raw);
                    let _ = self.store.save("wants_seen", &self.wants_seen);
                }
                answered
            }
            _ => None,
        };

        // "Do that tonight", "when I'm out".
        //
        // Read from the sentence rather than from a separate command: making
        // you say it twice is the friction that stops a feature being used.
        // Parked above the policy gate on purpose -- nothing is being done
        // yet, so nothing needs approving, and asking "may I?" about a thing
        // you have just asked to be *delayed* is the wrong question.
        //
        // Answering, reading and conversation are exempt: "what did I do
        // tonight" is a question, not an instruction to wait, and parking it
        // would make Atlas mute on the word.
        if !matches!(
            intent,
            Intent::Unknown(_) | Intent::Ask(_) | Intent::Say(_) | Intent::Why(_)
        ) {
            if let Some(blocker) = crate::backlog::asked_to_wait(said) {
                let id = self.backlog.record(said, blocker, _t);
                let _ = self.backlog.save(&self.store);
                return format!(
                    "Right — I'll hold {} until you're out of the way, and \
                     pick it up then. (#{id})",
                    intent.plain()
                );
            }
        }

        let call = crate::policy::classify_with_policy(&intent, &self.memory, &self.cfg.policy);

        // A voice that only *might* be yours turns a consequential action into
        // a question.
        //
        // "Consequential" is read from `policy::classify`, the intrinsic
        // judgement, rather than from `call` above — `classify_with_policy`
        // can downgrade an action to automatic because you approved one like
        // it before, and a standing grant is exactly what should not carry
        // when Atlas is unsure who is speaking. The grant records that *you*
        // approved it, which is the thing in doubt.
        //
        // This only ever escalates: `handle` never returns permission, so the
        // worst case is being asked a question you did not need to be asked.
        let consequential =
            !matches!(crate::policy::classify(&intent), Decision::AutoProceed);
        let call = match crate::voiceid::handle(
            self.last_verdict,
            consequential,
            &self.tools_cfg().voice_id,
        ) {
            crate::voiceid::Handling::Confirm => Decision::RequireApproval,
            _ => call,
        };

        // And a *reading* that only might be what you said does the same.
        //
        // The gate above grades what the action is. Until this, nothing
        // graded how Atlas came to believe you asked for it -- a dictation
        // matched from the phrase list and a dictation the model guessed out
        // of a mumble were graded identically and both simply happened.
        // `brain.rs` does tell the model to "use ask rather than guessing",
        // but that is an instruction to the very model whose confident
        // wrongness `certainty.rs` exists to catch, and nothing checked
        // whether it obeyed.
        //
        // Intrinsic `classify` again rather than `call`, on exactly the
        // reasoning in the block above: a standing grant records that you
        // approved something like this before, and what is in doubt here is
        // whether you asked for it at all. Escalation only, so the worst case
        // is a question you did not need.
        let understanding =
            crate::understood::Understanding::from_reached(decision.model);
        let call = crate::policy::Decision::max(
            call,
            crate::understood::grade(
                crate::policy::classify(&intent),
                understanding,
                &self.tools_cfg().understood,
            ),
        );
        // And a consequential action the MODEL chose -- a tool call, not
        // your words matching a phrase -- is always asked about first: handing
        // Atlas over, messaging someone, changing code, pressing a button.
        let call = if decision.model == brain::Reached::Yes && brain::model_must_ask(&intent) {
            crate::policy::Decision::max(call, Decision::RequireApproval)
        } else {
            call
        };
        // Another program's tool is asked about every time, unless its
        // server's entry lets this one tool run unasked (`mcp`).
        let call = self.mcp_gate(&intent, call);

        // A sensor may say something. It may not do anything.
        //
        // `Hint::offer` returns words and has no other effect -- it cannot
        // enter a handover and cannot leave one. This is the whole of what an
        // unfamiliar voice is permitted to cause: a sentence, once, telling
        // whoever is there that there is a way to say so out loud.
        if matches!(self.last_verdict, crate::voiceid::Verdict::NotYou(_))
            && !self.offered_handover
        {
            let handed = crate::handover::Handover::load(&crate::roots::install_state());
            if !handed.stance.handed_over() {
                if let Some(offer) = crate::handover::Hint::VoiceUnfamiliar.offer() {
                    self.session.ask(offer);
                    self.offered_handover = true;
                }
            }
        }
        // The turn's own time, for what the action reads the clock for
        // (`now_acting`): what was asked at 23:59 is about that day.
        self.acting_at = Some(_t);
        let reply = match from_notes {
            Some(answer) => answer,
            // A line Atlas didn't understand is answered, not approved.
            // `policy` still classes it as needing approval -- nothing Atlas
            // didn't understand may ever be *run* -- but asking "Go ahead?"
            // about it turned every unrecognised line into a question, and
            // the next thing said was then taken as the yes or no (found
            // testing with a friend, 26 Sep 2026). `execute(Unknown)` only
            // answers: from a written procedure, from your notes, or by
            // saying it can't.
            None if matches!(intent, Intent::Unknown(_)) => self.execute(&intent),
            None => match call {
            Decision::AutoProceed => self.execute(&intent),
            Decision::ProceedAndReport => self.execute(&intent),
            Decision::AskClarification => {
                // When the reason we are asking is that the *reading* was
                // inferred rather than matched, say the reading. The model's
                // own `say` is what it planned to announce while doing the
                // thing, which is the wrong sentence for a question -- and
                // "I didn't catch that" would throw away a reading Atlas
                // actually made and force you to start the sentence over.
                // Naming it lets you correct one word.
                let ask = if understanding == crate::understood::Understanding::Inferred
                    && crate::policy::classify(&intent) == Decision::ProceedAndReport
                {
                    crate::understood::checking(&intent.plain())
                } else {
                    decision.say.clone()
                };
                self.session.ask(&ask);
                ask
            }
            // Never ask about something that would be refused anyway: a
            // question whose "yes" leads to "you can't" is a question wasted,
            // and every wasted one teaches you to answer without reading.
            Decision::RequireApproval if self.refused_before_asking(&intent).is_some() => {
                self.refused_before_asking(&intent).unwrap_or_default()
            }
            Decision::RequireApproval => {
                if self.autonomy == Autonomy::Unattended {
                    // "I'll wait" used to be the whole of this branch, and
                    // nothing waited: no record was kept, so the next time you
                    // looked there was no trace you had been asked. A promise
                    // Atlas does not keep is worse than a refusal.
                    //
                    // `mend`'s rule is that the question has to be answerable
                    // without reading code, and its `parked()` is the backlog
                    // item it becomes -- the one producer of
                    // `Blocker::NeedsYourDecision`, which had no caller.
                    self.park_for_you(&crate::mend::about_approval(said), said, _t)
                } else {
                    // What kind of thing you're approving, when it matters:
                    // anything that leaves the machine, commits you, or
                    // changes Atlas says so (`categories::consent_line`);
                    // something local just asks.
                    let cat = crate::categories::category_of(&intent);
                    // Many commands have no stock phrase; the question still
                    // has to say what it is asking about.
                    let say = match brain::default_say(&intent) {
                        s if s.trim().is_empty() => format!("Just to check -- {}.", intent.plain()),
                        s => s,
                    };
                    let q = match cat {
                        crate::categories::Category::LocalOperational | crate::categories::Category::LocalCreative => {
                            format!("{say} Go ahead?")
                        }
                        _ => crate::categories::consent_line(cat, say.trim_end_matches('.')),
                    };
                    self.session.await_approval(intent.clone(), &q);
                    q
                }
            }
            },
        };
        self.acting_at = None;

        // A saved file that couldn't be read was set aside since you were
        // last told: said now, once, not only in Diagnose (28 Sep 2026).
        let reply = match crate::store::tell_set_aside(&self.store) {
            Some(line) => format!("{line} {reply}"),
            None => reply,
        };

        // If you have been gone a while, lead with what happened.
        let reply = match self.pending_brief.take() {
            Some(b) => {
                // It is now in the reply, so it has been handed over and can
                // be dropped. This is the only place the outbox is emptied.
                let cfg = self.notify_cfg();
                let _ = self.outbox.collect(_t, &cfg);
                format!("{b} {reply}")
            }
            // No away-brief -- either you never left, or the absence was too
            // short to owe you one. But the conversation itself can still have
            // a gap worth naming: turning back to Atlas after an hour on
            // something else reads as the same thread only if it says where
            // you left off. `resume_line` is asked against the thread as it
            // stood before this turn's `append` below, so the gap is measured
            // to the last thing actually said, and it stays silent unless that
            // gap has passed `gap_secs` and there is a topic to name. It fires
            // once -- the append that follows resets `last_active`, so the
            // next turn's gap is near zero and the line does not repeat.
            None => match self.thread.resume_line(&self.thread_cfg(), _t) {
                Some(r) => format!("{r} {reply}"),
                None => reply,
            },
        };

        // Remember what "it" now means.
        if let Some(app) = crate::session::app_of(&intent) {
            self.referents.last_app = Some(app);
        }

        // Anything blocked goes on the outstanding list rather than evaporating.
        // Asked through `connectivity::allows`, the predicate written to answer
        // "can this need be met right now?" — it folds the need and the cached
        // reach into one answer, where this line used to spell out the
        // Internet case by hand and the other needs (Local, PrefersInternet)
        // were simply never considered.
        if !self.connectivity.allows(need_of(&intent)) {
            self.backlog.record(said, Blocker::Offline, _t);
        }

        // Everything Atlas says goes through its manner — and the manner
        // depends on the moment. A task gets two sentences; a conversation
        // gets room to actually be one.
        // The register this turn was ANSWERED in -- the same one the model was
        // briefed with, not a second reading taken afterwards. Two readings of
        // one turn could disagree, and then Atlas would be instructed to
        // converse and trimmed as if it were working.
        let register = self.this_turn_register.unwrap_or_else(|| {
            crate::register::read(
                said,
                &crate::register::Moment {
                    recent: self
                        .thread
                        .recent
                        .iter()
                        .rev()
                        .take(3)
                        .map(|e| e.said.clone())
                        .collect(),
                    after_a_failure: self.last_turn_failed,
                    busy: self.queue_is_busy(),
                },
            )
        });
        // The same persona and the same cap the model was briefed with. See
        // the note where `this_turn_cap` is set: two independent computations
        // of "how long" is how Atlas ended up instructed to converse and
        // trimmed as if it were working.
        let mut persona = self.persona_now();
        persona.max_spoken_sentences = self.this_turn_cap.unwrap_or_else(|| {
            self.modes
                .verbosity_if_set()
                .map(|v| crate::modes::sentences_for(v))
                .unwrap_or(usize::MAX)
                .min(register.length())
                .max(1)
        });
        // A list you asked for is read whole. Round 11's tools answer with
        // numbered lists ("what am I waiting on", "what did I copy") that
        // "open 2" refers back to, and the trading check-in must end with
        // its line -- cut to two sentences, the list is useless and the line
        // is gone. Everything else is shaped for speech as before.
        // A reply the model wrote over the chat path was already stopped at
        // the end of a sentence, at the length it was asked for; cutting it
        // again here chopped answers mid-thought. It only loses its filler,
        // with a hard ceiling kept for safety.
        let chatted = decision.model == brain::Reached::Yes && matches!(intent, Intent::Say(_)) && self.by_chat;
        let reply = if crate::workday::reads_whole(&intent) {
            reply.trim().to_string()
        } else if chatted {
            let mut loose = persona.clone();
            loose.max_spoken_sentences = SAFETY_SENTENCES;
            loose.spoken(&reply)
        } else {
            persona.spoken(&reply)
        };

        // An action the phrase parser settled gets its words put in Atlas's
        // voice, here, with no model call -- so the action is not delayed and
        // the sentence is not a table entry.
        //
        // Only on the fast path (`Reached::NotNeeded`): when the model was
        // asked it wrote the reply itself under `prompt_for`, which already
        // carries the tone and the form of address, and dressing that a
        // second time would put ", Eric." on a sentence that had already
        // decided how to end.
        //
        // And only for an action. A fact, a count or a refusal keeps the
        // words it was given: "now" belongs on something being done, not on
        // something being reported.
        let reply = if decision.model == brain::Reached::NotNeeded && brain::is_an_action(&intent) {
            persona.acknowledge(&reply, _t)
        } else {
            reply
        };

        // Deliberately after `persona.spoken`, not before. `shape()` caps the
        // sentence count, so a caveat added earlier is exactly the sentence
        // most likely to be cut — the warning would be silently dropped from
        // the answers that most needed it.
        let used = crate::integrations::sources_for(need_of(&intent), decision.model);
        let reply = crate::integrations::mark(&reply, &used, &self.connections, _t);
        // The model didn't know, or it's the kind of thing that changes by
        // the day: offer to look it up, and a yes does.
        let reply = match self.offer_to_look_it_up(said, &intent, decision.model, &reply) {
            Some(r) => r,
            None => reply,
        };
        // Asked for two things in one breath, the model called two tools
        // and only the first runs: the second is named, not dropped.
        let also = std::mem::take(&mut self.also_asked);
        let reply = if also.is_empty() { reply } else { format!("{} {}", reply.trim(), one_at_a_time(&also)) };
        self.last_register = register;

        // Carried to the NEXT turn, for `register::Moment::after_a_failure`.
        // Read from the reply that was actually produced, which is the only
        // place the outcome is known, and used one turn later, which is where
        // the frustration lands.
        //
        // Only a task can fail. A conversation that mentions failing ("the
        // launch failed because…") left the next turn Rough, with no jokes and
        // no tangents (27 Sep 2026).
        self.last_turn_failed = !matches!(intent, Intent::Say(_) | Intent::Ask(_) | Intent::Unknown(_))
            && (reply.to_lowercase().contains("failed")
                || reply.to_lowercase().starts_with("error")
                || reply.contains("I couldn't")
                || reply.contains("unreachable"));
        self.this_turn_register = None;
        self.this_turn_cap = None;

        // What gets WRITTEN DOWN, which is not always what was said.
        //
        // The three calls below all persist: `session` is in memory but
        // `thread.append` and `journal.record_at` are both saved by
        // `persist()` two lines further on, into `data/state/thread.json` and
        // `data/state/activity.json` -- and `safety::back_up` copies that
        // whole folder into `data/backups/`, so anything here is replicated
        // into every backup too.
        //
        // `said` used to go in verbatim. `config/commands.yaml` ships
        // `"the passphrase is"` as an Unlock phrase that takes an argument,
        // so saying "the passphrase is hunter2" wrote **hunter2 in plaintext
        // into two files in the same directory as the vault it opens.** A
        // stolen laptop then carries the ciphertext and the passphrase side
        // by side, which is the exact thing `vault.rs`'s design exists to
        // prevent, and `typed.rs` opens by saying there is "exactly one thing
        // in Atlas that a microphone must never carry: the vault passphrase".
        // It was carrying it, and then writing it down.
        //
        // Reproduced before fixing: one turn, then `persist()`, then grep the
        // state folder -- it was in `thread.json` and `activity.json`.
        //
        // The description rather than a mask, because `describe` already says
        // what the turn was for and the thread stays readable: you can still
        // see that you unlocked the vault at that point, which is the part
        // worth keeping.
        let for_the_record: String = match &intent {
            Intent::Unlock(_) => {
                format!("[{} — what you said is not written down]", intent.plain())
            }
            _ => said.to_string(),
        };
        let said = for_the_record.as_str();

        // How long, said up front for the long kinds (F6).
        let reply = match self.how_long_up_front(&intent, _t) {
            Some(est) if !reply.is_empty() => format!("{reply} {est}"),
            _ => reply,
        };
        // An old correction about this kind of work (F9).
        let reply = match self.related_old_correction(&intent, said, _t) {
            Some(line) if !reply.is_empty() => format!("{reply} {line}"),
            _ => reply,
        };
        self.session.record(said, &intent, &reply);
        self.note_for_routines(said, &intent, _t);
        self.thread.append(said, &reply, topic_of(&intent), _t);
        // A tool that reads something back (your calendar, what's in your
        // notes, the machine's health) answered with its fixed words; the
        // model puts them into a short natural reply next (`RephraseAsk`).
        // Actions keep their fixed acknowledgements.
        if self.rephrase_ok && decision.model == brain::Reached::Yes && self.by_chat && reads_back(&intent) && worth_rephrasing(&reply) {
            self.rephrase_ask = Some(RephraseAsk { written: reply.clone(), keep_written: crate::workday::reads_whole(&intent) });
        }
        // The same question, come round again. `asked_before` is asked now,
        // with this turn already appended, so it skips the turn just added and
        // looks further back for an identical `said` -- exactly the shape its
        // own doc describes ("repeating an answer verbatim ... makes an
        // assistant feel like it isn't listening"). The answer still gets
        // given in full; a short lead-in just marks that Atlas noticed, rather
        // than replaying the reply as if for the first time. Left off what
        // gets written down: the record keeps the substantive answer, not the
        // conversational nudge.
        //
        // Not in conversation: "tell me another joke" twice is a request for
        // a second one, and a model that answers again answers afresh.
        let reply = match self.thread.asked_before(said) {
            Some(_) if !chatted => format!("You asked this a little earlier -- here it is again. {reply}"),
            _ => reply,
        };
        self.fold_if_due(_t);
        self.journal.record_at(Act::Asked, said, true, _t);
        self.persist();
        reply
    }

    /// Is there work in hand -- queued in a lane, or a workflow mid-run?
    ///
    /// Public because the busy signal is a fact about the daemon that the
    /// outside is entitled to ask, and the tests that hold the signal honest
    /// ask it here rather than reaching into the queue and re-deriving it.
    pub fn queue_is_busy(&self) -> bool {
        // A workflow mid-run is work in hand, exactly as queued work is --
        // it just lives in `current_flow` instead of a lane.
        self.queue.pending() > 0 || self.current_flow.is_some()
    }

    /// What "this" refers to right now.
    ///
    /// Returns a question when it genuinely can't tell, which stops the
    /// intent rather than guessing at it.
    fn resolve_subject(&mut self, intent: &Intent) -> Option<String> {
        let arg = match intent {
            Intent::Research(a) | Intent::DraftPost(a) | Intent::Capture(a) | Intent::Files(a) => a,
            // `ReviewPost`'s argument is the actual text being reviewed, not
            // a bare reference to it — "review this post: I love it when
            // things work" has "it" and "this" in it as ordinary English,
            // not as something to resolve. Treating it the same as
            // "explain this" meant any post whose own wording happened to
            // contain "it", "this" or "that" anywhere was silently
            // hijacked into a clipboard/selection question instead of ever
            // being reviewed at all.
            Intent::ReviewPost(_) => return None,
            // "Explain this" with UseClipboard already says where "this" is.
            Intent::UseClipboard(_) => return None,
            _ => return None,
        };
        // Only sentences that actually contain a pronoun need resolving.
        let lower = arg.to_lowercase();
        if !["this", "that", "it", "these", "those"]
            .iter()
            .any(|w| lower.split_whitespace().any(|t| t.trim_matches(',') == *w))
        {
            return None;
        }
        // Only what the daemon genuinely knows. Filling these in with guesses
        // would make the resolver confident about nothing — and leaving the
        // clipboard out made it resolve to nothing at all, which broke
        // "explain this".
        //
        // `only_on_request` (on by default) means Atlas does not reach for
        // what you copied unless your words point at it — "explain this" when
        // "this" could be the clipboard, not every pronoun. The predicate
        // written for exactly this (`refers_to_clipboard`) had no caller, so
        // the setting was dead and the clipboard was silently in scope for
        // every resolution.
        let use_clipboard = !self.clipboard_cfg().only_on_request
            || crate::clipboard::refers_to_clipboard(arg);
        let candidates = crate::subject::Candidates {
            clipboard: if use_clipboard { self.clipboard_text.clone() } else { None },
            ..Default::default()
        };
        match crate::subject::resolve(arg, &candidates) {
            // Two equally likely things is a question, not a coin toss.
            crate::subject::Resolution::Ambiguous { question, .. } => Some(question),
            crate::subject::Resolution::Nothing(why) => Some(why),
            crate::subject::Resolution::Found { .. } => None,
        }
    }

    /// Something learned. Merges with what's already known rather than
    /// adding a second copy.
    ///
    /// The point: asking about the same thing twice, worded differently,
    /// shouldn't cost you what you knew. Two notes means two decay curves and
    /// confidence in a settled fact drifting down because you happened to ask
    /// again.
    pub fn learned(&mut self, says: &str, source: &str, now: u64) -> bool {
        let strengthened = crate::consolidate::learn(&mut self.known, says, source, now);

        let cfg = self.tools_cfg().consolidate.clone();
        // Squeeze the long ones first, drop only if that isn't enough, and
        // keep a line for whatever goes (H10: memory keeps a stub of what it
        // forgot).
        let (_squeezed, dropped) =
            crate::consolidate::make_room(&mut self.known, &mut self.stones, cfg.keep_at_most, now);
        if !dropped.is_empty() {
            let _ = self.store.save("known_stones", &self.stones);
        }
        if let Some(note) = crate::consolidate::dropped_note(&dropped) {
            // Said rather than done quietly — a store that silently forgets is
            // one you stop trusting.
            self.history.note(&note, "knowledge",
                crate::undo::Undo::Cannot("what was dropped is gone".into()), false, now);
        }
        // Trim holds settled facts and things about your own setup out of the
        // budget entirely, because they cannot be looked up again. When those
        // alone exceed the cap, nothing is dropped and the store sits over
        // budget on purpose -- and until now, silently. A cap quietly exceeded
        // is a cap doing nothing, so which of the two it is gets said.
        if let Some(note) =
            crate::consolidate::over_budget_on_purpose(&self.known, cfg.keep_at_most)
        {
            self.history.note(&note, "knowledge",
                crate::undo::Undo::Cannot(
                    "nothing was dropped; the budget is exceeded by facts that can't be relearned"
                        .into(),
                ), false, now);
        }
        strengthened
    }

    /// Turn what Atlas records about itself into signals it can act on.
    ///
    /// The gap this closes: `selfaudit` could rank signals and had nothing
    /// producing them. A self-audit with an empty input list reports that
    /// everything is fine, which is the most misleading possible answer.
    pub fn refresh_signals(&mut self) {
        let never_used: Vec<String> = crate::capability::all()
            .into_iter()
            .filter(|c| c.state == crate::capability::State::Working)
            .map(|c| c.id.to_string())
            .collect();
        let total = crate::capability::all().len() as u32;
        self.signals = crate::signals::gather(
            &self.history,
            self.unknown_count,
            self.utterance_count,
            &self.last_unknown,
            &never_used,
            total,
        );
    }

    /// Anything that was running when the machine stopped.
    ///
    /// Resolved before the brief, not during it — you sat down to get on with
    /// something, and a system that opens with questions about last night has
    /// answered the wrong one. Bounded: anything it can't judge inside the
    /// budget is left alone and mentioned, never asked about.
    fn settle_interrupted(&mut self) -> Vec<String> {
        let mut said = Vec::new();
        let jobs = std::mem::take(&mut self.interrupted);
        for (what, checked, reversible) in jobs {
            let decision = crate::awake::on_waking_checked(&checked, reversible, 0);
            said.push(crate::awake::woke_checked(&what, decision, &checked));
        }
        said
    }

    /// Should the machine be kept awake right now?
    ///
    /// Only while something is actually running, and never with the lid shut
    /// or the battery low. Called by whatever is running rather than held
    /// open by the daemon.
    pub fn keep_awake(&self, why: crate::awake::Because, power: &crate::awake::Power, held_mins: u32)
        -> (crate::awake::Hold, String)
    {
        crate::awake::decide(why, power, held_mins, &self.tools_cfg().awake)
    }

    /// Something didn't work. Is there another way?
    ///
    /// Reporting a failure is what a program does. Trying the next route is
    /// what an assistant does, and the module for it has been sitting there
    /// unreachable.
    pub fn another_way(&self, kind: crate::route::Kind, failed: &str) -> Option<String> {
        let online = self.connectivity.cached() == crate::connectivity::Reach::Online;
        let next = crate::route::known_routes()
            .into_iter()
            .filter(|r| r.for_what == kind)
            .filter(|r| r.name != failed)
            .filter(|r| online || !crate::route::needs_internet(r))
            .max_by(|a, b| {
                a.reliability.partial_cmp(&b.reliability).unwrap_or(std::cmp::Ordering::Equal)
            })?;
        Some(next.name)
    }

    /// The apps Atlas has configured, by name — what `grants::check` counts
    /// as "known", and what `grant_in_instruction` matches a named tool
    /// against.
    fn known_app_names(&self) -> Vec<String> {
        self.cfg.apps.apps.keys().cloned().collect()
    }

    /// What Atlas knows about an app, for the permission check. `known` is
    /// simply whether it is in the configured apps; `confirm_each_time` is the
    /// per-app "ask every single time" flag, which today only the no-input
    /// apps (Discord and its kind) carry — a stray keystroke there is public
    /// and permanent, so touching one is always a question.
    fn app_facts(&self, app: &str) -> crate::grants::AppFacts {
        let spec = self.cfg.apps.apps.iter().find(|(name, _)| name.eq_ignore_ascii_case(app));
        crate::grants::AppFacts {
            known: spec.is_some(),
            confirm_each_time: spec.map(|(_, s)| s.no_input).unwrap_or(false),
        }
    }

    /// The permission gate for acting on an app. A configured app Atlas
    /// already knows sails straight through — the gate is not a nag, it is
    /// the "I don't know this one, may I?" question rule 1 of the grants
    /// module describes, plus the confirm-every-time apps. On a question it
    /// parks the action as a pending approval so the next thing you say is
    /// the answer; on a yes there, the grant is recorded (with the breadth
    /// you gave — once, this session, or always) before the action runs, so
    /// the re-check finds it and does not ask twice.
    fn gate_app(&mut self, app: &str, action: &str, intent: &Intent) -> AppGate {
        let facts = self.app_facts(app);
        match self.permissions.check(app, action, &facts) {
            crate::grants::Verdict::Allowed(_) => {
                // A one-off grant is spent the moment it is used, so the next
                // action on the same app asks again.
                self.permissions.consume(app, action);
                AppGate::Go
            }
            crate::grants::Verdict::Ask(question) => {
                self.session.await_approval(intent.clone(), &question);
                AppGate::Ask(question)
            }
        }
    }

    /// What Atlas can still do with the network unplugged.
    pub fn offline_coverage(&self, kind: crate::route::Kind) -> String {
        let (offline, all) = crate::route::coverage(kind);
        format!("{offline} of {all} ways work with no internet.")
    }

    /// Is this something Atlas already knows how to do?
    ///
    /// Checked before falling back to the model — a procedure Atlas has
    /// written down beats a guess, and it works with the network unplugged.
    fn known_procedure(&self, asked: &str) -> Option<String> {
        let book = crate::knowhow::Knowhow::load(&self.store);
        // Offline is the normal case, not the exception, so a procedure that
        // needs the internet is filtered out rather than offered and failed.
        let online = self.connectivity.cached() == crate::connectivity::Reach::Online;
        book.for_request(asked, online)
            .map(|p| crate::knowhow::announce(p, online))
    }

    /// A symptom you describe, matched to a known snag and its fix.
    ///
    /// The other half of `known_procedure`: that answers "how do I do X" from
    /// the procedures' goals; this reads `knowhow::for_symptom`, which scores
    /// what you say against every procedure's *snags* -- the things that
    /// usually go wrong -- and names the likely cause and what to do. Offline
    /// by construction, because the snags ship compiled in. Named
    /// `diagnose_symptom` rather than `diagnose` on purpose: `diagnose.rs`
    /// already owns the bare name `diagnose` for the machine-vitals check, and
    /// the reachability scan matches by bare name.
    fn diagnose_symptom(&self, symptom: &str) -> String {
        let book = crate::knowhow::Knowhow::load(&self.store);
        match book.for_symptom(symptom) {
            Some((p, snag)) => format!(
                "That's the sort of thing I've run into ({}): usually {}. \
                 What I'd do: {}. It comes up while trying to {}.",
                snag.looks_like, snag.cause, snag.fix, p.goal
            ),
            None if symptom.trim().is_empty() => {
                "Tell me what you're actually seeing and I'll check whether I know \
                 the cause -- something like \"an app that launches and closes \
                 immediately\"."
                    .into()
            }
            None => format!(
                "I can't match \"{}\" to any snag I've run into before. Describe \
                 what's on screen a bit differently and I'll look again.",
                symptom.trim()
            ),
        }
    }

    /// A task you name, read back as the steps to follow.
    ///
    /// The other side of `known_procedure`: that finds the same procedure with
    /// `for_request` but only says `announce` -- "I know this one, N steps" --
    /// and never the steps. This is the caller `knowhow::as_plan` never had,
    /// so a match becomes a numbered plan you can actually work through.
    /// Offline by construction: the procedures ship compiled in, and one that
    /// needs the internet is filtered out rather than offered and failed.
    ///
    /// Named `walk_me_through` rather than any bare `plan`/`steps` on purpose:
    /// the reachability scan matches by bare name, and this name is its own.
    fn walk_me_through(&self, asked: &str) -> String {
        let asked = asked.trim();
        if asked.is_empty() {
            return "Walk you through what? Name the task -- \"walk me through \
                    freeing up memory\", \"how do I open something that won't \
                    launch\"."
                .into();
        }
        let book = crate::knowhow::Knowhow::load(&self.store);
        let online = self.connectivity.cached() == crate::connectivity::Reach::Online;
        match book.for_request(asked, online) {
            Some(p) => {
                let mut s = format!("{} -- here's how:\n{}\n", p.goal, crate::knowhow::checklist(p));
                for (n, step) in crate::knowhow::as_plan(p).into_iter().enumerate() {
                    s.push_str(&format!("  {}. {}\n", n + 1, step));
                }
                s
            }
            // Not one of Atlas's own procedures -- "how do I make pancakes"
            // is a question, and it gets an answer when there's a model to
            // give one, not a pointer at a troubleshooting command (26 Sep
            // 2026).
            None => {
                if let Some(llm) = self.llm.clone() {
                    let system = format!(
                        "{}\n\n{}",
                        self.persona_now().prompt_for(crate::register::Register::Working),
                        crate::brain::TALK
                    );
                    if let Some(said) = llm
                        .complete(&system, &format!("{}\nUser said: how do I {asked}", crate::capability::about_atlas(asked, 6)))
                        .ok()
                        .and_then(|r| crate::brain::spoken_text(&r))
                    {
                        return said;
                    }
                }
                format!(
                    "I don't have steps for \"{asked}\" written down, and questions like that need my \
                     language model, which isn't running here. If something's gone wrong, tell me what \
                     you're seeing -- \"troubleshoot ...\" -- and I'll see if I know the cause."
                )
            }
        }
    }

    /// Before saying something, is it worth saying?
    ///
    /// The one gate everything unprompted goes through. Without it every
    /// feature politely announces itself and the sum is a system that talks
    /// constantly about nothing.
    pub fn worth_saying(&mut self, thing: &crate::interrupt::Thing, doing: crate::interrupt::Doing) -> Option<String> {
        let mut cfg = self.tools_cfg().interrupt.clone();
        // What you wrote in the config, plus what you said out loud. Two
        // lists because they come from two places and neither should silently
        // overwrite the other -- `interrupt::Muted` has the argument.
        cfg.muted.extend(crate::interrupt::Muted::load(&self.store).topics);
        match self.gate.consider(thing, doing, &cfg, clock()) {
            crate::interrupt::Decision::Say(s) => Some(s),
            _ => None,
        }
    }

    /// How sure is the answer, and should it say so?
    fn hedge(&self, answer: &str, grounding: crate::certainty::Grounding) -> String {
        let cfg = self.tools_cfg().certainty.clone();
        let (confidence, _, why) = crate::certainty::assess(answer, &grounding, &cfg);
        crate::certainty::phrase(answer, confidence, &why)
    }

    /// The tools config, or its defaults when there isn't one.
    ///
    /// Everything is off by default, so a missing tools.yaml means Atlas does
    /// less rather than more.
    /// Your time zone: the `time_zone` setting if you've chosen one, and
    /// this computer's own clock if you haven't (`tz::home`).
    pub fn home_zone(&self) -> crate::tz::Zone {
        crate::tz::home(self.tools_ref().map(|t| t.time_zone.as_str()).unwrap_or(""))
    }

    /// The zone you chose, or `None` for "this computer's clock": what
    /// `localclock` is told, so the hub and the calendar agree.
    fn chosen_zone(&self) -> Option<crate::tz::Zone> {
        let set = self.tools_ref().map(|t| t.time_zone.trim().to_string()).unwrap_or_default();
        (!set.is_empty() && !set.eq_ignore_ascii_case("automatic")).then(|| crate::tz::home(&set))
    }

    /// Your settings as they stand now: the ones Atlas started with, or the
    /// ones it has picked up since. Every read of a setting goes through here,
    /// which is what lets a change apply without a restart.
    pub(crate) fn tools_ref(&self) -> Option<&crate::voice::ToolsConfig> {
        self.tools_live.as_ref().or(self.cfg.tools.as_ref())
    }

    /// Watch this folder's settings, so a change made in the settings window,
    /// the hub, or by hand is picked up while Atlas runs.
    pub fn watch_settings(mut self, config_dir: std::path::PathBuf) -> Self {
        let seen = settings_fingerprint(&config_dir);
        self.settings_stamp = settings_stamp(&config_dir);
        self.settings_watch = Some((config_dir, seen));
        self
    }

    /// Pick up a change to your settings, if there's been one since the last
    /// look. Returns one line per setting that changed, saying whether it
    /// applies now or when Atlas next starts.
    ///
    /// Cheap enough for every tick: two files' sizes and modified times are
    /// looked at, and only when one of those has moved are the files read and
    /// hashed (27 Sep 2026 -- they used to be read and hashed every tick).
    /// Nothing is reloaded unless the contents differ from last time.
    pub fn pick_up_settings(&mut self) -> Vec<String> {
        let Some((dir, seen)) = self.settings_watch.clone() else { return Vec::new() };
        let stamp = settings_stamp(&dir);
        if stamp == self.settings_stamp {
            return Vec::new();
        }
        self.settings_stamp = stamp;
        let now = settings_fingerprint(&dir);
        if now == seen {
            return Vec::new();
        }
        let fresh = match crate::config::Config::load(&dir) {
            Ok(c) => c.tools.map(|t| t.anchored()),
            Err(e) => {
                // Half-written or hand-broken: keep running on what we have,
                // look again next tick, and say why once.
                self.settings_watch = Some((dir, now));
                return vec![format!("I couldn't read my changed settings, so I'm keeping the ones I had: {e}")];
            }
        };
        self.settings_watch = Some((dir.clone(), now));
        // Your settings file is applied quietly over tools.yaml, so one that
        // won't read would otherwise just look like every choice reverting.
        if let Err(e) = crate::preferences::Preferences::load_checked(&dir) {
            let line = format!("I couldn't read your settings file, so I'm using the defaults until it's fixed: {e}");
            self.log.warn(&line);
            return vec![line];
        }
        let Some(mut fresh) = fresh else { return Vec::new() };
        let before = self.tools_ref().cloned().unwrap_or_default();
        // `vars` aren't settings — nothing on the settings page changes them —
        // and some hold what Atlas worked out for itself at the start (the
        // microphone it picked, over the one the file names). So the running
        // ones stay, and only a var the file has newly gained is added.
        let mut vars = before.vars.clone();
        for (k, v) in std::mem::take(&mut fresh.vars) {
            vars.entry(k).or_insert(v);
        }
        fresh.vars = vars;
        let changed = crate::settings::registry(&before).differences(&crate::settings::registry(&fresh));
        // The three parts that keep their own copy of a setting, refreshed so
        // they read the new one.
        self.proactive.cfg = fresh.proactive.clone();
        self.persona = fresh.persona.clone();
        self.eyes.cfg = fresh.presence.clone();
        self.tools_live = Some(fresh);
        // The shared, resolved copy every `tools_cfg()` hands out, and the
        // home zone the clock follows, are rebuilt from the new settings.
        self.tools_resolved = std::sync::Arc::new(resolve_tools(self.tools_ref(), &self.store));
        crate::localclock::set_home_zone(self.chosen_zone());
        let crew_cfg = self.tools_resolved.crew.clone();
        self.crew.set_margins(crew_cfg.keep_free_mb, crew_cfg.battery_floor_percent);
        self.mcp_configure();
        // The wake word switched on or off from the hub or the settings
        // window takes effect now, not at the next start.
        if self.wake_on() != self.tiers.wake_on() {
            let on = self.wake_on();
            self.tiers.set_wake(on);
        }
        let lines: Vec<String> = changed
            .iter()
            .map(|s| {
                let v = s.value.as_display();
                if crate::settings::needs_a_restart(&s.key) {
                    format!("{} will be {v} when I next start.", s.name)
                } else {
                    format!("{} is now {v}.", s.name)
                }
            })
            .collect();
        for l in &lines {
            self.log.info(&format!("settings: {l}"));
        }
        lines
    }

    /// Your tools.yaml, resolved and shared.
    ///
    /// This was `self.cfg.tools.clone().unwrap_or_default()` on every call —
    /// every tool's command and arguments, the whole variable map, several
    /// hundred allocations — at over a hundred call sites, most of which
    /// wanted one field: `let cfg = self.tools_cfg().signin;` copied the lot
    /// to keep one. It is materialised once, handed out by reference count,
    /// and rebuilt only when your settings change (`pick_up_settings`). A
    /// site that needs to own a section clones that one section.
    pub fn tools_cfg(&self) -> std::sync::Arc<crate::voice::ToolsConfig> {
        self.tools_resolved.clone()
    }

    /// "Find me the thing about the budget."
    ///
    /// This used to answer by classifying the words as if they were a
    /// filename — "the budget is a Document. It needs an application that
    /// opens it" — which has the shape of an answer without being one.
    /// `index.search` had existed the whole time and nothing called it.
    ///
    /// The question is run through `asking` first. A spoken question is
    /// mostly scaffolding, and sometimes it points at something rather than
    /// naming it; searching the raw words either dilutes the one term that
    /// mattered or searches for nothing at all and reports that as an answer.
    /// The three things `Intent::Files` was written to do: convert one kind of
    /// file into another, join several, or find one. They share a phrase list
    /// (`convert this`, `join these`, `what is this file`) and, until now, a
    /// single answer -- every one of them went to `find_files` and came back as
    /// a keyword search. "Convert this pdf to text" looked the disk over for
    /// files whose *names* held the words *pdf*, *to* and *text*, which is a
    /// well-formed answer to a question nobody asked.
    ///
    /// A conversion is not a search, so it is answered as one: `files::convert`
    /// knows what each change costs you (a sheet to text loses its formulas)
    /// and what it plainly cannot do (an archive does not become a PDF). Only
    /// what is *not* a recognised conversion falls through to the filename
    /// search, so the existing "find me the note about..." path is untouched.
    fn files_request(&self, what: &str) -> String {
        if let Some(answer) = convert_answer(what) {
            return answer;
        }
        self.find_files(what)
    }

    fn find_files(&self, what: &str) -> String {
        let prepared = match crate::asking::prepare(what) {
            Ok(p) => p,
            // Asked, not guessed. An invented referent retrieves confidently
            // and wrongly, and you cannot tell that from a real answer.
            Err(unsearchable) => return unsearchable.ask(),
        };

        // Each term separately: the index matches filenames, and a filename
        // almost never contains every word of a spoken question.
        //
        // Each term gives its own ranked list; they are merged by Reciprocal
        // Rank Fusion (`bm25::rrf`, k = 60), so a file several terms find
        // rises above one only the first term found. Before, the first term's
        // list simply came first, whatever the others said.
        let mut names: Vec<(String, String)> = Vec::new();
        let mut lists: Vec<Vec<u64>> = Vec::new();
        for term in &prepared.terms {
            let mut list = Vec::new();
            for e in self.index.search(term) {
                let at = match names.iter().position(|(p, _)| *p == e.path) {
                    Some(i) => i,
                    None => {
                        names.push((e.path.clone(), e.name.clone()));
                        names.len() - 1
                    }
                };
                list.push(at as u64);
            }
            lists.push(list);
        }
        let found: Vec<(String, String)> = crate::bm25::rrf(&lists, crate::bm25::RRF_K)
            .into_iter()
            .map(|(i, _)| names[i as usize].clone())
            .collect();

        let mut answer = if found.is_empty() {
            // Filenames matched nothing. Before giving up, look *inside* the
            // documents and code -- "the thing about the budget" rarely has
            // the word "budget" in its name. This is the expensive search: it
            // reads files off the disk, so it runs only now, once the cheap
            // pass over filenames has already come back empty. Skipped when
            // there is no indexing config, because then the size cap and the
            // roots that bound the read do not exist.
            let inside: Vec<crate::index::ContentHit> = match self.cfg.indexing.as_ref() {
                Some(idx_cfg) => {
                    // All the terms at once: the content search ranks by
                    // BM25, which scores a chunk holding several of them
                    // above one holding a single word, and that only works
                    // if it sees them together.
                    self.index.search_content(&prepared.terms.join(" "), idx_cfg, 3)
                }
                None => Vec::new(),
            };

            // Says what it looked for. "I couldn't find anything" leaves you
            // unable to tell a bad search from an empty disk.
            let caveat = match self.index.missed.caveat() {
                Some(c) => format!(" {c}"),
                None => String::new(),
            };

            if inside.is_empty() && self.index_load.is_loading() {
                // Not "nothing matches": the list isn't all here yet.
                crate::index::STILL_READING.to_string()
            } else if inside.is_empty() {
                // "Found nothing" and "could not look" are different answers,
                // and the index already knows which one this was.
                format!(
                    "Nothing in the {} indexed files matches {}.{}",
                    self.index.entries.len(),
                    prepared.searched_for(),
                    caveat
                )
            } else {
                // Nothing was *named* for it, but something *says* it. Show
                // the line each was found on, so you can tell a real match
                // from a word that happened to appear.
                let shown: Vec<String> = inside
                    .iter()
                    .take(3)
                    .map(|h| format!("{} -- {}", h.cite, h.excerpt))
                    .collect();
                format!(
                    "No filename matches {}, but it's written inside {}.{}",
                    prepared.searched_for(),
                    shown.join("; "),
                    caveat
                )
            }
        } else {
            // What each one is, not just what it's called. A search that
            // returns five names you can't open is a list, not an answer —
            // and `files::Sort` knows which of them needs something you may
            // not have.
            let shown: Vec<String> = found
                .iter()
                .take(5)
                .map(|(_, n)| {
                    let sort = crate::files::Sort::of(n);
                    if sort.readable_offline() {
                        n.clone()
                    } else {
                        format!("{n} (needs {})", sort.needs())
                    }
                })
                .collect();
            let more = found.len().saturating_sub(shown.len());
            let tail = if more > 0 {
                format!(" and {more} more")
            } else {
                String::new()
            };
            format!(
                "Searching {} — {}{}.",
                prepared.searched_for(),
                shown.join(", "),
                tail
            )
        };

        // Two questions in one utterance retrieve the average of two topics
        // and the best match for neither. Said rather than silently dropped.
        if prepared.is_more_than_one_question() {
            answer.push_str(&format!(
                " That was two questions — I answered the first. The other was: {}",
                prepared.also.join("; ")
            ));
        }
        answer
    }

}

/// Beyond the follow-up window itself, how long the words may take to be
/// made out before the loop stops waiting (`Daemon::follow_up`).
const FOLLOW_UP_HEARING_SECS: u64 = 30;

/// Where what cutting in by voice learned about the speakers and the
/// microphone is kept between runs (`micthread::Learned`).
const CUT_IN_STATE: &str = "cut_in";

/// The talk key, while a reply is being said: held, the reply stops at once
/// and you're listened to for as long as it's down. A stop or a pause is
/// returned as said; anything else is kept in `cut_in`, to be answered
/// next, and returned as "hold on".
fn key_cut_in(
    keys: Option<&crate::hotkeys::Hotkeys>,
    ears: &dyn Ears,
    cut_in: &std::cell::RefCell<Option<String>>,
) -> Option<String> {
    let keys = keys?;
    if !keys.held() {
        return None;
    }
    // Stopped now, not at the end of the sentence playing.
    crate::micthread::cut_playback();
    let words = ears.listen_while(&|| keys.held()).ok().flatten().unwrap_or_default();
    if crate::speech::is_interruption(&words) {
        return Some(words);
    }
    *cut_in.borrow_mut() = Some(words);
    Some("hold on".to_string())
}

/// A reply being said, as the loop sees it (`speakthread`).
impl crate::speakthread::Host for Daemon<'_> {
    fn line(&mut self, chunk: &str) {
        println!("{chunk}");
        self.log.info(chunk);
    }
    fn between(&mut self) {
        // The hub while Atlas speaks: every page used to wait for the
        // sentence being said, and before 28 Sep 2026 for the whole reply.
        self.answer_hub_mid_reply();
        // And the icon by the clock: its Pause quiets the reply (`hush`).
        self.answer_tray(crate::store::now());
    }
    fn hush(&mut self) -> Option<String> {
        // Paused on the hub or the icon mid-reply (possible now that the hub
        // is answered while a sentence plays): quiet at once, the rest kept
        // for "carry on".
        self.attention.is_paused().then(|| "pause".to_string())
    }
}

/// Whether a queued command cannot run without a connection.
///
/// The same shape as `lanes::lane_for`: a keyword reading of the command's
/// own words, not an inspection of what it will do — which is all that can
/// be known before it runs. Kept deliberately narrow: a false `false` means
/// a task fails with an honest network error when it runs offline, while a
/// false `true` means work silently parked on a machine that could have
/// done it. The second is worse, so only commands that are unambiguously
/// about the network are named.
fn command_needs_connection(command: &str) -> bool {
    let c = command.to_lowercase();
    const ONLINE: &[&str] = &[
        "research", "look into", "look up", "search the web", "fetch", "download",
        "check mail", "check my mail", "check the mail", "unsubscribe", "sync",
        "post ", "publish",
    ];
    ONLINE.iter().any(|k| c.contains(k))
}

/// Strip a leading correction lead-in so `facts::triple` reads the statement
/// itself, not the marker: "actually my car is a Toyota" → "my car is a
/// Toyota". Case-insensitive on the prefix, but the original casing of the
/// statement is preserved for the acknowledgement. Only removes a lead-in it
/// finds at the very start, so a mid-sentence "actually" is left alone.
fn strip_correction_lead(raw: &str) -> String {
    let mut s = raw.trim();
    const LEADS: &[&str] = &[
        "actually,", "actually", "correction:", "correction,", "correction",
        "i meant", "no,", "scratch that,", "scratch that", "update:", "just so you know,",
        "for the record,",
    ];
    loop {
        let low = s.to_ascii_lowercase();
        let mut cut = None;
        for lead in LEADS {
            if low.starts_with(lead) {
                cut = Some(lead.len());
                break;
            }
        }
        match cut {
            Some(n) => s = s[n..].trim_start(),
            None => break,
        }
    }
    // A trailing "now"/"anymore" is a correction marker, not part of the value.
    let mut out = s.to_string();
    for tail in [" now", " anymore", " these days"] {
        if out.to_lowercase().ends_with(tail) {
            out.truncate(out.len() - tail.len());
        }
    }
    out.trim().to_string()
}

/// The kind a refile correction names, if it names one.
///
/// "that's actually a task", "refile that as an idea" -- the word after the
/// article is what decides. Returns `None` when the correction is about where
/// the note belongs (a handle) rather than what sort of thing it is, which is
/// what `refile_handle_from` then reads. A bare "note" is deliberately not a
/// kind here: "file that under my notes" is a handle, and treating the word as
/// a kind would swallow it.
fn refile_kind_from(correction: &str) -> Option<crate::capture::Kind> {
    use crate::capture::Kind;
    let t = correction.to_lowercase();
    if t.contains("task") || t.contains("to do") || t.contains("todo") || t.contains("remind") {
        Some(Kind::Task)
    } else if t.contains("idea") {
        Some(Kind::Idea)
    } else if t.contains("question") {
        Some(Kind::Question)
    } else if t.contains("decision") || t.contains("decided") {
        Some(Kind::Decision)
    } else if t.contains("quote") {
        Some(Kind::Quote)
    } else if t.contains("fact") {
        Some(Kind::Fact)
    } else {
        None
    }
}

/// The handle a refile correction files the note under, if any.
///
/// "file that under the roof job" -> "roof job". The leading "under"/"with"
/// and any article are stripped so the handle is the name you would later
/// reach for it by, not the sentence around it.
fn refile_handle_from(correction: &str) -> Option<String> {
    let mut h = correction.trim().to_lowercase();
    for lead in ["under ", "with ", "as "] {
        if let Some(rest) = h.strip_prefix(lead) {
            h = rest.to_string();
            break;
        }
    }
    for article in ["the ", "a ", "an ", "my "] {
        if let Some(rest) = h.strip_prefix(article) {
            h = rest.to_string();
            break;
        }
    }
    let h = h.trim().to_string();
    if h.is_empty() {
        None
    } else {
        Some(h)
    }
}

/// A spoken conversion request turned into what `files::convert` says about
/// it, or `None` when it is not a conversion at all.
///
/// `Some` only when both sides of "... to ..." name a format Atlas
/// recognises. That is what keeps an ordinary search which happens to contain
/// the word " to " -- "the note about how to bake bread" -- out of the
/// conversion path: neither "how" nor "bake bread" is a format, so it declines
/// and the filename search answers it.
fn convert_answer(what: &str) -> Option<String> {
    use crate::files::{convert, Convert};

    let lower = what.to_lowercase();
    let (before, after) = lower.split_once(" to ")?;
    let from = sort_named(before)?;
    let to = sort_named(after)?;
    let said = match convert(from, to) {
        Convert::Can { how, loses: None } => format!("Yes -- I'd {how}."),
        Convert::Can { how, loses: Some(l) } => format!("I'd {how}, though you'd lose {l}."),
        Convert::CanWithLoss { how, loses } => format!("I can {how}, but you'd lose {loses}."),
        Convert::Cannot(why) => format!("I can't -- {why}."),
    };
    Some(said)
}

/// The formats people name out loud, each mapped to the `Sort` that carries
/// its conversion rules. Deliberately narrow: a word it does not know returns
/// `None`, so `convert_answer` declines rather than reading a conversion into
/// an ordinary sentence.
fn sort_named(text: &str) -> Option<crate::files::Sort> {
    use crate::files::Sort;
    text.split_whitespace().find_map(|w| {
        let w = w.trim_matches(|c: char| !c.is_alphanumeric());
        Some(match w {
            "text" | "txt" | "plain" => Sort::Text,
            "word" | "doc" | "docx" | "document" => Sort::Document,
            "excel" | "spreadsheet" | "sheet" | "xlsx" | "csv" => Sort::Sheet,
            "pdf" => Sort::Pdf,
            "picture" | "image" | "photo" | "png" | "jpg" | "jpeg" => Sort::Picture,
            "audio" | "sound" | "mp3" | "recording" => Sort::Audio,
            "video" | "movie" | "mp4" => Sort::Video,
            _ => return None,
        })
    })
}

/// A short, sayable title for a queued change, from the request. The user
/// refers to it by this — "implement the date parser" — so it is the first
/// meaningful few words, not a hash.
fn workshop_title(request: &str) -> String {
    // Drop a leading project/verb phrase so the title is about the change,
    // not the project. "on the atlas project, add a date parser" -> "add a
    // date parser".
    let mut r = request.trim();
    for lead in ["on the ", "on ", "in the ", "in ", "for the ", "for "] {
        if let Some(rest) = r.strip_prefix(lead) {
            // Skip up to the first comma, which usually ends the project clause.
            if let Some(idx) = rest.find(',') {
                r = rest[idx + 1..].trim();
                break;
            }
        }
    }
    let words: Vec<&str> = r.split_whitespace().take(7).collect();
    let title = words.join(" ");
    let title = title.trim_end_matches(['.', ',', '!', '?']).trim();
    if title.is_empty() {
        "the change".to_string()
    } else {
        title.to_string()
    }
}

/// A little context from a project folder to write the draft against — the
/// names and first lines of a few source files, not the whole tree. Empty
/// when the folder is unknown or unreadable. Deliberately bounded: this is
/// orientation for the model, not the whole codebase.
/// Turn a generated draft for a project into a queued change, verified as
/// strongly as the project allows, with a summary that never overstates what
/// was proven.
///
/// Two levels, and the honest gap between them is the whole reason this exists:
/// - **Integrated.** When the request names a file the project already has, and
///   the project is on disk and buildable, the change is proved *inside a copy
///   of the project* — the project still builds and its own tests still pass
///   with the change in it. This is real verification for that project.
/// - **Isolated.** Otherwise (a new file, no named target, or the project
///   can't be reached), the check is only that the code compiles on its own,
///   and the summary says exactly that — a proposal to read, not a proven
///   change. `Outcome::in_project` carries that wording.
///
/// Returns `(verified, summary, files)`. `verified` is only true for an
/// integrated pass that replaced real code; an isolated compile is never
/// `verified` for a project.
fn verify_project_change(
    project: &str,
    folder: &str,
    title: &str,
    lang: crate::craft::Lang,
    request: &str,
    code: &str,
    isolated: &crate::build_it::Outcome,
    base: &std::path::Path,
) -> (bool, String, Vec<crate::workshop::FileEdit>) {
    let fallback_path = format!("proposed_change.{}", ext_for(lang));
    let root = std::path::Path::new(folder);

    // The file this change belongs in, if the request names one the project
    // actually has. Deterministic — a real path in the words, not a guess —
    // because integrated verification is only sound when the change replaces
    // code the project already compiles and tests.
    let target = named_existing_file(request, root);

    // The project's own test command, from its language's ladder.
    let test_cmd = crate::craft::ladder(lang)
        .into_iter()
        .find(|g| g.tells == crate::craft::Tells::Behaviour)
        .map(|g| g.command);

    // The path the queued change writes to: the real file when known, else a
    // clearly-named proposal file.
    let write_path = target.clone().unwrap_or(fallback_path);
    let files = vec![crate::workshop::FileEdit { path: write_path.clone(), content: code.to_string() }];

    // Integrated verification is possible only for a built draft that replaces
    // a real file in a buildable project on disk.
    match (isolated.is_built(), target.as_ref(), test_cmd, root.is_dir()) {
        (true, Some(rel), Some(cmd), true) => {
            let edits = vec![crate::selfwork::Edit {
                path: rel.clone(),
                content: code.to_string(),
                reason: String::new(),
            }];
            match crate::selfwork::prove_in_project(root, &cmd, &edits, base) {
                Ok(proof) => {
                    let verified = proof.built_and_passed && proof.replaced_existing;
                    let summary = format!(
                        "Queued \"{title}\" on {project}. {} Say \"implement {title}\" and I'll \
                         write it with a .before backup; nothing running is touched.",
                        proof.plain(project)
                    );
                    (verified, summary, files)
                }
                // The project's suite couldn't be run — fall back to the honest
                // isolated wording rather than claiming a project check that
                // didn't happen.
                Err(why) => {
                    let mut s = isolated.in_project(project, title, lang);
                    s.push_str(&format!(
                        " (I couldn't run {project}'s own tests to check it in place: {why}.)"
                    ));
                    (false, s, files)
                }
            }
        }
        // Isolated only: a new file, no named target, an unbuildable/absent
        // project, or a draft that didn't even pass its own checks. An isolated
        // compile is never "verified" for a project — that flag is reserved for
        // an integrated pass.
        _ => (false, isolated.in_project(project, title, lang), files),
    }
}

/// A source file the request names that the project actually has, as a
/// project-relative path. `None` when the words name no such file — the signal
/// that this is a new-file change, not a replacement.
fn named_existing_file(request: &str, root: &std::path::Path) -> Option<String> {
    for tok in request.split(|c: char| c.is_whitespace() || c == '"' || c == '`' || c == '(' || c == ')') {
        let t = tok.trim_matches(|c: char| !c.is_alphanumeric() && c != '/' && c != '.' && c != '_' && c != '-');
        if t.is_empty() || crate::craft::Lang::of_path(t).is_none() {
            continue;
        }
        if root.join(t).is_file() {
            return Some(t.to_string());
        }
    }
    None
}

/// Does the request point at something Atlas built recently, rather than a file
/// or pasted code? Tied to Atlas's own action ("you built", "you made") so
/// "explain the code that builds X" isn't caught by the bare word "build".
fn references_a_build(low: &str) -> bool {
    [
        "you built", "you made", "you just built", "just built", "last build",
        "recently built", "the build", "thing you built",
    ]
    .iter()
    .any(|p| low.contains(p))
}

/// Does the request point at a change waiting to be implemented? "the change"
/// and "waiting" are broad, but the fallback when nothing is queued is a plain
/// "nothing waiting", so a false match is harmless.
fn references_a_queued_change(low: &str) -> bool {
    [
        "waiting", "queued", "to implement", "to be implemented", "the change",
        "proposed change", "on implementation", "change you",
    ]
    .iter()
    .any(|p| low.contains(p))
}

fn read_project_context(folder: &str) -> String {
    if folder.trim().is_empty() {
        return String::new();
    }
    let root = std::path::Path::new(folder);
    if !root.is_dir() {
        return String::new();
    }
    let mut out = String::new();
    let mut count = 0;
    // A shallow look at src/, then the root, for source files.
    for sub in ["src", "."] {
        let dir = root.join(sub);
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            if count >= 6 {
                break;
            }
            let path = entry.path();
            let is_source = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| matches!(e, "rs" | "py" | "js" | "ts" | "go"))
                .unwrap_or(false);
            if !is_source {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(&path) {
                let head: String = text.lines().take(40).collect::<Vec<_>>().join("\n");
                out.push_str(&format!("--- {} ---\n{head}\n\n", path.display()));
                count += 1;
            }
        }
    }
    // Bound the whole thing so a big file can't blow the prompt.
    if out.len() > 6000 {
        out.truncate(6000);
    }
    out
}

/// The file extension for a generated draft, so it lands under a name you can
/// open. The language itself is the authority on this, so there is no match to
/// keep in step here.
fn ext_for(lang: crate::craft::Lang) -> &'static str {
    lang.ext()
}

/// Copy the tree's compile inputs into a scratch directory, so a candidate
/// self-fix can be built and tested in isolation from the tree you run.
///
/// Everything a `cargo test` needs and nothing it produces: `src/`, `tests/`,
/// `config/`, `benches/`, the manifests and `build.rs` — never `target/`,
/// `.git/`, or another sandbox, which are huge and rebuildable. A cold build is
/// the cost of not touching your real files; that is the trade this makes on
/// purpose.
fn copy_compile_inputs(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    const TOP: &[&str] = &["src", "tests", "config", "benches", "examples"];
    const FILES: &[&str] = &["Cargo.toml", "Cargo.lock", "build.rs", "rust-toolchain.toml"];
    std::fs::create_dir_all(to)?;
    for d in TOP {
        let src = from.join(d);
        if src.is_dir() {
            copy_dir_shallowly(&src, &to.join(d))?;
        }
    }
    for f in FILES {
        let src = from.join(f);
        if src.is_file() {
            std::fs::copy(&src, to.join(f))?;
        }
    }
    Ok(())
}

/// Recursively copy a directory, following the same "no symlinks, no target"
/// rule the rest of the tree uses for copies.
fn copy_dir_shallowly(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_symlink() {
            continue;
        }
        let name = entry.file_name();
        // Never carry a build dir or a nested VCS/scratch tree along.
        if matches!(name.to_str(), Some("target") | Some(".git")) {
            continue;
        }
        let dst = to.join(&name);
        if ty.is_dir() {
            copy_dir_shallowly(&entry.path(), &dst)?;
        } else {
            std::fs::copy(entry.path(), dst)?;
        }
    }
    Ok(())
}

/// Scaffold a draft into a sandbox and run `craft`'s ladder against it — the
/// local fact-check that decides whether a draft is trusted. Returns the
/// ladder's verdict as a `build_it::Check`.
///
/// A Rust draft is written as a tiny crate (a `Cargo.toml` plus `src/lib.rs`)
/// so `cargo check`/`clippy`/`test` have something to build; a Python draft is
/// written as a single file. The gates run in the ladder's order and stop at
/// the first blocking failure, because a next attempt working from the output
/// of code that does not compile is working from noise.
fn check_draft_in_sandbox(
    sandbox: &mut crate::sandbox::Sandbox,
    lang: crate::craft::Lang,
    code: &str,
) -> crate::build_it::Check {
    use crate::craft::{ladder, read_ladder, Next, Ran, Tells};

    // Lay the draft down as something the toolchain can act on. The scaffold
    // is the language's own (a Rust crate, a Go module, a tsconfig beside the
    // file), so the ladder's commands always have what they expect.
    for (path, contents) in lang.draft_files(code) {
        if let Err(e) = sandbox.write(&path, &contents) {
            return crate::build_it::Check::Failed(format!("couldn't scaffold the draft: {e}"));
        }
    }

    let mut ran: Vec<Ran> = Vec::new();
    for gate in ladder(lang) {
        // Build a runnable tool from the gate's command line.
        let mut parts = gate.command.split_whitespace();
        let Some(program) = parts.next() else { continue };
        let args: Vec<String> = parts.map(str::to_string).collect();
        let tool = crate::tools::ExternalTool {
            command: program.to_string(),
            args,
            stdin_text: false,
            result_file: None,
            // Generous against the gate's rough estimate; a compile that runs
            // far past it is stuck, not slow.
            timeout_secs: (gate.seconds as u64) * 4 + 30,
        };
        let attempt = sandbox.run(&tool, &Default::default(), 4000);
        ran.push(Ran {
            command: gate.command.clone(),
            tells: gate.tells,
            passed: attempt.passed,
            output: attempt.output.clone(),
        });
        // Stop after the first blocking failure — the rest would be noise.
        if !attempt.passed && gate.tells == Tells::Sound {
            break;
        }
    }

    match read_ladder(lang, &ran) {
        Next::Good => crate::build_it::Check::Passed(vec![]),
        Next::WorksWithNotes(notes) => crate::build_it::Check::Passed(notes),
        Next::Fix { output, .. } => crate::build_it::Check::Failed(output),
    }
}

/// The permission gate's answer for acting on an app: go ahead, or ask this
/// first (and the action is parked as a pending approval).
enum AppGate {
    Go,
    Ask(String),
}

/// Spoken channel name to a Channel.
fn channel_named(name: &str) -> Channel {
    match name.trim().to_lowercase().as_str() {
        "x" | "twitter" => Channel::X,
        "linkedin" => Channel::LinkedIn,
        "instagram" | "insta" => Channel::Instagram,
        "facebook" => Channel::Facebook,
        "discord" => Channel::Discord,
        "email" | "mail" => Channel::Email { to: String::new(), subject: String::new() },
        other => Channel::Other(other.to_string()),
    }
}

/// The target an intent is acting on, if it has one.
/// The (app, action) pair an intent acts on, when it is one the permission
/// gate covers. Used to record a grant against the exact action that was
/// asked about, so approving "open Foo" does not silently also allow closing
/// it.
fn app_action_of(i: &Intent) -> Option<(String, String)> {
    match i {
        Intent::OpenApp(a) => Some((a.clone(), "open".into())),
        Intent::CloseApp(a) => Some((a.clone(), "close".into())),
        Intent::FocusApp(a) => Some((a.clone(), "focus".into())),
        _ => None,
    }
}

fn argument_of(i: &Intent) -> Option<String> {
    match i {
        Intent::OpenApp(a)
        | Intent::CloseApp(a)
        | Intent::FocusApp(a)
        | Intent::Research(a)
        | Intent::DraftPost(a)
        | Intent::SetMode(a) => Some(a.clone()),
        _ => None,
    }
}

/// A short label for what a turn was about, so returning to the thread days
/// later can name it.
fn topic_of(i: &Intent) -> Option<String> {
    match i {
        Intent::Research(t) => Some(t.clone()),
        Intent::OpenApp(a) | Intent::CloseApp(a) | Intent::FocusApp(a) => Some(a.clone()),
        Intent::DraftPost(c) => Some(format!("the {c} post")),
        Intent::SetMode(m) => Some(format!("{m} mode")),
        Intent::WorkspaceOn | Intent::WorkspaceOff => Some("the workspace".into()),
        _ => None,
    }
}

fn report(r: Result<workspace::Report>, ok: &str) -> String {
    match r {
        Ok(r) if r.ok() => ok.to_string(),
        // `Report::plain`, not a list of `failed`. This used to name only the
        // apps that failed, which meant a bring-up that ran out of its
        // wall-clock budget -- and so never even tried the last two apps --
        // reported no failures and came back "Workspace online." `not_reached`
        // and `abandoned` exist for exactly that, and `ok()` now counts them,
        // so this arm has to be able to say what they are.
        Ok(r) => format!("Partly. {}", r.plain()),
        Err(e) => format!("error: {e}"),
    }
}

fn act(r: Result<()>, ok: &str) -> String {
    match r {
        Ok(()) => ok.to_string(),
        Err(e) => format!("error: {e}"),
    }
}

/// A name as it comes off `raw_argument` parsing, cleaned up for use as a
/// pairing peer's name.
///
/// `raw_argument` exists so a pairing *code* survives intact -- but `Pair`
/// and `ForgetPeer` take a name, not a code, and the same raw path that
/// protects a code also lets a spoken sentence's trailing "." or "?" ride
/// straight into what gets stored. Trimmed here rather than by widening
/// `raw_argument` itself, which would corrupt a code the same way
/// `normalize()` used to -- see `intent.rs`'s doc comment on why codes need
/// the parser to leave them alone completely. Case is left as given: it's
/// `kin::Pairings`' own case-insensitive comparison (`same_name`) that makes
/// "Sarah" and "sarah" the same pairing, not lowercasing here, so a name
/// typed with its proper capitalization still displays that way.
fn spoken_name(raw: &str) -> &str {
    raw.trim().trim_end_matches(['.', '!', '?', ',', ';', ':'])
}


/// Should this go in the history, and how would it be taken back?
///
/// Reading, asking and explaining leave nothing behind. Only things that
/// changed your world are worth being able to undo.
/// Enough of what was asked to recognise a slow turn later.
///
/// Short on purpose: the timing window lives in memory for the life of the
/// process, and keeping whole utterances in it would turn a latency measure
/// into a transcript nobody asked for.

/// How long the video is, from what ffmpeg said about it.
///
/// Read from the scan Atlas already ran rather than a second `ffprobe` call.
/// Zero when it cannot be found, which `where_to_look` treats as "no gaps to
/// fill" — the cautious direction: fewer frames, never a runaway.
fn seconds_of(told: &str) -> f32 {
    told.split("Duration:")
        .nth(1)
        .and_then(|rest| rest.split(',').next())
        .and_then(|stamp| {
            let parts: Vec<&str> = stamp.trim().split(':').collect();
            match parts.as_slice() {
                [h, m, s] => Some(
                    h.parse::<f32>().ok()? * 3600.0
                        + m.parse::<f32>().ok()? * 60.0
                        + s.parse::<f32>().ok()?,
                ),
                _ => None,
            }
        })
        .unwrap_or(0.0)
}


/// Fill `{placeholders}` in a tool's arguments from the config vars.
///
/// The same substitution `ExternalTool` does when it runs something, needed
/// here because the camera arguments are reused rather than run.
fn resolved(args: &[String], vars: &std::collections::BTreeMap<String, String>) -> Vec<String> {
    args.iter()
        .map(|a| {
            let mut out = a.clone();
            for (k, v) in vars {
                out = out.replace(&format!("{{{k}}}"), v);
            }
            out
        })
        .collect()
}

/// "Sam: the roof quote came back high" -> ("Sam", "the roof quote...").
///
/// Two shapes, because people say both. A colon is unambiguous and is what
/// anybody typing will use. Without one, the first word is the name -- which
/// is why `commands.yaml` deliberately does not list "tell" as a phrase for
/// this: "tell me what's outstanding" would become a message to somebody
/// called "me".
/// Whether two member lists name the same set of people, case-insensitively.
/// Used to recognise "message Jordan and Maya" as the group you already have
/// with exactly those two, rather than minting a second one each time.
fn same_member_set(a: &[String], b: &[String]) -> bool {
    a.len() == b.len()
        && a.iter()
            .all(|x| b.iter().any(|y| crate::kin::same_name(x, y)))
}

fn split_who_and_what(raw: &str) -> (String, String) {
    let t = raw.trim();
    if let Some((who, body)) = t.split_once(':') {
        return (who.trim().to_string(), body.trim().to_string());
    }
    match t.split_once(char::is_whitespace) {
        Some((who, body)) => {
            let body = body.trim();
            // "Sam that the roof quote came back high" -- the filler word
            // people put in when they are speaking rather than typing.
            let body = body.strip_prefix("that ").unwrap_or(body);
            (who.trim().to_string(), body.trim().to_string())
        }
        None => (t.to_string(), String::new()),
    }
}

/// An hour of the day, the way a person says it.
fn oclock(hour: u32) -> String {
    match hour % 24 {
        0 => "midnight".into(),
        12 => "midday".into(),
        h if h < 12 => format!("{h}am"),
        h => format!("{}pm", h - 12),
    }
}

/// This machine's offset from UTC, in minutes.
///
/// Worked out rather than configured: a config field would be a second
/// declaration of a fact the operating system already knows, and it would be
/// wrong twice a year. Falls back to 0 when it cannot be read -- which shows
/// a message in UTC rather than refusing to send it, because the timestamp is
/// a convenience and the message is the point.
///
/// On Windows there is no `date +%z`; this used to fall back to 0 there, so
/// every chat timestamp was shown in UTC and anything scheduled "daily at
/// 08:00" would have fired at 08:00 UTC. Asked of Windows itself now, and
/// the answer is kept for ten minutes rather than asked for on every tick --
/// long enough to cost nothing, short enough to follow a clock change.
pub(crate) fn local_offset_mins() -> i16 {
    // Through `localclock`, which asks Windows directly. This asked
    // `date +%z`, which Windows doesn't have, so on the laptop every
    // message went out stamped as UTC.
    (crate::localclock::offset_secs() / 60) as i16
}

/// `+0530` / `-0500` -> minutes. Separate from the command that produces it
/// so the parsing can be tested without caring what this machine's clock is
/// set to.
pub fn parse_offset(raw: &str) -> i16 {
    let raw = raw.trim();
    if raw.len() < 5 {
        return 0;
    }
    let sign = match raw.as_bytes()[0] {
        b'-' => -1,
        b'+' => 1,
        _ => return 0,
    };
    // All four digits, or none of it. `unwrap_or(0)` per field was the first
    // version and it turned junk into a plausible answer: "++0500" parsed its
    // hours as "+0", failed, took 0 -- and then read "50" as the minutes and
    // returned a real-looking offset of fifty minutes. A parser that cannot
    // fail will always prefer a wrong answer to no answer.
    let digits = &raw[1..5];
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return 0;
    }
    let hours: i16 = digits[0..2].parse().unwrap_or(0);
    let mins: i16 = digits[2..4].parse().unwrap_or(0);
    if hours > 14 || mins > 59 {
        // No real zone is further out than 14 hours. Past that it is not an
        // offset, whatever it looks like.
        return 0;
    }
    sign * (hours * 60 + mins)
}

fn about_short(said: &str) -> String {
    let trimmed = said.trim();
    // Char boundaries, not byte offsets. Slicing a string by byte index
    // panics on any accented character or em dash, which is routine in
    // anything transcribed or pasted.
    let mut out: String = trimmed.chars().take(48).collect();
    if trimmed.chars().count() > 48 {
        out.push('\u{2026}');
    }
    out
}

fn clock() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn worth_recording(intent: &Intent, said: &str) -> Option<(String, &'static str, crate::undo::Undo)> {
    use crate::undo::Undo;
    // A failure didn't change anything, so it isn't undoable.
    if said.starts_with("I can't") || said.contains("didn't work") {
        return None;
    }
    Some(match intent {
        Intent::WorkspaceOn => ("brought the workspace up".into(), "windows",
            Undo::Atlas("take it back down".into())),
        Intent::WorkspaceOff => ("put the workspace away".into(), "windows",
            Undo::Atlas("bring it back".into())),
        Intent::OpenApp(a) => (format!("opened {a}"), "windows", Undo::Atlas(format!("close {a}"))),
        Intent::CloseApp(a) => (format!("closed {a}"), "windows", Undo::Atlas(format!("open {a}"))),
        Intent::SetMode(m) => (format!("switched to {m}"), "settings",
            Undo::Atlas("switch back".into())),
        Intent::DraftPost(_) => ("wrote a draft".into(), "posting",
            Undo::Atlas("throw the draft away".into())),
        Intent::BackUp => ("backed itself up".into(), "atlas",
            Undo::Cannot("a backup isn't something to take back".into())),
        // Everything else only read, asked or explained.
        _ => return None,
    })
}

/// Actually leaves the lists a cleanup plan named — the one send Atlas
/// never needs asking about first, per Eric's own rule. Returns how many
/// went through and what went wrong for the rest, so a partial failure
/// is never silently rounded up to "done."
fn carry_out_unsubscribes(
    cleanup: &crate::unsub::Cleanup,
    account: &crate::mail::Account,
    password: &str,
) -> (usize, Vec<String>) {
    let provider = crate::mail::Provider::from_address(&account.address);
    let smtp_host = provider.smtp_host();
    let mut done = 0;
    let mut failures = Vec::new();
    for (name, how) in &cleanup.unsubscribe {
        let Some((target, extra)) = crate::unsub::one_click(how) else {
            failures.push(format!("{name}: no safe one-click method found"));
            continue;
        };
        let result = if let Some(url) = target.strip_prefix("https://").map(|_| target.as_str()) {
            post_one_click(url, extra)
        } else if let Some(to) = target.strip_prefix("mailto:") {
            let to = to.split('?').next().unwrap_or(to);
            match smtp_host {
                Some(h) => send_unsubscribe_email(
                    provider.smtp_port(),
                    h,
                    &account.address,
                    password,
                    to,
                    account.oauth.then_some(account.client_id.as_str()),
                ),
                None => Err("no SMTP server known for this provider".into()),
            }
        } else {
            Err(format!("unrecognised unsubscribe method: {target}"))
        };
        match result {
            Ok(()) => done += 1,
            Err(e) => failures.push(format!("{name}: {e}")),
        }
    }
    (done, failures)
}

/// RFC 8058 one-click: a POST with the exact body `unsub::one_click`
/// already worked out, no browser visit — a browser visit is what would
/// load their tracking. curl rather than a hand-rolled HTTPS client:
/// this is exactly the protocol curl is solid at, unlike the IMAP
/// support that ruled curl out for the mail client itself.
fn post_one_click(url: &str, body: &str) -> std::result::Result<(), String> {
    let out = crate::tools::command("curl")
        .args(["-sS", "-m", "20", "-X", "POST", "-d", body, url])
        .output()
        .map_err(|e| format!("couldn't run curl: {e}"))?;
    if !out.status.success() {
        return Err(format!("curl failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(())
}

/// The `mailto:` half of one-click unsubscribing: a real, empty email to
/// the address the sender itself provided, sent from the same account
/// the original message arrived at.
/// Authenticates an already-connected IMAP session, either way: an app
/// password via `LOGIN`, or — when `oauth_client_id` is set — a stored
/// refresh token traded for a fresh access token and used with
/// `AUTHENTICATE XOAUTH2`. One place this decision is made, so the four
/// call sites that connect to IMAP never have to make it themselves.
fn authenticate_imap<S: std::io::Read + std::io::Write>(
    session: &mut crate::imap::Session<S>,
    address: &str,
    password: &str,
    oauth_client_id: Option<&str>,
) -> std::result::Result<(), String> {
    match oauth_client_id {
        Some(client_id) => {
            let tokens = crate::msoauth::refresh(client_id, password)?;
            session.auth_xoauth2(address, &tokens.access_token)
        }
        None => session.login(address, password),
    }
}

/// The SMTP half of `authenticate_imap` — same decision, same reasoning,
/// `AUTH LOGIN` or `AUTH XOAUTH2` depending on whether this account is
/// OAuth-based.
fn authenticate_smtp<S: std::io::Read + std::io::Write>(
    session: &mut crate::smtp::Session<S>,
    address: &str,
    password: &str,
    oauth_client_id: Option<&str>,
) -> std::result::Result<(), String> {
    match oauth_client_id {
        Some(client_id) => {
            let tokens = crate::msoauth::refresh(client_id, password)?;
            session.auth_xoauth2(address, &tokens.access_token)
        }
        None => session.auth_login(address, password),
    }
}

fn send_unsubscribe_email(
    port: u16,
    host: &str,
    from_address: &str,
    from_password: &str,
    to: &str,
    oauth_client_id: Option<&str>,
) -> std::result::Result<(), String> {
    // The port comes from the provider's own answer. Two call sites held
    // a literal 465 while `mail::smtp_port` -- the named fact -- sat
    // uncalled; the day one provider wants 587 is the day the literals
    // would silently disagree with the name.
    let mut session = crate::smtp::connect(host, port)?;
    session.ehlo("atlas")?;
    authenticate_smtp(&mut session, from_address, from_password, oauth_client_id)?;
    session.send_mail(from_address, to, "unsubscribe", "")?;
    session.quit();
    Ok(())
}

/// Sends a drafted, approved reply for real — the same SMTP path as
/// unsubscribe's `mailto:` case, just with a real subject and body
/// instead of an empty message. `from_address`'s own provider decides
/// the SMTP host, the same lookup `check_unsubscribe` already uses.
fn send_reply(
    pending: &crate::outbox::PendingReply,
    from_address: &str,
    from_password: &str,
    oauth_client_id: Option<&str>,
) -> std::result::Result<(), String> {
    // Before connecting: a refused send costs nothing and opens no socket.
    crate::smtp::may_send(from_address, crate::store::now().saturating_mul(1000))?;
    let provider = crate::mail::Provider::from_address(from_address);
    let host = provider
        .smtp_host()
        .ok_or_else(|| "no SMTP server known for this provider".to_string())?;
    let mut session = crate::smtp::connect(host, provider.smtp_port())?;
    session.ehlo("atlas")?;
    authenticate_smtp(&mut session, from_address, from_password, oauth_client_id)?;
    session.send_mail(from_address, &pending.to_address, &pending.subject, &pending.body)?;
    session.quit();
    Ok(())
}

/// One account's worth of "what's unread in the inbox" — the whole
/// connect/login/search/fetch/logout sequence in one place, so
/// `check_mail`'s crew errand reads as a loop over accounts rather than a
/// loop over protocol steps.
///
/// Searches `UNSEEN` rather than everything: re-triaging mail you've
/// already read and decided about on every check would be noise, not
/// help. What matters is what's new since last time, which unread already
/// captures without Atlas having to keep its own separate record of it.
/// Asks the model for a reply to a client's message. Kept to the plain
/// text of the reply — no signature, no subject line, both of which
/// `check_mail` builds itself from data it already trusts, rather than
/// hoping the model doesn't invent one.
fn draft_client_reply(
    llm: &dyn crate::brain::Llm,
    client_name: &str,
    subject: &str,
    body: &str,
) -> std::result::Result<String, String> {
    let system = "You draft a short, professional email reply on behalf of the person you work \
                  for. Write only the reply body -- no subject line, no signature, no \
                  placeholder brackets. Keep it brief.";
    let user = format!("Reply to {client_name}, who wrote:\n\nSubject: {subject}\n\n{body}");
    llm.complete(system, &user).map_err(|e| e.to_string())
}

fn connect_and_fetch_inbox(
    host: &str,
    address: &str,
    password: &str,
    oauth_client_id: Option<&str>,
    sent_since: Option<&str>,
) -> std::result::Result<(Vec<crate::imap::Message>, std::result::Result<Vec<crate::imap::Message>, String>), String> {
    if let Some((program, account)) = crate::himalaya::route(host) {
        return crate::himalaya::fetch_inbox(&program, &account, sent_since);
    }
    let mut session = crate::imap::connect(host, 993)?;
    authenticate_imap(&mut session, address, password, oauth_client_id)?;
    let msgs = session.fetch_matching("INBOX", "UNSEEN")?;
    // What you sent, in the same session (round 11): the waiting-for list
    // and meeting prep read it from the mail cache. A Sent folder that
    // can't be found costs the inbox nothing -- it's reported, and the
    // inbox result stands.
    let sent = match sent_since {
        Some(since) => session.sent_mailbox().and_then(|mb| session.fetch_matching(&mb, &format!("SINCE {since}"))),
        None => Ok(Vec::new()),
    };
    session.logout();
    Ok((msgs, sent))
}

/// Same connect/login/logout shape as `connect_and_fetch_inbox`, but
/// `SINCE <date>` rather than `UNSEEN` — everything in the window,
/// read or not, which is what a real per-sender engagement count needs.
fn connect_and_fetch_since(
    host: &str,
    address: &str,
    password: &str,
    since: &str,
    oauth_client_id: Option<&str>,
) -> std::result::Result<Vec<crate::imap::Message>, String> {
    if let Some((program, account)) = crate::himalaya::route(host) {
        return crate::himalaya::fetch_since(&program, &account, since);
    }
    let mut session = crate::imap::connect(host, 993)?;
    authenticate_imap(&mut session, address, password, oauth_client_id)?;
    let msgs = session.fetch_matching("INBOX", &format!("SINCE {since}"))?;
    session.logout();
    Ok(msgs)
}

/// Maps how a crew errand ended to the outcome `watching` records. In one
/// place, so a new crew citizen never has to decide this for itself.
fn outcome_of(ending: &crew::Ending) -> watching::Outcome {
    match ending {
        crew::Ending::Done(Ok(_)) => watching::Outcome::Finished,
        crew::Ending::Done(Err(_)) => watching::Outcome::Failed,
        // Asked to stop, and it did. `watching` has no separate "stopped"
        // outcome — getting this wrong the other way (Failed) would be told
        // off for changing your mind, so this reads as a clean finish.
        crew::Ending::Stopped => watching::Outcome::Finished,
        crew::Ending::Vanished => watching::Outcome::Vanished,
    }
}

/// A device name, made safe to be a filename.
///
/// Device names are yours to choose ("Eric's laptop"); a filename is not.
fn sanitise(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    let trimmed = cleaned.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "device".into()
    } else {
        trimmed
    }
}

// ---------- reminders and opportunities: free helpers ----------
// Used by `Daemon::remind_help` and `Daemon::spot_opportunity`. Free functions
// rather than methods because they touch no daemon state — just words in, a
// value out — which keeps them unit-testable without a whole daemon.

/// The thing to be reminded of, scheduling words removed. Reminders read
/// "… to <text>", so the text is what follows the last " to ".
fn reminder_text(said: &str) -> String {
    if let Some(i) = said.rfind(" to ") {
        return said[i + 4..].trim().to_string();
    }
    let s = said.trim();
    s.strip_prefix("remind me")
        .or_else(|| s.strip_prefix("Remind me"))
        .unwrap_or(s)
        .trim()
        .to_string()
}

/// "in 20 minutes" / "in 2 hours" -> seconds, or None.
fn relative_secs(low: &str) -> Option<u64> {
    let idx = low.find(" in ")?;
    let mut w = low[idx + 4..].split_whitespace();
    let n: u64 = w.next()?.parse().ok()?;
    let unit = w.next()?;
    if unit.starts_with("min") {
        Some(n * 60)
    } else if unit.starts_with("hour") || unit.starts_with("hr") {
        Some(n * 3600)
    } else {
        None
    }
}

/// When a reminder should first fire, on the local clock. `Err(Some(why))`
/// when the words name a time that could mean two things (or has passed, or
/// is a day with no hour) -- the caller asks; `Err(None)` when no time was
/// named at all.
fn first_occurrence(raw: &str, now: u64) -> std::result::Result<u64, Option<String>> {
    let p = crate::when::parse(raw, now).ok_or(None)?;
    if !p.sure {
        return Err(p.why.or_else(|| Some("I'm not sure which time you mean.".into())));
    }
    if p.all_day {
        let c = crate::civil::Civil::from_local(p.start as i64);
        return Err(Some(format!("what time on {:04}-{:02}-{:02}?", c.year, c.month, c.day)));
    }
    Ok(p.start)
}

/// A local moment said the way a person checks it: "today at 17:00",
/// "tomorrow at 09:00", "on Fri 2026-10-02 at 15:00".
fn local_moment(at: u64, now: u64) -> String {
    let c = crate::civil::Civil::from_local(at as i64);
    let days = (at / 86_400) as i64 - (now / 86_400) as i64;
    let day = match days {
        0 => "today".to_string(),
        1 => "tomorrow".to_string(),
        _ => {
            let wd = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][c.weekday() as usize];
            format!("on {wd} {:04}-{:02}-{:02}", c.year, c.month, c.day)
        }
    };
    format!("{day} at {:02}:{:02}", c.hour, c.minute)
}

fn say_duration(secs: u64) -> String {
    if secs >= 3600 && secs % 3600 == 0 {
        let h = secs / 3600;
        format!("{h} hour{}", if h == 1 { "" } else { "s" })
    } else {
        let m = (secs / 60).max(1);
        format!("{m} minute{}", if m == 1 { "" } else { "s" })
    }
}

/// Does this read as a want or an idea Atlas should weigh as an opportunity?
/// Cue-based on purpose: it only fires on a stated wish, never on a question
/// or a command, so it sits last in the Unknown chain and claims nothing that
/// another door would answer.
fn reads_as_a_want(said: &str) -> bool {
    let t = said.to_lowercase();
    const CUES: &[&str] = &[
        "i want to",
        "i'd like to",
        "i would like to",
        "i wish",
        "it'd be good if",
        "it would be good if",
        "i've been meaning to",
        "ive been meaning to",
        "i've got an idea",
        "ive got an idea",
        "here's an idea",
        "heres an idea",
        "what if i",
        "i keep meaning to",
        "i should really",
    ];
    CUES.iter().any(|c| t.contains(c))
}

/// What a settings folder's two files hold, as one number: changes when
/// either file's contents do, whatever the file system does with times.
/// The settings files' sizes and modified times, hashed: what
/// `pick_up_settings` checks every tick before reading anything.
fn settings_stamp(dir: &std::path::Path) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for name in ["tools.yaml", "settings.yaml"] {
        match std::fs::metadata(dir.join(name)) {
            Ok(m) => {
                m.len().hash(&mut h);
                m.modified().ok().hash(&mut h);
            }
            Err(_) => 0u8.hash(&mut h),
        }
    }
    h.finish()
}

fn settings_fingerprint(dir: &std::path::Path) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for name in ["tools.yaml", "settings.yaml"] {
        std::fs::read(dir.join(name)).unwrap_or_default().hash(&mut h);
    }
    h.finish()
}

/// Which picture `look_closer` takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Capture {
    Screen,
    Camera,
}

/// A window being worked on your behalf — an errand like any other.
///
/// It lives beside the crew rather than in it only because it drives a
/// window, and `crew` errands run on their own threads and must not touch
/// your screen (`lanes`). Everything else follows the crew's rules: a new
/// request doesn't stop it, "stop the Slack one" pauses just it
/// (`which_errand`), pausing Atlas holds it with nothing lost, and typing
/// waits for a gap in your own typing, the way foreground work does.
pub struct WorkingForYou {
    /// In the same id space as the crew's errands, above them
    /// (`WINDOW_JOB_IDS`), so `which_errand` can pick it.
    pub id: u64,
    pub job: crate::delegate::Delegation,
    pub win: crate::platform::WindowId,
    /// What the window said right after Atlas last wrote, so it only replies
    /// to something new.
    pub after_mine: Option<String>,
    pub looked: u64,
    pub started: u64,
    /// Paused on its own ("pause the Slack one"). Holds; loses nothing.
    pub held: bool,
    /// Has something to write and is waiting for you to stop typing. Said
    /// once, not every look.
    pub waiting_for_gap: bool,
    /// The crew errand writing the next reply, while it writes.
    pub composing: Option<u64>,
    /// The screen that reply was written from.
    pub composed_from: Option<String>,
    /// A reply written and waiting for a gap to be typed.
    pub ready: Option<String>,
}

/// Ask every seat in one round and read back what each said. A seat that
/// couldn't be reached is left out; every call is noted for the record.
fn ask_seats(
    llm: &dyn crate::brain::Llm,
    prompts: &[(String, String)],
    ctl: &crew::Control,
    calls: &mut Vec<SeatCall>,
    retests: bool,
) -> Vec<crate::council::Opinion> {
    let mut opinions = Vec::new();
    for (seat, prompt) in prompts {
        if ctl.checkpoint() {
            break;
        }
        let started = std::time::Instant::now();
        let answer = llm.complete("Answer as the seat you are given. Be brief.", prompt);
        // Graded from what the seat did with its brief: committed, or not;
        // in a retesting room, a no that says what would make it a yes.
        let grade = answer.as_ref().ok().map(|reply| {
            let o = crate::council::parse_opinion(seat, reply);
            if o.lean == crate::council::Lean::Depends && o.would_change_my_mind.is_none() {
                (false, "wouldn't commit".to_string())
            } else if retests && o.lean == crate::council::Lean::Against && o.would_change_my_mind.is_none() {
                (false, "said no without saying what would change it".to_string())
            } else {
                (true, String::new())
            }
        });
        calls.push(SeatCall {
            took_ms: started.elapsed().as_millis() as u64,
            prompt_chars: prompt.len(),
            reply_chars: answer.as_ref().map(|r| r.len()).unwrap_or(0),
            failed: answer.as_ref().err().map(|e| e.to_string()),
            grade,
            words: answer.as_ref().ok().map(|reply| crate::trace::Words {
                system: "Answer as the seat you are given. Be brief.".into(),
                user: prompt.clone(),
                reply: reply.clone(),
            }),
        });
        if let Ok(reply) = answer {
            opinions.push(crate::council::parse_opinion(seat, &reply));
        }
    }
    opinions
}

/// What the picture reader's errand hands back: the answer, and its model
/// call for the record.
#[derive(serde::Serialize, serde::Deserialize)]
struct PictureOutcome {
    answer: String,
    failed: Option<String>,
    took_ms: u64,
    prompt_chars: usize,
}

/// What a crew errand writing a window reply hands back, so every model
/// call it made — the reply, and a rewrite if it needed one — is recorded
/// like every other (`record_model_call`).
#[derive(serde::Serialize, serde::Deserialize)]
struct ReplyOutcome {
    text: String,
    failed: Option<String>,
    calls: Vec<SeatCall>,
}

/// A model that notes each call made through it, for an errand that can't
/// reach `record_model_call` itself.
struct CountedLlm<'a> {
    inner: &'a dyn crate::brain::Llm,
    calls: std::sync::Mutex<Vec<SeatCall>>,
}

impl crate::brain::Llm for CountedLlm<'_> {
    fn complete(&self, system: &str, user: &str) -> crate::error::Result<String> {
        let started = std::time::Instant::now();
        let r = self.inner.complete(system, user);
        if let Ok(mut c) = self.calls.lock() {
            c.push(SeatCall {
                took_ms: started.elapsed().as_millis() as u64,
                prompt_chars: system.len() + user.len(),
                reply_chars: r.as_ref().map(|t| t.len()).unwrap_or(0),
                failed: r.as_ref().err().map(|e| e.to_string()),
                grade: None,
                words: r.as_ref().ok().map(|reply| crate::trace::Words {
                    system: system.to_string(),
                    user: user.to_string(),
                    reply: reply.clone(),
                }),
            });
        }
        r
    }
}

/// Where window jobs' ids start, well clear of the crew's own counter.
pub const WINDOW_JOB_IDS: u64 = 1 << 62;

// ---------------------------------------------------------------------------
// Two-factor, signing in, and signing up (Eric's rulings B1, B4, B6)
// ---------------------------------------------------------------------------

/// What a sign-in errand hands back.
#[derive(serde::Serialize, serde::Deserialize)]
struct SignInOutcome {
    site: String,
    account: String,
    outcome: crate::webrun::SignedIn,
}

/// What a security-change errand hands back.
#[derive(serde::Serialize, serde::Deserialize)]
struct SecurityOutcome {
    asked: crate::confirmed::Asked,
    url: String,
    pressed: Option<crate::confirmed::Pressed>,
    /// It had to sign in first, and the site wanted a code.
    wants_code: bool,
    failed: Option<String>,
}

/// What a sign-up errand hands back.
#[derive(serde::Serialize, serde::Deserialize)]
struct SignUpOutcome {
    enrolment: crate::enrol::Enrolment,
    outcome: crate::webrun::SignedUp,
}

/// The site in "turn off two-factor on github" / "…for my google account".
fn site_named_in(said: &str) -> Option<String> {
    if let Some(d) = crate::enrol::domain_from(said) {
        return Some(d);
    }
    let words: Vec<&str> = said.split_whitespace().collect();
    let at = words.iter().rposition(|w| matches!(*w, "on" | "for" | "at"))?;
    // "turn on two factor" — the "on" is the phrase, not the site.
    let rest: Vec<&str> = words[at + 1..]
        .iter()
        .copied()
        .filter(|w| !matches!(*w, "my" | "the" | "account" | "accounts" | "please" | "two" | "factor" | "2fa" | "step"))
        .collect();
    let site = rest.join(" ");
    (!site.is_empty()).then_some(site)
}

// ---------------------------------------------------------------------------
// Acting on its own (Eric's rulings E1–E4)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Your goals (Eric's ruling F4: nudges toward goals you set)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// The list for later, and self-change reminders (Eric's ruling F8)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Sorting your mailbox (Eric's ruling G1: yes; delete only when told)
// ---------------------------------------------------------------------------

/// The IMAP host for an account: the one set on it, or the provider's.
fn mail_host(account: &crate::mail::Account) -> Option<String> {
    // Sorting moves and labels over IMAP itself; through Himalaya it isn't
    // offered (yet), so such an account is passed over here.
    if crate::himalaya::route(&account.imap_host).is_some() {
        return None;
    }
    if !account.imap_host.is_empty() {
        return Some(account.imap_host.clone());
    }
    crate::mail::Provider::from_address(&account.address).imap_host().map(str::to_string)
}

// ---------------------------------------------------------------------------
// Scheduling and sending posts (Eric's ruling G2)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Pressing buttons in your apps by name (Eric's ruling G4)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Moving big files to another drive (Eric's ruling G5)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Carrying out an undo (Eric's ruling G6)
// ---------------------------------------------------------------------------

/// The action that takes back something Atlas did, from what was recorded.
pub fn undo_intent(what: &str) -> Option<Intent> {
    if what == "brought the workspace up" {
        return Some(Intent::WorkspaceOff);
    }
    if what == "put the workspace away" {
        return Some(Intent::WorkspaceOn);
    }
    if let Some(app) = what.strip_prefix("opened ") {
        return Some(Intent::CloseApp(app.to_string()));
    }
    if let Some(app) = what.strip_prefix("closed ") {
        return Some(Intent::OpenApp(app.to_string()));
    }
    if what.starts_with("switched to ") {
        return Some(Intent::SetMode("off".into()));
    }
    None
}

// ---------------------------------------------------------------------------
// Editing media on a copy (Eric's ruling G8)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Reading PDFs, Word files and scans; unzipping; a virus scan before either
// (Eric's ruling H3, 25 Sep 2026)

/// How long a question about your files waits for the index to finish
/// being read at start before saying it isn't all here yet.
const INDEX_WAIT_FOR_A_QUESTION: std::time::Duration = std::time::Duration::from_secs(2);

/// Which file errand (`Daemon::file_work_off_the_loop`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileJob {
    Read,
    Unzip,
}

/// What a file errand came to: a sentence (and whether it read the file,
/// and a line for the log), or a file the scan couldn't check, to be asked
/// about.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
enum FileDone {
    Said { said: String, ok: bool, log: Option<String> },
    Ask { what: String, path: String, question: String },
}

impl FileDone {
    fn no(said: String) -> FileDone {
        FileDone::Said { said, ok: false, log: None }
    }
}

/// Scan, then say whether it may be opened: `Ok` with a note to add (when
/// you said to open it anyway), or what to say instead.
fn scanned_ok_off(path: &std::path::Path, what: &str, anyway: bool, tools: &crate::voice::ToolsConfig) -> std::result::Result<Option<String>, FileDone> {
    if anyway {
        return Ok(Some("not scanned — you said to open it anyway".into()));
    }
    let verdict = crate::unpack::scan(path, &tools.files.virus_scan);
    match verdict {
        crate::unpack::Verdict::Clean => Ok(None),
        crate::unpack::Verdict::Threat(_) => Err(FileDone::Said {
            said: format!("{}: {}.", path.display(), verdict.said()),
            ok: false,
            log: Some(format!("virus scan: {} — {}", path.display(), verdict.said())),
        }),
        crate::unpack::Verdict::NotScanned(_) => Err(FileDone::Ask {
            what: what.to_string(),
            path: path.display().to_string(),
            question: format!("{}. Open it anyway?", verdict.said()),
        }),
    }
}

/// Read a PDF or Word file, after scanning it. A scanned PDF is read page
/// by page with the word reader. Runs on the crew's thread.
fn read_document_off(path: &str, anyway: bool, tools: &crate::voice::ToolsConfig) -> FileDone {
    let p = std::path::PathBuf::from(path);
    if !p.is_file() {
        return FileDone::no(format!("I can't find {path}."));
    }
    let note = match scanned_ok_off(&p, "read", anyway, tools) {
        Ok(n) => n,
        Err(done) => return done,
    };
    let lower = path.to_lowercase();
    let read: std::result::Result<(usize, String), String> = (|| {
        Ok(if lower.ends_with(".docx") {
            (0, crate::unpack::docx_text(&p)?)
        } else if lower.ends_with(".pdf") {
            let bytes = std::fs::read(&p).map_err(|e| format!("I couldn't open it: {e}"))?;
            let pdf = crate::pdftext::read(&bytes).map_err(|e| format!("I couldn't read it: {e}."))?;
            let scan = crate::files::pdf_is_really_a_scan(pdf.text_chars(), pdf.pages) || !crate::pdftext::looks_like_words(&pdf.text);
            if scan {
                (pdf.pages, read_pdf_photos_off(&pdf, tools)?)
            } else {
                (pdf.pages, pdf.text)
            }
        } else {
            (0, std::fs::read_to_string(&p).map_err(|e| format!("I couldn't open it: {e}"))?)
        })
    })();
    let (pages, text) = match read {
        Ok(r) => r,
        Err(why) => return FileDone::no(why),
    };
    if text.trim().is_empty() {
        return FileDone::no("I opened it and there's no text in it I can read.".into());
    }
    // The whole text is kept beside Atlas's other readings, and the start
    // is said. Reading forty pages aloud is not what "read this" means.
    let dir = crate::roots::data_sub("reading");
    let _ = std::fs::create_dir_all(&dir);
    let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "document".into());
    let kept = dir.join(format!("{name}.txt"));
    let _ = std::fs::write(&kept, &text);
    let words = text.split_whitespace().count();
    let start = crate::research::first_sentences(&text, 4);
    let size = if pages > 0 { format!("{pages} page{}, {words} words", if pages == 1 { "" } else { "s" }) } else { format!("{words} words") };
    let scanned = match note {
        Some(n) => format!(" ({n})"),
        None => String::new(),
    };
    FileDone::Said {
        said: format!("{name}: {size}{scanned}. It starts: {start} The whole text is in {}.", kept.display()),
        ok: true,
        log: None,
    }
}

/// A PDF that's photos of pages: each photo through the word reader.
fn read_pdf_photos_off(pdf: &crate::pdftext::Pdf, tools: &crate::voice::ToolsConfig) -> std::result::Result<String, String> {
    if pdf.images.is_empty() {
        return Err("It's a scan, but the pages aren't stored as photos I can read.".into());
    }
    let models = std::path::PathBuf::from(&tools.models.dir);
    if !crate::words::Reader::installed(&models) {
        return Err("It's a scanned PDF, and the two reading models that read scans aren't installed. \
                    They're an optional one-off download, and Atlas's window doesn't offer it yet."
            .into());
    }
    let mut reader = crate::words::Reader::open(&models).map_err(|e| format!("I couldn't start the reader: {e}"))?;
    let dir = crate::roots::tmp_dir().join("pdf-pages");
    let _ = std::fs::create_dir_all(&dir);
    let mut out = Vec::new();
    for (i, jpg) in pdf.images.iter().enumerate() {
        let f = dir.join(format!("page-{}.jpg", i + 1));
        if std::fs::write(&f, jpg).is_err() {
            continue;
        }
        if let Ok(read) = crate::words::read_file(&mut reader, &tools.video.ffmpeg, &tools.vars, &f.display().to_string(), &tools.words) {
            out.push(read.text());
        }
        let _ = std::fs::remove_file(&f);
    }
    let text = out.join("\n\n");
    if text.trim().is_empty() {
        return Err("It's a scan, and I couldn't make out the writing on its pages.".into());
    }
    Ok(text)
}

/// Unpack a zip beside itself, scanning it first and what came out after.
fn unzip_off(path: &str, anyway: bool, tools: &crate::voice::ToolsConfig) -> FileDone {
    let p = std::path::PathBuf::from(path);
    if !p.is_file() {
        return FileDone::no(format!("I can't find {path}."));
    }
    let note = match scanned_ok_off(&p, "unzip", anyway, tools) {
        Ok(n) => n,
        Err(done) => return done,
    };
    let dest = crate::unpack::folder_beside(&p);
    let files = match crate::unpack::unzip(&p, &dest, &tools.files) {
        Ok(f) => f,
        Err(why) => return FileDone::no(format!("I haven't unpacked it: {why}.")),
    };
    // Everything it unpacked to, scanned as one folder.
    let after = crate::unpack::scan(&dest, &tools.files.virus_scan);
    let n = files.len();
    let head = format!("Unpacked {n} file{} into {}", if n == 1 { "" } else { "s" }, dest.display());
    let said = match (after, note) {
        (crate::unpack::Verdict::Clean, None) => format!("{head}, and Windows Defender found nothing in them."),
        (crate::unpack::Verdict::Threat(t), _) => format!(
            "{head}, and Windows Defender found {t} among them. Don't open them — Defender's own screen can quarantine it."
        ),
        (_, Some(n)) => format!("{head} ({n})."),
        (crate::unpack::Verdict::NotScanned(why), None) => format!("{head}, but I couldn't scan what came out ({why}), so be careful opening them."),
    };
    FileDone::Said { said, ok: true, log: None }
}

// ---------------------------------------------------------------------------
// Dropping a task, and bringing back what you dropped (Eric's ruling H8)

/// What comes after the first of `phrases` found in `said`, or all of it.
fn words_after(said: &str, phrases: &[&str]) -> String {
    for p in phrases {
        if let Some(i) = said.find(p) {
            return said[i + p.len()..].trim().to_string();
        }
    }
    said.trim().to_string()
}

// ---------------------------------------------------------------------------
// What Atlas let go of, remembered as having known it (Eric's ruling H10)

// ---------------------------------------------------------------------------
// Eric's H13 rulings: lock awareness, suggestions by name, notes that point
// nowhere, hearing each ear, learning from your edits, the overnight account

// ---------------------------------------------------------------------------
// Video and creator advice (Eric's ruling I: kept, he makes video content),
// and general money questions (J: kept, general only)

// ---------------------------------------------------------------------------
// Teaching a gesture (H6), and the languages Atlas can hear (H5)

/// "a gesture called thumbs up that opens spotify" → ("thumbs up", "open spotify").
pub fn gesture_asked(said: &str) -> Option<(String, String)> {
    let l = said.to_lowercase();
    let after = ["called ", "named "].iter().find_map(|k| l.find(k).map(|i| &l[i + k.len()..]))?;
    let (name, rest) = [" that ", " to ", " for ", " which "]
        .iter()
        .find_map(|sep| after.split_once(sep))
        .unwrap_or((after, ""));
    let does = rest.trim().trim_end_matches(['.', '!']);
    // "opens spotify" is the command "open spotify".
    let does = match does.split_once(' ') {
        Some((verb, obj)) if verb.ends_with('s') && !verb.ends_with("ss") => format!("{} {obj}", &verb[..verb.len() - 1]),
        _ => does.to_string(),
    };
    let name = name.trim().trim_matches('"').to_string();
    (!name.is_empty()).then_some((name, does))
}

// ---------------------------------------------------------------------------
// The keys, changed by saying them (Eric, 26 Sep 2026: "it needs to be
// customizable. I don't have an Alt button")

/// "control shift space" → "ctrl+shift+space"; "caps lock" → "capslock".
pub fn key_spoken(words: &str) -> String {
    let w = words.to_lowercase().replace(" plus ", "+").replace(" and ", "+").replace('-', "+");
    let w = w
        .replace("caps lock", "capslock")
        .replace("scroll lock", "scrolllock")
        .replace("right control", "rightctrl")
        .replace("right ctrl", "rightctrl")
        .replace("left control", "leftctrl")
        .replace("page up", "pageup")
        .replace("page down", "pagedown")
        .replace("windows key", "win")
        .replace("control", "ctrl");
    let mut parts: Vec<String> = Vec::new();
    for chunk in w.split('+') {
        for p in chunk.split_whitespace() {
            let p = p.trim_matches(|c: char| c == '.' || c == ',' || c == '"');
            if !p.is_empty() && p != "the" && p != "key" {
                parts.push(p.to_string());
            }
        }
    }
    parts.join("+")
}

// ---------------------------------------------------------------------------
// Talking freely (Eric, 27 Sep 2026: "I need to be able to freely speak with
// Atlas not just scripted lines but full conversations").
//
// What the model is told, in an order that keeps the model server's cache
// useful; the model call off the daemon's loop; and the replies that used to
// vanish from the Talk page.
// ---------------------------------------------------------------------------

/// How long a question Atlas asked waits for its answer. After that, what
/// you say next is taken as itself.
pub const QUESTION_LIFETIME_SECS: u64 = 600;

/// The most sentences a model-written conversational reply is cut to. The
/// model is asked for fewer and stopped at a sentence's end when it reaches
/// them; this is only the ceiling for a model that runs on.
pub const SAFETY_SENTENCES: usize = 16;

/// How long the floor stays open after Atlas speaks, in a conversation.
pub const CHATTING_FOLLOWUP_SECS: u32 = 10;

/// How many earlier exchanges go to the model as turns, and roughly how many
/// tokens they may take.
pub const HISTORY_EXCHANGES: usize = 6;
pub const HISTORY_TOKENS: usize = 1200;

/// How many facts you told Atlas go in front of the model every turn.
pub const FACTS_IN_PROMPT: usize = 10;

/// A note's score below which it isn't worth putting in front of the model.
pub const NOTE_HINT_FLOOR: f32 = 0.25;

/// A turn whose model call is running on a worker thread.
pub(crate) struct PendingTurn {
    /// Which turn this is (`Daemon::pending_seq`).
    id: u64,
    said: String,
    t: u64,
    rx: std::sync::mpsc::Receiver<TurnNews>,
    started: std::time::Instant,
    by_chat: bool,
    /// The Talk page's words it answers: (words, said aloud, how long the
    /// thread was before).
    talk: Option<(String, bool, usize)>,
    /// The conversation as the model read it, and the model, for putting a
    /// tool's result into words afterwards.
    msgs: Vec<brain::Msg>,
    llm: std::sync::Arc<dyn Llm>,
    /// This is that second call: the tool's fixed words it is rewording.
    rephrasing: Option<RephraseAsk>,
}

/// A tool's result for the model to put into words (28 Sep 2026: "what's on
/// my calendar tomorrow" was answered with the calendar's fixed list, read
/// out as written).
#[derive(Debug, Clone)]
pub(crate) struct RephraseAsk {
    /// The result as written into the conversation.
    written: String,
    /// A list meant to be referred back to ("open 2"): the conversation and
    /// the page keep it whole, and only what is said is reworded.
    keep_written: bool,
}

/// The tools whose result is information to put into words, not an action
/// to acknowledge.
fn reads_back(i: &Intent) -> bool {
    matches!(
        i,
        Intent::Agenda(_)
            | Intent::FindFile(_)
            | Intent::WhatIHave(_)
            | Intent::MarketDay(_)
            | Intent::Recap
            | Intent::MachineHealth
            | Intent::WaitingFor(_)
            | Intent::TimeSpent(_)
            | Intent::HowAmIDoing
            | Intent::KnowledgeSize
            | Intent::Outstanding
    )
}

/// Long or list-shaped enough to be worth a model call to say naturally.
fn worth_rephrasing(reply: &str) -> bool {
    let r = reply.trim();
    !r.is_empty() && (r.split_whitespace().count() > 25 || r.lines().filter(|l| !l.trim().is_empty()).count() > 2)
}

/// Longest tool result handed back to the model to reword.
const REPHRASE_INPUT_CHARS: usize = 3000;
/// How long the reworded reply may be.
const REPHRASE_TOKENS: u32 = 150;

/// What a worker sends back.
pub(crate) enum TurnNews {
    /// A whole sentence of the reply, as soon as it is written.
    Sentence(String),
    /// Other things the model was asked to do in the same reply, which
    /// won't be (`Brain::converse_noting`).
    Also(Vec<String>),
    /// The model is done.
    Done(brain::Decision),
}

/// What a worker that died without an answer amounts to.
fn no_answer_came_back() -> brain::Decision {
    brain::Decision {
        intent: Intent::Say(String::new()),
        say: "Model unreachable: the model stopped before it answered.".into(),
        model: brain::Reached::No,
    }
}

/// What's left of `reply` once the sentences already spoken are taken out.
fn not_yet_said(reply: &str, spoken: &[String]) -> String {
    let mut rest = reply.to_string();
    for s in spoken {
        for candidate in [s.trim().to_string(), crate::persona::strip_filler(s).trim().to_string()] {
            if candidate.is_empty() {
                continue;
            }
            if let Some(i) = rest.find(&candidate) {
                rest.replace_range(i..i + candidate.len(), "");
                break;
            }
        }
    }
    rest.split_whitespace().collect::<Vec<_>>().join(" ").trim_start_matches(['.', ',', ' ']).to_string()
}

/// Is `rest` nothing but the stock acknowledgement for the action
/// (`brain::default_say`) -- "Opening Chrome." -- with nothing it found or
/// failed to do?
fn only_acknowledges(rest: &str, stock: &str) -> bool {
    let norm = |s: &str| s.to_lowercase().chars().filter(|c| c.is_alphanumeric() || c.is_whitespace()).collect::<String>();
    let (r, k) = (norm(rest), norm(stock));
    let r = r.split_whitespace().collect::<Vec<_>>().join(" ");
    let k = k.split_whitespace().collect::<Vec<_>>().join(" ");
    !r.is_empty() && (r == k || (r.starts_with(&k) && r.split_whitespace().count() <= k.split_whitespace().count() + 2))
}

/// What is said about the other things asked for in the same breath, which
/// weren't done.
fn one_at_a_time(also: &[String]) -> String {
    format!("One thing at a time: I haven't done {} -- ask me for that next.", also.join(" or "))
}

/// How long a finding that another model server is (or isn't) up holds.
const MODEL_SERVER_RECHECK: std::time::Duration = std::time::Duration::from_secs(60);
/// The longest a turn waits to learn whether one is up.
const MODEL_PROBE_WAIT: std::time::Duration = std::time::Duration::from_millis(800);

/// How long `speak_while_thinking` waits on the model before answering the
/// hub and checking for a pause again.
const THINKING_SLICE_MS: u64 = 50;

/// How many tools beyond the core ones a sentence is offered.
pub const RETRIEVED_TOOLS: usize = 6;

/// Does the sentence open by asking for the ways to do something ("what are
/// the ways…", "list the ways…", "how else could you…")? `ways_in_help`
/// answered "other ways" anywhere in a sentence.
fn opens_with_ways(said: &str) -> bool {
    let w = crate::intent::normalize(said);
    let w = crate::intent::without_fillers(&w);
    [
        "what are the ways", "what are all the ways", "what are the other ways", "what other ways", "what ways",
        "list the ways", "list all the ways", "how else could you", "how else can you", "all the ways", "every way",
        "other ways", "give me the ways", "tell me the ways",
    ]
    .iter()
    .any(|p| w.starts_with(p))
}

/// Does the sentence open as a decision to work through ("should I…",
/// "help me decide…"), or ask to get back to one? `decide::wants_working`
/// matched "should i" anywhere, so "what should I eat" became a decision.
fn opens_with_deciding(said: &str) -> bool {
    let w = crate::intent::normalize(said);
    let w = crate::intent::without_fillers(&w);
    if w.contains("back to the decision") || w.contains("that decision again") || w.starts_with("set aside ") || w.starts_with("rule out ") {
        return true;
    }
    [
        "should i ", "should we ", "help me decide", "i cant decide", "cant decide", "i can't decide", "which should i",
        "which should we", "which one should", "is it worth", "im torn between", "i'm torn between", "torn between",
        "do i go with", "would it be better to",
    ]
    .iter()
    .any(|p| w.starts_with(p))
}
