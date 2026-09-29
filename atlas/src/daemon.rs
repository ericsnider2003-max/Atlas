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

    /// `execute`, timed, for the typed prompt.
    ///
    /// A typed turn has no listening, hearing, speaking or playing — only the
    /// work. Those stages are left absent rather than written down as zero,
    /// which is the whole discipline of `timing::Turn`: a stage that did not
    /// run and a stage that took no time are different facts.
    ///
    /// Open question, deliberately not decided here: typed and spoken turns
    /// share one window, so `typical_ms()` is a median over two populations
    /// with genuinely different costs. `worst_stage()` is unaffected — it sums
    /// per stage, and a typed turn only ever adds to `Doing`.
    pub fn execute_timed(&mut self, intent: &Intent, said: &str) -> String {
        // A setting changed since the last thing you asked applies to this
        // one. The voice loop and the typed prompt don't tick, so this is
        // where they pick changes up.
        let _ = self.pick_up_settings();
        // Also set here, not only in `turn_from`: the one-shot CLI path
        // (`atlas "..."`) parses and executes without going through a turn at
        // all, and a correction typed at the command line is still a
        // correction. Missed on the first pass, and the symptom was Atlas
        // asking "what did I get wrong?" about a sentence that had just said.
        self.last_said = said.to_string();
        let started = std::time::Instant::now();
        let out = self.execute(intent);
        let mut timed = crate::timing::Turn {
            about: about_short(said),
            ..Default::default()
        };
        timed.note(
            crate::timing::Stage::Doing,
            started.elapsed().as_millis().min(u32::MAX as u128) as u32,
        );
        self.timing.add(timed);
        out
    }

    pub fn execute(&mut self, intent: &Intent) -> String {
        // The last line, and the one that catches the callers a gate placed
        // in `turn_from` alone never sees.
        //
        // The one that made this necessary rather than tidy: a parked
        // approval. Atlas asks "shut the workspace down, go ahead?", you hand
        // the laptop over, and the next person says "yes" -- and the answer
        // branch calls `execute` directly, several hundred lines above where
        // `turn_from` does its own check. Same for the work queue and for a
        // scheduled job coming due. Every one of them arrives here.
        //
        // `turn_from` keeps its check anyway: this one is about not *doing*
        // the thing, and that one is also about not asking you a question
        // about something that was never going to happen, and about not
        // answering out of your notes on the way past.
        if let Some(refusal) = self.handed_over_refusal(intent) {
            self.log.info(&format!("refused while handed over: {}", kind_of(intent)));
            return refusal;
        }

        // "Read this" means nothing until "this" has a referent. Resolving it
        // before acting is the difference between an assistant and a command
        // line — and when two candidates are equally likely it asks rather
        // than picking.
        if let Some(question) = self.resolve_subject(intent) {
            return question;
        }
        let said = self.execute_inner(intent);
        // Say how sure it is, where being wrong would matter. Answers that
        // are just Atlas reporting its own state don't need it.
        let said = match intent {
            // `Why` reads back decisions Atlas recorded itself — the most
            // grounded thing it can say. It was scored as invention: these
            // answers are full of the word "your" ("your workspace", "your
            // files"), which cost 0.4 under the rule for claiming things
            // about your machine without looking. Atlas was hedging its own
            // written record.
            Intent::Why(_) => self.hedge(&said, crate::certainty::Grounding::from_what_it_holds()),
            // `Ask` and `Research` are assessed where their grounding is
            // actually known — in the arm that knows whether the notes
            // answered, and in the errand that knows how many sources the
            // note read. Assessing them here meant assessing a string with
            // no idea where it came from.
            //
            // For `Research` it was worse than uninformative: what this saw
            // was "Looking into {topic}. I'll let you know what I find." —
            // an acknowledgement, not an answer. It hedged the receipt and
            // never touched the finding.
            _ => said,
        };
        // Anything that changed something goes in the one history. Without
        // this, "what did you do" answers from an empty list and looks like
        // it works.
        // You did something. Atlas's own overnight work is recorded with
        // by_you false elsewhere and never shifts your day.
        let hour = crate::localclock::hour_here(self.now_acting()) as u32;
        self.rhythm.saw(hour, true);

        // How it turned out, per kind of work. Recorded here rather than at
        // the point of deciding, because whether Atlas was right is knowable
        // only after the fact — and a record written at decision time would be
        // a record of intentions.
        //
        // Provisionally good: an action that completed and was not taken back.
        // `Intent::Undo` below rewrites the last one, which is the only signal
        // available without asking you to grade everything.
        let kind = crate::earned::kind_of(intent);
        let went_wrong = said.starts_with("I couldn't")
            || said.starts_with("I can't")
            || said.contains("isn't built")
            || said.contains("switched off");
        self.earned.note(kind, !went_wrong, &said, clock());
        let _ = self.earned.save(&self.store);

        if let Some((what, area, undo)) = worth_recording(intent, &said) {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            self.history.note(&what, area, undo, true, now);
            let _ = self.store.save("undo_history", &self.history);
        }
        said
    }
    /// What the health check actually managed to read.
    ///
    /// A reading that came back empty is not a healthy machine, it is a
    /// question that went unanswered — and a summary built only from the
    /// readings that worked cannot tell you which it was.
    fn health_steps(&self, r: &Readings) -> Vec<crate::faithful::Step> {
        let mut steps = Vec::new();
        let mut note = |what: &str, ok: bool| {
            steps.push(if ok {
                crate::faithful::Step::did(what)
            } else {
                crate::faithful::Step::skipped(what, "couldn't read it")
            });
        };
        note("memory", r.ram_total_gb > 0.0);
        note("disk", r.disk_total_gb > 0.0);
        steps
    }


    fn execute_inner(&mut self, intent: &Intent) -> String {
        match intent {
            // Atlas's own updates and feedback about it, by voice (8.2, 8.14).
            Intent::Updates(what) => self.updates_said(what),
            Intent::Feedback(what) => self.feedback_said(what),
            Intent::PhoneModel(what) => self.phone_model_said(what),
            // Two-factor codes (Eric, B1): read out, or found in your email
            // or texts, typed where you asked.
            Intent::TypeCode(said) => self.type_code(said, crate::store::now()),
            // Turning two-factor on or off: read back, then your yes.
            Intent::TwoFactor(said) => self.two_factor(said),
            // The last build that ran out of tries, as a long job (E3).
            Intent::KeepAtIt => self.keep_at_it(crate::store::now()),
            // Goals, for the nudges toward them (F4).
            Intent::Goals(said) => self.goals(said, crate::store::now()),
            // The list for later (F8).
            Intent::Later(said) => self.later_list(said, crate::store::now()),
            // Sorting your mailbox (G1).
            Intent::SortMail(said) => self.sort_mail(said, crate::store::now()),
            // When a post goes (G2).
            Intent::SchedulePost(said) => self.schedule_post(said, crate::store::now()),
            // A button in an app, by name (G4).
            Intent::PressButton(said) => self.press_button(said),
            // Big folders to another drive, findable afterwards (G5).
            Intent::MoveBigFiles(said) => self.move_big_files(said, crate::store::now()),
            // A video edited on a copy; the original only after you say (G8).
            Intent::EditMedia(said) => self.edit_media(said, crate::store::now()),
            Intent::Clock => crate::localclock::spoken_now(crate::store::now(), crate::localclock::offset_secs()),
            Intent::SetKey(said) => self.set_key(said),
            Intent::Languages(_) => self.languages_heard(),
            Intent::TeachGesture(said) => self.teach_gesture(said, crate::store::now()),
            Intent::MoneyAdvice(said) => self.money_advice(said),
            Intent::CreatorAdvice(said) => self.creator_advice(said),
            Intent::Overnight => self.overnight_account(),
            Intent::Dangling => self.dangling_notes(),
            Intent::Suggestions(said) => self.suggestions(said),
            Intent::DropTask(said) => self.drop_or_bring_back(said, crate::store::now()),
            Intent::Unzip(said) => self.unzip_asked(said),
            Intent::ReadDocument(said) => self.read_document_asked(said),
            // The two halves of knowing the map. `RebuildIndex` is what the
            // drift nudge offers, so saying yes to that nudge lands here and
            // something actually happens.
            Intent::RebuildIndex => {
                let said = self.rebuild_index();
                // A rebuild settles the drift. Leaving it held would have the
                // nudge raise the same thing again an hour later, about a
                // folder that now matches.
                self.index_drifted = None;
                said
            }
            // The one caller `nudge::trace_line` was written for. Reading a
            // log Atlas keeps about itself is the cheapest thing it does, and
            // it is the only way to answer "is the local model good enough"
            // with a measurement rather than an opinion.
            Intent::ModelTrace => crate::nudge::trace_line(&self.trace),
            // The caller `council.rs` never had. Everything in that module
            // decided what to do with a list of opinions; nothing ever built
            // one, so `tally`, `Verdict` and `spoken` all computed over a list
            // only a test had filled.
            Intent::AskTheRoom(q) => self.ask_the_room(&q),
            // The callers `revise.rs` never had.
            // The whole sentence, not the intent's argument: the parser
            // hands over what came *after* the phrase it matched, so the
            // argument is the fix and the complaint has been dropped. Both
            // halves matter — see `last_said`.
            Intent::GotItWrong(_) => {
                let (said, now) = (self.last_said.clone(), crate::store::now());
                self.got_it_wrong(&said, now)
            }
            Intent::ApplyLesson => self.apply_lesson(),
            Intent::WhichModel => self.which_model(),
            Intent::HowAmIDoing => self.mending_line(),
            Intent::TimeSpent(what) => self.time_spent(what, clock()),
            Intent::ClipHistory(said) => self.wd_clip_history(said, clock()),
            Intent::ScreenText(said) => self.wd_screen_text(said, clock()),
            Intent::MarketDay(said) => self.wd_market_day(said, clock()),
            Intent::WaitingFor(said) => self.wd_waiting_for(said, clock()),
            Intent::NoteReview(said) => self.wd_note_review(said, clock()),
            Intent::Launch(said) => self.wd_launch(said, clock()),
            Intent::TradeDay(said) => self.wd_trade_day(said, clock()),
            Intent::MeetingPrep(said) => self.wd_meeting_prep(said, clock()),
            Intent::Snippet(said) => self.wd_snippet(said, clock()),
            Intent::FindFile(said) => self.wd_find_file(said, clock()),
            Intent::Pdf(said) => self.wd_pdf(said, clock()),
            Intent::People(said) => self.wd_people(said, clock()),
            Intent::Feeds(said) => self.wd_feeds(said, clock()),
            Intent::Receipt(said) => self.wd_receipt(said, clock()),
            Intent::Habit(said) => self.wd_habit(said, clock()),
            Intent::Cards(said) => self.wd_cards(said, clock()),
            Intent::Translate(said) => self.wd_translate(said, clock()),
            Intent::WhatIHave(q) if q.trim().is_empty() => {
                crate::nudge::what_i_know_of(&self.contents)
            }
            Intent::WhatIHave(q) => {
                // The fact book first: what you actually told Atlas to remember
                // answers "what do you know about X" directly, rather than
                // pointing at a note to open. The index makes this fast however
                // much is remembered. Notes worth opening are the fallback.
                let now = crate::store::now();
                let facts = self.facts.recall_in_context(q, &self.context_terms(q), now);
                if !facts.is_empty() {
                    let lines: Vec<String> = facts.iter().take(3).map(|f| f.summary.clone()).collect();
                    let more = facts.len().saturating_sub(lines.len());
                    let tail = if more > 0 { format!(" (and {more} more I've got on that)") } else { String::new() };
                    let mut out = format!("{}{}", lines.join("; "), tail);
                    // Associative recall: what else it knows that connects to the
                    // answer — the neighbours by shared topic or explicit link —
                    // so asking about one thing surfaces the things bound up with
                    // it, not just the exact match.
                    let shown: std::collections::BTreeSet<&str> =
                        facts.iter().take(3).map(|f| f.name.as_str()).collect();
                    let related: Vec<String> = self
                        .facts
                        .related(facts[0], now, 3)
                        .into_iter()
                        .filter(|f| !shown.contains(f.name.as_str()))
                        .map(|f| f.summary.clone())
                        .collect();
                    if !related.is_empty() {
                        out.push_str(&format!(" You also know: {}.", related.join("; ")));
                    }
                    out
                } else {
                    let open = crate::contents::what_to_open(&self.contents, q);
                    match open.len() {
                        // A real answer, and a better one than opening everything
                        // on the chance something matches.
                        0 => format!("Nothing in my notes on {}.", q.trim()),
                        _ => {
                            let named: Vec<String> = open.iter().take(3).cloned().collect();
                            let more = open.len().saturating_sub(named.len());
                            let tail = if more > 0 { format!(" (and {more} more)") } else { String::new() };
                            // Led with words rather than the name, because the
                            // phrasing layer capitalises the first letter of a
                            // reply and a note's name is a filename, not a
                            // sentence — "Spain-trip." is not what it is called.
                            format!("Worth opening: {}{}.", named.join(", "), tail)
                        }
                    }
                }
            }
            Intent::WorkspaceOn => report(workspace::workspace_on(self.cfg, self.plat), "Workspace online."),
            Intent::WorkspaceOff => report(workspace::workspace_off(self.cfg, self.plat), "Workspace down."),
            Intent::OpenApp(a) => match self.gate_app(a, "open", intent) {
                AppGate::Ask(q) => q,
                // Not one of your configured apps: found by name among the
                // Start menu's shortcuts instead (round 11, `launcher`) --
                // after the gate, so an unknown app is still asked about.
                AppGate::Go if !self.cfg.apps.apps.contains_key(a.as_str()) => self.wd_launch(&format!("start up {a}"), clock()),
                AppGate::Go => act(workspace::open_app(self.cfg, self.plat, a), &format!("{a} is up.")),
            },
            Intent::CloseApp(a) => match self.gate_app(a, "close", intent) {
                AppGate::Ask(q) => q,
                AppGate::Go => act(workspace::close_app(self.cfg, self.plat, a), &format!("Closed {a}.")),
            },
            Intent::FocusApp(a) => match self.gate_app(a, "focus", intent) {
                AppGate::Ask(q) => q,
                AppGate::Go => act(workspace::focus_app(self.cfg, self.plat, a), &format!("There's {a}.")),
            },
            // A question with a number in it. The shelf answers, or says it
            // can't — a model asked a specific question it doesn't know
            // answers anyway, so falling through quietly is the failure.
            //
            // This has to sit above the catch-all below, which matched Ask
            // and swallowed it.
            Intent::Ask(q) if crate::reference::needs_the_shelf(q) => {
                let cfg = self.tools_cfg().reference.clone();
                // Your `reference.shelves`, which this ignored until 18 Sep
                // 2026 -- see `reference::chosen` for what empty means and
                // why.
                let shelves = crate::reference::chosen(&cfg);
                // `warn_when_stale` ships on and nothing read it. It matters
                // more than it looks: this sentence is what Atlas says
                // instead of inventing a number, and it was claiming to hold
                // shelves nothing has ever fetched.
                crate::reference::nothing_found(
                    q,
                    &shelves,
                    cfg.warn_when_stale,
                    crate::store::now(),
                )
            }
            // Ask the notes before answering from the model. Atlas writing a
            // research note and then being unable to find it again is the
            // whole reason `recall` exists, and it sat unwired while
            // `Intent::Ask` echoed whatever the model said.
            Intent::Ask(q) => match self.from_notes(q, crate::store::now()) {
                // Came out of the library, so it is grounded and says so.
                Some(answer) => {
                    self.hedge(&answer, crate::certainty::Grounding::from_what_it_holds())
                }
                // Nothing matched. What goes back is the question itself, for
                // the layer above to carry on with — hedging that produced
                // "What's the capital of France? I'm not certain", which is
                // Atlas casting doubt on your own sentence.
                None => q.clone(),
            },
            Intent::Say(s) => s.clone(),
            Intent::Research(topic) => self.research(topic),
            Intent::McpTool(p) => self.use_mcp_tool(p),
            Intent::ViewDisplay => self.look_closer(Capture::Screen),
            Intent::CaptureWebcam => self.look_closer(Capture::Camera),
            Intent::Gestures(on) => self.watch_hands(*on, crate::store::now()),
            Intent::WhatsThere => self.whats_there(),
            Intent::WhatsThis => self.whats_this(),
            Intent::CallNotes(what) => self.call_notes_command(what),
            Intent::Delegate(said) => self.start_working_for_you(said, intent),
            // Said only because you asked. Atlas never announces it.
            Intent::AfterMe => {
                // Eric, 25 Sep 2026: "no one else can ask Atlas." A handover
                // refuses it before it gets here, a guest profile can't reach
                // it (`profiles::ONLY_YOU_MAY_ASK`), and a voice that isn't
                // yours — or might not be — is told no here.
                match self.last_verdict {
                    crate::voiceid::Verdict::NotYou(_) => {
                        return "That's only for the person this Atlas belongs to.".into()
                    }
                    crate::voiceid::Verdict::Unsure(_) => {
                        return "I couldn't be sure that was your voice, and that one's only for you — \
                                ask me again, or type it."
                            .into()
                    }
                    _ => {}
                }
                let arrangement: crate::afterme::Arrangement = self.store.load(crate::afterme::RECORD);
                arrangement.spoken(&self.tools_cfg().after_me, crate::store::now()).trim().to_string()
            }
            Intent::NameThis(name) => self.name_this(name, crate::store::now()),

            // A self-report against what this machine actually measured --
            // never Machine::default()'s fixed laptop numbers. See
            // wants.rs's own doc on why that distinction is the whole point.
            Intent::Recommend => {
                let mut obs = crate::wants::Observations::default();
                let now = crate::store::now();
                for (stage, name) in [
                    (crate::timing::Stage::Hearing, "transcribe"),
                    (crate::timing::Stage::Understanding, "think"),
                    (crate::timing::Stage::Speaking, "speak"),
                ] {
                    if let Some(ms) = self.timing.typical_for(stage) {
                        obs.time(name, ms as u64, now);
                    }
                }
                let models = std::path::Path::new(&self.tools_cfg().models.dir).to_path_buf();
                for (kind, _path) in crate::infer::whats_missing(&models, &crate::infer::Kind::all()) {
                    obs.missing.push(kind.file().to_string());
                }
                // Things you asked for that Atlas had no way to do, carried
                // over from every turn that landed as an unanswerable
                // `Intent::Unknown`. `recommend()` turns each one into a
                // concrete suggestion ("you asked me to X and I couldn't"),
                // which is the whole reason the field exists. `failures` still
                // has no source and stays empty rather than being guessed at:
                // `recommend()` degrades to no recommendation from an empty
                // signal, which is the honest behaviour, not a wrong one.
                obs.unsupported_requests = self.wants_seen.unsupported_requests.clone();
                let machine = crate::wants::machine_from(&crate::fit::measure());
                let recs = crate::wants::recommend(&obs, &machine);
                let advice = crate::wants::ask(&recs);
                // Lead with the measured bottleneck. `recommend`/`ask` answer
                // "what would help", but fall silent when nothing here is worth
                // changing -- and "what could make you faster" still has an
                // honest answer then: the stage that is in fact slowest right
                // now, named from the same timings this self-report is built on.
                // Phrased the way `recommend` phrases a stage time, so the two
                // lines read as one measurement rather than two conventions.
                match obs.slowest() {
                    Some((stage, ms)) => format!(
                        "Right now the slowest part of a turn is {stage}, about {:.1} seconds. {advice}",
                        ms as f32 / 1000.0
                    ),
                    None => advice,
                }
            }

            // "Call me boss" / "stop calling me that" / "just talk" --
            // `returning::address_change` does its own phrase detection on
            // the raw text, so the whole utterance is passed through
            // untouched rather than whatever the parser trimmed as "the
            // argument".
            Intent::AddressAs(said) => match crate::returning::address_change(said) {
                Some(new_address) => {
                    let reply = crate::returning::confirm_address(&new_address);
                    if let Err(e) = new_address.save(&self.store) {
                        return format!("{reply} (though I couldn't save that: {e})");
                    }
                    // The same fact lands in memory's preference store,
                    // which is what `called()` reads. `memory.prefer` had
                    // no production writer, so `preference("called")` could
                    // only ever return None and the parked-question flow
                    // addressed you as nobody — a third notion of your name
                    // beside `ReturnConfig` and `Persona`, and the only one
                    // nothing set. Cleared means empty, not left stale.
                    match &new_address {
                        crate::returning::Address::None => self.memory.prefer("called", ""),
                        crate::returning::Address::Name(n)
                        | crate::returning::Address::Title(n) => {
                            self.memory.prefer("called", n)
                        }
                    }
                    let _ = self.memory.save(&self.store);
                    reply
                }
                None => "I didn't catch a name or title in that.".into(),
            },
            Intent::Dictate(first) => {
                // `self.last_present` is this turn's `t`, set at the top of
                // `turn_from`. Reaching for `store::now()` here instead —
                // which is what every other arm does, because none of them
                // keeps a clock — put `last_spoke` on a different clock from
                // the one `tick` hands `idle_check`, so dictation could never
                // time out. Caught by the idle test, not by reading.
                let first = first.clone();
                let t = self.last_present;
                self.start_dictating(&first, t)
            }
            Intent::Pause => {
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
                self.drop_pending_turn(crate::store::now(), "Paused before I answered -- nothing was done.");
                self.attention.pause(self.current_work(), crate::store::now())
            }
            Intent::Resume => {
                // "I'm back" is the same four words in two different
                // situations, and the situation tells them apart.
                //
                // Paused, it means carry on -- and that is handled above
                // this, in `hear`, so it works mid-task. Nothing paused and
                // the machine handed over, it means the owner is back at the
                // desk, so it summons the passphrase prompt. The phrase is
                // not in `take_it_back`'s own list precisely because it is
                // already `resume`'s, and two commands claiming one phrase
                // make one of them unreachable.
                //
                // The remaining ambiguity is a guest who paused Atlas and
                // then said "carry on" -- they get a prompt they cannot
                // answer, and nothing happens. That is the harmless side of
                // the mistake; the other ordering would leave the owner
                // saying "I'm back" to an assistant that answers "Wasn't
                // paused." and keeps holding their things.
                if !self.attention.is_paused()
                    && self.handover().stance.handed_over()
                {
                    self.take_it_back()
                } else {
                    let msg = self.attention.resume(crate::store::now());
                    let held = self.attention.release();
                    let names: Vec<String> = held
                        .iter()
                        .filter_map(|id| {
                            self.queue
                                .tasks
                                .iter()
                                .find(|x| x.id == *id)
                                .map(|x| x.command.clone())
                        })
                        .collect();
                    if names.is_empty() {
                        msg
                    } else {
                        format!("{msg} Still in hand: {}.", names.join(", "))
                    }
                }
            }
            // What's outstanding, said the way it's actually useful: the one
            // thing to do next, and the one thing holding several others up.
            // A count is what every app already tells you.
            Intent::Outstanding if !self.workspace.is_empty() => {
                let now = clock();
                // Anything that stopped overnight is settled first and stated
                // in one line each, so the brief that follows is about today.
                let mut settled = self.settle_interrupted().join(" ");
                if !settled.is_empty() {
                    settled.push(' ');
                }
                let mut cfg = self.tools_cfg().daily.clone();
                // The hour your day turns over, as learned from your hours
                // rather than fixed (F2).
                cfg.rolls_at_hour = self.rhythm.rolls_at(&cfg);
                let mut said = settled;

                // If the day has turned since you last looked, that comes
                // first — the list you're about to see is a new one.
                if crate::daily::has_rolled(self.last_seen, now, &cfg) {
                    let day = crate::daily::day_of(self.last_seen, &cfg);
                    let (closed, carried) =
                        crate::daily::close(&self.workspace, day, &self.carried, &cfg);
                    said.push_str(&crate::daily::opening(&closed, &carried, &cfg));
                    said.push(' ');
                    self.carried = carried.into_iter().map(|(t, c)| (t, c.0)).collect();
                    self.last_seen = now;
                    // A day watched, so the hour your day ends can be learned
                    // (`Rhythm::quiet_hour` needs days to go on) — and kept,
                    // since it was forgotten at every restart.
                    self.rhythm.note_day();
                    let _ = self.store.save("rhythm", &self.rhythm);
                    // "You're usually done by …" — when it's first worked out,
                    // and again only when your hours have really moved (F2).
                    if let (Some(h), Some(line)) = (self.rhythm.quiet_hour(), self.rhythm.noticed()) {
                        let last: Option<(u32, u64)> = self.store.load("rhythm_said");
                        if crate::daily::worth_saying_again(h, last, now) {
                            said.push_str(&line);
                            said.push(' ');
                            let _ = self.store.save("rhythm_said", &Some((h, now)));
                        }
                    }
                    // Archived rather than discarded, so there is finally
                    // something for `daily::still_keep` to prune -- see the
                    // hourly sweep below.
                    self.daily_history.retain(|c| c.day != closed.day);
                    self.daily_history.push(closed);
                    let _ = self.store.save("daily_history", &self.daily_history);
                }

                said.push_str(&crate::workspace_view::spoken(&self.workspace, now));

                // Anything with someone else's time in it, waiting on you.
                let stale = crate::booking::going_stale(&self.proposals, now);
                if let Some(p) = stale.first() {
                    said.push(' ');
                    said.push_str(&crate::booking::stale_nudge(p));
                }
                said
            }
            // The backlog summary, with the morning pass in front of it. The
            // brief answers "what should I do", the summary answers "what is
            // outstanding", and asked together the first one goes first.
            Intent::Outstanding => {
                let b = self.brief_now(crate::store::now());
                let head = crate::brief::spoken(&b);
                let tail = self.backlog.summary();
                // On screen as well as spoken. A list read aloud is gone the
                // moment it finishes; this is the one you can look back at.
                //
                // The brief's items rather than the backlog's, so the panel
                // shows the handoff at the door and the job that failed
                // overnight alongside what Atlas could not finish — the
                // backlog is one source of several now, and a panel that
                // showed only it would disagree with the line just spoken.
                let lines: Vec<String> = b
                    .yours
                    .iter()
                    .chain(b.drafted.iter())
                    .map(|i| format!("{} — {}", i.source.plain(), i.headline()))
                    .collect();
                self.show_panel(crate::window::Panel::Outstanding, "Outstanding", lines);
                if b.is_empty() { tail } else { format!("{head} {tail}") }
            }
            Intent::Queued => {
                // Used to answer only about posts waiting to be sent, while
                // `crew::queued`, `crew::in_hand` and `crew::why_waiting` --
                // written for exactly this question -- had no caller at all.
                // Asked "what's queued", you mean everything Atlas is holding,
                // not one kind of it.
                let posts = self.publisher.summary(crate::store::now());
                // The lane queue's parked sets, by the readers written for
                // them. Work held for a connection or for a gap in your day
                // was invisible in this answer before.
                let mut parked: Vec<String> = Vec::new();
                let offline: Vec<&str> =
                    self.queue.waiting_for_network().iter().map(|w| w.command.as_str()).collect();
                if !offline.is_empty() {
                    parked.push(format!(
                        "waiting for a connection: {}",
                        offline.join(", ")
                    ));
                }
                let gapped: Vec<&str> =
                    self.queue.waiting_for_gap().iter().map(|w| w.command.as_str()).collect();
                if !gapped.is_empty() {
                    parked.push(format!("waiting for a quiet moment: {}", gapped.join(", ")));
                }
                // Waiting changes whose code has moved on under them.
                for (project, title, files) in self.workshop.outdated() {
                    parked.push(format!(
                        "the \"{title}\" change for {project} is out of date ({} changed since)",
                        files.join(", ")
                    ));
                }
                // Windows being worked for you, and where each stands.
                let windows: Vec<String> = self
                    .working_for_you
                    .iter()
                    .map(|w| {
                        let state = if w.held {
                            "paused, nothing lost"
                        } else if w.waiting_for_gap {
                            "has a reply to type, waiting for you to stop typing"
                        } else {
                            "watching for something new"
                        };
                        format!("the conversation in {} is {state}", w.job.app)
                    })
                    .collect();
                if !windows.is_empty() {
                    parked.push(windows.join("; "));
                }
                // Said after everything above is gathered. It was built
                // before the windows were added, so "what's queued" never
                // mentioned a window being worked.
                let posts = if parked.is_empty() {
                    posts
                } else {
                    format!("{posts} Also {}.", parked.join("; "))
                };
                // Errands on hold are named with where they stand: holding
                // at a safe point, or still working its way to one.
                let on_hold: Vec<String> = self
                    .crew
                    .errands()
                    .into_iter()
                    .filter_map(|e| {
                        let c = self.errand_candidates().into_iter().find(|c| c.id == e.id)?;
                        let name = crate::which_errand::describe(&c);
                        match e.state {
                            crew::State::Holding => Some(format!("{name} is paused, holding with nothing lost")),
                            crew::State::Pausing => Some(format!("{name} is pausing — it holds at its next safe point")),
                            crew::State::WaitingPaused => Some(format!("{name} is paused before it started")),
                            _ => None,
                        }
                    })
                    .collect();
                let posts = if on_hold.is_empty() {
                    posts
                } else {
                    format!("{posts} {}. Say 'carry on with …' to pick one back up.", on_hold.join("; "))
                };
                let waiting = self.crew.queued();
                if waiting == 0 {
                    posts
                } else {
                    // Named, not just counted. "Two errands waiting" is a
                    // number; "research is behind one other" is an answer.
                    let mut lines: Vec<String> = Vec::new();
                    let links: Vec<(u64, &'static str)> = self
                        .crew_links
                        .iter()
                        .map(|(id, l)| (*id, l.label))
                        .collect();
                    for (id, label) in links {
                        if !self.crew.in_hand(id) {
                            continue;
                        }
                        match self.crew.why_waiting(id) {
                            Some(why) => lines.push(format!("{label} is {why}")),
                            None => lines.push(format!("{label} is running")),
                        }
                    }
                    lines.sort();
                    if lines.is_empty() {
                        format!(
                            "{posts} And {waiting} errand{} waiting.",
                            if waiting == 1 { "" } else { "s" }
                        )
                    } else {
                        format!("{posts} {}.", lines.join(", "))
                    }
                }
            }
            Intent::DraftPost(channel) => {
                let ch = channel_named(channel);
                let id = self.publisher.draft(ch, "");
                format!("Drafting for {channel}. What should it say? (#{id})")
            }
            // Bare "undo" now goes through the one history first, so it can
            // take back a setting or a mail action and not only a file move.
            // The trash is the fallback, not the whole answer.
            // Taking something back is the clearest statement that it was
            // wrong, and the only one available without making you grade every
            // action. It rewrites the outcome recorded a moment ago rather
            // than adding a second one, or a single mistake would count twice.
            Intent::Undo => {
                self.earned.taken_back();
                let _ = self.earned.save(&self.store);
                // "Undo" means the last thing that CAN be taken back.
                // `Undo::possible` existed to ask exactly that and nothing
                // asked it: when the newest action was irreversible, the
                // answer was its refusal and nothing else — even with a
                // perfectly reversible action sitting right behind it.
                let newest = self.history.last();
                if let Some(n) = newest {
                    if !n.undo.possible() {
                        let older = self
                            .history
                            .done
                            .iter()
                            .rev()
                            .find(|d| !d.undone && d.undo.possible())
                            .cloned();
                        if let Some(o) = older {
                            let refusal = crate::undo::say(&crate::undo::reverse(Some(n)));
                            let offer = crate::undo::say(&crate::undo::reverse(Some(&o)));
                            return format!("{refusal} Before that: {offer}");
                        }
                    }
                }
                match crate::undo::reverse(self.history.last()) {
                // Asked first, then actually done (Eric, G6): this used to
                // stop at the question.
                crate::undo::Reversal::CanDo { id, confirm, .. } => {
                    self.session.ask(&confirm);
                    self.pending_undo = Some(id);
                    confirm
                }
                crate::undo::Reversal::Nothing => match self.trash.undo_last() {
                Ok(d) => {
                    let name = std::path::Path::new(&d.original)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or(d.original);
                    format!("Put {name} back.")
                }
                    Err(e) => format!("{e}"),
                },
                other => crate::undo::say(&other),
                }
            }
            Intent::BackUp => {
                let cfg = self.backup_cfg();
                match back_up(self.store.root(), &cfg, crate::store::now()) {
                    Ok(b) => {
                        prune_backups(&cfg);
                        // back_up() just wrote every file itself and counted as it went, so
                        // this is never the unreadable case -- unlike `Backup`s read back off
                        // disk by `backups()`, which is where files: None matters.
                        format!("Backed up {} files.", b.files.unwrap_or(0))
                    }
                    Err(e) => format!("Backup failed: {e}"),
                }
            }
            Intent::SetMode(name) => {
                let n = name.trim().trim_end_matches(" mode").trim();
                // Leaving a mode. `enter` was wired and `leave` was not, so a
                // mode could be turned on and never cleanly turned off -- and a
                // mode you cannot get out of is a mode you stop using, which is
                // exactly what `modes::leave`'s own doc warns about. "mode off",
                // "go into normal mode", "mode normal" now restore what was open
                // before, drop the mode's rules and clear the active mode. A
                // leaving word only counts as leaving when a mode is actually
                // on; otherwise it falls through to `enter`, so someone who
                // genuinely built a mode named "normal" can still turn it on.
                let leaving = matches!(
                    n.to_lowercase().as_str(),
                    "off" | "leave" | "exit" | "normal" | "none" | "nothing"
                        | "standard" | "default" | "out"
                );
                if leaving && self.modes.active().is_some() {
                    return match self.modes.leave() {
                        Some(t) => {
                            let _ = self.modes.save(&self.store);
                            t.say
                        }
                        None => "You're not in a mode.".into(),
                    };
                }
                match self.modes.enter(n, &[]) {
                    Some(t) => t.say,
                    None if leaving => "You're not in a mode.".into(),
                    None => format!("I don't have a {n} mode."),
                }
            }
            Intent::SelfCheck => self.self_check(),
            Intent::Shakedown => self.shakedown(),
            Intent::MachineHealth => {
                let r = self.readings();
                let f = assess_machine(&r, &self.health_cfg());
                let watched = self.watcher.summary();
                let mut s = health_summary(&r, &f);
                // What's using the machine now, and separately what you could
                // change so it stops. The second half was written months ago
                // and had never been reachable.
                // What's measured is passed in. This was an empty survey, so
                // this half could never say anything. Memory by app, startup
                // items, disposable folders and other drives aren't measured
                // yet, and are left empty rather than guessed.
                let models_mb = std::fs::read_dir(self.store.install_root().join("models"))
                    .map(|rd| rd.flatten().filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum::<u64>() / 1_000_000)
                    .unwrap_or(0);
                let survey = crate::tune::Survey {
                    disk_free_gb: r.disk_free_gb,
                    disk_total_gb: r.disk_total_gb,
                    ram_used_gb: r.ram_used_gb,
                    ram_total_gb: r.ram_total_gb,
                    atlas_mb: models_mb,
                    ..Default::default()
                };
                let findings = crate::tune::examine(
                    &survey,
                    &self.tools_ref().map(|t| t.tune.clone()).unwrap_or_default(),
                );
                if let Some(first) = crate::tune::actionable(&findings).first() {
                    let (mb, _) = crate::tune::worth_it(&findings);
                    s.push(' ');
                    s.push_str(&if mb > 0 {
                        format!("{} — about {mb}MB back.", first.what)
                    } else {
                        format!("{}.", first.what)
                    });
                }
                if !watched.starts_with("Not watching") {
                    s.push(' ');
                    s.push_str(&watched);
                }
                // Anything the health check could not actually read is a gap
                // in the answer, not a detail. Say it first rather than
                // letting a fluent summary imply it covered everything.
                {
                    // Through the channel rather than straight to faithful:
                    // the health answer is a result, so it also has to be
                    // sayable to someone who heard no progress at all.
                    let (said, leaks) = self.run.close(&self.health_steps(&r), &s);
                    for l in &leaks {
                        self.log.info(&format!("result leans on progress: {}", l.plain()));
                    }
                    said
                }
            }
            Intent::UseClipboard(what) => {
                let cfg = self.clipboard_cfg();
                if !cfg.enabled {
                    return "Using the clipboard is switched off.".into();
                }
                // What was copied: whatever a caller already set (a test, or a
                // future push path), else read the OS clipboard now. On request
                // only — this is the sole place it's read, never in the
                // background. `None` from the platform means there's no way to
                // reach the clipboard here, which is not the same as it being
                // empty.
                let copied = match self.clipboard_text.clone() {
                    Some(t) => Some(t),
                    None => self.plat.read_clipboard().ok().flatten(),
                };
                match copied {
                    None => "I can't reach the clipboard on this machine — there's no clipboard tool \
                             available for me to read it.".into(),
                    Some(text) => {
                        let grab = crate::clipboard::take(&text, &cfg);
                        if grab.kind == crate::clipboard::Kind::Empty {
                            return "There's nothing on the clipboard.".into();
                        }
                        let prompt = crate::clipboard::prompt(what, &grab);
                        // Kept for the model-context path and so a turn without
                        // a model still has the question ready.
                        self.pending_clipboard = Some(prompt.clone());
                        // With a model, answer now and — if you've left it on —
                        // put the answer back on the clipboard so you can paste
                        // it where you were. Without one, just say what was
                        // picked up.
                        match self.llm.clone() {
                            Some(llm) => match llm.complete(crate::clipboard::ANSWER_SYSTEM, &prompt) {
                                Ok(answer) => {
                                    let answer = answer.trim().to_string();
                                    if answer.is_empty() {
                                        grab.describe()
                                    } else if cfg.reply_to_clipboard {
                                        // Actually put it back, and only claim
                                        // so when it landed. The old code set a
                                        // field nothing drained and told you it
                                        // was on the clipboard regardless — a
                                        // hollow promise this closes.
                                        match self.plat.write_clipboard(&answer) {
                                            Ok(()) => {
                                                self.clipboard_writeback = Some(answer.clone());
                                                format!(
                                                    "{}\n\n{answer}\n\n(It's back on your clipboard — \
                                                     paste it where you were.)",
                                                    grab.describe()
                                                )
                                            }
                                            Err(_) => format!(
                                                "{}\n\n{answer}\n\n(I couldn't put it back on the \
                                                 clipboard on this machine, so copy it from here.)",
                                                grab.describe()
                                            ),
                                        }
                                    } else {
                                        format!("{}\n\n{answer}", grab.describe())
                                    }
                                }
                                // A model that couldn't answer shouldn't lose
                                // you the grab; you can still ask again.
                                Err(_) => grab.describe(),
                            },
                            None => grab.describe(),
                        }
                    }
                }
            }
            Intent::Rehearse(command) => {
                // Runs against the same fake operating system the tests use,
                // so there is no path from here to your real windows.
                let mock = crate::platform::mock::MockPlatform::new(
                    self.plat.monitors().unwrap_or_default(),
                );
                let inner = self.parser.parse(command);
                let reply = match &inner {
                    Intent::WorkspaceOn => {
                        let _ = workspace::workspace_on(self.cfg, &mock);
                        String::new()
                    }
                    Intent::WorkspaceOff => {
                        let _ = workspace::workspace_off(self.cfg, &mock);
                        String::new()
                    }
                    Intent::OpenApp(a) => {
                        let _ = workspace::open_app(self.cfg, &mock, a);
                        String::new()
                    }
                    Intent::CloseApp(a) => {
                        let _ = workspace::close_app(self.cfg, &mock, a);
                        String::new()
                    }
                    other => format!("I don't know how to rehearse {}.", other.plain()),
                };
                if !reply.is_empty() {
                    return reply;
                }
                let r = crate::rehearse::from_actions(command, &mock.actions());
                self.last_rehearsal = Some(r.detail());
                // The "!" that flags an irreversible step lives only in
                // `detail`, which is stored for reading and never spoken. So
                // the spoken line -- the one you actually hear before saying
                // "go" -- gave a step count and no hint that one of those
                // steps closes an app or types into it and cannot be taken
                // back. `touches_anything_irreversible` answers exactly that,
                // and now the warning reaches your ear, not just the stored
                // walk-through.
                let mut spoken =
                    format!("{} {}", crate::rehearse::preamble(command), r.summary());
                if r.touches_anything_irreversible() {
                    spoken.push_str(" Heads up: some of these can't be undone.");
                }
                spoken
            }
            Intent::Ready => {
                // The waking moment: the mark, then the one thing that matters.
                self.wants_panel = Some(crate::panel::Panel::Waking);
                self.panel_shown_at = crate::store::now();
                self.draw_panel(crate::panel::Panel::Waking);
                let brief = crate::mind::speak_brief(&self.brief_items());
                self.pending_brief = None;
                brief
            }
            Intent::Show(what) => {
                let w = what.trim().to_lowercase();
                let panel = if w.contains("outstanding") || w.contains("task") || w.contains("list") {
                    crate::panel::Panel::Tasks
                } else if w.contains("think") || w.contains("working") || w.contains("doing") {
                    crate::panel::Panel::Mind
                } else if w.contains("setting") || w.contains("hub") || w.contains("control") {
                    crate::panel::Panel::Controls
                } else {
                    return format!("I don't have a {w} to show you.");
                };
                let monitors = self.plat.monitors().unwrap_or_default();
                match crate::panel::place(panel, &monitors, &self.panel_cfg()) {
                    crate::panel::Decision::Show(_) => {
                        self.wants_panel = Some(panel);
                        self.draw_panel(panel);
                        // Spoken as well: the panel is for glancing at, the
                        // words are what you actually take in.
                        // `panel::narration` owns the "what to say per panel,
                        // and whether to say it at all" decision -- it returns
                        // None when speaking would be wrong -- so the daemon
                        // does not keep a second copy of that table.
                        //
                        // RECOVERED IN THE 17 SEP MERGE. On 16 Sep this branch
                        // was hand-rolling `narration`'s logic; wiring the real
                        // function removed the duplicate. The improvements
                        // side's `daemon.rs` still had the hand-rolled version,
                        // and taking it whole re-orphaned `panel::narration` --
                        // caught by `dead_capabilities` listing it as an
                        // ORPHAN. Two copies of one decision is how they drift.
                        let content = match panel {
                            crate::panel::Panel::Tasks => self.backlog.summary(),
                            crate::panel::Panel::Mind => self.mind_summary(),
                            // Settings and the hub live in Atlas's own window
                            // now — its Settings and Hub pages, not a panel
                            // that listed nothing.
                            _ if w.contains("hub") => {
                                match crate::firstlaunch::open_atlas_window(&crate::firstlaunch::First::Hub("/hub".into())) {
                                    Ok(()) => "The hub is open.".into(),
                                    Err(e) => format!("I couldn't open the hub: {e}"),
                                }
                            }
                            _ => match crate::firstlaunch::open_atlas_window(&crate::firstlaunch::First::Settings) {
                                Ok(()) => "Settings are open.".into(),
                                Err(e) => format!("I couldn't open my settings: {e}"),
                            },
                        };
                        crate::panel::narration(panel, &content, &self.panel_cfg())
                            .unwrap_or_default()
                    }
                    crate::panel::Decision::AskFirst(q) => {
                        self.session.ask(&q);
                        self.pending_panel = Some(panel);
                        q
                    }
                    crate::panel::Decision::SpeakOnly(_) => match panel {
                        crate::panel::Panel::Tasks => self.backlog.summary(),
                        crate::panel::Panel::Mind => self.mind_summary(),
                        _ => "No screen for that, so ask me and I'll tell you.".into(),
                    },
                }
            }
            Intent::Dismiss => {
                self.wants_panel = None;
                self.pending_panel = None;
                String::new()
            }
            // What works on whatever this is running on. Asked before
            // handing it to someone, and the honest answer differs enough by
            // platform to be worth giving properly.
            // The machine ladder, weighed rather than tried in order.
            //
            // This one was not wrong -- the `if/else` gave the right answer
            // for every sentence naming one machine. It is here because
            // `contains` is a substring search, so "can you do this on a
            // macbook" and "can you do this on macos" worked by accident
            // while "is there an iPad version" fell through to the final
            // `else` and was answered about iOS for the right reason by
            // luck rather than by matching. Weighing it puts the vocabulary
            // in one list the guard can check, beside the other two sets.
            Intent::Capabilities(what)
                if crate::whichone::weigh(
                    what,
                    crate::whichone::WHICH_MACHINE,
                    &self.tools_cfg().whichone,
                )
                .settled()
                .is_some() =>
            {
                let chose = crate::whichone::weigh(
                    what,
                    crate::whichone::WHICH_MACHINE,
                    &self.tools_cfg().whichone,
                )
                .settled();
                let p = match chose {
                    Some("mac") => crate::portable::Platform::Mac,
                    Some("linux") => crate::portable::Platform::Linux,
                    Some("android") => crate::portable::Platform::Android,
                    Some("windows") => crate::portable::Platform::Windows,
                    // `ios` and, unreachably, anything else. The guard above
                    // only admits a settled reading, and every id in
                    // WHICH_MACHINE is named here --
                    // `every_machine_reading_maps_to_a_platform` fails the
                    // build if a sixth is added without an arm.
                    _ => crate::portable::Platform::Ios,
                };
                // Answered from the catalogue rather than from the list of
                // machine powers. "9 of 10 work" was true and useless: what
                // somebody asking this wants is how many of the things Atlas
                // does survive the move, which is a question only the join of
                // the two tables answers.
                format!(
                    "{} {} {}",
                    crate::capability::on_platform_summary(p),
                    crate::portable::honest_summary(p),
                    crate::portable::for_a_friend(p)
                )
            }

            Intent::Capabilities(what) => {
                let w = what.trim().to_lowercase();
                if w.contains("offline")
                    || w.contains("without internet")
                    || w.contains("no internet")
                    || w.contains("without a connection")
                    || w.contains("no connection")
                    || w.contains("unplugged")
                {
                    // Offline-first is the whole point of this tree, so "what
                    // works without the internet" is a first-class question,
                    // not one that should fall through to the catch-all and be
                    // matched against a single capability by keyword. Each
                    // capability records whether it needs the network;
                    // `offline_count` is the join, and the honest answer is a
                    // count rather than a promise.
                    let (offline, total) = crate::capability::offline_count();
                    format!("{offline} of {total} things work with the network unplugged.")
                } else if w.contains("in detail")
                    || w.contains("full list")
                    || w.contains("one by one")
                    || w.contains("each one")
                    || w.contains("broken down")
                    || w.contains("list them")
                    || w.contains("list all")
                    || w.contains("list everything")
                    || w.contains("whole list")
                {
                    // "list them all", "what can you do, in detail" -- the
                    // itemised answer rather than the counts. `summary` gives
                    // "N things work right now"; someone asking to see the list
                    // wants the list, grouped by area with a legend, which is
                    // exactly what `full` builds and nothing until now called.
                    crate::capability::full()
                } else if w.contains("finish")
                    || w.contains("to build")
                    || w.contains("still to build")
                    || w.contains("unfinished")
                {
                    // "what's left to finish", "what's left to build" -- the
                    // honest completion backlog, split by who can finish each
                    // part: what Atlas could wire itself, what only needs a real
                    // run on the machine, and what waits on you.
                    // `capability::to_finish_report` is that view and nothing
                    // until now called it -- the self-finishing picture a person
                    // actually means by "what's left".
                    crate::capability::to_finish_report()
                } else if w.contains("right now")
                    || w.contains("working")
                    || w.contains("work now")
                    || w.contains("actually do")
                {
                    // "what can you do right now", "what's working" -- name the
                    // things usable this moment, not the counts and not the
                    // whole catalogue. `summary` says "13 things work right now"
                    // and `full` lists everything including what's blocked or
                    // off; someone asking what *works* wants only the usable set
                    // by name. `capability::working` is that filter and nothing
                    // until now asked it -- the question fell through to the
                    // keyword match and came back "I don't have anything for
                    // that."
                    let working = crate::capability::working();
                    if working.is_empty() {
                        "Nothing's usable right now.".into()
                    } else {
                        // Grouped by area (28 Sep 2026): with the catalogue
                        // complete, 55-odd names in one run-on sentence was a
                        // list nobody could follow, heard or read.
                        let mut parts: Vec<String> = Vec::new();
                        for area in crate::capability::EVERY_AREA {
                            let names: Vec<&str> =
                                working.iter().filter(|c| c.area == *area).map(|c| c.what).collect();
                            if !names.is_empty() {
                                parts.push(format!("{} ({}): {}", area.plain(), names.len(), names.join("; ")));
                            }
                        }
                        format!("{} things work right now: {}.", working.len(), parts.join(". "))
                    }
                } else if w.is_empty() || w.contains("what can you") || w.contains("everything") {
                    crate::capability::summary()
                } else if w.contains("new") {
                    let recent = crate::capability::since(17);
                    match recent.first() {
                        Some(c) => format!(
                            "{} new things, most recently: {}.",
                            recent.len(),
                            c.what
                        ),
                        None => "Nothing new.".into(),
                    }
                } else if w.contains("can't") || w.contains("cant") || w.contains("waiting") {
                    let blocked = crate::capability::what_would_unblock_most();
                    match blocked.first() {
                        Some((need, n)) => format!("{n} things are waiting on {need}."),
                        None => "Nothing's blocked.".into(),
                    }
                } else {
                    // "can you read my email?" — match it to a capability.
                    match crate::capability::all()
                        .into_iter()
                        .find(|c| w.contains(c.id) || c.what.split(' ').any(|word| word.len() > 4 && w.contains(word)))
                    {
                        Some(c) => crate::capability::can(c.id).map(|(_, why)| why).unwrap_or_default(),
                        None => "I don't have anything for that.".into(),
                    }
                }
            }
            // What did you do, and take it back. One list across everything,
            // because at the moment you ask you don't know which area it was.
            Intent::History(what) => {
                use crate::undo::{reverse, say, tell, understand, Asking};
                match understand(if what.trim().is_empty() { "what did you do" } else { what }) {
                    Asking::WhatDidYouDo { since_mins } => {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0);
                        let since = now.saturating_sub(since_mins * 60);
                        tell(&self.history.since(since))
                    }
                    // "What did you do on your own?" answers from the
                    // unprompted actions alone. `on_its_own` was written for
                    // exactly this review and nothing reached it: the general
                    // list mentioned how many were unasked but gave no way to
                    // see only those, so the one question a person asks to
                    // check what Atlas took upon itself fell through to the
                    // whole log or, worse, to "did you mean...".
                    Asking::WhatOnYourOwn { since_mins } => {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0);
                        let since = now.saturating_sub(since_mins * 60);
                        let mine = self.history.on_its_own(since);
                        match mine.first() {
                            None => "Nothing on my own — everything I did, you asked for.".into(),
                            Some(newest) => {
                                let n = mine.len();
                                let word = if n == 1 { "thing" } else { "things" };
                                let mut s =
                                    format!("{n} {word} I did on my own. Most recent: {}.", newest.what);
                                s.push_str(" Say undo and I'll take back the last one.");
                                s
                            }
                        }
                    }
                    Asking::UndoLast => say(&reverse(self.history.last())),
                    Asking::UndoIn(area) => say(&reverse(self.history.last_in(&area))),
                    Asking::SomethingElse => {
                        "Did you mean what I've done, or undoing something?".into()
                    }
                }
            }


            // Why did you do that — with where the setting is.
            Intent::Why(about) => {
                let matching: Vec<&crate::why::Decision> = self
                    .decisions
                    .decisions
                    .iter()
                    .filter(|_| about.trim().is_empty() || crate::why::is_asking_why(about))
                    .collect();
                if about.trim().is_empty() {
                    // A bare "why" — "why is my workspace like this?" — asks
                    // for everything decided, not just the one thing said
                    // last. `why::account` was built for exactly this and
                    // had no caller: `answer` only ever received the single
                    // latest `Decision`, so a general question got a
                    // one-line answer about whatever happened to be most
                    // recent rather than an actual account.
                    let last_steps = matching.last().map(|d| crate::why::steps(d));
                    let account = crate::why::account(&matching);
                    if let Some(steps) = last_steps {
                        self.show_panel(crate::window::Panel::Thinking, "How I got there", steps);
                    }
                    // The live half `why::account` cannot know: what the
                    // mind is on RIGHT NOW, in its own recent words. A bare
                    // "why" during a running job is usually about the job.
                    if let Some(w) = self.mind.focus() {
                        let recent: Vec<String> = w
                            .recent_thinking(3)
                            .iter()
                            .map(|th| th.text.clone())
                            .collect();
                        if !recent.is_empty() {
                            return format!(
                                "Right now, {}: {}. {account}",
                                w.asked,
                                recent.join("; ")
                            );
                        }
                    }
                    return account;
                }
                // Cloned out before the panel call, which needs `&mut self`.
                let latest = matching.last().map(|d| (*d).clone());
                if let Some(d) = &latest {
                    self.show_panel(
                        crate::window::Panel::Thinking,
                        "How I got there",
                        crate::why::steps(d),
                    );
                }
                match latest.as_ref() {
                    Some(d) => crate::why::answer(Some(d)),
                    None => crate::why::answer(None),
                }
            }

            // Making an account is a commitment made as you, so it is
            // refused outright on anything financial before any config is
            // even read, and needs approval everywhere else.
            Intent::CreateAccount(where_) => {
                let money = self.tools_ref()
                    .map(|t| t.finance.clone())
                    .unwrap_or_default();
                match crate::enrol::domain_from(where_) {
                    None => "I couldn't tell which site you meant.".into(),
                    Some(domain) => {
                        let cfg = self.tools_ref()
                            .map(|t| t.enrol.clone())
                            .unwrap_or_default();
                        // One gate, not a hand-rolled subset of it. This
                        // branch used to check `never_enrols_on` and
                        // `cfg.enabled` itself -- and not `cfg.never_on`, so
                        // a domain on your OWN never list sailed past the
                        // daemon while `enrol::permitted` would have refused
                        // it. Two copies of one decision, one of them short a
                        // check, is exactly the drift `permitted` exists to
                        // prevent.
                        match crate::enrol::Enrolment::permitted(&domain, &cfg, &money) {
                            // `permitted`'s own wording for this one is terse;
                            // keep the line that tells you where the switch is.
                            Err(why) if why == "account creation is switched off" => format!(
                                "Signing up is switched off. Turn on \"Make accounts\" in \
                                 settings if you want me doing that."
                            ),
                            Err(why) => why,
                            // Eric, B6: Atlas may make accounts. It
                            // stops for good at payment or ID, and hands
                            // a robot check or a code to you.
                            Ok(()) => self.start_sign_up(&domain, crate::store::now()),
                        }
                    }
                }
            }

            // Signing you in. The credentials come from the vault and the
            // domain has to match exactly — that check is what makes this
            // safer than typing it yourself.
            Intent::SignIn(where_) => {
                let cfg = self.tools_cfg().signin.clone();
                if !cfg.enabled {
                    "Signing in is switched off. Turn on \"Sign you into sites\" in settings.".into()
                } else if self.vault.state() != crate::vault::State::Open {
                    "The vault's locked — say the passphrase first.".into()
                } else {
                    match self.access.which_account(where_, Some(where_)) {
                        crate::signin::Which::None => {
                            format!("I don't have access to {where_}. Want to give me it?")
                        }
                        crate::signin::Which::Several(_) => self
                            .access
                            .which_account(where_, None)
                            .ask(where_)
                            .unwrap_or_default(),
                        crate::signin::Which::One(account) => {
                            // The grant is actually checked now. This branch
                            // used to announce "signing you into X" having
                            // checked only that the feature was on — the
                            // lookalike-domain check, the grant's own
                            // `Allowed::Nothing`, and the vault state were
                            // all in `may_fill`, which nothing called.
                            //
                            // `you_are_here` is true by construction: this
                            // intent exists because you just asked for it.
                            // The two questions `may_fill` also asks — is
                            // this a login form, was it reached by typing the
                            // address — belong to a browser, and nothing
                            // here has one open, so `may_start` is the
                            // honest half rather than two invented `true`s.
                            let vault_open =
                                self.vault.state() == crate::vault::State::Open;
                            // A credential that has stopped working is worth
                            // saying before the attempt, not after: "sign-in
                            // failed" sends you to check the site, and the
                            // answer is usually that you changed the password
                            // somewhere else.
                            if let Some(g) = self
                                .access
                                .find_account(where_, &account)
                                .filter(|g| g.looks_superseded)
                            {
                                return crate::signin::probably_changed(g);
                            }
                            match self.access.may_start(where_, vault_open, true, &cfg) {
                                Err(refused) => {
                                    // Every attempt is recorded, whether it
                                    // worked or not. A refusal is a use of
                                    // the credential that did not happen, and
                                    // the trail is the only way a wrong one
                                    // is visible afterwards.
                                    self.access.note_use(
                                        where_,
                                        &account,
                                        where_,
                                        false,
                                        true,
                                        crate::store::now(),
                                    );
                                    refused.say()
                                }
                                Ok(_) => {
                                    self.access.note_use(
                                        where_,
                                        &account,
                                        where_,
                                        true,
                                        true,
                                        crate::store::now(),
                                    );
                                    // Your bank is a site like any other for
                                    // signing in. What it isn't is a site
                                    // Atlas can do anything else on, and that
                                    // line is drawn in finance::allowed
                                    // rather than here.
                                    let money = self.tools_cfg().finance.clone();
                                    let care = if money.is_financial(where_) {
                                        " I'll read what's there and nothing else — no \
                                         transfers, no orders."
                                    } else {
                                        ""
                                    };
                                    if crate::signin::Access::asks_first(&cfg) {
                                        // Asked, and now actually waited on:
                                        // this question used to go nowhere.
                                        let q = format!(
                                            "Sign you into {where_} as {account}? You've asked \
                                             me to check first each time.{care}"
                                        );
                                        self.session.ask(&q);
                                        self.pending_signin = Some((where_.clone(), account.clone()));
                                        q
                                    } else {
                                        // Autofill, for real (Eric, B4): the
                                        // login goes in on the site's own
                                        // domain, in Atlas's browser.
                                        let started = self.start_sign_in(where_, &account, crate::store::now());
                                        format!("{started}{care}")
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Read them, rather than count them. "3 unread" is the
            // count-shaped notification this codebase is written against: it
            // tells you something happened and makes you go and look.
            Intent::Messages => self.read_messages(),

            // The conversation itself, read back. `session::transcript` already
            // assembles the recent turns for the model; nothing until now read
            // them to you. Distinct from `History`, which is the log of what
            // Atlas *did* -- this is what was said. The current "recap" turn is
            // recorded after this returns, so it is never in its own answer.
            Intent::Recap => {
                let convo = self.session.transcript(20);
                if convo.trim().is_empty() {
                    "We haven't said anything yet this session.".into()
                } else {
                    format!("Here's our conversation so far:\n{convo}")
                }
            }

            // The autonomy ledger, read back. `earned::may_act_alone` and
            // `earned::rope` decide this on every turn and were reached by
            // nothing a person could ask; this is the door to them.
            Intent::ActAlone => self.what_i_can_do_alone(),

            // The size of the knowledge store, read back. `consolidate::size_note`
            // states the whole module's promise in numbers -- how much is known
            // and roughly what it costs, and that it does not grow while you are
            // not asking -- and was reached only by its own test. This is the
            // question that finally asks it.
            Intent::KnowledgeSize => self.knowledge_store_size(),

            // "File that under groceries" / "that's actually a task". The
            // caller `capture::Notebook::correct` never had: capture only ever
            // *added* notes, so the filing guess it makes was never correctable
            // by anything a person could say. This is the door to it -- it
            // fixes the most recent note's kind or handle and marks it
            // confirmed, so the correction survives and `find` reaches it by
            // the handle you actually used.
            Intent::Refile(correction) => self.refile_note(correction),

            // The caller `knowhow::for_symptom` never had. `known_procedure`
            // above answers "how do I do X" from `for_request`; this starts
            // from what went wrong and scores the symptom against every
            // procedure's snags, so the one function that reads a symptom
            // finally has a question that asks it.
            Intent::Diagnose(symptom) => self.diagnose_symptom(symptom),

            // The producer `knowhow::as_plan` never had. `known_procedure`
            // (reached from the Unknown fallback) matches a task with
            // `for_request` but only ever says `announce` -- "I know this one,
            // 3 steps" -- and stops. This asks for the same match and reads
            // the steps out, so "I know how" is finally followed by "here's
            // how". Offline by construction; the procedures ship compiled in.
            Intent::WalkThrough(asked) => self.walk_me_through(asked),

            Intent::WhoIsIn(arg) => self.who_is_in(arg),

            Intent::NameGroup(arg) => self.rename_group(arg),

            Intent::LeaveGroup(arg) => self.leave_group(arg),
            Intent::ChangeGroup(said) => self.change_group(said),
            Intent::Friend(said) => self.friend(said),

            Intent::Message(raw) => self.send_message(raw),

            // Coming back to what setup skipped. `FirstRun::resume` has
            // existed since firstrun was written; this is the first thing
            // that ever called it.
            Intent::FinishSetup => {
                let mut fr = crate::firstrun::FirstRun::load(&self.store);
                match fr.resume() {
                    Some(step) => {
                        if let Err(e) = fr.save(&self.store) {
                            return format!(
                                "I couldn't write that down ({e}), so it would ask you \
                                 again next time. Left alone."
                            );
                        }
                        format!(
                            "Picking up {}. Start Atlas again and its setup carries on from there.",
                            step.plain()
                        )
                    }
                    // Said plainly rather than starting setup over. Somebody
                    // who says this when nothing was skipped meant "is there
                    // anything left", and the answer is no.
                    None => "Nothing was left unfinished — setup is done.".into(),
                }
            }

            // Muting a topic, or unmuting it. The sentence decides which.
            Intent::MuteTopic(said) => self.mute_topic(said),

            // The face in front of the camera is yours. Goes through the same
            // `name_this` the album already uses, under the name config says
            // means you, so there is one path that writes a face and not two.
            Intent::ThisIsMe => {
                let who = self.tools_cfg().vision.your_face.clone();
                let who = if who.trim().is_empty() { "me".to_string() } else { who };
                self.name_this(&who, crate::store::now())
            }

            // "Hand over." Free to say, by anybody, with nothing checked --
            // which is the whole design and is why it took so long to notice
            // that no phrase reached it.
            Intent::HandOver(note) => {
                let state = crate::roots::install_state();
                let mut h = crate::handover::Handover::load(&state);
                let said = h.hand_over(note, clock());
                if let Err(e) = h.save(&state) {
                    // A handover that did not survive being written down is
                    // one that ends the moment Atlas restarts, which is worse
                    // than refusing: you would have handed the laptop over
                    // believing it was narrowed.
                    return format!(
                        "I couldn't write that down ({e}), so I can't promise it holds. \
                         Try handing it over again."
                    );
                }
                said
            }

            // "I'm back." The one spoken phrase that ends a handover, and it
            // ends nothing by itself -- it asks for the passphrase somewhere
            // it can be typed. See `take_it_back` below for the whole of it.
            Intent::TakeItBack => self.take_it_back(),

            // Opening the vault. Nothing that needs a secret works until this
            // has happened, and it re-locks itself.
            Intent::Unlock(phrase) => {
                let now = clock();
                match self.vault.open(phrase, now, &self.tools_cfg().vault) {
                    Ok(()) => {
                        // Your sign-in copy follows the setting: made at the
                        // first unlock after it's turned on, taken away at
                        // the first unlock after it's turned off.
                        let sign_in = self.keep_sign_in_copy(now);
                        // The first unlock is where the salt and the check
                        // value come into existence. Not writing them here
                        // means the next start has neither, and a vault with
                        // no check value opens for anything.
                        let first = !self.vault.proved_it();
                        if let Err(e) = self.vault.save(&crate::roots::install_state()) {
                            format!(
                                "Open, but I couldn't write the vault to disk ({e}), so this \
                                 passphrase won't be remembered past this run."
                            )
                        } else if first {
                            format!("Open. That's the passphrase set — it's checked from now on.{sign_in}")
                        } else {
                            format!("Open.{sign_in}")
                        }
                    }
                    Err(why) => why,
                }
            }

            // Generate an invite for someone by name. Off by default (the
            // door itself defaults off -- see kin.rs's module doc on why
            // "enabled with nobody registered" isn't a safe middle state),
            // and refuses cleanly rather than guessing when the two things a
            // spoken pairing can't ask you to say out loud -- your own name,
            // your own Tailscale address -- haven't been set once in config.
            Intent::Pair(their_name) => {
                let their_name = spoken_name(their_name);
                if their_name.is_empty() {
                    return "Who should I pair with?".into();
                }
                let kin_cfg = self.tools_cfg().kin.clone();
                if !kin_cfg.enabled {
                    return "Pairing is switched off. Turn on kin in settings first.".into();
                }
                let (Some(my_name), Some(my_host)) =
                    (kin_cfg.my_name.clone(), kin_cfg.my_host.clone())
                else {
                    return "I need your name and Tailscale address set in config first -- \
                            kin.my_name and kin.my_host -- before I can pair with anyone."
                        .into();
                };
                // One directory for pairings, and it is the install's, not
                // the active profile's. See `kin::where_pairings_live`.
                let dir = self.peer_dir.clone();
                let mut pairings = crate::kin::Pairings::load(&dir);
                let token = match crate::server::new_token() {
                    Ok(t) => t,
                    Err(e) => return format!("Couldn't generate a secure token: {e}"),
                };
                match crate::kin::invite(&mut pairings, their_name, &my_name, &my_host, kin_cfg.port, &token) {
                    None => "Your name or host can't contain '|' -- fix that in config and try \
                             again."
                        .into(),
                    Some(code) => {
                        // Onto the live door as well, so "can already reach
                        // you" is true now rather than after a restart. The
                        // mirror of the revoke below: the door is built once
                        // at startup, so a peer written only to the file
                        // could not be served -- the pairing completed on
                        // both ends and then did not work, with nothing
                        // saying why.
                        let now_open = match (&self.signal_listener, pairings.peers.last()) {
                            (Some(l), Some(p)) => l.admit_peer(p.clone()),
                            // No listener: nothing is reachable either way,
                            // and the sentence below says so.
                            _ => false,
                        };
                        match pairings.save(&dir) {
                            Err(e) => format!("Couldn't save the pairing: {e}"),
                            Ok(()) if now_open => format!(
                                "{their_name} can already reach you -- I've remembered them. Send \
                                 them exactly this: {code}"
                            ),
                            Ok(()) => format!(
                                "Remembered {their_name}. The door for peers isn't open in this \
                                 session, so they can reach me from my next start. Send them \
                                 exactly this: {code}"
                            ),
                        }
                    }
                }
            }

            // The other half of a pairing. Realistically typed or pasted --
            // see the doc comment on the intent itself -- but wired the same
            // way as every other intent rather than given a second path.
            Intent::AcceptPairing(code) => {
                let code = code.trim();
                if code.is_empty() {
                    return "Paste the pairing code someone sent you.".into();
                }
                let kin_cfg = self.tools_cfg().kin.clone();
                if !kin_cfg.enabled {
                    return "Pairing is switched off. Turn on kin in settings first.".into();
                }
                let (Some(my_name), Some(my_host)) =
                    (kin_cfg.my_name.clone(), kin_cfg.my_host.clone())
                else {
                    return "I need your name and Tailscale address set in config first -- \
                            kin.my_name and kin.my_host -- before I can accept a pairing."
                        .into();
                };
                // One directory for pairings, and it is the install's, not
                // the active profile's. See `kin::where_pairings_live`.
                let dir = self.peer_dir.clone();
                let mut pairings = crate::kin::Pairings::load(&dir);
                match crate::kin::accept(&mut pairings, code, &my_name, &my_host, kin_cfg.port) {
                    Err(e) => format!("Couldn't accept that: {}", e.plain()),
                    Ok(accepted) => match pairings.save(&dir) {
                        Err(e) => format!("Couldn't save the pairing: {e}"),
                        Ok(()) => match accepted.return_block {
                            None => format!("Paired with {}.", accepted.from),
                            Some(block) => format!(
                                "Paired with {}. Send this back so they can reach you too: {block}",
                                accepted.from
                            ),
                        },
                    },
                }
            }

            // Undo a pairing, both directions. Reports rather than asks --
            // see policy::classify -- because removing standing access is
            // the safer direction to move in. Deliberately does NOT check
            // kin_cfg.enabled the way Pair/AcceptPairing do: revoking
            // should never be harder to reach than granting was, including
            // when you've switched the feature off specifically to clean up
            // stale pairings before turning it back on.
            Intent::ForgetPeer(name) => {
                let name = spoken_name(name);
                if name.is_empty() {
                    return "Who should I forget the pairing with?".into();
                }
                // One directory for pairings, and it is the install's, not
                // the active profile's. See `kin::where_pairings_live`.
                let dir = self.peer_dir.clone();
                let mut pairings = crate::kin::Pairings::load(&dir);
                if !pairings.forget(name) {
                    return format!("I don't have a pairing with {name}.");
                }
                // The live door, not only the file.
                //
                // `Door` is built once at startup from a cloned peer list. So
                // rewriting `kin_peers.yaml` took effect at the next restart
                // and not before -- while this arm replied "Forgotten. {name}
                // can no longer reach you", and the next `/signal` or
                // `/handoff` from that peer was accepted and delivered. A
                // stated protection the code did not implement, for as long
                // as Atlas stayed up.
                let door_closed = match &self.signal_listener {
                    Some(l) => l.forget_peer(name),
                    // No listener means nothing is open in the first place,
                    // so there is nothing to close and nothing to qualify.
                    None => true,
                };
                match pairings.save(&dir) {
                    Err(e) => format!(
                        "I've stopped letting {name} in, but I couldn't write it down: {e}. \
                         That means it comes back when I restart -- worth running this again."
                    ),
                    Ok(()) if door_closed => format!(
                        "Forgotten. {name} can no longer reach you, and you can no longer reach them."
                    ),
                    // Honest about the half that did not happen. The file is
                    // right, so the next restart is right; until then the
                    // open door is still open.
                    Ok(()) => format!(
                        "Written down, so {name} is gone from my pairings. I couldn't close the \
                         door that's already open, though -- they can still reach me until I \
                         restart."
                    ),
                }
            }

            // Catch it now, file it afterwards. Asking where it goes at the
            // moment you have the thought is what loses the thought.
            Intent::Capture(text) => {
                let now = clock();
                let cfg = self.tools_cfg().capture.clone();

                // A sentence that already contains the whole record makes an
                // item, not a note. A note you file later is a debt.
                let people: Vec<String> = Vec::new();
                let spoken = crate::capture::read_spoken(text, &cfg.projects, &people);
                // A capture that names a project is the project being
                // worked on — `touch_project` stamps `last_touched` so
                // "what was I on?" has an answer. The `projects` store had
                // no production writer at all before this.
                if let Some(p) = &spoken.project {
                    self.memory.touch_project(p, None);
                    let _ = self.memory.save(&self.store);
                    // If it names a project Atlas is tracking in the workshop,
                    // the capture is also an outstanding task for that project
                    // — so it shows up in that project's window, not just as a
                    // loose note. `title` is what to do, if the capture named
                    // one, else the raw text.
                    if self.workshop.resolve(p).is_some() {
                        let task = spoken.title.clone().unwrap_or_else(|| text.clone());
                        self.workshop.add_task(p, &task, now);
                        let _ = self.workshop.save(&self.store);
                    }
                }
                if spoken.is_a_whole_item() {
                    crate::capture::made(&spoken)
                } else {
                    let id = self.notebook.capture(text, None, now, &cfg);
                    // "Call the bank Friday at 10" is dated, so it comes up
                    // in Friday's brief (round 11).
                    self.wd_date_note(id, now);
                    // Recorded for your other devices at the same moment it
                    // is written here. A capture is the cleanest thing sync
                    // carries -- `What::Captured` can never clash, because
                    // adding something is not a change to anything.
                    self.synclog.append(
                        crate::sync::What::Captured {
                            id: id.to_string(),
                            text: text.clone(),
                        },
                        now,
                    );
                    // Written down before Atlas says it was. Acknowledging
                    // first and saving afterwards leaves a window where the
                    // reply is true and the disk is not, and a thought caught
                    // before it was gone is exactly the thing not to lose to
                    // a window.
                    if let Err(e) = self.notebook.save(&self.store) {
                        return format!(
                            "I couldn't write that down ({e}). Say it again once \
                             that's sorted, because I haven't kept it."
                        );
                    }
                    // Also remember it as a typed fact, so "what do you know
                    // about the wifi" finds it later — the one capture feeding
                    // both the note store and the fact book, not two islands
                    // that never see each other's contents. A task is an action
                    // to do, not a fact about your world, so only non-tasks are
                    // filed as knowledge; the kind (a preference, a pointer, or
                    // a fact about you) is read from the words.
                    if crate::capture::kind_of(text) != crate::capture::Kind::Task {
                        // `learn`, not `put`: saying the same thing again
                        // strengthens the one fact instead of adding a duplicate,
                        // which is what keeps the book bounded through normal use.
                        // `trim` then holds the footprint under budget by fading
                        // only Atlas's own stale guesses — never anything you
                        // stated. See facts::Book::trim.
                        self.facts.learn(crate::facts::Fact::stated(text, now), now);
                        self.facts.trim(crate::facts::MEMORY_BUDGET_BYTES, now);
                        let _ = self.facts.save(&self.store);
                    }
                    // "Turn tasks into outstanding items automatically" --
                    // `tasks_become_work` ships `true` and was read by
                    // nothing, so a captured task stayed a note and never
                    // reached the list Atlas reads back to you. Both halves
                    // existed: `capture::kind_of` already says which captures
                    // are tasks, and `Backlog` is already what `outstanding()`
                    // and the brief report from.
                    //
                    // Filed as `Unsupported`, which reads "I can't do that one
                    // for you yet" and, unlike every other blocker, is never
                    // offered back (`is_blocked` returns true for it
                    // unconditionally). That is the honest shape: your task on
                    // your list, not Atlas volunteering to phone the dentist.
                    if cfg.tasks_become_work
                        && crate::capture::kind_of(text) == crate::capture::Kind::Task
                    {
                        self.backlog.record(
                            text,
                            crate::backlog::Blocker::Unsupported("do that one for you".into()),
                            now,
                        );
                        let _ = self.backlog.save(&self.store);
                    }
                    match spoken.worth_asking() {
                        // One question, and only when it would turn a note
                        // into something finished -- and only when you have
                        // not said not to. `never_ask_on_capture` ships
                        // `true` ("Ask nothing at capture time. This is the
                        // whole point") and was read by nothing until 18 Sep
                        // 2026, so the default install did the opposite of
                        // its own default.
                        Some(q) if spoken.title.is_some() && !cfg.never_ask_on_capture => {
                            format!("{} {q}", self.notebook.acknowledge(id))
                        }
                        _ => self.notebook.acknowledge(id),
                    }
                }
            }

            // Everyday money. Trading has its own rules and its own
            // module; this is the other 95%.
            // Trading has rules of its own that catch people out. Asked
            // about tax or a statement, the trading knowledge comes first
            // because it's the part nobody has.
            // The three mail readings, weighed together rather than tried in
            // order. Until 19 Sep 2026 these were three `match` arms guarded
            // by `what.contains(..)`, so "how much did I spend on my trading
            // statement last month" -- a money question -- reached the tax arm
            // because the tax arm is written first. Nothing failed; the answer
            // was about the wrong thing.
            //
            // `whichone::weigh` scores every reading against the same sentence
            // independently, so the order these arms appear in no longer
            // decides anything, and a win too narrow to trust becomes a
            // question instead of a guess. The guard is one arm above.
            Intent::Mail(what)
                if crate::whichone::weigh(
                    what,
                    crate::whichone::ABOUT_MAIL,
                    &self.tools_cfg().whichone,
                )
                .clarity
                    == crate::whichone::Clarity::Close =>
            {
                let w = crate::whichone::weigh(
                    what,
                    crate::whichone::ABOUT_MAIL,
                    &self.tools_cfg().whichone,
                );
                crate::whichone::which_did_you_mean(&w, crate::whichone::ABOUT_MAIL)
                    .unwrap_or_else(|| "Which did you mean?".into())
            }

            Intent::Mail(what)
                if crate::whichone::weigh(
                    what,
                    crate::whichone::ABOUT_MAIL,
                    &self.tools_cfg().whichone,
                )
                .settled()
                    == Some("rules") =>
            {
                let rules = crate::ledger::trading_rules();
                // The one that costs people money, and why it applies to you
                // rather than in general.
                match rules.first() {
                    Some(r) => format!("{} — {}", r.what, r.why_you),
                    None => "Nothing I know about that.".into(),
                }
            }

            Intent::Mail(what)
                if crate::whichone::weigh(
                    what,
                    crate::whichone::ABOUT_MAIL,
                    &self.tools_cfg().whichone,
                )
                .settled()
                    == Some("statements") =>
            {
                let cfg = self.tools_cfg().money.clone();
                if !cfg.enabled {
                    "Reading your statements is switched off.".into()
                } else {
                    // This passed `&[]`. `money::spoken` opens with
                    // `if m.by_bucket.is_empty() { return "Nothing to go on." }`
                    // -- so with no entries that was the *only* reachable
                    // answer, and the reason was that nothing had ever read a
                    // statement rather than that there was nothing in it.
                    // Same shape as `goingaway::spoken(&[])` and
                    // `messaging::spoken(&[], ..)` before it.
                    let this: Vec<crate::money::Entry> =
                        self.store.load(crate::money::THIS_MONTH);
                    if this.is_empty() {
                        "I haven't read a statement yet. Hand me your bank's export (.csv) \
                         on the Give page and I'll sort it."
                            .into()
                    } else {
                        let last: Vec<crate::money::Entry> =
                            self.store.load(crate::money::LAST_MONTH);
                        let jump = self.tools_cfg().finance.category_jump as f32;
                        let mut changes = crate::money::new_or_grown(&this, &last, jump);
                        changes.extend(crate::money::buckets_that_jumped(&this, &last, jump));
                        crate::money::spoken(&crate::money::summarise(&this), &changes)
                    }
                }
            }

            // The inbox, sorted by what it asks of you.
            // Messages, sorted the same way the inbox is — by what they ask
            // of you. Group chatter never interrupts.
            Intent::Mail(what)
                if crate::whichone::weigh(
                    what,
                    crate::whichone::ABOUT_MAIL,
                    &self.tools_cfg().whichone,
                )
                .settled()
                    == Some("messages") =>
            {
                let cfg = self.tools_cfg().messaging.clone();
                if !cfg.enabled {
                    "I'm not connected to any messaging.".into()
                } else {
                    // Was `messaging::spoken(&[], ..)`, which on an empty
                    // slice returns literally "0 messages, all group chat" --
                    // a count of something nothing had read, stated as fact.
                    // `imap.rs` reads mail; there is no messaging reader.
                    //
                    // Then it was a fixed sentence saying so, which was true
                    // and useless: `messaging.platforms` is a list you wrote
                    // and nothing read, and two of the six things you can put
                    // in it can never work at all.
                    //
                    // There is a reader now. `telegram.rs` reads a bot's
                    // messages and `atlas telegram` keeps them, so the
                    // sorting that was written and unreachable -- `sort`,
                    // `folder_for`, `note_on`, `spoken` -- finally runs on
                    // something somebody sent. Whether there is anything to
                    // sort decides which answer this is, and the empty case
                    // still refuses to give a count: "nothing read yet" and
                    // "nothing in it" are different facts and the old code
                    // could only ever state the second.
                    let kept: Vec<crate::messaging::Message> =
                        self.store.load(crate::telegram::KEPT);
                    if kept.is_empty() {
                        crate::messaging::what_you_asked_for(&cfg.platforms)
                    } else {
                        let mut answer = crate::messaging::spoken(&kept, &cfg.your_names);
                        // Reading the messages is also the only moment Atlas
                        // learns who has been in touch. `note_on` reads a
                        // sender's messages and decides which folder they
                        // belong in; kept here, the contact book grows from
                        // real messages instead of being typed in by hand, and
                        // a new work approach gets said rather than buried in a
                        // count. Anything already personal or unsorted is filed
                        // silently.
                        answer.push_str(&self.note_the_senders(&kept, &cfg.your_names));
                        answer
                    }
                }
            }

            Intent::Mail(what) => {
                let cfg = self.tools_cfg().mail.clone();
                if what.contains("outlook setup") || what.contains("connect outlook") && what.contains("help") {
                    crate::msoauth::SETUP.to_string()
                } else if what.contains("connect") && what.contains("outlook") {
                    self.connect_outlook_from_request(what)
                } else if !cfg.enabled {
                    "Reading your email is switched off.".into()
                } else if what.contains("clear") || what.contains("unsubscribe") {
                    self.check_unsubscribe()
                } else if what.contains("outreach") {
                    self.draft_outreach_from_request(what)
                } else if (what.contains("discard") || what.contains("throw") || what.contains("scrap"))
                    && (what.contains("draft") || what.contains("reply"))
                {
                    let who = what.rsplit("to ").next().unwrap_or(what).trim();
                    self.discard_draft(who)
                } else if what.contains("draft") || what.contains("reply") {
                    let who = what.rsplit("to ").next().unwrap_or(what).trim();
                    self.read_draft(who)
                } else if what.contains("order") {
                    self.read_order(what)
                } else {
                    self.check_mail()
                }
            }

            // Getting what happened here to your other devices.
            //
            // The four arguments to `mesh::choose` are
            // `same_network, mesh_up, cloud_ok, plugged_in`, and all four
            // were hardcoded literals rather than observations -- so the
            // answer was invariably "Sending it through the cloud folder --
            // it'll land shortly", `Path::SameNetwork`, `Path::Mesh` and
            // `Path::Cable` were unreachable, and nothing was transferred
            // either way. `path` was computed and discarded.
            //
            // There is no transfer in this tree yet. So this now says which
            // route it *would* take, from what Atlas can actually observe,
            // and says plainly that the sending half is not built -- which is
            // the difference between a plan and a lie.
            Intent::Sync(_) => self.carry_to_your_other_devices(clock()),

            // Asking one of your other Atlases how it is getting on.
            //
            // `kin.rs` is the door another Atlas knocks on, and its rule --
            // a signal becomes exactly one thing, a nudge, never a command --
            // is right and is untouched. But it runs one way, and the machine
            // that most needs looking in on is a server with nobody logged
            // into it.
            //
            // This is the other direction and it is read-only: your Atlas
            // asks, over a door that checks a token, for things that Atlas
            // already serves. What comes back is words you read. Nothing here
            // turns a reply into an intent, an action or an approval, and
            // `a_brief_is_words_not_instructions` fails the build if that
            // stops being true.
            Intent::BriefOn(who) => {
                let cfg = self.tools_cfg().elsewhere.clone();
                if !cfg.enabled {
                    "Asking your other Atlases is switched off in your settings.".into()
                } else if who.trim().is_empty() {
                    match cfg.names().as_slice() {
                        [] => "I don't know any other Atlas to ask. Add one under \
                               `elsewhere` in your settings."
                            .into(),
                        names => format!("Which one? I can ask: {}.", names.join(", ")),
                    }
                } else {
                    match cfg.find(who) {
                        None => {
                            let known = cfg.names();
                            if known.is_empty() {
                                format!(
                                    "I don't know an Atlas called {}. Add one under \
                                     `elsewhere` in your settings.",
                                    who.trim()
                                )
                            } else {
                                format!(
                                    "I don't know an Atlas called {} — I can ask: {}.",
                                    who.trim(),
                                    known.join(", ")
                                )
                            }
                        }
                        Some(e) => match crate::elsewhere::ask(e, &cfg) {
                            Ok(brief) => crate::elsewhere::spoken(&brief),
                            // Said plainly rather than swallowed. A check-in
                            // that goes quiet when the machine is down is
                            // useless exactly when it matters.
                            Err(why) => why,
                        },
                    }
                }
            }

            // Before it goes out: what's in the frame, and whether it opens.
            // A brand asked something. The reply is drafted, never sent.
            // Colour, checked against the fixed tree rather than by taste.
            // Does the draft actually say anything? A piece with no position
            // reads as competent and is forgotten.
            // Same treatment as the mail arms above. "Grade this post -- does
            // it say anything?" carries a word for each reading, and taking
            // the first arm meant answering about the argument when the
            // sentence led with the colour.
            Intent::ReviewPost(what)
                if crate::whichone::weigh(
                    what,
                    crate::whichone::ABOUT_A_POST,
                    &self.tools_cfg().whichone,
                )
                .clarity
                    == crate::whichone::Clarity::Close =>
            {
                let w = crate::whichone::weigh(
                    what,
                    crate::whichone::ABOUT_A_POST,
                    &self.tools_cfg().whichone,
                );
                crate::whichone::which_did_you_mean(&w, crate::whichone::ABOUT_A_POST)
                    .unwrap_or_else(|| "Which did you mean?".into())
            }

            Intent::ReviewPost(what)
                if crate::whichone::weigh(
                    what,
                    crate::whichone::ABOUT_A_POST,
                    &self.tools_cfg().whichone,
                )
                .settled()
                    == Some("stance") =>
            {
                // A post is a case being made, so that's what it's judged as.
                let kind = crate::stance::Kind::Case;
                let s = crate::stance::assess(what, kind);
                crate::stance::brief(&s, kind)
                    .unwrap_or_else(|| "It says something and supports it. Nothing I'd add.".into())
            }

            Intent::ReviewPost(what)
                if crate::whichone::weigh(
                    what,
                    crate::whichone::ABOUT_A_POST,
                    &self.tools_cfg().whichone,
                )
                .settled()
                    == Some("grading") =>
            {
                let cfg = self.tools_cfg().grading.clone();
                if !cfg.enabled {
                    "Colour checking is switched off.".into()
                } else {
                    // Was `grading::spoken(&[])`, whose empty case is
                    // "Grade looks clean." So turning colour checking *on*
                    // changed the answer from an honest refusal into a false
                    // pass on a file nothing had opened. Nothing in this tree
                    // produces grade notes yet -- there is no node-tree
                    // reader -- so the honest answer is that, not a verdict.
                    "Colour checking is on, but I've no way to read the grade tree yet, \
                     so I can't tell you whether it's clean. I'd rather say that than \
                     pass a file I never opened."
                        .into()
                }
            }

            Intent::ReviewPost(what)
                if crate::editcraft::what_they_asked(what) != crate::editcraft::BrandAsks::Other =>
            {
                let asked = crate::editcraft::what_they_asked(what);
                match crate::editcraft::reply_to(asked) {
                    Some(r) => format!("{r}\n\n{}", crate::editcraft::THE_PRINCIPLE),
                    None => "I don't have a line for that one.".into(),
                }
            }

            Intent::ReviewPost(what) => {
                let opsec = self.tools_cfg().opsec.clone();
                // Was a fixed, aging string, and `check` was called against
                // `""` rather than the post -- meaning this arm has never
                // actually scanned a real post since it was written. `now()`
                // already exists on every daemon; slicing `iso_utc`'s first
                // ten characters is the same YYYY-MM-DD `still_applies` and
                // `days_left` compare against lexicographically, so nothing
                // about their own logic needed to change.
                let now = crate::store::now();
                let today = crate::digest::iso_utc(now);
                let today = &today[..10.min(today.len())];
                let found = crate::opsec::check(what, &[], &opsec, today);
                let mut said = crate::opsec::spoken(&found, &opsec, today);

                // Prose sits alongside opsec rather than gating on a phrase
                // the way the colour/stance/editcraft arms above do — every
                // review should still catch a dropped apostrophe or a
                // doubled word whether or not that's what was asked about.
                // But a review isn't live dictation either: a `Certain` fix
                // is applied rather than narrated one at a time -- fixing is
                // the job, not a running commentary on it -- and only the
                // ones Atlas genuinely can't tell were intentional are
                // worth asking about. Guessing wrong and silently changing
                // someone's deliberate wording is worse than asking once.
                let prose_cfg = self.tools_cfg().prose.clone();
                let mut fixes = crate::prose::check(what, &prose_cfg);
                // "better then", "more ... then": phrase-level fixes, only ever offered.
                fixes.extend(crate::prose::check_phrases(what));
                let (corrected, fixed_count) = crate::prose::apply_certain(what, &fixes);
                if fixed_count > 0 {
                    if !said.is_empty() {
                        said.push(' ');
                    }
                    said.push_str(&format!(
                        "Fixed {fixed_count} thing{} in the wording: \"{corrected}\"",
                        if fixed_count == 1 { "" } else { "s" }
                    ));
                }
                let unsure: Vec<&crate::prose::Fix> =
                    fixes.iter().filter(|f| f.kind != crate::prose::Kind::Certain).collect();
                if let Some(first) = unsure.first() {
                    if !said.is_empty() {
                        said.push(' ');
                    }
                    said.push_str(&format!(
                        "You wrote \"{}\" — {}. Did you mean it, or should I fix it?",
                        first.was, first.because
                    ));
                    if unsure.len() > 1 {
                        said.push_str(&format!(" ({} more like that.)", unsure.len() - 1));
                    }
                }

                // If a draft is open, the reviewed words become the draft.
                // `DraftPost` created an empty post and asked "what should
                // it say?" — and no arm ever answered: `edit` and
                // `request_approval` had no production caller, so every
                // post ever drafted stayed an empty Draft forever and the
                // corrected text this arm computed was thrown away.
                // `edit` voids any prior approval by design (the approval
                // was for the words you read), and the approval question
                // carries the full final text. Sending stays unbuilt:
                // `delivery::plan` still short-circuits, so nothing here
                // can post — it can only put the words where an eventual,
                // separately-ruled send would find them.
                let open_draft = self
                    .publisher
                    .posts
                    .iter()
                    .filter(|p| p.state == crate::publish::PostState::Draft)
                    .map(|p| p.id)
                    .next_back();
                if let Some(id) = open_draft {
                    let final_text =
                        if fixed_count > 0 { corrected.as_str() } else { what.as_str() };
                    if self.publisher.edit(id, final_text) {
                        if let Some(q) = self.publisher.request_approval(id) {
                            let _ = self.publisher.save(&self.store);
                            // The question now waits for its answer (G2):
                            // before this, a yes went nowhere and no post
                            // could ever be approved.
                            self.session.ask(&q);
                            self.pending_post_approval = Some(id);
                            if !said.is_empty() {
                                said.push(' ');
                            }
                            said.push_str(&q);
                        }
                    }
                }

                if said.is_empty() {
                    "Nothing I'd stop for.".into()
                } else {
                    said
                }
            }

            // What would lock you out if you left tomorrow.
            //
            // This passed `&[]`. `goingaway::spoken` opens with
            // `if stuck.is_empty() { return "You'd get into all of these from
            // anywhere. Nothing to do." }` -- so with no accounts that was
            // the *only* reachable answer, and the entire lock-out warning
            // below it was unreachable. You would ask "am I ready to travel?"
            // before a flight, be told every account was reachable from
            // anywhere, and land somewhere your SMS codes do not arrive.
            //
            // The real book was on `self` the whole time, and the hub already
            // used it correctly: `hublive.rs` does
            // `goingaway::plan(&self.accounts.accounts)`. Two answers to one
            // question, and the spoken one -- the one you would actually
            // rely on at an airport -- was the wrong one.
            Intent::TravelPrep => {
                if self.accounts.accounts.is_empty() {
                    "I don't know about any of your accounts yet, so I can't tell you \
                     which ones would lock you out. The Accounts page is where they go."
                        .into()
                } else {
                    // Both halves. "Every account is reachable from anywhere"
                    // is no comfort with no codes printed, and the codes
                    // count is no comfort for the account that has none --
                    // and this is the question you ask at an airport.
                    let names: Vec<String> =
                        self.accounts.accounts.iter().map(|a| a.site.clone()).collect();
                    let ccfg = self.tools_cfg().codes.clone();
                    let away: crate::goingaway::Away =
                        self.store.load(crate::goingaway::AWAY_RECORD);
                    let mut said = crate::goingaway::spoken(&self.accounts.accounts);
                    said.push(' ');
                    said.push_str(&crate::codes::before_you_go(
                        &self.code_sets,
                        &names,
                        &ccfg,
                    ));
                    if let Some(days) = away.days_until(crate::store::now()) {
                        said.push_str(&format!(" You're going in {days} days."));
                    }
                    said
                }
            }

            // Reading, converting and joining whatever you point at.
            Intent::Files(what) => {
                // A question about your files waits a moment for the list
                // on disk (`index::Loading`), then answers honestly either way.
                self.wait_for_index(INDEX_WAIT_FOR_A_QUESTION);
                self.files_request(what)
            }


            // Something Atlas doesn't have a command for. Before shrugging,
            // check whether it's a procedure it already knows — that was
            // written months ago and had never been consulted.
            // Atlas changing itself. It cannot start without a diagnosis
            // that holds up — a cause that isn't the symptom said again, and
            // a proving test that fails today.
            // What Atlas would change about itself, from what it can see
            // that you can't — which of its own routes keep failing, what you
            // keep correcting.
            // `!self.mid_self_work()` guards both of the arms above the main
            // one, and it is not a refinement.
            //
            // While a diagnosis is being collected, EVERY `WorkOnYourself`
            // turn is an answer to the question just asked. These two arms
            // fire on `what.contains("what")`, `"anything"`, `"yes"`,
            // `"do it"` and a blank turn — all of which are ordinary answers.
            // "every row re-reads whatever the config says" is a cause;
            // "yes, the left panel" is a location; a blank turn is someone
            // pressing enter. Without this, each of those was swallowed by a
            // self-audit or a grant reply, the question came back unchanged,
            // and the diagnosis could not be completed.
            Intent::WorkOnYourself(what)
                if !self.mid_self_work()
                    && (what.trim().is_empty()
                        || what.contains("what")
                        || what.contains("anything")) =>
            {
                let cfg = self.tools_cfg().self_audit.clone();
                if !cfg.enabled {
                    "Looking at myself is switched off.".into()
                } else {
                    self.refresh_signals();
                    let recs = crate::selfaudit::recommend(&self.signals, cfg.most_at_once);
                    let mut said = crate::selfaudit::spoken(&recs);
                    // The hollow check, actually run.
                    //
                    // `hollow.rs` said in its own docstring that it was used
                    // three ways -- doctor, the nightly self-audit, and the
                    // source ratchet. Only the ratchet was true: `audit`,
                    // `judge` and `judge_readings` were called by nothing. A
                    // bug-detector that never runs is the bug it was written
                    // to find.
                    let found = self.hollow_answers();
                    if !found.is_empty() {
                        said.push(' ');
                        said.push_str(&crate::hollow::spoken(&found, &self.tools_cfg().judgment));
                    }
                    // Speed is the thing a person actually feels, and this
                    // answer was silent on it. `refresh_signals` above rebuilds
                    // `self.signals` from undo, misunderstandings and unused
                    // capabilities alone -- so the `GotSlower` signal pushed
                    // during the hourly tidy is overwritten here and never
                    // reaches "anything to look at?". Ask the timing window
                    // itself: when a turn has measurably slowed, say why in one
                    // plain sentence -- `why_slow` is the sentence written for
                    // exactly that and had no caller. Gated on `got_slower` so
                    // it speaks only when there is a real regression, not on
                    // every fast machine.
                    if self.timing.got_slower().is_some() {
                        said.push(' ');
                        said.push_str(&self.timing.why_slow());
                    }
                    said
                }
            }

            // "Yes, go on" — the grant makes the difference between a system
            // that recommends and one that fixes.
            Intent::WorkOnYourself(what)
                if !self.mid_self_work()
                    && (what.contains("go on")
                        || what.contains("go ahead")
                        || what.contains("do it")
                        || what.contains("yes")) =>
            {
                let grants = self.tools_cfg().self_grant.clone();
                match grants.granted() {
                    None => crate::selfgrant::asking_for(
                        crate::selfgrant::Reach::WhatItSays,
                        &grants,
                    ),
                    Some(reach) => format!(
                        "Right — I'll fix {} on my own and tell you after. Anything past that \
                         still comes to you.",
                        reach.plain()
                    ),
                }
            }

            Intent::WorkOnYourself(what) => self.work_on_myself(&what),

            Intent::Build(what) => self.build_from_description(what),

            Intent::Improve(what) => self.improve_project(what),

            Intent::Implement(what) => self.implement_change(what),

            Intent::DesignReview(what) => self.design_review(what),

            Intent::Animate(what) => self.animate(what),
            Intent::Scene(what) => self.scene(what),

            Intent::Explain(what) => self.explain_code(what),
            Intent::PlainChange(_) => self.plain_change(),
            Intent::Booking(what) => self.booking(what),
            Intent::Learn(what) => self.learn_knowledge(what),

            Intent::Schedule(what) => self.schedule_event(what),

            Intent::Agenda(what) => self.read_agenda(what),

            // Before giving up, look at what Atlas has already written down.
            //
            // This is where recall earns its place. Most of the questions a
            // note answers — "what did I find out about X", "how much was the
            // quote for Y" — are not phrases the command parser knows, so
            // they arrive here as Unknown. Answering "I didn't catch that"
            // while holding a note on exactly that subject is the worst of
            // both: the search existed, the note existed, and neither was
            // consulted.
            Intent::Unknown(raw) => match self.known_procedure(raw) {
                Some(how) => how,
                None => match self.from_notes(raw, crate::store::now()) {
                    Some(answer) => answer,
                    // A question Atlas has no way to answer here is said to
                    // be one, rather than "I didn't catch that" -- which tells
                    // a person who spoke clearly that they didn't.
                    None if self.llm.is_none() && crate::wanted::is_a_question(raw) => {
                        "I can't answer that one here — general questions need my language model, and there \
                         isn't one on this machine yet. Start Atlas again and its setup fetches it (about 3 GB); the hub's Health page says where it stands."
                            .into()
                    }
                    None => "I didn't catch that.".into(),
                },
            },
        }
    }

    /// Errand kinds that call `crew::Control::checkpoint` at safe points, so
    /// a pause actually holds them. The rest run as one step and can only
    /// finish or be called off — said so, rather than claimed paused.
    const HOLDS_AT_A_SAFE_POINT: &'static [&'static str] = &[
        "research", "council", "build", "improve", "mail", "unsubscribe", "outreach",
        "outlook-connect", "search-check",
    ];

    fn readings(&self) -> Readings {
        crate::health::read_machine()
    }

    fn current_work(&self) -> Option<String> {
        // Crew work counts, and so do the windows being worked for you —
        // both are things "Paused. … on hold." should name. (This counted
        // only scheduled jobs that were due, so a render half an hour into
        // running reported as nothing.)
        let n = self.scheduler.due(crate::store::now()).len()
            + self.crew.errands().len()
            + self.working_for_you.iter().filter(|w| !w.held).count();
        match n {
            0 => None,
            1 => Some("1 job".into()),
            n => Some(format!("{n} jobs")),
        }
    }

    // ---------- background ----------

    /// Work Atlas does without being asked. Returns anything worth saying.
    pub fn tick(&mut self, t: u64) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(line) = self.model_warmed.lock().ok().and_then(|mut w| w.take()) {
            self.log.info(&line);
        }
        // The Talk page's waiting words, one turn each (`talk_queue`). A
        // turn that needs the model hands the call to a worker and is
        // finished on a later tick, so the loop -- and the hub -- never waits
        // on the model.
        self.talk_queue_turns(t);
        // A question left standing is stamped here, so its age is known.
        self.expire_stale_question(t);

        // THE HEARTBEAT, FIRST, before anything that can return early.
        //
        // It says one thing -- "this process is alive and holding the lock" --
        // and that is true on every tick regardless of mode, pause state, or
        // whether Atlas has anything to say. It has no business being
        // conditional on any of them.
        //
        // It used to sit 440 lines further down, behind two `return out`s: the
        // pause check and `if !self.modes.may_interrupt(false)`. Both are
        // reachable from shipped state -- `modes::suggested()` ships a focus
        // mode with `Interruptions::Urgent` and an on-a-call mode with
        // `Interruptions::Silent`, and `may_interrupt(false)` is false for
        // both. So:
        //
        //   say "focus mode", or pause Atlas, and the heartbeat stopped while
        //   Atlas was still running. `GONE_AFTER_SECS` is 150. After two and a
        //   half minutes the lock read `Abandoned`, and the next launch -- the
        //   shortcut, ATLAS.bat, a logon task -- took it and ran a SECOND
        //   daemon beside the live one. Both then load `data/state`, change it
        //   in memory, and write the whole thing back, so the second write
        //   erases whatever the first learned.
        //
        // `onlyone.rs`'s own module doc describes that outcome exactly: "The
        // damage is quiet... You notice weeks later that something you told it
        // didn't stick." The guard was built and then placed where the two
        // commonest quiet states switched it off. Found 17 Sep 2026 by reading
        // every `return` between the top of this function and the beat.
        // The heartbeat, and whether it landed.
        //
        // A beat that silently fails to write -- a full disk, a file a sync
        // client has locked -- lets the lock age past GONE_AFTER_SECS while
        // Atlas is running perfectly well. The next start then reads
        // `Abandoned`, takes the lock, and two instances write the same state
        // folder from their own memory. The holder used to have no way to
        // know: the write's result was discarded.
        //
        // Counted rather than reported on the first failure, because one
        // missed write is a blip and the staleness window is five beats wide.
        // Three in a row is a pattern with two beats of margin left.
        if crate::onlyone::OnlyOne::at(&self.store.data_dir()).beat(t) {
            self.missed_beats = 0;
        } else {
            self.missed_beats = self.missed_beats.saturating_add(1);
            const ENOUGH: u32 = 3;
            if self.missed_beats == ENOUGH {
                out.push(format!(
                    "I can't refresh my own instance lock at {}. If that keeps up, \
                     another Atlas started in the next couple of minutes will think \
                     this one is gone, take over, and the two of us will write over \
                     each other's memory. Worth checking the disk isn't full and that \
                     nothing has the file open.",
                    crate::onlyone::OnlyOne::at(&self.store.data_dir()).path().display()
                ));
            }
        }

        if let Some(w) = self.journal_warning.take() {
            self.log.warn(&w);
            out.push(w);
        }

        // Locked, or the screens off, is not asleep (H13a): the work carries
        // on, and the change is logged rather than treated as a night.
        self.note_running_state();
        // Mishearing you often enough to say so (H5), once.
        if let Some(line) = self.heard_note.take() {
            out.push(line);
        }

        // What a restart cut off: windows being worked are picked back up,
        // work that only makes an answer is redone once, and the rest is
        // named (`resume`).
        out.extend(self.pick_up_after_restart(t));
        self.keep_window_jobs();

        // Your settings, if you changed one while Atlas was running. Before
        // the pause check, like the heartbeat: switching something off is
        // exactly what you might do while Atlas is paused. Logged, not
        // spoken — whoever changed it has just seen the change.
        let _ = self.pick_up_settings();

        // Calls: notice one starting or ending, and hand back finished notes.
        out.extend(self.call_notes_tick(t));
        // A window being worked for you.
        // (Windows being worked for you go further down, once Atlas knows
        // whether you're at the keyboard.)

        // Heavyweight helpers that have gone idle. These are real kills, not
        // a walk over an empty list.
        //
        // Also moved up here, and for the same reason as the heartbeat: this
        // is housekeeping, and it was sitting below the pause check. So
        // pausing Atlas -- the thing you do to make it get out of the way --
        // pinned the model server's memory indefinitely, because the one
        // thing that brings an idle helper down never ran while paused. On a
        // laptop that is multiple gigabytes held precisely when you asked
        // Atlas to stand down.
        //
        // Reaped rather than force-stopped, deliberately. `Helpers::stop_all`
        // would take whisper down too, and "paused means only listening" --
        // the listening has to keep working. Reaping respects each helper's
        // own idle timeout, so what goes away is what was not being used.
        // A helper that died on its own comes off the books, so the next
        // time it is needed it is started again rather than trusted.
        for (name, how) in self.helpers.died() {
            self.log.warn(&format!("{name} stopped on its own ({how}); it will be started again when next needed"));
        }
        for said in self.helpers.reap(t) {
            self.log.info(&said);
        }

        // Answer a peer dialing us for a direct same-network sync, every beat —
        // so a phone coming onto the wifi is caught up at once, not only when
        // this device happens to run its own sync pass. Passive receive, so it
        // runs regardless of pause, like the housekeeping above.
        out.extend(self.serve_direct_sync(t));

        // Meaning vectors for notes that don't have one yet, a couple per
        // tick. Housekeeping, so it sits with the other housekeeping — but
        // unlike the heartbeat it spawns a process, and paused means Atlas
        // gets out of the way, so it respects the pause the way the reap
        // above deliberately does not.
        if !self.attention.is_paused() {
            self.embed_backlog();
        }

        // The exit statuses of the processes nobody waits for -- a panel, an
        // app the person opened, the headless browser. Never blocks. On unix,
        // not collecting them leaves one zombie per panel in the process
        // table for the life of the daemon, and a daemon runs for days. Here
        // with the other housekeeping, above the early returns, because a
        // paused Atlas still opens panels. See `unwaited`.
        crate::unwaited::reap();

        // "I can't write things down" is the one piece of housekeeping news
        // that has to reach you rather than the log, and it goes here for the
        // same reason as the two above: it is true regardless of mode, and a
        // paused or focused Atlas that cannot save is still an Atlas that is
        // losing everything you tell it.
        if let Some(trouble) = self.persist_trouble() {
            out.push(trouble);
        }

        // Re-lock the vault when it has been open too long. Above the pause
        // check, and among the security housekeeping, for the same reason as
        // the heartbeat: a vault left open is a vault open whether Atlas is
        // paused, focused, or idle, and `lock_after_mins` (15 by default) is a
        // promise that a walk-away does not leave every secret readable.
        //
        // Until this call the vault was locked in exactly one place --
        // `take_it_back`, immediately after proving identity -- so an ordinary
        // `atlas vault` unlock stayed open for the life of the process. The
        // idle rule the config already carried was honoured by nothing.
        //
        // `screen_locked` is `false` because no platform reading for it exists
        // in this tree yet: unknown reads as "not observed", which degrades to
        // the idle timeout rather than inventing a lock event. When the screen
        // state is plumbed through, it passes here and the second clause of
        // `should_lock` starts firing on its own.
        let vault_cfg = self.tools_cfg().vault.clone();
        if self.vault.should_lock(t, false, &vault_cfg) {
            self.vault.lock();
            self.log.info("Re-locked the vault after it sat open past its idle limit.");
        }

        let signals = self.observe(t);
        self.note_work(&signals, t);

        // Windows being worked for you. Typing waits for a gap in yours
        // (`lanes`): reading and deciding go on meanwhile.
        {
            let lanes = self.lane_cfg();
            let busy = signals.in_conversation || signals.idle_secs < lanes.gap_secs;
            out.extend(self.work_for_you(t, busy));
        }

        // Dictation that nobody is feeding stops itself.
        //
        // Above the pause check on purpose: a microphone that quietly stayed
        // in typing mode because you paused Atlas and walked off is the same
        // failure as a camera left watching an empty room, and `idle_stop_secs`
        // exists precisely so it cannot happen. `idle_check` only fires once,
        // since it flips the state that lets it fire.
        let dcfg = self.dictate_cfg();
        if let Some(d) = self.dictation.as_mut() {
            if d.idle_check(&dcfg, t) {
                self.dictation = None;
                out.push("Stopped dictating — nothing said for a while.".into());
            }
        }

        // Anything written to somebody else, tried again.
        //
        // Above the pause check: pausing Atlas stops it *speaking to you* and
        // stops it starting work, and a message you already wrote and asked
        // to be sent is neither. It is a thing already promised.
        //
        // Silent when nothing changed -- `Round::spoken` returns empty on a
        // round where everybody was simply offline, which is the ordinary
        // case and is not news.
        {
            // Built fresh each tick from the pairings and rooms as they are
            // now, so a peer paired or a room opened a moment ago is already
            // reachable. It owns its snapshot, holding no borrow on `chats`
            // while the courier mutates it.
            // Introductions and group lists first, so a message to a group
            // goes to the people its latest list says are in it.
            self.peer_upkeep(t);
            self.settle_owned_groups();
            if let Some(said) = self.post_release_notices(t) {
                out.push(said);
            }
            let link = {
                let pairings = crate::kin::Pairings::load(&self.peer_dir);
                self.peer_link(&pairings)
            };
            let round = crate::courier::run(
                &mut self.chats,
                &link,
                &mut self.tries,
                &crate::courier::Attempts::default(),
                t,
            );
            if !round.nothing_happened() {
                let _ = self.chats.save(&self.store);
                out.push(round.spoken());
            }

            // Read receipts owed to the people whose messages we've read,
            // sent on the same link and in the same pass as delivery. Silent:
            // a receipt is not news to the person who read the message, only
            // to the person who sent it, and this end is the reader. Saved
            // only when a receipt actually advanced a room's mark, so a
            // failed pass leaves the state to be retried next tick.
            if crate::courier::send_receipts(&mut self.chats, &link, t) > 0 {
                let _ = self.chats.save(&self.store);
            }
        }

        // Paused means paused: no jobs, no posts, no offers. Only listening.
        if self.attention.is_paused() {
            return out;
        }

        // Scheduled jobs.
        for id in self.scheduler.due(t) {
            let Some(job) = self.scheduler.jobs.iter().find(|j| j.id == id).cloned() else { continue };
            let intent = self.parser.parse(&job.command);
            let call = crate::policy::classify_with_policy(&intent, &self.memory, &self.cfg.policy);

            // A scheduled job you never individually approved must not run
            // itself just because a timer fired. Approving the job unlocks it
            // permanently; until then it waits, supervised or not.
            if call.needs_consent() && !job.approved {
                self.scheduler.park_for_approval(id);
                if self.autonomy == Autonomy::Supervised {
                    let q = format!("Scheduled: {}. Go ahead?", job.command);
                    self.session.await_approval(intent.clone(), &q);
                    self.pending_job = Some(id);
                    out.push(q);
                }
                continue;
            }
            let result = self.execute(&intent);
            let ok = !result.starts_with("error");
            self.journal.record_at(Act::Scheduled, &job.command, ok, t);
            self.scheduler.complete(id, t, &result, ok);
            if call != Decision::AutoProceed || !ok {
                out.push(result);
            }
        }

        // --- Add-on sequences that run by themselves ---
        //
        // Approving the add-on is the consent for these, the rule scheduled
        // jobs already follow; each step is still checked as it runs, and a
        // step that asks still asks. One at a time, and never over a sequence
        // already in hand -- a due one waits for the next tick rather than
        // being dropped.
        if self.current_flow.is_none() {
            let reg = crate::plugins::Registry::load_kept(&mut self.plugins_kept, &self.plugins_dir, &self.cfg.commands, &self.store).clone();
            let mut runs = crate::plugins::ScheduleRuns::load(&self.store);
            let before = runs.clone();
            if let Some((id, f, key)) = reg.due(&mut runs, t, local_offset_mins()).into_iter().next() {
                runs.ran(&key, t);
                let said = self.start_flow(f, Some(id), None, t);
                out.push(said);
            }
            if runs != before {
                let _ = runs.save(&self.store);
            }
        }

        // --- Scheduled posts and emails ---
        //
        // The authorization question, stated plainly: **approving a post at
        // schedule time IS the consent to send it at its time.** That is the
        // entire point of scheduling; requiring you to be present at 7am
        // would make the feature pointless.
        //
        // What that consent covers is narrow, and re-verified here: this exact
        // text, to this channel. Editing after approval voids it, the length
        // and media rules are checked again, and a failure never silently
        // retries forever — it lands on the outstanding list.
        // Never waited for on the loop (`status_now`). Until the first
        // answer is in (`Unknown`, a moment after start) nothing here is
        // decided: no post is sent, and the board isn't told the connection
        // failed.
        let reach = self.connectivity.status_now(t);
        let known = reach != Reach::Unknown;
        let online = reach == Reach::Online;
        // The probe already happened; this just writes down what it saw. Left
        // unrecorded, the board could never learn the one thing it is best
        // placed to notice — that an answer was produced with no network.
        if let Some(i) = self.connections.get_mut(crate::integrations::INTERNET).filter(|_| known) {
            if online {
                i.worked(t);
            } else {
                i.failed(t, "no route out");
            }
        }
        let due = if known { self.publisher.due(t, online) } else { Vec::new() };
        for id in due {
            match delivery::plan(&self.publisher, &self.browser_cfg(), id, online) {
                // Sent through Atlas's browser now (G2). The comment that
                // stood here recorded why it used to be held instead: nothing
                // could send, and a post left `Scheduled` came round every tick.
                // A post in flight is skipped until its errand comes back.
                Ok(_) => {
                    let _ = self.send_post(id, t, online);
                }
                Err(delivery::Outcome::Retry(_)) => {}
                Err(delivery::Outcome::Blocked(why)) => {
                    let what = self.publisher.get(id).map(|p| p.describe()).unwrap_or_default();
                    self.backlog.record(&format!("send {what}"), Blocker::Failed(why.clone()), t);
                    self.journal.record_at(Act::Blocked, &format!("{what}: {why}"), false, t);
                    out.push(format!("Couldn't send: {why}"));
                }
                Err(delivery::Outcome::Sent(m)) => out.push(m),
            }
        }
        // Problems you would want to know about before the send time arrives.
        // (`blocked` never counts "no connection", so it needs no answer.)
        for (id, why) in self.publisher.blocked(t, online) {
            if self.warned_posts.contains(&id) {
                continue;
            }
            self.warned_posts.push(id);
            out.push(format!("Heads up — a scheduled post can't go: {why}"));
        }

        // --- Run what's queued ---
        //
        // Background work goes immediately; anything needing the screen waits
        // for a gap. This is what makes "Atlas is busy" stop meaning "you are
        // waiting".
        let lanes = self.lane_cfg();
        for id in self.queue.ready_with(&signals, &lanes, t, self.connectivity.cached()) {
            let Some(task) = self.queue.tasks.iter().find(|x| x.id == id).cloned() else { continue };
            let intent = self.parser.parse(&task.command);
            let call = crate::policy::classify_with_policy(&intent, &self.memory, &self.cfg.policy);

            // Queued work never sneaks past the approval gate.
            if call.needs_consent() {
                self.queue.finish(id, "needs your go-ahead", false);
                self.backlog.record(&task.command, Blocker::NeedsApproval, t);
                continue;
            }
            let result = self.execute(&intent);
            let ok = !result.starts_with("error");
            self.queue.finish(id, &result, ok);
            self.journal.record_at(Act::Scheduled, &task.command, ok, t);
            if !ok || call == Decision::ProceedAndReport {
                out.push(result);
            }
        }

        // A workflow mid-run keeps moving between turns. One paused for
        // your yes stays paused — the ask already happened, and asking
        // every two seconds is the scheduled-post bug this file already
        // fixed once.
        if self
            .current_flow
            .as_ref()
            .is_some_and(|r| r.state == crate::flow::RunState::Running)
        {
            if let Some(line) = self.drive_flow(t) {
                out.push(line);
            }
        }

        // --- Things Atlas couldn't do earlier ---
        let conditions = Conditions {
            online,
            screen_free: signals.idle_secs > 20,
            you_are_here: !self.attention.is_paused(),
            tools: Vec::new(),
        };
        let backlog_cfg = crate::backlog::BacklogConfig::default();
        if let Some(item) = self.backlog.next_offer(&conditions, &backlog_cfg, t) {
            let q = crate::backlog::Backlog::phrase(&item);
            self.session.ask(&q);
            // Remember which item this yes/no is about, so the answer can run
            // it or, on a no, dismiss it -- otherwise "no" only clears the
            // question and the item is raised again next quiet tick.
            self.pending_backlog = Some(item.id);
            out.push(q);
        }

        // --- The machine itself ---
        //
        // A notice waits for a quiet moment; something urgent doesn't. Either
        // way one thing at a time, and not again for a week.
        let hcfg = self.health_cfg();
        let findings = assess_machine(&self.readings(), &hcfg);
        self.health.reconcile(&findings);
        let quiet = signals.idle_secs > 60 && !self.session.is_waiting();
        if let Some(f) = self.health.next(&findings, quiet, &hcfg, t) {
            if self.modes.may_interrupt(f.severity == crate::health::Severity::Urgent) {
                // Routed rather than only spoken. A disk warning is the
                // clearest case for this: it matters most exactly when you are
                // not at the desk to hear it.
                let urgency = match f.severity {
                    crate::health::Severity::Urgent => crate::notify::Urgency::Urgent,
                    _ => crate::notify::Urgency::Routine,
                };
                let note = crate::notify::Note::new("Atlas — your machine", &f.say, urgency, t);
                let sent = self.reach_you(note, t);
                if sent.reached_you() {
                    out.push(f.say.clone());
                }
                // Recorded as offered only when it actually went somewhere a
                // person could see. Journalling a held note as delivered is
                // how "I told you" becomes untrue.
                self.journal.record_at(Act::Offered, &f.say, sent.reached_you(), t);
                if let crate::notify::Sent::Failed(why) = &sent {
                    self.log.warn(&format!("could not notify: {why}"));
                }
            }
        }

        // --- Other machines, watched from the outside ---
        let names: Vec<String> = self.watcher.targets.iter().map(|x| x.name.clone()).collect();
        for name in names {
            if !self.watcher.due(&name, t) {
                continue;
            }
            let addr = self
                .watcher
                .targets
                .iter()
                .find(|x| x.name == name)
                .map(|x| x.address.clone())
                .unwrap_or_default();
            let up = crate::watch::reachable(&addr, 1500);
            match self.watcher.observe(&name, up, t) {
                crate::watch::Alert::None => {}
                crate::watch::Alert::WentDown { say, .. }
                | crate::watch::Alert::CameBack { say, .. }
                | crate::watch::Alert::Flapping { say, .. } => {
                    self.journal.record_at(Act::Offered, &say, true, t);
                    out.push(say);
                }
            }
        }

        // --- Re-plan when the machine changes (`fit.replan_on_change`) ---
        //
        // Hourly. The plan made on the day Chrome had forty tabs open is not
        // the one to keep forever; `fit::worth_replanning` is what decides.
        let fit_cfg = self.tools_cfg().fit.clone();
        if fit_cfg.replan_on_change && t.saturating_sub(self.fit_measured.1) >= 3600 {
            let now_m = crate::fit::measure();
            if crate::fit::worth_replanning(&self.fit_measured.0, &now_m) {
                let before = self.fit.tier;
                self.fit = crate::fit::plan_as_set(&now_m, &fit_cfg);
                self.log.info(&format!("re-planned for this machine: {} -> {}", before.name(), self.fit.tier.name()));
                self.fit_measured.0 = now_m;
            }
            self.fit_measured.1 = t;
        }

        // --- A new Atlas dropped into updates/ ---
        //
        // Checked a few times a day, said once per version: it goes in the
        // next time Atlas starts (`upgrade::swap_checked`, in main).
        if t.saturating_sub(self.last_update_check.0) >= 6 * 3600 {
            self.last_update_check.0 = t;
            match crate::upgrade::waiting(&self.store.install_root()) {
                Some(Ok((_, v))) if self.last_update_check.1 != v => {
                    let say = format!("Atlas {v} is waiting in the updates folder. It goes in the next time I start; the one running now is kept so you can go back.");
                    self.log.info(&say);
                    out.push(say);
                    self.last_update_check.1 = v;
                }
                Some(Err(why)) if self.last_update_check.1 != why => {
                    self.log.warn(&format!("updates folder: {why}"));
                    out.push(format!("There's something in the updates folder I won't use: {why}."));
                    self.last_update_check.1 = why;
                }
                _ => {}
            }
        }

        // --- The waking panel leaves by itself ---
        if let Some(p) = self.wants_panel {
            if crate::panel::faded(p, self.panel_shown_at, t, &self.panel_cfg()) {
                self.wants_panel = None;
            }
        }

        // --- Is the sync folder still syncing? ---
        //
        // Every `cloud.check_every_hours`. It was checked once, at setup; a
        // client that signed out in March looked fine until someone noticed
        // their phone hadn't seen anything since. Said once per finding, and
        // `Trouble::fix` says what to do about it.
        let sync_cfg = self.tools_cfg().sync.clone();
        let cloud_cfg = self.tools_cfg().cloud.clone();
        let every = cloud_cfg.check_every_hours.max(1) as u64 * 3600;
        if sync_cfg.enabled && !sync_cfg.folder.trim().is_empty() && t.saturating_sub(self.last_sync_check) >= every {
            self.last_sync_check = t;
            let mine = format!("{}.bundle", sanitise(&self.synclog.device));
            let others = !self.seen_up_to.is_empty();
            if let Some((trouble, what)) = crate::cloudsync::still_syncing(std::path::Path::new(sync_cfg.folder.trim()), &mine, others, t, &cloud_cfg) {
                let fix = trouble.fix();
                let say = format!("Your sync folder: {what}. {}{}.", fix[..1].to_uppercase(), &fix[1..]);
                self.log.warn(&say);
                out.push(say);
            }
        }

        // Shared-page edits made from the command line (`atlas doc`)
        // wait in an inbox; this is where they join the sync log, which
        // has one writer — here. The files go only once the log holding
        // them is on disk.
        let inbox = self.store.data_dir().join("doc-inbox");
        let taken = crate::yata::take_queued(&inbox, &mut self.synclog, t);
        if !taken.is_empty() && self.store.save("synclog", &Some(self.synclog.clone())).is_ok() {
            for f in taken {
                let _ = std::fs::remove_file(f);
            }
        }

        // --- Your standing watches ---
        //
        // Readings the tick already takes, offered as named things a rule can
        // watch. The rules come from settings and are re-read each tick, so
        // editing one takes effect without a restart; what they remember
        // (pending "for" timers, last firing) is kept apart and survives one.
        let specs = self.tools_cfg().automations.clone();
        if !specs.is_empty() {
            let rules: Vec<crate::automation::Automation> =
                specs.iter().filter_map(|s| crate::automation::Automation::from_spec(s).ok()).collect();
            self.automations.rules = rules;
            let r = self.readings();
            let mut seen: Vec<(String, String)> = vec![
                ("machine.disk_free_gb".into(), format!("{:.1}", r.disk_free_gb)),
                ("machine.ram_used_gb".into(), format!("{:.1}", r.ram_used_gb)),
            ];
            if r.disk_total_gb > 0.0 {
                seen.push(("machine.disk_used_pct".into(), format!("{:.1}", 100.0 * (1.0 - r.disk_free_gb / r.disk_total_gb))));
            }
            if r.ram_total_gb > 0.0 {
                seen.push(("machine.ram_used_pct".into(), format!("{:.1}", 100.0 * r.ram_used_gb / r.ram_total_gb)));
            }
            if let Some(b) = r.battery_percent {
                seen.push(("machine.battery_pct".into(), b.to_string()));
            }
            if let Some(d) = r.days_since_backup {
                seen.push(("machine.days_since_backup".into(), d.to_string()));
            }
            for (name, st) in &self.watcher.status {
                let word = match st.health {
                    crate::watch::Health::Up => "up",
                    crate::watch::Health::Down => "down",
                    crate::watch::Health::Flapping => "flapping",
                    crate::watch::Health::Unknown => "unknown",
                };
                seen.push((format!("watch.{name}"), word.to_string()));
            }
            // "at 7" in a watch means 7 on your clock, and the offset moves
            // twice a year, so it is taken fresh each tick.
            self.automations.utc_offset = self.home_zone().offset_at(t as i64);
            let before = (self.automations.mem.pending.clone(), self.automations.mem.latched.clone());
            let mut fired = Vec::new();
            for (entity, value) in &seen {
                fired.extend(self.automations.observe(entity, value, t as i64));
            }
            fired.extend(self.automations.tick(t as i64));
            for f in &fired {
                let say = format!("{} ({})", f.action, f.because);
                self.journal.record_at(Act::Offered, &say, true, t);
                out.push(say);
            }
            // Saved when something that must survive a restart moved — a
            // timer started or cleared, a watch fired — not every tick.
            let after = (&self.automations.mem.pending, &self.automations.mem.latched);
            if !fired.is_empty() || (&before.0, &before.1) != after {
                let _ = self.store.save("automations", &self.automations.mem);
            }
        }

        // --- Work prepared before you ask for it ---
        let moment = Moment {
            new_files: signals.recent_changes.added.clone(),
            returned: false,
            idle_secs: signals.idle_secs,
            last_said: self.thread.last().map(|e| e.said.clone()).unwrap_or_default(),
            // Never prepare anything while you're mid-task; anticipation that
            // makes the laptop stutter is worse than none.
            can_afford_work: signals.idle_secs > 30 && !signals.in_conversation,
            ..Default::default()
        };
        for rule in self.anticipator.due(&moment, t) {
            let lane = lane_for(&rule.command);
            // A task that cannot run without a connection says so at the
            // moment it is queued. `push_online` and the offline hold in
            // `ready_with` existed for this and were unreachable: every
            // task ever queued had `needs_net: false`, so the connectivity
            // argument the tick passes in could never change the answer.
            if command_needs_connection(&rule.command) {
                self.queue.push_online(&rule.command, lane);
            } else {
                self.queue.push(&rule.command, lane);
            }
            if rule.announce && self.modes.may_interrupt(false) {
                out.push(crate::anticipate::ready_line(&rule));
            }
            self.journal.record_at(Act::Offered, &rule.name, true, t);
        }

        // --- Round 11's tools: key chords, clipboard history, feeds read in
        // the background, meeting prep and the trading check-in. Each costs
        // nothing unless it's switched on and has something to do.
        let may_speak = self.proactive.may_interrupt(&signals, t);
        let paused = self.attention.is_paused();
        let online = self.connectivity.cached() == Reach::Online;
        out.extend(self.workday_tick(t, may_speak, paused, online));

        // --- Calendar reminders coming due ---
        // An event with a reminder lead-time gets spoken once as its start
        // comes within that lead. Repeating events expand, so a daily standup
        // reminds each day; the `reminded` set (id, occurrence start) keeps any
        // one occurrence from firing twice.
        self.reminded.retain(|(_, start)| *start > t);
        for occ in self.calendar.due_reminders(t) {
            let key = (occ.id, occ.start);
            if self.reminded.contains(&key) {
                continue;
            }
            let mins_away = occ.start.saturating_sub(t) / 60;
            let when = if mins_away >= 60 {
                format!("in {} hour{}", mins_away / 60, if mins_away / 60 == 1 { "" } else { "s" })
            } else if mins_away <= 1 {
                "in a moment".to_string()
            } else {
                format!("in {mins_away} minutes")
            };
            out.push(format!("Reminder: \"{}\" {when}.", occ.title));
            self.reminded.insert(key);
            self.journal.record_at(Act::Offered, &format!("reminder: {}", occ.title), true, t);
        }

        // Anything hand tracking has said since the last tick. It runs on its
        // own thread precisely so the pointer does not wait for this — only
        // the reporting does.
        for said in self.hands.as_ref().map(|h| h.heard()).unwrap_or_default() {
            match said {
                crate::handloop::Said::Did(what) => out.push(what),
                crate::handloop::Said::Trouble(why) => out.push(why),
                crate::handloop::Said::HandsGone => {
                    self.steering_until = None;
                    self.carrying = None;
                }
            }
        }

        // Look at the room. Only a gesture answering a pending question ever
        // reaches you from this; presence just informs everything else.
        if let Some(said) = self.look_at_the_room(t) {
            out.push(said);
        }

        // Anything you handed over from another device. Read one per tick;
        // said only when Atlas may speak at all, so a tray filling up while
        // you are in a meeting does not become a queue of interruptions.
        if let Some(said) = self.read_one_handed_thing() {
            if self.proactive.may_interrupt(&signals, t) {
                out.push(said);
            }
        }

        // --- Backup, on its own schedule ---
        //
        // Copying everything in the store can genuinely take a while on a
        // real vault, and this fires unprompted on a timer — nobody is
        // waiting on it the way they are for a spoken command, which makes
        // it the worst possible thing to block the tick. Handed to the
        // crew; `take_crew_news` reports it when it actually finishes.
        let bcfg = self.backup_cfg();
        if due_for_backup(&bcfg, t) && t.saturating_sub(self.last_backup) > 3600 {
            let root = self.store.root().to_path_buf();
            let cfg_for_errand = bcfg.clone();
            let work: crew::Work = Box::new(move |_stop| match back_up(&root, &cfg_for_errand, crate::store::now()) {
                Ok(b) => {
                    prune_backups(&cfg_for_errand);
                    Ok(format!("backed up {} files", b.files.unwrap_or(0)))
                }
                Err(e) => Err(e.to_string()),
            });
            // Only mark the schedule satisfied if the crew actually took
            // it. The bounded waiting list refusing is vanishingly
            // unlikely for one periodic job, but if it happens, leaving
            // `last_backup` alone means the next tick simply tries again
            // rather than the backup silently never running.
            if self.hand_off("backup", t, work, None, SpeakPolicy::ViaWatcher) {
                self.last_backup = t;
            }
        }

        // --- Housekeeping: hourly, cheap, never in your way ---
        // The night's work.
        //
        // `overnight.rs` is a complete state machine -- the window, the
        // give-up-after-N-failures rule, the per-night problem cap, the
        // budget check, the morning brief -- and until now nothing called any
        // of it. The settings were in `tools.yaml` and read by nothing, which
        // is the config-that-lies shape this tree keeps finding.
        //
        // What runs here is the `ask_you_later` brain, which is the shipped
        // default and the only one that is free and needs no network: Atlas
        // works through what it could not finish, writes each one up, and
        // leaves the brief for the morning. The `local`, `hosted` and
        // `delegate` brains drive a solver, cost money or drive another
        // application, and are deliberately still unwired -- that is a
        // decision, not an oversight, and it is named in OUTSTANDING_TASKS.
        //
        // Nothing is ever applied while you are asleep. That is not a setting
        // here: `OvernightConfig::apply_while_asleep` is `#[serde(skip)]` and
        // hard-wired false, and `the_night_never_applies_anything` holds it.
        self.run_the_night(t);

        // --- How you're working, when it's worth a word ---
        //
        // `Noticed` had four variants, each with a sentence and a rule about
        // repeating, and nothing in the tree ever built one. Three settings
        // were thresholds on it — `notice_patterns`,
        // `late_nights_before_saying`, `hours_before_saying` — and a fourth,
        // `quiet_days`, decided how often the same remark could be made. All
        // four were thresholds on something nobody produced.
        //
        // An observation and an offer, never a diagnosis.
        {
            let pcfg = self.tools_cfg().person.clone();
            let hour = crate::localclock::hour_here(t) as u32;
            // A quarter of an hour away is a break; a pause to read something
            // is not. Counting a pause would mean the stretch reset all day
            // and the threshold was never reached.
            const A_BREAK: u64 = 900;
            if signals.idle_secs >= A_BREAK {
                self.working_since = None;
            } else if self.working_since.is_none() {
                self.working_since = Some(t);
                // Recorded on the hour you actually started, which is what
                // decides whether tonight counts as a late one.
                self.person.working_now(hour, t, &self.tools_cfg().judgment);
            }
            let hours_straight = self
                .working_since
                .map(|from| (t.saturating_sub(from) / 3600) as u32)
                .unwrap_or(0);

            if self.proactive.may_interrupt(&signals, t) {
                if let Some(seen) = crate::person::noticing(&self.person, &pcfg, hours_straight, t)
                {
                    let mut said: crate::person::Said =
                        self.store.load(crate::person::SAID_RECORD);
                    if said.may_say(&seen, &pcfg, t) {
                        said.record(&seen, t);
                        if self.store.save(crate::person::SAID_RECORD, &said).is_ok() {
                            out.push(seen.spoken());
                        }
                    }
                }
            }
        }

        // --- Going somewhere your texts won't reach you ---
        //
        // `going_away.remind_days_before` is "remind you this many days
        // before a trip you've told it about" and there was no way to tell it
        // about a trip; `periodic_nudge` takes `days_since_last` and nothing
        // kept a last; `codes.check_days_before` was a threshold on the same
        // missing date. One record answers all three.
        //
        // Both halves are said, because either alone leaves a wrong
        // impression: "every account is reachable" is no comfort with no
        // codes printed, and "ten codes in hand" is no comfort for the
        // account that has none.
        if self.proactive.may_interrupt(&signals, t) {
            let acfg = self.tools_cfg().going_away.clone();
            let ccfg = self.tools_cfg().codes.clone();
            let mut away: crate::goingaway::Away =
                self.store.load(crate::goingaway::AWAY_RECORD);

            // At most once a day, both halves. A trip reminder that fires
            // every tick for a fortnight is one you stop reading, and then
            // stop reading on the fortnight it mattered.
            if away.days_since_asked(t) >= 1 {
                let mut said: Vec<String> = Vec::new();
                if let Some(line) = crate::goingaway::the_trip_is_close(
                    &away,
                    &self.accounts.accounts,
                    &acfg,
                    t,
                ) {
                    said.push(line);
                }
                if crate::codes::worth_raising_now(&ccfg, away.days_until(t)) {
                    let names: Vec<String> =
                        self.accounts.accounts.iter().map(|a| a.site.clone()).collect();
                    said.push(crate::codes::before_you_go(&self.code_sets, &names, &ccfg));
                }
                // No trip, and long enough since anyone asked. Dates change
                // and the preparation takes days, so this is the check that
                // catches a deployment you have not written down yet.
                if said.is_empty() {
                    if let Some(line) = crate::goingaway::periodic_nudge(
                        &self.accounts.accounts,
                        away.days_since_asked(t),
                        &acfg,
                    ) {
                        said.push(line);
                    }
                }
                if !said.is_empty() {
                    // Marked as asked before it is said, so a failure to
                    // write it down is a reminder you do not get rather than
                    // one you get every tick.
                    away.last_checked = t;
                    if self.store.save(crate::goingaway::AWAY_RECORD, &away).is_ok() {
                        out.push(said.join(" "));
                    }
                }
            }
        }

        // --- The envelope, checked on the interval you set ---
        //
        // `after_me.review_every_days` is the only part of that arrangement
        // that has to run on a clock — everything else is decided once, and
        // the failure mode is a plan that was true three years ago and names
        // somebody you have since fallen out with. Nothing read it, and the
        // whole `after_me:` section was in `config::PARSED_AND_NEVER_READ`.
        //
        // Only ever a nudge to go and look. Nothing about this arrangement is
        // acted on by Atlas, and it never holds the passphrase.
        if self.proactive.may_interrupt(&signals, t) {
            let acfg = self.tools_cfg().after_me.clone();
            let arrangement: crate::afterme::Arrangement =
                self.store.load(crate::afterme::RECORD);
            if let Some(said) = arrangement.nudge(&acfg, t) {
                // Marked as reviewed-asked rather than reviewed: saying it
                // once a year is the point, saying it every tick for a year
                // is how it gets ignored.
                let mut asked = arrangement.clone();
                asked.reviewed_at = t;
                if self.store.save(crate::afterme::RECORD, &asked).is_ok() {
                    out.push(said);
                }
            }
        }

        // --- Looking at itself, on the cadence you set ---
        //
        // `self_audit.every_days` and `self_audit.act_without_asking` were
        // both changeable and both inert: `recommend` was only ever reached
        // from `Intent::WorkOnYourself`, which runs because you asked. So
        // "how often to look" described a looking nothing did, and "act
        // without asking" described an asking that was the only way in.
        //
        // Gated on `may_interrupt` before the look rather than after it. The
        // clock is only marked when it actually looked, so a week spent in
        // meetings delays the check rather than silently spending it.
        if self.proactive.may_interrupt(&signals, t) {
            let acfg = self.tools_cfg().self_audit.clone();
            let last: crate::selfaudit::LastLook =
                self.store.load(crate::selfaudit::LOOK_RECORD);
            if crate::selfaudit::time_to_look(&acfg, last.at, t) {
                let _ = self.store.save(
                    crate::selfaudit::LOOK_RECORD,
                    &crate::selfaudit::LastLook { at: t },
                );
                self.refresh_signals();
                let recs = crate::selfaudit::recommend(&self.signals, acfg.most_at_once);
                // The hollow check, on the pass that happens without you
                // asking. It used to run only when you asked -- so the one
                // sweep nobody triggers was the one that skipped the
                // bug-detector, which is the shape `hollow.rs` was written
                // about, one level up.
                //
                // Raised on a judgment rather than a count. Six findings that
                // all say "not wired up" are a backlog; one that says
                // "claimed fine while every number was zero" is worth
                // tonight. `worth_raising` weighs both, including the
                // evidence *against* interrupting, and stays quiet when it
                // cannot tell -- which is the right way round for something
                // that speaks while you did not ask.
                let hollow_found = self.hollow_answers();
                let raise = crate::judgment::worth_raising(&hollow_found, &self.tools_cfg().judgment);
                if raise.settled() == Some(true) {
                    out.push(crate::hollow::spoken(&hollow_found, &self.tools_cfg().judgment));
                }
                if let Some(u) = crate::selfaudit::unprompted(&acfg, &recs) {
                    out.push(u.said);
                    // Only when told not to ask. `work_on_myself` opens the
                    // session and refuses on its own if the pipeline is off,
                    // which is the right place for that answer to live.
                    if let Some(goal) = u.goal {
                        // Its own diagnosis goes straight in (E2): the four
                        // answers are already known, so the work starts at
                        // proving the test fails. Landing still waits for
                        // your OK (`selfgrant`).
                        if let Some(thought) = u.thought {
                            let mut session = crate::selfwork::Session::new(&goal, 0);
                            for answer in [&thought.symptom, &thought.cause, &thought.where_, &thought.proof] {
                                let _ = session.diagnosing.answer(answer);
                            }
                            self.selfwork = Some(session);
                        }
                        let opened = self.work_on_myself(&goal);
                        out.push(opened);
                    }
                }
            }
        }

        if t.saturating_sub(self.last_tidy) >= 3600 {
            self.last_tidy = t;
            // Atlas's own things, fixed without asking (E1).
            let _ = self.fix_my_own_things(t);
            // Routines: asked about once, and run when due (E4).
            if self.proactive.may_interrupt(&signals, t) {
                out.extend(self.routines_on_the_hour(t));
                // A change to itself waiting on your OK (F8).
                if let Some(line) = self.remind_about_staged_change(t) {
                    out.push(line);
                }
            }
            // The hours you were active, kept across a restart.
            let _ = self.store.save("rhythm", &self.rhythm);
            // Changes waiting on you, checked against the code as it is now.
            // One hash per file it writes; said once when a change goes out
            // of date, not every hour after.
            let newly = self.workshop.mark_outdated();
            if !newly.is_empty() {
                let _ = self.workshop.save(&self.store);
                for (project, title, files) in newly {
                    out.push(format!(
                        "The \"{title}\" change for {project} is out of date — {} changed since I wrote it. \
                         Ask me to redo it against the current code before implementing it.",
                        files.join(", ")
                    ));
                }
            }
            // Your `retention.approvals_detailed`, not a literal that happened
            // to equal it. Hoisted rather than inlined because `tools_cfg`
            // borrows `&self` and `compact_approvals` needs `&mut`.
            let keep_detailed = self.tools_cfg().retention.approvals_detailed;
            self.memory.compact_approvals(keep_detailed);
            // The producer selfaudit::Kind::GotSlower never had. A signal
            // nothing can raise is a signal that will never fire.
            if let Some(sig) = self.timing.got_slower() {
                self.signals.push(sig);
            }
            // Anything Atlas worked out for itself and has not seen hold up
            // since is a guess that has been sitting long enough to look like
            // a fact. Surfaced, not deleted — you decide.
            for f in self.facts.guesses_worth_checking(t) {
                self.log.info(&format!(
                    "worth re-checking: {} ({})",
                    f.summary,
                    f.kind.plain()
                ));
            }
            self.backlog.tidy(&backlog_cfg, t);
            self.scheduler.prune();
            // A trash record that cannot be read stops `expire` and `take`
            // dead — both refuse rather than guess an id, which is right, and
            // both used to do it in silence. `LedgerState::trouble()` exists
            // to say which of the two it is and had no caller, so the trash
            // quietly stopped accepting anything and quietly stopped
            // expiring, for as long as the file stayed bad.
            if let Some(why) = self.trash.read_ledger().trouble() {
                self.log.info(&format!(
                    "{why} — nothing new can go to the trash and nothing in it is \
                     expiring until that file is readable again"
                ));
            }
            self.trash.expire(t);
            let _ = self.queue.save(&self.store);
            self.queue.prune();
            // Walking the whole data/ tree and deciding what to reclaim is
            // real filesystem I/O — on a machine that's accumulated a lot
            // of logs and backups, "hourly, cheap, never in your way" is a
            // promise the walk itself can break if it runs on the tick.
            // Self-contained (owns a path, a config, and nothing of the
            // daemon's), so it goes to the crew like the backup does.
            //
            // Resolved against the install's own root now, via
            // `Store::install_root` — retention means the whole `data/`
            // tree (state, backups, notes, logs), which is a sibling of
            // `store.root()` (`data/state`), not something nested under
            // it. Named as an open question in an earlier pass; the
            // concept it was waiting on now exists.
            let root_for_errand = self.store.install_root().join("data");
            // Your budget, not the built-in one.
            //
            // This built `RetentionConfig::default()` inside the errand while
            // the `retention:` section of your tools.yaml sat unread -- so a
            // person who set a 2 GB budget still got the shipped 500 MB, and
            // the pass that *deletes files to stay in budget* was the one
            // ignoring the number. `config::PARSED_AND_NEVER_READ` named it;
            // nothing acted on it.
            let retention_cfg = self.tools_cfg().retention.clone();
            let work: crew::Work = Box::new(move |_stop| {
                let items = crate::retention::survey(&root_for_errand);
                let plans = crate::retention::plan(&items, &retention_cfg, crate::store::now());
                let mut lines = Vec::new();
                // Anything the plan named outside data/ is a bug upstream.
                // Say so rather than quietly skipping it.
                for stray in crate::retention::out_of_bounds(&plans, &root_for_errand) {
                    lines.push(format!("refused to delete outside data/: {}", stray.display()));
                }
                let freed = crate::retention::apply(&plans, &root_for_errand);
                if freed > 0 {
                    lines.push(format!("reclaimed {} MB", freed / (1024 * 1024)));
                }
                // `retention::irreducible` had no caller: the case where notes
                // and learned state alone (never evicted for space, per
                // `plan`'s own rule) already exceed the budget, so no amount
                // of deletion here will fix it. That is a configuration
                // problem, not a cleanup problem, and it was previously
                // invisible — `apply` just quietly freed whatever it could
                // and stopped.
                let usage = crate::retention::usage(&items);
                if crate::retention::irreducible(&usage, &retention_cfg) {
                    lines.push(format!(
                        "notes and learned state alone are {} MB, over the {} MB budget — \
                         deleting scratch/logs/captures won't fix this",
                        (usage.notes + usage.state) / (1024 * 1024),
                        retention_cfg.total_budget_mb
                    ));
                }
                Ok(lines.join("; "))
            });
            self.hand_off("housekeeping", t, work, None, SpeakPolicy::ViaWatcher);
            // `daily::still_keep` had no caller at all: closed days were
            // computed fresh each rollover and handed straight to the brief
            // opener, with nothing archived to ever prune. Now that
            // `daily_history` (above, in `Intent::Outstanding`) actually
            // keeps them, this is the other half -- a real retention pass
            // over what accumulated, same hourly cadence as the rest of
            // this block.
            let daily_cfg = self.tools_cfg().daily.clone();
            let before = self.daily_history.len();
            self.daily_history.retain(|c| crate::daily::still_keep(c, t, &daily_cfg));
            if self.daily_history.len() != before {
                let _ = self.store.save("daily_history", &self.daily_history);
            }
        }
        // Reaping idle helpers moved to the top of `tick`, above the pause
        // check, for the reason given there: it is housekeeping, and pausing
        // Atlas is exactly when you want the memory back.
        // Anything the crew finished, vanished on, or still won't stop for.
        out.extend(self.take_crew_news(t));

        // --- The day's run, when you come in rather than when a clock says ---
        //
        // For one afternoon this fired at `brief.at_hour`, because that was
        // the setting sitting unread and wiring it was the obvious fix. It
        // was the wrong fix, and the reason is worth keeping: **a brief is
        // worth having when you start, and you do not start at the same time
        // every day.** Seven o'clock greets a night session at its fourth
        // hour and misses the morning that began at ten.
        //
        // So arrival decides. `daily::arriving` asks two things -- has there
        // been a real gap since your last turn, and has your day turned since
        // the last brief -- and the day it measures against is the one
        // `Rhythm` worked out from when you actually stop, not the calendar's.
        // A night owl's day ends at four in the morning and nobody had to say
        // so.
        //
        // The case this exists for: you work eleven until six, through the
        // four o'clock rollover. At 4am there is no gap, so this is
        // `StillGoing` and nothing is said. You sleep and come back at two --
        // a gap, and a day that turned -- so the brief arrives then. Held,
        // not missed, which is the rule `morning_brief` already keeps for the
        // night's work.
        //
        // `not_before_hour` is the only thing the clock still decides, and it
        // decides one thing: not before then. Asking works at any hour.
        {
            let bcfg = self.tools_cfg().brief.clone();
            let dcfg = self.tools_cfg().daily.clone();
            let rolls_at = self.rhythm.rolls_at(&dcfg);
            let this_hour = crate::localclock::hour_here(t) as u8;
            let arrival =
                crate::daily::arriving(self.last_turn_of_yours, self.last_brief_at, t, rolls_at, &dcfg);

            if bcfg.enabled
                && arrival == crate::daily::Arrival::Starting
                && this_hour >= bcfg.not_before_hour
            {
                self.last_brief_at = t;
                let _ = self.store.save("last_brief_at", &self.last_brief_at);
                let b = self.brief_now(t);
                // `b.is_empty()` rather than a check on the sentence: an
                // empty brief still *speaks* -- "Nothing needs you. I'll get
                // on with the rest." -- which is the right answer to someone
                // who just asked and the wrong thing to volunteer every
                // morning for the rest of your life.
                if !b.is_empty() {
                    let line = crate::brief::spoken(&b);
                    match self.morning_brief.take() {
                        // The night finished and this is the same morning.
                        // Both, in the order they happened, rather than one
                        // silently overwriting the other -- which is what a
                        // plain assignment here would have done, on exactly
                        // the mornings there was most to say.
                        Some(night) => self.morning_brief = Some(format!("{night} {line}")),
                        None => self.morning_brief = Some(line),
                    }
                }
            }
        }

        // The morning brief, once, the first time Atlas gets to speak after a
        // night. Taken rather than read: a brief said twice is worse than one
        // said late, and the night it describes is already over.
        if let Some(brief) = self.morning_brief.take() {
            out.push(brief);
        }

        // --- Has the index stopped matching the folder? ---
        //
        // Measured hourly, above the speak gate, because noticing and saying
        // are two different things: a drift found while Atlas is not allowed
        // to interrupt still belongs in the log, and a check that only ran on
        // the ticks Atlas may speak would go unrun for a whole focus session.
        // What it finds is held for the raise at the end of the tick.
        if t.saturating_sub(self.last_index_check) >= 3600 {
            self.last_index_check = t;
            let d = self.index_drift();
            if d.is_clean() {
                self.index_drifted = None;
            } else {
                self.log.info(&format!("notes index has drifted: {}", d.plain()));
                self.index_drifted = Some(d);
            }
        }

        // Should Atlas speak first?
        if !self.modes.may_interrupt(false) {
            self.persist_after(t, !out.is_empty());
            return out;
        }
        // Initiative before reaction. A stalled commitment matters more than
        // a folder that filled up, and if both are true you should only hear
        // one of them.
        let hour = crate::localclock::hour_here(t) as u8;
        // Record that the connections were looked at. Without this the
        // board reports its last answer in the present tense.
        // The heartbeat used to be here, and being here was the bug: two
        // `return out`s above it meant a paused or focused Atlas stopped
        // beating while it was still running. It is now the first thing
        // `tick` does. See the note at the top of this function.
        // Once per run, so the notification path knows whether headphones are
        // connected without launching anything itself.
        self.refresh_audio_once();
        self.connections.swept(t);
        // A connection that has genuinely started failing is worth saying out
        // loud once, not only writing to a log nobody opens. `link_broke`
        // existed for exactly this and was called by nothing — which was
        // academic while the board was empty, and is not any more.
        //
        // `Failing` only, deliberately: `Unknown` is the state every
        // dependency starts in, so nudging on it would greet every fresh
        // start with a complaint about connections that are probably fine.
        // Two failures, not one — one failure does not announce that your
        // bank is down. `health()` reports Failing on a single failure, which
        // is right for a status page and wrong for an interruption, so the
        // threshold lives here where the interrupting happens rather than by
        // changing a `health()` other callers already depend on.
        let broken: Vec<crate::nudge::Nudge> = self
            .connections
            .needs_you(t)
            .into_iter()
            .filter(|i| {
                i.health(t) == crate::integrations::Health::Failing && i.failures_running >= 2
            })
            .map(crate::nudge::link_broke)
            .collect();
        // Written when it changes, not on every tick: on Eric's laptop the
        // same "never seen it work" line went to the log four times a
        // second (26 Sep 2026), which buries everything else in it.
        let lines: Vec<String> = self.connections.needs_you(t).iter().map(|i| i.line(t)).collect();
        for l in lines.iter().filter(|l| !self.connections_logged.contains(l)) {
            self.log.info(l);
        }
        self.connections_logged = lines;
        // At most one, and only when Atlas is allowed to speak at all. A
        // machine that has just gone offline has several things fail at once,
        // and hearing about each of them separately is noise. Behind the same
        // interrupt gate as every other nudge — without it this spoke in
        // unattended mode, where the whole contract is that Atlas does not
        // start conversations.
        if let Some(n) = broken
            .into_iter()
            .next()
            .filter(|_| self.proactive.may_interrupt(&signals, t))
        {
            let subject = n.subject.clone().unwrap_or_default();
            if self.nudger.may_raise(&subject, t) {
                self.nudger.raised(&subject, &n.message, t);
                let offer = crate::proactive::from_nudge(&n);
                self.session.ask(&offer.message);
                out.push(offer.message.clone());
                self.pending_offer = Some(offer);
            }
        }
        // Once there is enough history to judge by, worth_saying() names a
        // real problem -- reports that never cover what you ask -- rather
        // than a routine status update, so it goes to the log rather than
        // interrupting you as a nudge -- once each time it changes, not on
        // every tick.
        if let Some(m) = self.tier_mix.worth_saying() {
            if !self.connections_logged.contains(&m) {
                self.log.info(&m);
            }
            self.connections_logged.push(m);
        }
        let nudge = if self.proactive.may_interrupt(&signals, t) {
            self.nudger.consider(t, hour, signals.dwell_secs, signals.idle_secs)
        } else {
            None
        };
        // A daypart greeting on its own says "there is a lot on", which is a
        // notification. With the run attached it says what to start with,
        // which is the difference between being told you are busy and being
        // helped -- `daypart_with_brief`'s own doc, and it had no caller
        // because there was no brief worth attaching.
        //
        // Only when the brief has something in it. A greeting that says
        // "nothing needs you" every morning is the count-shaped notification
        // this module is written against, and `Brief::is_empty` is the check
        // that already knows the difference.
        let nudge = match nudge {
            Some(n) if n.trigger == crate::nudge::Trigger::Daypart => {
                let b = self.brief_now(t);
                match (crate::nudge::Part::from_hour(hour), b.is_empty()) {
                    (Some(part), false) => Some(crate::nudge::daypart_with_brief(part, &b)),
                    _ => Some(n),
                }
            }
            other => other,
        };
        // A nudge and an offer on the same tick are weighed together
        // (`bandit`, by how you've answered each kind) instead of the nudge
        // always taking the floor; either way only one question is asked.
        let from_nudge = nudge.as_ref().map(crate::proactive::from_nudge);
        let nudge_kind = from_nudge.as_ref().map(|o| o.kind.clone());
        let chosen = self.proactive.consider_with(&signals, &self.memory, t, from_nudge);
        // The nudge lost (or nothing was said at all): the nudger must not
        // go on believing it spoke, or its one-shot question and its
        // morning greeting are spent on something you never heard.
        if let Some(n) = &nudge {
            if chosen.as_ref().map(|o| Some(&o.kind)) != Some(nudge_kind.as_ref()) {
                self.nudger.unsaid(n);
            }
        }
        if let Some(offer) = chosen {
            self.session.ask(&offer.message);
            out.push(offer.message.clone());
            self.pending_offer = Some(offer);
        }

        // --- The index, if nothing else took the floor ---
        //
        // Raised last and only when nothing else is already on the table. A
        // stale index is a real fault — Atlas trusts it and stops looking —
        // but it is never the most urgent thing in the room, and two
        // questions asked in one tick means one of them gets the answer meant
        // for the other. Through `may_raise`/`raised` like every other nudge,
        // so a "no" sticks and silence widens the gap.
        //
        // The drift itself was measured before the speak gate above, so it is
        // found and logged even on the ticks where Atlas may not say anything.
        if let Some(d) = self.index_drifted.clone() {
            if let Some(n) = crate::nudge::drifted(&d) {
                let subject = n.subject.clone().unwrap_or_default();
                if self.pending_offer.is_none()
                    && self.proactive.may_interrupt(&signals, t)
                    && self.nudger.may_raise(&subject, t)
                {
                    self.index_drifted = None;
                    self.nudger.raised(&subject, &n.message, t);
                    let offer = crate::proactive::from_nudge(&n);
                    self.session.ask(&offer.message);
                    out.push(offer.message.clone());
                    self.pending_offer = Some(offer);
                }
            }
        }

        self.persist_after(t, !out.is_empty());
        out
    }

    /// The end-of-tick save: now when the tick had something to say, and
    /// otherwise on a minute's sweep.
    ///
    /// `persist` saved every state file at the end of every tick — up to
    /// every two seconds, forever, whether or not a byte had changed, and
    /// one of them is the file index. `Store::save` now skips identical
    /// bytes, but *serialising* everything thirty times a minute to discover
    /// nothing happened is about one percent of a core, permanently, and
    /// that is the entire idle budget. What this risks is up to a minute of
    /// internal bookkeeping on an unclean kill — never anything Atlas told
    /// you (a tick that spoke is a tick that saved) and never anything you
    /// did (a turn saves at its own call sites).
    pub fn persist_after(&mut self, t: u64, spoke: bool) {
        if spoke || self.last_persist == 0 || t.saturating_sub(self.last_persist) >= PERSIST_SWEEP_SECS {
            self.persist();
            self.last_persist = t.max(1);
        }
    }

    /// Take in the file index once it has been read (`index::Loading`).
    /// True when it is here (or there was nothing to read).
    fn settle_index(&mut self) -> bool {
        match self.index_load.settle(&mut self.index) {
            crate::index::Settled::StillLoading => false,
            crate::index::Settled::Took(on_disk) => {
                self.index_on_disk = Some(on_disk);
                true
            }
            crate::index::Settled::KeptNewer | crate::index::Settled::Ready => true,
        }
    }

    /// Wait up to `max` for the file index to be read (tests, and anything
    /// that needs the whole list before it can answer).
    pub fn wait_for_index(&mut self, max: std::time::Duration) -> bool {
        if let crate::index::Settled::Took(on_disk) = self.index_load.wait(&mut self.index, max) {
            self.index_on_disk = Some(on_disk);
        }
        !self.index_load.is_loading()
    }

    /// For a test: the index as if its read from disk hadn't finished.
    #[doc(hidden)]
    pub fn index_loading_for_test(&mut self, l: crate::index::Loading) {
        self.index = Index::default();
        self.index_on_disk = Some(self.index.written_as());
        self.index_load = l;
    }

    /// Is the file index still being read from disk? (A question about
    /// your files waits up to `INDEX_WAIT_FOR_A_QUESTION` for it first.)
    pub fn index_still_loading(&self) -> bool {
        self.index_load.is_loading()
    }

    pub fn observe(&mut self, t: u64) -> Signals {
        // Not before the index on disk is in: a walk compared against the
        // empty stand-in would report every file you have as new.
        let loaded = self.settle_index();
        // A rescan of the index costs disk and battery: none below your
        // battery floor, and none once it's past the size you set
        // (`perf::Throttle::may_scan`; `max_index_entries` was read by nothing).
        let may_scan = loaded && crate::perf::Throttle::new(self.tools_cfg().perf.clone()).may_scan(self.power, self.index.entries.len());
        self.awareness.observe(
            self.plat,
            &mut self.index,
            if may_scan { self.cfg.indexing.as_ref() } else { None },
            self.session.is_waiting(),
            t,
        )
    }

    /// What the model gets to see: the machine, plus what you're doing, plus
    /// the last few exchanges.
    /// Write what happened here into a folder, and take in whatever the other
    /// side left there.
    ///
    /// A folder is the carrier `sync.rs` already names -- `Carry::CloudFolder`,
    /// "it'll land next time both are on" -- and it is the only one that needs
    /// nothing running on the other machine and no network of its own. Point
    /// two Atlases at the same synced folder, a share, or a USB stick, and
    /// they carry each other.
    ///
    /// This replaces a message that said "moving files between your devices
    /// isn't built yet". That message was honest about itself, which is the
    /// better kind of stub, but `sync.rs` was 443 lines of working merge
    /// sitting behind it: the append-only log, the bundle, the version check,
    /// the clash rule that only `Changed` and `Removed` can conflict, and
    /// `already_seen` so a folder read twice does not double anything.
    /// Our own log as bytes ready for the wire — sealed with the household key
    /// when sealing is on, plain JSON otherwise, exactly as the folder path
    /// writes it. `None` only if sealing is on and there is no key to seal with,
    /// which is the same "don't ship it in the clear" refusal the folder makes.
    fn wire_bytes(
        &mut self,
        cfg: &crate::sync::SyncConfig,
        key: Option<&[u8]>,
        now: u64,
    ) -> Option<Vec<u8>> {
        let mut bundle =
            {
                self.note_addon_changes(now);
                crate::sync::make_bundle(&self.synclog, &self.synclog.device.clone(), 0, now)
            };
        bundle.belongs_to = cfg.belongs_to.clone();
        if cfg.encrypt_bundles {
            crate::sync::seal(&bundle, key?).ok().map(String::into_bytes)
        } else {
            serde_json::to_vec(&bundle).ok()
        }
    }

    /// Take a bundle in off the wire: the same acceptance the folder path does
    /// — skip our own, refuse another household's or a newer format, keep only
    /// what we haven't seen, merge, advance the clock, and append. Returns how
    /// many events were taken, any clash lines, and any clock-skew lines. The
    /// transport is just bytes; this is where a wire bundle becomes ours.
    fn take_in_wire(
        &mut self,
        incoming: &[u8],
        cfg: &crate::sync::SyncConfig,
        key: Option<&[u8]>,
        now: u64,
    ) -> (usize, Vec<String>, Vec<String>) {
        let mut clashes = Vec::new();
        let mut skews = Vec::new();
        let Ok(text) = std::str::from_utf8(incoming) else {
            return (0, clashes, skews);
        };
        let bundle = match crate::sync::read_bundle(text, key) {
            Ok(b) => b,
            Err(_) => return (0, clashes, skews),
        };
        if bundle.from_device == self.synclog.device
            || crate::sync::can_open(&bundle).is_err()
            || crate::sync::from_the_same_atlas(&bundle, &cfg.belongs_to).is_err()
        {
            return (0, clashes, skews);
        }
        let fresh: Vec<crate::sync::Event> = bundle
            .events
            .iter()
            .filter(|e| !self.seen_up_to.iter().any(|(d, s)| *d == e.device && e.seq <= *s))
            .cloned()
            .collect();
        if fresh.is_empty() {
            return (0, clashes, skews);
        }
        let merged = crate::sync::merge(&self.synclog.events, &fresh, now);
        if !merged.clashes.is_empty() {
            clashes.push(crate::sync::spoken(&merged, &bundle.from_name));
        }
        if let Some(skew) = self.synclog.note_seen(&fresh, now) {
            skews.push(skew.plain(&bundle.from_name));
        }
        let sealed = crate::sync::peek(text).is_some();
        for said in self.take_in_synced(&fresh, sealed, &bundle.from_name) {
            clashes.push(said);
        }
        let taken = fresh.len();
        for e in fresh {
            match self.seen_up_to.iter_mut().find(|(d, _)| *d == e.device) {
                Some((_, s)) => *s = (*s).max(e.seq),
                None => self.seen_up_to.push((e.device.clone(), e.seq)),
            }
            self.synclog.events.push(e);
        }
        (taken, clashes, skews)
    }

    /// Sync straight to the peers you've named by address — the case a tailnet
    /// (or any VPN, or a reachable public host) exists to serve.
    ///
    /// The same-network path only reaches a peer that answered a LAN broadcast,
    /// so two devices on *different* networks never sync directly even when both
    /// are online and mutually routable — exactly the phone-away-from-home case.
    /// Every `elsewhere` peer already carries a stable `host`; this dials that
    /// host on the fixed sync port and does the same one-round-trip exchange the
    /// LAN path does. `host` being a tailnet IP (100.x on Tailscale/WireGuard,
    /// already in the tree) is what makes it reach across networks — no relay,
    /// no server, nothing hardcoded.
    ///
    /// Offline-first stays intact: the folder path has already run by the time
    /// this is called, so a peer that is asleep or unroutable costs nothing here
    /// — it syncs through the folder when both are next on it, silently, exactly
    /// like a LAN miss. `skip` names peers the LAN path already reached this
    /// pass, so a device that is both on the same wifi *and* named by tailnet
    /// address doesn't get sent to twice. The bundle is re-made per peer so a
    /// single pass carries forward whatever the previous peer just handed us.
    fn dial_configured_peers(
        &mut self,
        cfg: &crate::sync::SyncConfig,
        key: Option<&[u8]>,
        now: u64,
        skip: &[String],
    ) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        let peers = self.tools_cfg().elsewhere.known.clone();
        for p in peers {
            let name = p.name.trim().to_string();
            let host = p.host.trim().to_string();
            if name.is_empty() || host.is_empty() {
                continue;
            }
            if skip.iter().any(|s| s.eq_ignore_ascii_case(&name)) {
                continue;
            }
            // Re-made per peer: if a previous peer this pass handed us events,
            // the next one gets them too, so one pass can chain across a tailnet.
            let Some(bytes) = self.wire_bytes(cfg, key, now) else {
                continue;
            };
            let port = p.sync_port.unwrap_or(crate::transport::SYNC_PORT);
            match crate::transport::exchange(
                &host,
                port,
                &bytes,
                std::time::Duration::from_secs(4),
            ) {
                Ok(reply) => {
                    let (t, cl, sk) = self.take_in_wire(&reply, cfg, key, now);
                    let took = if t > 0 { format!(", took in {t}") } else { String::new() };
                    lines.push(format!(
                        "Synced straight across to {name}{took} — reached by address, \
                         so it works off your local network too."
                    ));
                    lines.extend(cl);
                    lines.extend(sk);
                }
                // Asleep, or not routable right now. The folder carries it; a
                // named peer being unreachable is not worth a line every pass.
                Err(_) => {}
            }
        }
        lines
    }

    /// Record what changed about your add-ons since your other devices were
    /// last told, as ordinary sync events (`plugins::changes_to_carry`).
    fn note_addon_changes(&mut self, now: u64) {
        for (id, field, to) in crate::plugins::changes_to_carry(&self.store, &self.plugins_dir) {
            self.synclog.append(crate::sync::What::Changed { id, field, to }, now);
        }
        // Your groups' lists, and this device's key, for your other devices:
        // what lets your phone manage a group your laptop made.
        let me = self.my_key().unwrap_or_default();
        for (id, field, to) in crate::groups::changes_to_carry(&self.store, &me) {
            self.synclog.append(crate::sync::What::Changed { id, field, to }, now);
        }
    }

    /// Make what your other device did true here too.
    ///
    /// Until this existed, sync carried events and nothing applied them: a
    /// note captured on the phone crossed the wire, sat in this device's log,
    /// and never reached the notebook -- "Atlas on every device" was a log
    /// that travelled. Now a capture lands in the notebook, and an add-on
    /// change lands in the add-ons (`plugins::take_synced`, which only takes
    /// an approval from a sealed bundle). Returns anything worth saying.
    fn take_in_synced(&mut self, fresh: &[crate::sync::Event], sealed: bool, from: &str) -> Vec<String> {
        let mut said = Vec::new();
        let mut notes_changed = false;
        for e in fresh {
            match &e.what {
                crate::sync::What::Captured { text, .. } => {
                    let cfg = self.tools_cfg().capture.clone();
                    self.notebook.capture(text, None, e.at, &cfg);
                    notes_changed = true;
                }
                crate::sync::What::Changed { id, to, .. }
                    if id.starts_with(crate::groups::SYNC_GROUP) || id.starts_with(crate::groups::SYNC_DEVICE) =>
                {
                    let me = crate::peerkey::Identity::load_or_create(&self.peer_dir).ok();
                    if let Some(s) = crate::groups::take_synced(&self.store, me.as_ref(), id, to, sealed) {
                        said.push(s);
                    }
                }
                crate::sync::What::Changed { id, field, to } if id.starts_with(crate::plugins::SYNC_PREFIX) => {
                    if let Some(s) = crate::plugins::take_synced(
                        &self.store,
                        &self.plugins_dir,
                        &self.cfg.commands,
                        id,
                        field,
                        to,
                        sealed,
                        from,
                    ) {
                        said.push(s);
                    }
                }
                _ => {}
            }
        }
        if notes_changed {
            if let Err(e) = self.notebook.save(&self.store) {
                said.push(format!("Notes came over from {from} and I couldn't save them: {e}"));
            }
        }
        said
    }

    /// The household key for sync, loaded and derived the same way the folder
    /// path does — factored out so the per-tick serve can reuse it. Derivation
    /// is Argon2id (~0.5s), so this is called only once a peer has actually
    /// connected, never on an idle tick.
    fn sync_key(&self) -> Option<Vec<u8>> {
        let kept: crate::sync::KeptKey = self.store.load(crate::sync::KEY_FILE);
        if !kept.is_set() {
            return None;
        }
        kept.phrase().and_then(|p| crate::sync::key_from_phrase(&p)).ok()
    }

    /// Listen for a peer dialing us for a direct sync — once, without blocking —
    /// called every tick so a device is reachable continuously, not only while
    /// it is itself running a sync pass. That is the difference between "we
    /// happen to sync when both of us are syncing" and "a phone coming onto the
    /// wifi is answered at once."
    ///
    /// Passive: it only receives a bundle and hands ours back, so it runs even
    /// when Atlas is paused — the same rule the heartbeat and the helper-reap
    /// follow, "paused means only listening," and this is listening. The accept
    /// is non-blocking, so an idle tick spends one syscall and returns; the key
    /// derivation happens only when a peer is actually on the socket. Returns a
    /// line only when something arrived.
    fn serve_direct_sync(&mut self, now: u64) -> Vec<String> {
        let cfg = self.tools_cfg().sync.clone();
        if !cfg.enabled {
            return Vec::new();
        }
        // A bundle is your notes; if Atlas has been handed to someone else,
        // don't serve — the same boundary the folder path draws before writing.
        if self.handover().stance.handed_over() {
            return Vec::new();
        }
        if self.sync_server.is_none() {
            self.sync_server = crate::transport::Server::bind(crate::transport::SYNC_PORT).ok();
        }
        let Some(server) = self.sync_server.take() else {
            return Vec::new();
        };
        let mut lines: Vec<String> = Vec::new();
        let _ = server.poll(std::time::Duration::from_millis(50), |incoming| {
            // A peer actually connected — now the key derivation is worth it.
            let key = self.sync_key();
            let (t, cl, sk) = self.take_in_wire(&incoming, &cfg, key.as_deref(), now);
            if t > 0 {
                lines.push(format!(
                    "Took in {t} from another of your devices, straight across the network."
                ));
            }
            lines.extend(cl);
            lines.extend(sk);
            self.wire_bytes(&cfg, key.as_deref(), now).unwrap_or_default()
        });
        self.sync_server = Some(server);
        lines
    }

    fn carry_to_your_other_devices(&mut self, now: u64) -> String {
        let cfg = self.tools_cfg().sync.clone();
        if !cfg.enabled {
            return "Syncing between your devices is switched off in your settings.".into();
        }
        // A bundle is your captures. Whoever is at the machine decides where
        // they go, so this is the one place the question "is that actually
        // you?" has to be asked before anything is written.
        //
        // Handed over means you have said out loud that somebody else is
        // using Atlas. They may talk to it; they may not push your notes into
        // a folder. This was missing from the first version of this function
        // and is the leak it would have caused.
        if self.handover().stance.handed_over() {
            return "Not while somebody else is using Atlas — a bundle is your \
                    own notes, and that's yours to send."
                .into();
        }
        // The best route available (H13i): the folder you set, or else a
        // cloud folder this machine already syncs.
        let mut dir = cfg.folder.trim().to_string();
        if dir.is_empty() {
            match crate::sync::best_folder() {
                Some((p, _)) => dir = p.display().to_string(),
                None => {
                    return "I've nowhere to put it — there's no cloud folder on this machine. Set \
                            `sync.folder` to a folder both machines can see (a share, or a USB stick \
                            you plug in) and I'll carry it through there."
                        .into();
                }
            }
        }
        let (_carry, route) = crate::sync::route_of(std::path::Path::new(&dir));
        self.log.info(&format!("syncing through {dir}: {route}"));
        let dir = std::path::Path::new(&dir);
        if let Err(e) = std::fs::create_dir_all(dir) {
            return format!("I couldn't open {}: {e}", dir.display());
        }

        // Anything left from a pairing that was started and never finished.
        // The window is three minutes and a taken handoff deletes itself, so
        // this is only for the abandoned case -- which is exactly the one
        // nobody would think to tidy up.
        let swept = crate::sync::sweep_handoffs(dir, now)
            + crate::household::sweep_invitations(dir, now);
        if swept > 0 {
            self.log.info(&format!("cleared {swept} expired key handoff(s) from the sync folder"));
        }

        // The household key, derived once for this pass rather than once per
        // bundle -- Argon2id at 64MiB is half a second, and a folder with six
        // bundles in it would otherwise spend three.
        //
        // Loaded whether or not sealing is on: reading is never gated by the
        // switch, so turning it off does not strand bundles already written,
        // and a device that has the key can always open what it is sent.
        let kept: crate::sync::KeptKey = self.store.load(crate::sync::KEY_FILE);
        let key: Option<Vec<u8>> = if kept.is_set() {
            match kept.phrase().and_then(|p| crate::sync::key_from_phrase(&p)) {
                Ok(k) => Some(k),
                Err(why) => {
                    self.log.info(&format!("the household key would not load: {why}"));
                    None
                }
            }
        } else {
            None
        };

        // Take in first, then write out -- so the bundle we leave already
        // includes anything that just arrived, and one pass on each machine
        // is enough to converge rather than two.
        let mut taken = 0usize;
        let mut clashes: Vec<String> = Vec::new();
        let mut refused: Vec<String> = Vec::new();
        // A device whose clock is wildly off — noticed while taking its bundle
        // in, never a reason to refuse it. See `sync::Log::note_seen`.
        let mut skews: Vec<String> = Vec::new();

        // Direct, same network: if the other device dialed us since last pass,
        // serve it now — take its bundle in and hand ours straight back over the
        // socket. Bound lazily and polled once, without blocking: a step at a
        // time, like everything else on this clock. The folder below still runs,
        // so this is pure acceleration when both are on the same wifi.
        if self.sync_server.is_none() {
            self.sync_server = crate::transport::Server::bind(crate::transport::SYNC_PORT).ok();
        }
        if let Some(server) = self.sync_server.take() {
            let key_ref = key.as_deref();
            let _ = server.poll(std::time::Duration::from_millis(200), |incoming| {
                let (t, mut cl, mut sk) = self.take_in_wire(&incoming, &cfg, key_ref, now);
                taken += t;
                clashes.append(&mut cl);
                skews.append(&mut sk);
                self.wire_bytes(&cfg, key_ref, now).unwrap_or_default()
            });
            self.sync_server = Some(server);
        }
        if let Ok(entries) = std::fs::read_dir(dir) {
            let mut paths: Vec<std::path::PathBuf> =
                entries.flatten().map(|e| e.path()).collect();
            paths.sort();
            for p in paths {
                if p.extension().and_then(|x| x.to_str()) != Some("bundle") {
                    continue;
                }
                let Ok(raw) = std::fs::read_to_string(&p) else { continue };
                // Sealed or plain, through one reader. Two paths here is how
                // a plaintext fallback survives a feature meant to remove
                // one -- and a sealed bundle with no key says which command
                // fixes that rather than reporting an unreadable file.
                let bundle = match crate::sync::read_bundle(&raw, key.as_deref()) {
                    Ok(b) => b,
                    Err(why) => {
                        refused.push(format!("{}: {why}", p.display()));
                        continue;
                    }
                };
                // Ours, on a folder we also write to. Skipping it is what
                // makes a shared folder work at all.
                if bundle.from_device == self.synclog.device {
                    continue;
                }
                if let Err(why) = crate::sync::can_open(&bundle) {
                    refused.push(why);
                    continue;
                }
                // Your other Atlas is not your other machine. A work Atlas and
                // the personal one do not carry each other; they are linked
                // through the business hub, item by item, on purpose.
                if let Err(why) = crate::sync::from_the_same_atlas(&bundle, &cfg.belongs_to) {
                    refused.push(why);
                    continue;
                }
                // Only what we have not already taken from that device.
                // Counting the whole bundle each time would report two new
                // things every morning for a folder that had not changed.
                let (new, _skipped) =
                    crate::sync::already_seen(&bundle.events, &self.seen_up_to);
                if new == 0 {
                    continue;
                }
                let fresh: Vec<crate::sync::Event> = bundle
                    .events
                    .iter()
                    .filter(|e| {
                        !self
                            .seen_up_to
                            .iter()
                            .any(|(d, s)| *d == e.device && e.seq <= *s)
                    })
                    .cloned()
                    .collect();

                let merged = crate::sync::merge(&self.synclog.events, &fresh, now);
                taken += fresh.len();
                if !merged.clashes.is_empty() {
                    clashes.push(crate::sync::spoken(&merged, &bundle.from_name));
                }
                // Advance this device's clock past everything the bundle
                // carried, so anything done after this reconnect sorts after
                // what was just learned — and catch a wrong remote clock while
                // we're here. The events are kept either way.
                if let Some(skew) = self.synclog.note_seen(&fresh, now) {
                    skews.push(skew.plain(&bundle.from_name));
                }
                let sealed = crate::sync::peek(&raw).is_some();
                for said in self.take_in_synced(&fresh, sealed, &bundle.from_name) {
                    clashes.push(said);
                }
                for e in fresh {
                    match self.seen_up_to.iter_mut().find(|(d, _)| *d == e.device) {
                        Some((_, s)) => *s = (*s).max(e.seq),
                        None => self.seen_up_to.push((e.device.clone(), e.seq)),
                    }
                    self.synclog.events.push(e);
                }
            }
        }

        let mut bundle =
            {
                self.note_addon_changes(now);
                crate::sync::make_bundle(&self.synclog, &self.synclog.device.clone(), 0, now)
            };
        // Stamped on the way out, so the other side can tell whose it is
        // without having to know which folder it came from.
        bundle.belongs_to = cfg.belongs_to.clone();
        let name = format!("{}.bundle", sanitise(&self.synclog.device));
        let out = dir.join(name);

        // Sealed, if you asked for that. With sealing on and no key, nothing
        // is written at all: falling back to plaintext would be the switch
        // quietly doing the opposite of what it says, which is the whole
        // class of defect this was found in.
        let mut made_a_key = String::new();
        let written = if cfg.encrypt_bundles {
            // No key yet? Make one. The first version refused here and told
            // you to run a command, which is a wall in front of a switch you
            // just turned on -- and the switch is in the hub, where somebody
            // who does not use a terminal found it.
            //
            // Making one is safe to do unasked precisely because losing it
            // costs nothing durable: a bundle is a courier, every bundle
            // carries the whole log, and the sync page has a button that
            // starts a new key.
            // Cloned so the outer `key` survives for the direct same-network
            // send below, which seals with the same household key.
            let key = match key.clone() {
                Some(k) => Ok(k),
                None => match crate::sync::ensure_key(&self.store, now) {
                    Ok((k, made)) => {
                        if let Some(setup) = made {
                            made_a_key = crate::sync::made_one(&setup);
                            self.log.info("made a household key for sealing bundles");
                        }
                        Ok(k)
                    }
                    Err(why) => Err(why),
                },
            };
            match key {
                Err(why) => Err(why),
                Ok(k) => crate::sync::seal(&bundle, &k)
                    .and_then(|raw| crate::sync::write_whole(&out, raw.as_bytes())),
            }
        } else {
            serde_json::to_string_pretty(&bundle)
                .map_err(|e| e.to_string())
                .and_then(|raw| crate::sync::write_whole(&out, raw.as_bytes()))
        };

        // --- Which route this would have taken ---
        //
        // `mesh::choose` picks between `SameNetwork`, `Mesh`, `Cable` and
        // `Cloud`, and until 19 Sep 2026 every argument it got was a
        // hardcoded literal, so the answer was invariably `Cloud` and the
        // other three were unreachable. `Cloud` is a folder and needs no
        // address, which is why it was the only one that ever worked.
        //
        // `same_network` is a real observation now: `nearby::look` shouts on
        // the local network and `mesh::on_this_network` asks whether the
        // machine you actually sync with answered. Nothing is *sent* that way
        // yet -- the bundle still goes through the folder, which is written
        // above -- so this says what it found rather than claiming a
        // transport that is not built. That is the same line `atlas mesh`
        // takes and it is drawn in the same place.
        //
        // Looked at only when there is somebody to look for: a broadcast on
        // every sync, on a machine with no peers configured, is a packet sent
        // to ask a question nobody is waiting to answer.
        let mcfg = self.tools_cfg().mesh.clone();
        let ncfg = self.tools_cfg().nearby.clone();
        let peers = self.tools_cfg().elsewhere.names().iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let (route, peer_addr) = if peers.is_empty() {
            (None, None)
        } else {
            let found = crate::nearby::look(&ncfg).unwrap_or_default();
            let here = peers.iter().any(|p| crate::mesh::on_this_network(p, &found));
            // The peer that answered — its name (so the configured-address dial
            // below can skip it) and its host (the one we send straight to). Its
            // shout carries its host; the sync port is fixed.
            let addr = found.iter().find_map(|f| {
                peers
                    .iter()
                    .find(|p| p.eq_ignore_ascii_case(&f.name))
                    .map(|p| (p.clone(), f.host.clone(), crate::transport::SYNC_PORT))
            });
            // `mesh_up` stays false: a private network is the one route with
            // nothing behind it, and saying otherwise here would be the
            // hardcoded literal coming back wearing an observation's clothes.
            (Some(crate::mesh::choose(here, false, true, false, &mcfg)), addr)
        };

        let mut said = match written {
            Ok(()) => format!(
                "Left {} thing{} for your other devices in {}{}.{}",
                bundle.events.len(),
                if bundle.events.len() == 1 { "" } else { "s" },
                dir.display(),
                if cfg.encrypt_bundles { ", sealed" } else { "" },
                made_a_key
            ),
            Err(e) => format!("I couldn't write the bundle: {e}"),
        };
        if taken > 0 {
            said.push_str(&format!(" Took in {taken} from the other side."));
        }
        for c in clashes {
            said.push(' ');
            said.push_str(&c);
        }
        for r in refused {
            said.push_str(&format!(" {r}"));
        }
        for s in skews {
            said.push(' ');
            said.push_str(&s);
        }
        // Said last, and only when there was something to notice. On the same
        // wifi the folder is a slow way to move something across the room,
        // and that is worth knowing even while it is still the only way.
        // Peers reached directly this pass, by name — so the configured-address
        // dial below never sends to the same device twice.
        let mut synced_directly: Vec<String> = Vec::new();
        if let Some((crate::mesh::Path::SameNetwork, why)) = route {
            match &peer_addr {
                Some((name, host, port)) => {
                    // Straight across the wifi. Send ours, take theirs back, in
                    // one round trip. The folder above already ran, so a miss
                    // here costs nothing — it just means the folder does it.
                    match self.wire_bytes(&cfg, key.as_deref(), now) {
                        Some(bytes) => match crate::transport::exchange(
                            host,
                            *port,
                            &bytes,
                            std::time::Duration::from_secs(3),
                        ) {
                            Ok(reply) => {
                                let (t, cl, sk) =
                                    self.take_in_wire(&reply, &cfg, key.as_deref(), now);
                                synced_directly.push(name.clone());
                                said.push_str(&format!(
                                    " ({why} — synced straight across to your other Atlas, no folder needed{}.)",
                                    if t > 0 { format!(", took in {t}") } else { String::new() }
                                ));
                                for c in cl {
                                    said.push(' ');
                                    said.push_str(&c);
                                }
                                for s in sk {
                                    said.push(' ');
                                    said.push_str(&s);
                                }
                            }
                            Err(_) => said.push_str(&format!(
                                " ({why} — your other Atlas answered on this network, but the \
                                 direct send didn't land; it's in the folder.)"
                            )),
                        },
                        None => said.push_str(&format!(
                            " ({why} — your other Atlas is on this network; it's in the folder.)"
                        )),
                    }
                }
                None => said.push_str(&format!(
                    " ({why} — your other Atlas is on this network; it's in the folder.)"
                )),
            }
        }

        // Then the peers you've named by address that we did *not* just reach on
        // the local network — the tailnet/VPN/public-host case. This is what
        // lets the phone sync with the laptop when they're on different networks
        // but both online, without a folder in between. Silent when they're
        // asleep; the folder still carries it.
        for line in self.dial_configured_peers(&cfg, key.as_deref(), now, &synced_directly) {
            said.push(' ');
            said.push_str(&line);
        }
        said
    }

    /// One step of the night, at most once an hour.
    ///
    /// Written as "advance by one" rather than "run the whole night in a
    /// loop" on purpose: a loop here would hold the tick for as long as the
    /// night lasts, and the tick is also how Atlas answers you. The night is
    /// hours long and has nobody waiting on it, so it advances a step at a
    /// time like everything else on this clock.
    fn run_the_night(&mut self, t: u64) {
        let cfg = self.tools_cfg().overnight.clone();
        let dcfg = self.tools_cfg().daily.clone();

        // --- Where you are, rather than what time it is ---
        //
        // This was `cfg.in_window(hour)` -- `start_hour` to `stop_hour`, a
        // fixed clock window, and exactly the mistake `brief.at_hour` was. It
        // cannot tell you asleep from you at a desk at two in the morning,
        // and it has no idea you left for work at eight. So the night's work
        // ran on a timetable: it ran while you were up working, and it did
        // not run on the Saturday you were out all day.
        //
        // `daily::whereabouts` asks instead whether you are gone, and gone
        // long enough that starting an hour of work will not be interrupted.
        // Two kinds of gone, because they are different permissions:
        // `Asleep` is your quiet stretch, which `Rhythm` worked out from when
        // you actually stop; `Out` is a gap in hours you are usually about,
        // which can end at any moment.
        //
        // Nothing asks you to say you are stepping away. Being told is a
        // thing people do not do, and a system that needs telling does
        // nothing.
        let where_you_are = crate::daily::whereabouts(
            self.last_turn_of_yours,
            t,
            &self.rhythm,
            &dcfg,
            &self.tools_cfg().judgment,
        );
        // Recorded only while you are gone, so it holds where you were
        // *while the work happened*. Set unconditionally it is overwritten by
        // the first tick after you come back, and the write-up then opens
        // "You're about:" -- which is true of this moment and false of the
        // six hours it is describing. Caught by the night's own test
        // reporting itself that way.
        if where_you_are.free_to_work() {
            self.worked_while = Some(where_you_are);
        }

        // Back at the machine with a night behind us: write it up once, then
        // forget it. Checked before `enabled` so that switching overnight off
        // mid-session still gives you the brief for the work already done
        // rather than swallowing it.
        if !where_you_are.free_to_work() {
            if let Some(line) = crate::inhibit::apply(&mut self.sleep_hold, crate::awake::Hold::Release, "") {
                self.log.info(&line);
            }
            if let Some(session) = self.overnight.take() {
                if !session.results.is_empty() {
                    // Where you were while it ran, not where you are now.
                    // Taken from what was recorded at the time -- asking now
                    // would say "you're about", which is true and is a
                    // different fact.
                    let then = self.worked_while.unwrap_or(crate::daily::Whereabouts::Asleep);
                    let mut brief = crate::overnight::morning_brief(&session, then);
                    if let Some(why) = &session.ended_because {
                        brief.push_str(&format!(" I stopped because I {why}."));
                    }
                    // The long account (H13j): kept as a note, mentioned in
                    // the brief, and there when asked for.
                    let detail = crate::overnight::morning_detail(&session);
                    let _ = self.store.save("overnight_detail", &detail);
                    let ccfg = self.tools_cfg().capture.clone();
                    self.notebook.capture(&detail, Some("overnight"), t, &ccfg);
                    let _ = self.notebook.save(&self.store);
                    brief.push_str(" The full account is in your notes, or ask \"what did you do overnight\".");
                    self.morning_brief = Some(brief);
                }
            }
            return;
        }
        if !cfg.enabled {
            return;
        }
        // The guarantee, read rather than assumed.
        //
        // `apply_while_asleep` is `#[serde(skip)]` and hard-wired false, so
        // this cannot fire today. It is here because a flag nothing reads is
        // a promise nothing keeps: if someone ever makes it settable, the
        // night refuses to run rather than quietly starting to apply things
        // while you are asleep. Defaulting to "do nothing" is the only safe
        // direction for a mistake in this particular setting.
        if cfg.apply_while_asleep {
            self.log.warn(
                "overnight is set to apply changes while you're asleep — refusing to run",
            );
            return;
        }
        if t.saturating_sub(self.last_overnight) < 3600 {
            return;
        }
        self.last_overnight = t;

        // What Atlas could not finish, minus anything that needs a decision
        // only you can make. `worth_doing_overnight` reads the request the
        // way you said it -- "which of these", "should I", "send", "pay" --
        // and leaves those alone. Working on one of those overnight would
        // mean guessing at the answer, which is the one thing the night must
        // not do.
        let queue: Vec<String> = self
            .backlog
            .outstanding()
            .iter()
            .map(|i| i.request.clone())
            .filter(|r| {
                let p = crate::handoff::Problem {
                    goal: r.clone(),
                    ..Default::default()
                };
                crate::overnight::worth_doing_overnight(&p)
            })
            .collect();

        // May the machine be held for the night's work? `keep_awake` and the
        // `awake` module were built for exactly this moment. Since round 5 the
        // answer is acted on (`inhibit`, below). It also gates the work: on
        // battery below the give-up
        // line, starting an hour of work that will die and cost you the
        // morning's charge is worse than not starting it. So the decision
        // gates whether tonight's work begins, and its reason is recorded
        // once rather than discarded.
        let r = crate::health::read_machine();
        let power = crate::awake::Power {
            on_battery: r.on_battery,
            battery_pct: r.battery_percent.map(|p| p as u32).unwrap_or(100),
            // The lid state has no reader on any platform, so it is reported
            // as unknown rather than guessed — the battery branch below does
            // not depend on it.
            lid_closed: false,
            lid_action: crate::awake::LidAction::Unknown,
            external_display: false,
        };
        let held_mins = t.saturating_sub(self.overnight.as_ref().map(|s| s.started_at).unwrap_or(t))
            / 60;
        let (hold, why_not) =
            self.keep_awake(crate::awake::Because::OvernightWork, &power, held_mins as u32);
        // And now it is acted on: the machine is actually held awake while
        // the night runs, and let go when the decision says so.
        if let Some(line) = crate::inhibit::apply(&mut self.sleep_hold, hold, "tonight's work") {
            if line.starts_with("couldn't") {
                self.log.warn(&line);
            } else {
                self.log.info(&line);
            }
        }
        if hold == crate::awake::Hold::Release && !why_not.is_empty() {
            // Said once, when the night declines to start, not on every tick.
            if self.overnight.is_none() {
                self.log.info(&format!("not starting overnight work: {why_not}"));
            }
            return;
        }

        let session = self.overnight.get_or_insert_with(|| crate::overnight::Session::start(t));
        // No hosted brain is wired, so there is no spend to have left. Passed
        // explicitly rather than defaulted so that wiring one later has to
        // come back through here.
        let budget_left = 0.0;
        match session.next(&queue, &cfg, where_you_are, budget_left) {
            crate::overnight::Step::Work(problem) => {
                // `ask_you_later`: the night's work is the writing-up. Recorded
                // as `NeedsYou` because that is what is true -- it is waiting
                // on you, not stuck and not solved. Claiming `Solved` here
                // would be the morning brief lying about a night's work.
                session.record(crate::overnight::Result_ {
                    problem: problem.clone(),
                    outcome: crate::overnight::Outcome::NeedsYou,
                    attempts: 0,
                    sandbox_path: None,
                    tests_passed: None,
                    note: crate::overnight::note_for(&problem, &cfg),
                    dollars: 0.0,
                });
            }
            crate::overnight::Step::Finish(why) | crate::overnight::Step::Abandon(why) => {
                // The night is over, and the brief waits for the morning.
                //
                // It used to be said the moment the queue emptied, which on a
                // short backlog is two in the morning -- a "morning brief"
                // delivered while you are asleep, to nobody, and gone by the
                // time you are up. The session is marked ended and held; the
                // out-of-window branch above is what speaks it.
                session.ended_at = Some(t);
                if session.ended_because.is_none() {
                    session.ended_because = Some(why);
                }
                if let Some(line) = crate::inhibit::apply(&mut self.sleep_hold, crate::awake::Hold::Release, "") {
                    self.log.info(&line);
                }
            }
        }
    }

    pub fn context(&mut self) -> String {
        let mut s = brain::context(self.cfg, self.plat);

        // The question this turn is answering, first, because everything else
        // in the context is standing information and this is the one thing
        // that makes the next sentence mean what it means. "The second one"
        // is unreadable without it.
        if let Some(q) = self.answering.take() {
            s.push_str(&format!(
                "You just asked: {q}\nWhat follows is their answer to that, not a new \
                 request. Take it as the missing piece and carry on.\n"
            ));
        }

        // Where the model is decides how much of this goes in. See
        // `brain::focus_line`: a window title is both written by someone else
        // and private, and those need different answers.
        //
        // No configured model means nothing is sent at all, so the question is
        // moot -- but `CannotTell` is the honest answer for a context built
        // without one, and it is the cautious one, so that is what it gets.
        // With no hand-written `llm:`, the connection is the one Atlas built
        // for itself from `models:` -- local unless the server is elsewhere.
        let at = match self.tools_ref() {
            Some(t) => match &t.llm {
                Some(l) => l.endpoint(),
                None => crate::models::self_built_endpoint(&t.models),
            },
            None => brain::Endpoint::CannotTell,
        };

        let active = self.plat.active_window().unwrap_or(None);
        if let Some(a) = &active {
            s.push_str(&brain::focus_line(a, at));
        }
        let names: Vec<String> =
            self.index.recent(5).iter().map(|e| e.name.clone()).collect();
        s.push_str(&brain::recent_files_line(&names, at));

        // Somebody wrote an instruction into a window title or a file name.
        //
        // The quoting above is what protects Atlas; this is so Eric hears
        // about it. Recorded under the same store key `atlas read` uses, so
        // "what have you been fed" has ONE answer covering everything Atlas
        // has taken in -- whether it fetched the page or merely had it in
        // front of it.
        //
        // Inside the `if`, so the ordinary turn -- which is every turn --
        // neither loads nor saves the inbox. Reading a file on each context
        // build to record nothing would be a real cost for the rare case.
        let tried = brain::orders_in_view(active.as_ref(), &names);
        if !tried.is_empty() {
            let what = tried.join("; ");
            self.log.info(&format!("order-shaped text in view: {what}"));
            let mut fed: crate::untrusted::Inbox = self.store.load("read-from-outside");
            fed.took_in(
                crate::untrusted::Read::new(
                    active.as_ref().map(|a| a.process.as_str()).unwrap_or("a file name"),
                    active.as_ref().map(|a| a.title.as_str()).unwrap_or_default(),
                    crate::store::now(),
                ),
                500,
            );
            // Same `let _ =` as the other saves on this path: a context build
            // must not fail because the record of an attempt could not be
            // written, and `log.info` above has already said it happened.
            let _ = self.store.save("read-from-outside", &fed);
        }
        // What you have corrected, in front of the model on every turn.
        //
        // This is the line that makes `revise.rs` mean anything. Its rule 1 is
        // that where a lesson is written decides whether it works -- a lesson
        // about how a task is done has to be read *every time* the task runs,
        // or it is a note in a diary nobody opens. Before this, nothing
        // learned reached the model at all: context was displays, apps, the
        // focused window, recent files and the conversation.
        //
        // Bounded by `MAX_STANDING` for the same reason the brief is bounded:
        // a context that grows with every correction eventually crowds out the
        // thing you just said.
        let learned = crate::revise::standing(&self.mending.applied);
        if !learned.is_empty() {
            s.push_str(&learned);
            s.push('\n');
        }
        let convo = self.thread.context(&self.thread_cfg());
        if !convo.trim().is_empty() {
            s.push_str("Conversation so far:\n");
            s.push_str(&convo);
            s.push('\n');
        }
        s
    }

    /// Look something up on the web and write a note about it.
    ///
    /// Used to run the whole thing here, synchronously, on the tick or
    /// turn thread — a search-and-summarize can easily run twenty or
    /// thirty seconds, which is exactly the class of thing `crew.rs`
    /// exists for: it is not fixing a chore nobody's waiting on (that's
    /// `backup`/`housekeeping`), it is the difference between Atlas
    /// answering everything else while it works, and going silent for
    /// half a minute because you asked it to look something up.
    ///
    /// So `research` is spoken in two parts now. This function returns the
    /// honest immediate truth — "looking into it" — and the real answer,
    /// whichever it turns out to be, is reported later by
    /// `take_crew_news` once the crew errand actually finishes. Every
    /// failure below that can be known *now* (switched off, offline, no
    /// model) still returns immediately and finally — there is nothing to
    /// wait on in those cases, so there is nothing to be honest about
    /// deferring.
    ///
    /// The note is saved automatically either way — Eric's call: he
    /// doesn't want the save narrated (where it went, or whether it
    /// worked). Asking for a note by name, or asking Atlas to save
    /// something, are separate, deliberate asks; this one just happens.
    /// "What's in my inbox." Every account, fetched for real — each one is
    /// a TCP connection, a TLS handshake, a login, and a search, which is
    /// genuinely slow with several accounts, so this goes to the crew the
    /// same way `research` and `ask_the_room` do: an honest immediate
    /// acknowledgment, and the real sorted inbox once every reachable
    /// account has actually answered.
    /// The vault lookup every mail feature needs first: for each
    /// configured account, work out its vault entry and read the actual
    /// password out — on the tick thread, since a crew errand can't
    /// borrow `self.vault`. Shared between `check_mail` and
    /// `check_unsubscribe` rather than duplicated, since it's the same
    /// question either way: which accounts can Atlas actually get into
    /// right now.
    fn resolve_mail_jobs(
        &mut self,
        cfg: &crate::mail::MailConfig,
        now: u64,
    ) -> (Vec<(crate::mail::Account, String)>, Vec<String>) {
        let mut jobs = Vec::new();
        let mut problems = Vec::new();
        // Scheduled work with nobody there to type: the sign-in copy, if you
        // made one (`vault.open_on_this_login`).
        self.open_vault_for_scheduled_work(now);
        for account in &cfg.accounts {
            // Himalaya keeps its own passwords: nothing comes out of the vault
            // for it, and its errand asks Himalaya rather than a server.
            if cfg.by_himalaya() {
                let mut a = account.clone();
                a.imap_host = crate::himalaya::as_host(&cfg.himalaya, account.for_himalaya());
                jobs.push((a, String::new()));
                continue;
            }
            let vault_name = match crate::mail::credential_source(account) {
                Ok(n) => n.to_string(),
                Err(e) => {
                    problems.push(format!("{}: {e}", account.name));
                    continue;
                }
            };
            match self.vault.get(&vault_name, now) {
                Ok(password) => jobs.push((account.clone(), password)),
                Err(e) => problems.push(format!("{}: {e}", account.name)),
            }
        }
        (jobs, problems)
    }

    /// Make or remove the vault's sign-in copy to match the setting, while
    /// the vault is open with your passphrase. Returns a sentence to add.
    fn keep_sign_in_copy(&mut self, now: u64) -> String {
        let want = self.tools_cfg().vault.open_on_this_login;
        let have = self.vault.sealed_to_this_login();
        let said = if want && !have {
            match self.vault.seal_to_this_login(now) {
                Ok(()) => " Sealed a copy to your Windows sign-in, so scheduled mail checks can open it while you're signed in.".to_string(),
                Err(why) => format!(" I couldn't seal it to your sign-in: {why}."),
            }
        } else if !want && have {
            self.vault.unseal_from_this_login();
            " Took away the sign-in copy, as your settings say.".to_string()
        } else {
            String::new()
        };
        if !said.is_empty() {
            let _ = self.vault.save(&crate::roots::install_state());
        }
        said
    }

    /// Open the vault for scheduled work through the sign-in copy, when the
    /// setting is on and it is shut. Never counts as proving it's you.
    fn open_vault_for_scheduled_work(&mut self, now: u64) {
        if self.vault.state() == crate::vault::State::Open || !self.tools_cfg().vault.open_on_this_login {
            return;
        }
        if !self.vault.sealed_to_this_login() {
            return;
        }
        if let Err(why) = self.vault.open_unattended(now) {
            self.log.warn(&format!("couldn't open the vault on your sign-in: {why}"));
        }
    }

    /// Update the contact book from the messages just read, and say what is
    /// worth saying about it.
    ///
    /// Groups the kept messages by sender, asks `messaging::note_on` for a
    /// note on each (which is where the folder is decided from what they
    /// wrote), and merges those into the stored notes so the book accumulates
    /// across reads rather than being rebuilt each time. The only thing said
    /// out loud is a *new* work or prospect contact -- the approach you would
    /// otherwise miss in a count of "3 messages". Personal and unsorted senders
    /// are kept silently.
    fn note_the_senders(
        &mut self,
        kept: &[crate::messaging::Message],
        names: &[String],
    ) -> String {
        // By sender, in first-seen order, so each note is built from that one
        // person's messages the way `note_on` expects.
        let mut order: Vec<String> = Vec::new();
        let mut by_sender: std::collections::HashMap<String, Vec<crate::messaging::Message>> =
            std::collections::HashMap::new();
        for m in kept {
            if !by_sender.contains_key(&m.from) {
                order.push(m.from.clone());
            }
            by_sender.entry(m.from.clone()).or_default().push(m.clone());
        }

        let mut people: Vec<crate::messaging::Person> =
            self.store.load(crate::messaging::PEOPLE);
        let mut fresh_work: Vec<crate::messaging::Person> = Vec::new();
        for from in &order {
            let msgs = &by_sender[from];
            let Some(note) = crate::messaging::note_on(msgs, names) else {
                continue;
            };
            let is_work = matches!(
                note.folder,
                crate::messaging::Folder::Work | crate::messaging::Folder::Prospect
            );
            match people
                .iter_mut()
                .find(|p| p.name == note.name && p.platform == note.platform)
            {
                Some(existing) => {
                    // Was this already someone we knew was work? Only a folder
                    // that firms up now is news.
                    let was_work = matches!(
                        existing.folder,
                        crate::messaging::Folder::Work | crate::messaging::Folder::Prospect
                    );
                    existing.last_at = note.last_at;
                    existing.messages = existing.messages.saturating_add(note.messages);
                    existing.folder = note.folder;
                    if is_work && !was_work {
                        fresh_work.push(existing.clone());
                    }
                }
                None => {
                    if is_work {
                        fresh_work.push(note.clone());
                    }
                    people.push(note);
                }
            }
        }
        let _ = self.store.save(crate::messaging::PEOPLE, &people);

        // The first new work contact is said the way you asked (F3), with the
        // offer to find the answer; saying yes starts the research. Any others
        // are named.
        let Some(first) = fresh_work.first() else { return String::new() };
        let mut said = String::new();
        if let Some(line) = crate::messaging::filed(first) {
            let topic = first.first_about.trim().to_string();
            if !topic.is_empty() && self.pending_offer.is_none() {
                self.session.ask(&line);
                self.pending_offer = Some(crate::proactive::Offer {
                    kind: "answer_a_contact".into(),
                    message: line.clone(),
                    command: format!("research {topic}"),
                    confidence: 0.8,
                    cost: 1,
                });
            }
            said.push(' ');
            said.push_str(&line);
        }
        if fresh_work.len() > 1 {
            let rest: Vec<String> = fresh_work[1..].iter().map(|p| format!("{} ({})", p.name, p.folder.name())).collect();
            said.push_str(&format!(" Also new and worth a note: {}.", rest.join(", ")));
        }
        said
    }

    fn check_mail(&mut self) -> String {
        let cfg = self.tools_cfg().mail.clone();
        if !cfg.enabled {
            return "Reading your email is switched off.".into();
        }
        if cfg.accounts.is_empty() {
            return "I don't have any mail accounts set up yet.".into();
        }
        if self.connectivity.cached() == Reach::Offline {
            return "I can't check your mail without a connection.".into();
        }

        let now = crate::store::now();
        // The password comes out of the vault here, on the tick thread —
        // a crew errand can't borrow `self.vault`, and a decrypted app
        // password shouldn't be threaded through any more code than it
        // has to be. What actually crosses into the errand is the
        // account and its password, already resolved, nothing else.
        let (jobs, setup_problems) = self.resolve_mail_jobs(&cfg, now);
        if jobs.is_empty() {
            return format!("I couldn't get at any of your mail accounts: {}", setup_problems.join("; "));
        }

        let store = self.store.clone();
        let llm = self.llm.clone();
        let draft_cfg = self.tools_cfg().draft.clone();
        let may_email_clients = cfg.may_email_clients;

        // The mail cache the waiting-for list and meeting prep read from
        // (round 11): letters, not whole messages -- excerpts, scrubbed.
        let wd = self.workday_cfg();
        let mail_book_on = wd.waiting_for.enabled;
        let look_back_days = wd.waiting_for.look_back_days;
        let keep_days = wd.mail_keep_days;
        let work: crew::Work = Box::new(move |ctl| {
            let clients = crate::clients::ClientList::load(&store);
            let mut outbox = crate::outbox::Outbox::load(&store);
            let mut mail_book: crate::mailbook::MailBook = store.load(crate::mailbook::MailBook::FILE);
            let mut orders = crate::orders::Orders::load(&store);
            let mut triaged = Vec::new();
            let mut failures = setup_problems;
            // Names of clients a reply was just drafted for, so the
            // spoken report can say "there's a reply to Jane ready" —
            // built as messages are processed, not recomputed from the
            // outbox afterward, so it only ever names what happened on
            // *this* check.
            let mut fresh_drafts: Vec<String> = Vec::new();
            let mut fresh_sent: Vec<String> = Vec::new();
            let mut order_updates: Vec<String> = Vec::new();
            // Everything fetched, for grouping into conversations afterwards.
            let mut for_threads: Vec<crate::mailthread::Mail> = Vec::new();
            // The domains you deal with — your clients' and your own — which
            // is exactly the list a lookalike sender is built against.
            let mut known_domains: Vec<String> = clients
                .all()
                .iter()
                .filter_map(|c| c.address.rsplit_once('@').map(|x| x.1.to_lowercase()))
                .chain(jobs.iter().filter_map(|(a, _)| a.address.rsplit_once('@').map(|x| x.1.to_lowercase())))
                .collect();
            known_domains.sort();
            known_domains.dedup();
            let mut careful: Vec<String> = Vec::new();
            for (account, password) in &jobs {
                // Between accounts: a pause holds here, nothing half-read.
                if ctl.checkpoint() {
                    break;
                }
                let host = if !account.imap_host.is_empty() {
                    account.imap_host.clone()
                } else {
                    match crate::mail::Provider::from_address(&account.address).imap_host() {
                        Some(h) => h.to_string(),
                        None => {
                            failures.push(format!(
                                "{}: unrecognised provider, needs imap_host set explicitly",
                                account.name
                            ));
                            continue;
                        }
                    }
                };
                // An Outlook/Microsoft 365 account with `oauth` off
                // lands here and fails cleanly at login — Microsoft
                // killed password-based IMAP entirely, and there's no
                // password to fall back to. The server's own error comes
                // back rather than anything guessed at.
                let sent_since = mail_book_on.then(|| {
                    let last = mail_book.checked.get(&account.address).copied().unwrap_or(0);
                    let back = crate::store::now().saturating_sub(last.max(crate::store::now().saturating_sub(look_back_days * 86_400)));
                    crate::triage::imap_date((back / 86_400 + 1) as u32, crate::store::now())
                });
                match connect_and_fetch_inbox(
                    &host,
                    &account.address,
                    password,
                    account.oauth.then_some(account.client_id.as_str()),
                    sent_since.as_deref(),
                ) {
                    Ok((msgs, sent)) => {
                        if mail_book_on {
                            let fetched = crate::store::now();
                            let mut letters: Vec<crate::mailbook::Letter> = msgs.iter().map(|m| crate::mailbook::Letter::from_imap(m, false, fetched)).collect();
                            match sent {
                                Ok(sent) => {
                                    letters.extend(sent.iter().take(300).map(|m| crate::mailbook::Letter::from_imap(m, true, fetched)));
                                    mail_book.checked.insert(account.address.clone(), fetched);
                                }
                                Err(e) => failures.push(format!("{} (sent mail): {e}", account.name)),
                            }
                            mail_book.add(letters, look_back_days.max(keep_days), fetched);
                        }
                        for_threads.extend(msgs.iter().map(|m| crate::mailthread::Mail::from_imap(m)));
                        for m in &msgs {
                            let t: crate::triage::Message = m.into();
                            let triaged_one = crate::triage::triage(&t);
                            let (_, address) = crate::unsub::split_from(&m.from);
                            // A sender that only looks like someone you deal
                            // with, or that its own server says is forged,
                            // is named and gets no draft, however polite.
                            let suspect = crate::lookalike::sender_warning(&m.from, &m.authentication_results, &known_domains);
                            if let Some(w) = &suspect {
                                careful.push(w.clone());
                            }
                            if let Some(status) = crate::orders::status_from_subject(&m.subject) {
                                if let Some(merchant) = crate::orders::merchant_from_address(&address) {
                                    if let Some(order) =
                                        orders.update(&merchant, &m.subject, status, crate::store::now())
                                    {
                                        order_updates
                                            .push(format!("{}: {}", order.merchant, order.status.spoken()));
                                    }
                                }
                            }
                            if triaged_one.draftable
                                && suspect.is_none()
                                && matches!(triaged_one.needs, crate::triage::Needs::Reply)
                                && clients.is_client(&address)
                            {
                                if let Some(llm) = &llm {
                                    if let Some(client) = clients.get(&address) {
                                        match draft_client_reply(
                                            llm.as_ref(),
                                            client.name_or_address(),
                                            &m.subject,
                                            &m.body,
                                        ) {
                                            Ok(body) => {
                                                // Rewrite it, up to max_passes,
                                                // keeping only genuine
                                                // improvements — the revise
                                                // loop that `max_passes` was
                                                // bounding but that nothing
                                                // ever ran.
                                                let body = match crate::draft::revise(
                                                    &body,
                                                    llm.as_ref(),
                                                    &draft_cfg,
                                                ) {
                                                    crate::draft::Outcome::Revised { text, .. } => text,
                                                    _ => body,
                                                };
                                                let notes =
                                                    crate::draft::critique(&body, None, &draft_cfg);
                                                let created = crate::store::now();
                                                let id = crate::outbox::Outbox::make_id(
                                                    &account.name,
                                                    &client.address,
                                                    created,
                                                );
                                                let mut pending = crate::outbox::PendingReply {
                                                    id,
                                                    account: account.name.clone(),
                                                    to_address: client.address.clone(),
                                                    to_name: client.name_or_address().to_string(),
                                                    subject: format!("Re: {}", m.subject),
                                                    body,
                                                    kind: crate::outbox::Kind::Client,
                                                    critique: notes,
                                                    created_at: created,
                                                    status: crate::outbox::Status::Waiting,
                                                };
                                                if may_email_clients {
                                                    match send_reply(
                                                        &pending,
                                                        &account.address,
                                                        password,
                                                        account.oauth.then_some(account.client_id.as_str()),
                                                    ) {
                                                        Ok(()) => {
                                                            pending.status = crate::outbox::Status::Sent;
                                                            fresh_sent.push(pending.to_name.clone());
                                                        }
                                                        Err(e) => failures.push(format!(
                                                            "sending a reply to {address}: {e}"
                                                        )),
                                                    }
                                                } else {
                                                    fresh_drafts.push(pending.spoken_notice());
                                                }
                                                outbox.add(pending);
                                            }
                                            Err(e) => failures
                                                .push(format!("drafting a reply to {address}: {e}")),
                                        }
                                    }
                                }
                            }
                            triaged.push(t);
                        }
                    }
                    Err(e) => failures.push(format!("{}: {e}", account.name)),
                }
            }
            let _ = outbox.save(&store);
            let _ = orders.save(&store);
            if mail_book_on {
                let _ = store.save(crate::mailbook::MailBook::FILE, &mail_book);
            }
            let sorted = crate::triage::sort_all(&triaged);
            let mut said = crate::triage::spoken(&sorted);
            // Said first-thing after the summary, not buried under drafts.
            for w in careful.iter().take(3) {
                said.push_str(&format!(" {w}"));
            }
            // Several new messages that are one conversation are said as one:
            // "the installer thread has 3 new" is the thing to know, not three
            // separate lines that happen to share a subject.
            for (subject, n) in crate::mailthread::conversations(&for_threads).iter().take(3) {
                said.push_str(&format!(" {n} of these are one conversation: \"{subject}\"."));
            }
            // The sentence is the outbox's own, not a second copy of it.
            for notice in &fresh_drafts {
                said.push_str(&format!(" {notice}"));
            }
            for name in &fresh_sent {
                said.push_str(&format!(" Sent a reply to {name}."));
            }
            for update in &order_updates {
                said.push_str(&format!(" Order update — {update}."));
            }
            if !failures.is_empty() {
                said.push_str(&format!(
                    " ({} account{} had trouble: {})",
                    failures.len(),
                    if failures.len() == 1 { "" } else { "s" },
                    failures.join("; ")
                ));
            }
            Ok(said)
        });

        if self.hand_off("mail", now, work, None, SpeakPolicy::Always) {
            "Checking your mail. I'll let you know what's in it.".into()
        } else {
            "I'm swamped with background work right now — ask me to check mail again in a moment."
                .into()
        }
    }

    /// "Clear out my inbox." Unlike `check_mail`'s `UNSEEN` search, this
    /// needs a wider window and *every* message in it, read or not — a
    /// sender's engagement ratio (opened, replied, out of how many) is
    /// only real once it's counted over more than the last few unread
    /// messages. `\Seen`/`\Answered` are the server's own record of what
    /// you did with each one, so one fetch over that window is a real
    /// verdict, not a guess — no separate history for Atlas to keep.
    fn check_unsubscribe(&mut self) -> String {
        let cfg = self.tools_cfg().mail.clone();
        if !cfg.enabled {
            return "Reading your email is switched off.".into();
        }
        if cfg.accounts.is_empty() {
            return "I don't have any mail accounts set up yet.".into();
        }
        if self.connectivity.cached() == Reach::Offline {
            return "I can't look at your mail without a connection.".into();
        }

        let now = crate::store::now();
        let (jobs, setup_problems) = self.resolve_mail_jobs(&cfg, now);
        if jobs.is_empty() {
            return format!("I couldn't get at any of your mail accounts: {}", setup_problems.join("; "));
        }

        let unsub_cfg = self.tools_cfg().unsub.clone();
        const WINDOW_DAYS: u32 = 60;
        let since = crate::triage::imap_date(WINDOW_DAYS, now);

        let work: crew::Work = Box::new(move |ctl| {
            let mut failures = setup_problems;
            let mut per_account_spoken = Vec::new();
            let mut total_unsubscribed = 0usize;
            for (account, password) in &jobs {
                // Between accounts: a pause holds here, nothing half-read.
                if ctl.checkpoint() {
                    break;
                }
                let host = if !account.imap_host.is_empty() {
                    account.imap_host.clone()
                } else {
                    match crate::mail::Provider::from_address(&account.address).imap_host() {
                        Some(h) => h.to_string(),
                        None => {
                            failures.push(format!(
                                "{}: unrecognised provider, needs imap_host set explicitly",
                                account.name
                            ));
                            continue;
                        }
                    }
                };
                let messages = match connect_and_fetch_since(
                    &host,
                    &account.address,
                    password,
                    &since,
                    account.oauth.then_some(account.client_id.as_str()),
                ) {
                    Ok(m) => m,
                    Err(e) => {
                        failures.push(format!("{}: {e}", account.name));
                        continue;
                    }
                };
                let senders = crate::unsub::senders_from(&messages, crate::store::now());
                let cleanup = crate::unsub::plan(&senders, &unsub_cfg);
                // Per account, not combined: a real send has to come from
                // the same mailbox the message arrived at, and knowing
                // that only works while each account's own cleanup plan
                // is still its own.
                if unsub_cfg.bulk_without_asking && !cleanup.unsubscribe.is_empty() {
                    let (done, send_failures) = carry_out_unsubscribes(&cleanup, account, password);
                    total_unsubscribed += done;
                    failures.extend(send_failures);
                } else {
                    per_account_spoken.push(crate::unsub::spoken(&cleanup));
                }
            }
            let mut said = if unsub_cfg.bulk_without_asking {
                format!("Unsubscribed from {total_unsubscribed}.")
            } else if per_account_spoken.iter().all(|s| s.starts_with("Nothing worth")) {
                "Nothing worth clearing out — you read most of what you get.".into()
            } else {
                per_account_spoken.join(" ")
            };
            if !failures.is_empty() {
                said.push_str(&format!(
                    " ({} thing{} had trouble: {})",
                    failures.len(),
                    if failures.len() == 1 { "" } else { "s" },
                    failures.join("; ")
                ));
            }
            Ok(said)
        });

        if self.hand_off("unsubscribe", now, work, None, SpeakPolicy::Always) {
            format!("Looking at the last {WINDOW_DAYS} days of mail to see what's worth clearing out.")
        } else {
            "I'm swamped with background work right now — ask me again in a moment.".into()
        }
    }

    /// "Connect my outlook account me@outlook.com with client id
    /// abc-123." Pulls the address (the word with an `@` in it) and
    /// everything after "client" as the client ID — no fancier parsing
    /// than that, since both are things you'd type or paste exactly
    /// rather than phrase naturally.
    fn connect_outlook_from_request(&mut self, what: &str) -> String {
        let Some(address) = what.split_whitespace().find(|w| w.contains('@')) else {
            return "What's the Outlook address, and what's the client ID from your app \
                     registration? Ask me for outlook setup help if you haven't registered one yet."
                .into();
        };
        let Some(client_pos) = what.find("client ") else {
            return "I have the address, but I need the client ID from your Azure app \
                     registration too."
                .into();
        };
        let client_id = what[client_pos + "client ".len()..].trim();
        if client_id.is_empty() {
            return "I have the address, but the client ID looks empty.".into();
        }
        let address = address.to_string();
        let client_id = client_id.to_string();
        self.connect_outlook(&address, &client_id)
    }

    /// Starts the device code flow: one fast request for a code, an
    /// immediate honest answer (the code and where to enter it), and a
    /// crew errand that polls in the background — for as long as fifteen
    /// minutes, per Microsoft's own `expires_in` — until you've actually
    /// gone and approved it somewhere else. Nothing here drives a
    /// browser; that's the entire reason this flow exists instead of a
    /// redirect-based one.
    fn connect_outlook(&mut self, address: &str, client_id: &str) -> String {
        let dc = match crate::msoauth::request_device_code(client_id) {
            Ok(dc) => dc,
            Err(e) => return format!("Couldn't start connecting that account: {e}"),
        };
        let now = crate::store::now();
        let client_id_owned = client_id.to_string();
        let device_code = dc.device_code.clone();
        let mut wait_secs = dc.interval.max(5);
        let expires_in = dc.expires_in;

        let work: crew::Work = Box::new(move |stop| {
            let started = std::time::Instant::now();
            loop {
                if stop.checkpoint() {
                    return Err("cancelled".to_string());
                }
                if started.elapsed().as_secs() >= expires_in {
                    return Err("the code expired before it was approved".to_string());
                }
                std::thread::sleep(std::time::Duration::from_secs(wait_secs));
                match crate::msoauth::poll_once(&client_id_owned, &device_code) {
                    crate::msoauth::PollOutcome::Ready(tokens) => return Ok(tokens.refresh_token),
                    crate::msoauth::PollOutcome::Pending => {}
                    // The server's own request to poll less often — not a
                    // fixed guess, since a fixed backoff either ignores
                    // this or reinvents it worse.
                    crate::msoauth::PollOutcome::SlowDown => wait_secs += 5,
                    crate::msoauth::PollOutcome::Denied => {
                        return Err("you declined the sign-in".to_string())
                    }
                    crate::msoauth::PollOutcome::Expired => {
                        return Err("the code expired before it was approved".to_string())
                    }
                    crate::msoauth::PollOutcome::Other(e) => return Err(e),
                }
            }
        });

        if self.hand_off("outlook-connect", now, work, Some(address.to_string()), SpeakPolicy::Always) {
            dc.message
        } else {
            "I'm swamped with background work right now — try connecting that account again in a moment."
                .into()
        }
    }

    /// "What's up with my Amazon order" / "where's my cable." Reads the
    /// order store directly, the same reasoning as `read_draft`: local
    /// data, already fetched, never needs the crew.
    fn read_order(&mut self, what: &str) -> String {
        let orders = crate::orders::Orders::load(&self.store);
        match orders.find(what) {
            Some(order) => {
                format!("{} — {}: {}", order.merchant, order.description, order.status.spoken())
            }
            None => "I don't have an order matching that.".into(),
        }
    }

    /// "Pull up the reply to Jane." Reads the outbox directly — this is
    /// always fast (one file, already local), so unlike `check_mail` it
    /// never needs the crew or an acknowledgment first.
    fn read_draft(&mut self, who: &str) -> String {
        if who.is_empty() {
            return "Pull up the draft to who?".into();
        }
        let outbox = crate::outbox::Outbox::load(&self.store);
        match outbox.waiting_for(who) {
            Some(reply) => format!(
                "Reply to {}: \"{}\"",
                reply.to_name,
                reply.body.trim()
            ),
            None => format!("I don't have a draft waiting for {who}."),
        }
    }

    /// "Throw away the reply to Jane." / "Scrap the draft to Jane." You
    /// looked at a held draft and said no, so it is marked `Discarded` and
    /// leaves `waiting()`/`waiting_for()`. Without this there was no way to
    /// reject a held draft: it stayed `Waiting` for ever, so "pull up the
    /// reply to Jane" kept surfacing the thing you'd already turned down and
    /// the count of waiting drafts never came down. `read_draft` let you look
    /// at one; this is how you say no to one.
    fn discard_draft(&mut self, who: &str) -> String {
        if who.is_empty() {
            return "Throw away the draft to who?".into();
        }
        let mut outbox = crate::outbox::Outbox::load(&self.store);
        let Some(id) = outbox.waiting_for(who).map(|r| r.id.clone()) else {
            return format!("I don't have a draft waiting for {who}.");
        };
        let to_name = outbox.get(&id).map(|r| r.to_name.clone()).unwrap_or_else(|| who.to_string());
        outbox.mark_discarded(&id);
        if let Err(e) = outbox.save(&self.store) {
            return format!("I couldn't set that draft aside: {e}");
        }
        format!("Thrown away — the reply to {to_name} won't go out.")
    }

    /// "Draft outreach to brand@example.com about a partnership." Pulls
    /// the recipient out of the request; everything after "about" is the
    /// purpose, handed to the model as-is rather than parsed further.
    fn draft_outreach_from_request(&mut self, what: &str) -> String {
        let Some(rest) = what.split("to ").nth(1) else {
            return "Outreach to whom?".into();
        };
        let (address, purpose) = match rest.split_once("about ") {
            Some((a, p)) => (a.trim(), p.trim().to_string()),
            None => (rest.trim(), "introducing yourself and exploring working together".to_string()),
        };
        if address.is_empty() {
            return "Outreach to whom?".into();
        }
        self.draft_outreach(address, &purpose)
    }

    /// The cold-outreach half of the drafting feature. Always goes to the
    /// crew — the model call is the same class of slow as `research`'s —
    /// and, unlike a client reply, has nowhere reactive to hang off: this
    /// only ever runs because you asked for it by name.
    fn draft_outreach(&mut self, to_address: &str, purpose: &str) -> String {
        let cfg = self.tools_cfg().mail.clone();
        if !cfg.enabled {
            return "Reading your email is switched off.".into();
        }
        let Some(account) = cfg.accounts.first().cloned() else {
            return "I don't have any mail accounts set up yet.".into();
        };
        let Some(llm) = self.llm.clone() else {
            return "I need a model to draft that, and I haven't got one configured.".into();
        };
        let now = crate::store::now();
        let vault_name = match crate::mail::credential_source(&account) {
            Ok(n) => n.to_string(),
            Err(e) => return format!("Can't draft from {}: {e}", account.name),
        };
        let password = match self.vault.get(&vault_name, now) {
            Ok(p) => p,
            Err(e) => return format!("Can't draft from {}: {e}", account.name),
        };

        let store = self.store.clone();
        let draft_cfg = self.tools_cfg().draft.clone();
        let may_email_brands = cfg.may_email_brands;
        let daily_cap = cfg.cold_outreach_daily_cap;
        let to_address = to_address.to_string();
        let purpose = purpose.to_string();
        let to_address_for_ack = to_address.clone();

        let work: crew::Work = Box::new(move |ctl| {
            if ctl.checkpoint() {
                return Err("stopped before drafting".into());
            }
            let system = "You draft a short, professional cold outreach email on behalf of the \
                          person you work for, to someone who has never heard from them before. \
                          Write only the reply body -- no subject line, no signature, no \
                          placeholder brackets. Keep it brief and respectful of their time.";
            let user = format!("Write an outreach email to {to_address} about: {purpose}");
            let body = llm.complete(system, &user).map_err(|e| e.to_string())?;
            let notes = crate::draft::critique(&body, None, &draft_cfg);
            let created = crate::store::now();
            let id = crate::outbox::Outbox::make_id(&account.name, &to_address, created);
            let mut pending = crate::outbox::PendingReply {
                id,
                account: account.name.clone(),
                to_address: to_address.clone(),
                to_name: to_address.clone(),
                subject: "Introduction".into(),
                body,
                kind: crate::outbox::Kind::ColdOutreach,
                critique: notes,
                created_at: created,
                status: crate::outbox::Status::Waiting,
            };

            let mut outbox = crate::outbox::Outbox::load(&store);
            let targets = crate::outreach::OutreachTargets::load(&store);
            let today_start = crate::localclock::midnight(created, crate::localclock::offset_secs());
            let sent_today = outbox.cold_outreach_sent_since(today_start);

            let said = if !may_email_brands {
                format!("There's an outreach draft to {to_address} ready to look at.")
            } else if !targets.is_approved(&to_address) {
                format!(
                    "There's an outreach draft to {to_address} ready, but that recipient isn't \
                     on your approved outreach list yet."
                )
            } else if sent_today >= daily_cap as usize {
                format!(
                    "There's an outreach draft to {to_address} ready, but today's outreach cap \
                     ({daily_cap}) is already reached."
                )
            } else {
                match send_reply(
                    &pending,
                    &account.address,
                    &password,
                    account.oauth.then_some(account.client_id.as_str()),
                ) {
                    Ok(()) => {
                        pending.status = crate::outbox::Status::Sent;
                        format!("Sent the outreach to {to_address}.")
                    }
                    Err(e) => format!("Tried to send the outreach to {to_address}, but: {e}"),
                }
            };
            outbox.add(pending);
            let _ = outbox.save(&store);
            Ok(said)
        });

        if self.hand_off("outreach", now, work, None, SpeakPolicy::Always) {
            format!("Drafting outreach to {to_address_for_ack}. I'll let you know when it's ready.")
        } else {
            "I'm swamped with background work right now — ask me again in a moment.".into()
        }
    }

    /// A Cloudflare worker to delegate to, when the machine is online and the
    /// provider is set up — otherwise `None` and the local model does the
    /// work, exactly as before. The token is fetched here, on the tick
    /// thread, and baked into the returned worker's vars: a crew errand runs
    /// on another thread and cannot unlock the vault, so the secret has to be
    /// pulled before the errand is built (the same rule mail follows for its
    /// IMAP password). Returns the ready worker as an `Arc<dyn Llm>` so it can
    /// be moved into an errand.
    fn cloudflare_worker(
        &mut self,
    ) -> Option<(std::sync::Arc<dyn crate::brain::Llm>, crate::tools::Vars)> {
        let cfg = self.tools_cfg().cloudflare.clone();
        if !cfg.enabled || self.connectivity.cached() != Reach::Online {
            return None;
        }
        let inference = cfg.inference.clone()?;
        let now = crate::store::now();
        // The token, tick-side. If the vault is locked or the entry is
        // missing, the provider is not ready and the local path is used —
        // `readiness` would say the same, and this is the one place that can
        // actually try the vault.
        let token = self.vault.get(&cfg.token_vault, now).ok()?;
        if !crate::online::readiness(&cfg, true).ready_now() {
            return None;
        }
        // The worker's vars: the shared vars, plus the account id and token
        // the Cloudflare request template expects as `{account_id}`/`{token}`.
        // Returned alongside the worker so a delegated page fetch (Browser
        // Rendering) can reuse the same secret without unlocking the vault
        // again on another thread.
        let mut vars = self.tools_ref().map(|t| t.vars.clone()).unwrap_or_default();
        vars.insert("account_id".into(), cfg.account_id.clone());
        vars.insert("token".into(), token);
        vars.insert("model".into(), cfg.model.clone());
        Some((std::sync::Arc::new(crate::brain::ShellLlm { cfg: inference, vars: vars.clone() }), vars))
    }

    /// Build code from a description, check it against the real toolchain, and
    /// hand over what passes.
    ///
    /// The generating model is chosen the same way research chooses its
    /// summariser: a Cloudflare worker when the machine is online and the
    /// provider is set up, the local model otherwise — you never have to say
    /// "do it online", Atlas delegates the drafting to free itself up when it
    /// can and falls back to local when it can't. Either way the *check* is
    /// local: the draft is written into a throwaway sandbox and run through
    /// `craft`'s ladder (compile, lint, test) on this machine, because a
    /// model's confidence is worth nothing and the compiler's verdict is worth
    /// everything. The whole thing runs as a crew errand so the turn is not
    /// blocked while cargo grinds.
    fn build_from_description(&mut self, what: &str) -> String {
        let cfg = self.tools_cfg().build.clone();
        if !cfg.enabled {
            return "Building code is switched off in your settings.".into();
        }
        let what = what.trim();
        if what.is_empty() {
            return "Tell me what to build — \"write me a script that renames files by date\", say."
                .into();
        }
        // Local model, or a worker when online and set up. This is the
        // auto-delegation: no explicit "online" needed.
        let worker = self.cloudflare_worker();
        let gen_llm: std::sync::Arc<dyn crate::brain::Llm> = match &worker {
            Some((w, _)) => w.clone(),
            None => match self.llm.clone() {
                Some(l) => l,
                None => {
                    return "I can write code, but I need a model to draft it and I haven't got \
                            one configured."
                        .into()
                }
            },
        };
        let delegated = worker.is_some();
        let max_rounds = cfg.max_fix_rounds;
        let out_dir = crate::roots::data_sub("builds");

        // A web page goes through the taste gate instead of the compiler: draft
        // the HTML, review it against the house style, and iterate until it
        // clears the blocking floors or runs out of budget. The review is the
        // reliable half — it can't say the design is good, only that it's
        // consistent and accessible, and the reply says exactly that.
        if crate::taste::wants_web_page(what) {
            let rules = self.tools_cfg().taste.clone();
            let brief = what.to_string();
            let out_dir = out_dir.clone();
            let work: crew::Work = Box::new(move |ctl| {
                let outcome = crate::taste::build_web(&brief, gen_llm.as_ref(), max_rounds, |html| {
                    // Between rounds: a pause holds with the draft so far intact.
                    let _ = ctl.checkpoint();
                    crate::taste::review(html, &rules)
                });
                let mut said = outcome.spoken();
                if let Some(html) = outcome.html() {
                    let _ = std::fs::create_dir_all(&out_dir);
                    let built = matches!(outcome, crate::taste::Outcome::Built { .. });
                    let name = if built { "page.reviewed.html" } else { "page.draft.html" };
                    let path = out_dir.join(name);
                    // Said only if it's true. A full disk used to fail here
                    // silently and still announce where the page was.
                    match std::fs::write(&path, html) {
                        Ok(()) => said.push_str(&format!("\n\nSaved to {}.", path.display())),
                        Err(e) => said.push_str(&format!("\n\nI couldn't save it to {} ({e}) — is the disk full?", path.display())),
                    }
                }
                Ok(said)
            });
            let taken = self.hand_off("build", crate::store::now(), work, Some(what.to_string()), SpeakPolicy::Always);
            return if taken {
                if delegated {
                    "On it — a worker online will draft the page and I'll review it against the house \
                     style here before I show you.".into()
                } else {
                    "On it — I'll draft the page, review it against the house style, and iterate until \
                     it's consistent and accessible.".into()
                }
            } else {
                "I'm swamped with background work right now — ask me to build it again in a moment.".into()
            };
        }

        let lang = crate::build_it::lang_from_words(what, cfg.default_language);
        let desc = what.to_string();
        let base = crate::roots::tmp_dir().join("builds");

        let work: crew::Work = Box::new(move |ctl| {
            let mut sandbox = match crate::sandbox::Sandbox::create(&base, "build") {
                Ok(s) => s,
                Err(e) => return Err(format!("couldn't make a sandbox to build in: {e}")),
            };
            // The checker: scaffold the draft into the sandbox and run the
            // ladder. Injected into `build_loop` so the loop logic is testable
            // without a toolchain; here it is the real compiler.
            let mut check = |code: &str| -> crate::build_it::Check {
                // Between rounds: a pause holds with the draft so far intact.
                let _ = ctl.checkpoint();
                check_draft_in_sandbox(&mut sandbox, lang, code)
            };
            let outcome = crate::build_it::build_loop(&desc, lang, gen_llm.as_ref(), max_rounds, &mut check);
            // The sandbox has done its job; left behind, one piled up per build.
            drop(check);
            let _ = sandbox.discard();
            // A build that ran out of tries is kept, so "keep at it" carries
            // on from its best draft (E3).
            let mut offer_more = false;
            if let crate::build_it::Outcome::Struggled { code, last_failure, .. } = &outcome {
                let s = crate::build_it::Struggle {
                    description: desc.clone(),
                    lang,
                    code: code.clone(),
                    failure: last_failure.clone(),
                };
                let path = crate::build_it::Struggle::path();
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                offer_more = serde_json::to_string(&s).ok().map(|j| std::fs::write(&path, j).is_ok()).unwrap_or(false);
            }
            // Verified code is written where you can pick it up; a struggle
            // still leaves its best draft there, clearly named.
            let mut said = outcome.spoken(lang);
            if let Some(code) = outcome.code() {
                let _ = std::fs::create_dir_all(&out_dir);
                let name = if outcome.is_built() { "build.verified" } else { "build.draft" };
                let path = out_dir.join(format!("{name}.{}", ext_for(lang)));
                if let Err(e) = std::fs::write(&path, code) {
                    said.push_str(&format!("\n\n(I couldn't save it to {} — {e}. Is the disk full?)", path.display()));
                }
                // Auto-explain: generated code never arrives without a plain-
                // English summary of what it does, iterated to read plainly.
                if let Some(plain) = crate::explain::in_plain_english(code, gen_llm.as_ref(), max_rounds) {
                    said.push_str(&format!("\n\nIn plain English: {plain}"));
                }
            }
            if offer_more {
                said.push_str("\n\nSay \"keep at it\" and I'll carry on from this draft on my own until it passes.");
            }
            Ok(said)
        });

        let taken = self.hand_off("build", crate::store::now(), work, Some(what.to_string()), SpeakPolicy::Always);
        if taken {
            if delegated {
                "On it — I've handed the drafting to a worker online and I'll check what comes back \
                 against the compiler here before I show you."
                    .into()
            } else {
                "On it — I'll write it, check it against the compiler and tests, and tell you how it went."
                    .into()
            }
        } else {
            "I'm swamped with background work right now — ask me to build it again in a moment.".into()
        }
    }

    /// Work on one of your projects: scope it, build it (itself or delegated),
    /// check it, and file a proposed change into that project's queue for you
    /// to implement when you're ready. Nothing touches the project's real
    /// files here — that only happens on `implement`.
    fn improve_project(&mut self, what: &str) -> String {
        let cfg = self.tools_cfg().build.clone();
        if !cfg.enabled {
            return "Building is switched off in your settings.".into();
        }
        let what = what.trim();
        if what.is_empty() {
            return "Tell me the project and what to change — \"on the Atlas project, add a date parser\"."
                .into();
        }
        // Which project is this for? A registered name mentioned anywhere in
        // the request wins; otherwise a "project X"/"on X"/"in X" phrase names
        // a new one.
        let project = match self.detect_project(what) {
            Some(p) => p,
            None => {
                return "Which project is this for? Name it — \"on the Atlas project, …\" — and I'll \
                        queue the change there."
                    .into()
            }
        };
        let worker = self.cloudflare_worker();
        let gen_llm: std::sync::Arc<dyn crate::brain::Llm> = match &worker {
            Some((w, _)) => w.clone(),
            None => match self.llm.clone() {
                Some(l) => l,
                None => return "I can do that, but I need a model to build it and none is configured.".into(),
            },
        };
        let delegated = worker.is_some();
        let lang = crate::build_it::lang_from_words(what, cfg.default_language);
        let max_rounds = cfg.max_fix_rounds;
        let desc = what.to_string();
        let title = workshop_title(what);
        let project_name = project.clone();
        // Copies for the acknowledgement, since the originals move into the
        // errand closure below.
        let title_ack = title.clone();
        let project_ack = project_name.clone();
        // Any context Atlas can read from the project folder now, so the draft
        // is written against what's actually there. Read on the tick thread;
        // the errand runs elsewhere and gets the text, not the folder.
        let folder = self.workshop.resolve(&project).map(|p| p.folder.clone()).unwrap_or_default();
        let context = read_project_context(&folder);
        // What the files this could write look like now, as the model is
        // about to read them — taken here, at the start, not when the change
        // lands, so an edit you make while it's being written still counts
        // as the code moving on.
        let start_bases: Vec<(String, String)> = {
            let mut paths = vec![format!("proposed_change.{}", ext_for(lang))];
            if let Some(t) = named_existing_file(what, std::path::Path::new(&folder)) {
                paths.push(t);
            }
            crate::workshop::bases_in(&folder, &paths)
        };
        let base = crate::roots::tmp_dir().join("improve");
        // Each phase written down as it finishes (`phases`): asked again, or
        // redone after a restart (`resume`), the work carries on after the
        // last finished phase instead of starting over.
        let phases = crate::phases::Phases::for_work(self.store.root(), "improve", &format!("{project}\n{what}"));
        let carrying_on = {
            let done = phases.finished_phases();
            let named: Vec<&str> = done
                .iter()
                .filter_map(|p| match p.as_str() {
                    "1-draft" => Some("the draft"),
                    "2-checked" => Some("checking it"),
                    "3-explained" => Some("explaining it"),
                    _ => None,
                })
                .collect();
            if named.is_empty() {
                String::new()
            } else {
                format!(" Carrying on from where it stopped — {} already done.", named.join(", "))
            }
        };

        let work: crew::Work = Box::new(move |ctl| {
            let prompt = if context.is_empty() {
                desc.clone()
            } else {
                format!("{desc}\n\nHere is some of the existing project for context:\n{context}")
            };
            let mut sandbox = match crate::sandbox::Sandbox::create(&base, "improve") {
                Ok(s) => s,
                Err(e) => return Err(format!("couldn't make a sandbox to work in: {e}")),
            };
            let mut check = |code: &str| -> crate::build_it::Check {
                // Between rounds: a pause holds with the draft so far intact.
                let _ = ctl.checkpoint();
                check_draft_in_sandbox(&mut sandbox, lang, code)
            };
            let outcome = match phases.done::<crate::build_it::Outcome>("1-draft") {
                Some(o) => o,
                None => {
                    let o = crate::build_it::build_loop(&prompt, lang, gen_llm.as_ref(), max_rounds, &mut check);
                    if o.code().is_some() {
                        phases.finished("1-draft", &o);
                    }
                    o
                }
            };
            // The sandbox has done its job; left behind, one piled up per change.
            drop(check);
            let _ = sandbox.discard();
            if ctl.checkpoint() {
                return Err("you asked me to stop".into());
            }
            let Some(code) = outcome.code().map(str::to_string) else {
                return Err(match &outcome {
                    crate::build_it::Outcome::NoDraft(w) => w.clone(),
                    _ => "couldn't build that".into(),
                });
            };
            // Verify as strongly as the project allows: isolated is where
            // `build_loop` stops; if the request names an existing file in a
            // buildable project, prove it *inside a copy of the project*
            // instead. The queued change carries whichever was actually done.
            let (verified, mut summary, files) = match phases.done::<(bool, String, Vec<crate::workshop::FileEdit>)>("2-checked") {
                Some(v) => v,
                None => {
                    let v = verify_project_change(&project_name, &folder, &title, lang, &desc, &code, &outcome, &base);
                    phases.finished("2-checked", &v);
                    v
                }
            };
            if ctl.checkpoint() {
                return Err("you asked me to stop".into());
            }
            // Auto-explain: the queued change is described in plain English, so
            // you know what it does before deciding whether to implement it.
            let plain = match phases.done::<Option<String>>("3-explained") {
                Some(p) => p,
                None => {
                    let p = crate::explain::in_plain_english(&code, gen_llm.as_ref(), max_rounds);
                    phases.finished("3-explained", &p);
                    p
                }
            };
            if let Some(plain) = plain {
                summary.push_str(&format!("\n\nIn plain English: {plain}"));
            }

            let bases: Vec<(String, String)> = start_bases
                .iter()
                .filter(|(p, _)| files.iter().any(|f| &f.path == p))
                .cloned()
                .collect();
            let envelope = ImproveOutcome {
                project: project_name.clone(),
                title: title.clone(),
                what: desc.clone(),
                files,
                verified,
                note: summary.clone(),
                summary,
                bases,
            };
            serde_json::to_string(&envelope).map_err(|e| format!("couldn't package the change: {e}"))
        });

        let taken = self.hand_off("improve", crate::store::now(), work, Some(project), SpeakPolicy::Always);
        if taken {
            if delegated {
                format!(
                    "On it — scoping \"{title_ack}\" for {project_ack}, handing the drafting to a worker \
                     online, and checking it here before it hits your queue.{carrying_on}"
                )
            } else {
                format!(
                    "On it — scoping \"{title_ack}\" for {project_ack}, building and checking it, then \
                     it'll land in that project's queue for your go-ahead.{carrying_on}"
                )
            }
        } else {
            "I'm swamped with background work right now — ask me again in a moment.".into()
        }
    }

    // (verify_project_change is a free function below — it needs no `self`.)

    /// Apply a change you reviewed and named. Writes its files into the
    /// project folder (keeping a backup so it can be undone), marks it
    /// implemented, and files a fresh master document for the project.
    pub(crate) fn implement_change(&mut self, what: &str) -> String {
        let what = what.trim();
        if what.is_empty() {
            return "Which change? Say \"implement <title>\" — the title I gave it when I queued it.".into();
        }
        let plan = match self.workshop.plan_implementation(what) {
            Ok(p) => p,
            Err(crate::workshop::ImplementError::NoMatch) => {
                return format!(
                    "I don't have a change ready under \"{what}\". Ask me what's in the queue if you're \
                     not sure of the title."
                )
            }
            Err(crate::workshop::ImplementError::NoFolder(name)) => {
                return format!(
                    "That change is ready, but I don't know where the {name} project lives. Tell me its \
                     folder and I'll apply it."
                )
            }
            Err(crate::workshop::ImplementError::Outdated { title, files }) => {
                return format!(
                    "I didn't apply \"{title}\": {} changed since I wrote it, so it was written against \
                     code that isn't there any more and would overwrite the newer version. Ask me to \
                     redo it against the current code — nothing was written.",
                    files.join(", ")
                )
            }
        };
        let root = std::path::Path::new(&plan.folder);
        if !root.is_dir() {
            return format!(
                "That change is ready, but the {} folder ({}) isn't reachable from here.",
                plan.project, plan.folder
            );
        }
        // Write each file, keeping a .before backup of anything replaced so
        // the whole thing is reversible.
        let now = crate::store::now();
        let mut written = 0usize;
        let mut failures = Vec::new();
        for f in &plan.files {
            let target = root.join(&f.path);
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            // No backup, no overwrite: the .before copy is what makes this
            // reversible, and writing over the original after the backup
            // failed (a full disk) made it not.
            if target.exists() {
                if let Ok(prev) = std::fs::read(&target) {
                    if let Err(e) = std::fs::write(target.with_extension("before"), prev) {
                        failures.push(format!("{}: left as it was — I couldn't keep a backup first ({e})", f.path));
                        continue;
                    }
                }
            }
            match std::fs::write(&target, &f.content) {
                Ok(()) => written += 1,
                Err(e) => failures.push(format!("{}: {e}", f.path)),
            }
        }
        if written > 0 {
            self.workshop.mark_implemented(&plan.project, plan.change_id, now);
            // A fresh master document for the project, filed where you can find
            // it — a record of what the project now contains and what just went
            // in.
            let doc = self.write_project_master_doc(&plan.project);
            let _ = self.workshop.save(&self.store);
            self.history.note(
                &format!("implemented \"{}\" on {}", plan.title, plan.project),
                "code",
                crate::undo::Undo::You(format!(
                    "the previous versions are saved as .before files in {}",
                    plan.folder
                )),
                true,
                now,
            );
            let mut msg = format!(
                "Implemented \"{}\" on {} — wrote {written} file{}.",
                plan.title,
                plan.project,
                if written == 1 { "" } else { "s" }
            );
            if let Some(d) = doc {
                msg.push_str(&format!(" Filed a fresh master document at {d}."));
            }
            if !failures.is_empty() {
                msg.push_str(&format!(" Couldn't write: {}.", failures.join(", ")));
            }
            msg
        } else {
            format!("Couldn't apply \"{}\": {}.", plan.title, failures.join(", "))
        }
    }

    /// Put something on your calendar. Reads the time from your words; if it
    /// can't, it asks rather than guessing. Notes any clash, but still files it
    /// — a clash is yours to sort out, not a reason to refuse.
    fn schedule_event(&mut self, what: &str) -> String {
        let cfg = self.calendar_cfg();
        if !cfg.enabled {
            return "The calendar is switched off in your settings.".into();
        }
        let what = what.trim();
        if what.is_empty() {
            return "What should I put on, and when? — \"schedule lunch tomorrow at 12\".".into();
        }
        // The turn's time, as the agenda reads it (`now_acting`): scheduled at
        // one time and read back at another, "tomorrow" was two different days.
        let now = self.now_acting();
        let zone = self.home_zone();
        // Does it repeat? "every weekday", "every Monday", "daily" — read the
        // same way the time is, and never guessed. Read before the time so a
        // repeat named without an explicit day ("standup every weekday at 9")
        // can start today rather than being turned away for want of a date.
        let repeat = crate::calendar::repeat_from(what);
        // Read on your clock, stored in UTC.
        let lnow = zone.to_local(now as i64).max(0) as u64;
        let Some(when) = crate::calendar::resolve_recurring_when(what, lnow, &repeat).map(|w| {
            let start = zone.to_utc(w.start as i64).max(0) as u64;
            crate::calendar::When { start, end: start + (w.end - w.start), all_day: w.all_day }
        }) else {
            return format!(
                "I've got \"{what}\" but not when. Give me a day and a time — \"tomorrow at 3pm\", \
                 \"Monday at 9\" — and I'll put it on."
            );
        };
        // The title is the request with the time words trimmed off the ends,
        // so "schedule lunch tomorrow at 12" files as "lunch".
        let title = crate::calendar::event_title(what);
        // Expanded so a booking landing on a recurring slot — next Tuesday's
        // standup — is caught, not just one whose single stored time overlaps.
        let clashes = self.calendar.clashes_expanded(when.start, when.end);
        let clash_note = match clashes.first() {
            Some(c) if !when.all_day => {
                format!(" Heads up — it runs into \"{}\" ({}).", c.title, c.say_when_in(&zone))
            }
            _ => String::new(),
        };
        // Which side of the firewall this belongs on: an event that names a
        // business you have is filed there, everything else is yours. The
        // roster is the list of businesses you actually have, so a plain
        // "lunch with Sam" is never mistaken for one.
        let roster = crate::roster::Roster::load(&self.store);
        let space = crate::calendar::space_for_request(what, &roster.businesses());
        let side = match &space {
            crate::earned::Space::Business(b) => format!(" (on {b})"),
            crate::earned::Space::Personal => String::new(),
        };
        // Meeting, or time reserved for yourself — "block off two hours for the
        // Q3 summary" is the latter. Both are booked time and still clash; they
        // just read differently.
        let kind = crate::calendar::kind_for_request(what);
        let lead = match kind {
            crate::calendar::EventKind::TimeBlock => "Blocked off",
            crate::calendar::EventKind::Meeting => "On the calendar",
        };
        let id = self.calendar.add_full(&title, when, None, space, kind, repeat, now);
        self.calendar.keep_wall_clock(id, &zone);
        // A reminder, if you asked for one — "remind me 10 minutes before".
        let remind = crate::calendar::reminder_from(what);
        if remind.is_some() {
            self.calendar.set_reminder(id, remind);
        }
        let _ = self.calendar.save(&self.store);
        let ev = self.calendar.event(id);
        let whenn = ev.map(|e| e.say_when_in(&zone)).unwrap_or_default();
        self.history.note(
            &format!("scheduled \"{title}\" for {whenn}"),
            "calendar",
            crate::undo::Undo::You("say \"cancel\" that and I'll take it off".into()),
            false,
            now,
        );
        let remind_note = match remind {
            Some(m) if m % 60 == 0 && m >= 60 => {
                let h = m / 60;
                format!(" I'll remind you {h} hour{} before.", if h == 1 { "" } else { "s" })
            }
            Some(m) => format!(" I'll remind you {m} minutes before."),
            None => String::new(),
        };
        format!("{lead}: \"{title}\"{side}, {whenn}.{clash_note}{remind_note}")
    }

    /// Read your calendar back — a window of days, soonest first.
    fn read_agenda(&self, what: &str) -> String {
        let cfg = self.calendar_cfg();
        if !cfg.enabled {
            return "The calendar is switched off in your settings.".into();
        }
        // The turn's time, not the wall clock's (28 Sep 2026): "what's on
        // today" asked at 23:59 and answered at 00:00 read the wrong day.
        let now = self.now_acting();
        let low = what.to_lowercase();
        // "what's on for Northwind" narrows to one side of the firewall; the
        // same classifier the scheduler uses, so a business you have is
        // recognised and a plain "what's on this week" shows everything.
        let roster = crate::roster::Roster::load(&self.store);
        let only = crate::calendar::space_for_request(what, &roster.businesses());
        let business = matches!(only, crate::earned::Space::Business(_));
        let days = if low.contains("week") { 7 } else { cfg.horizon_days };
        // The window to read, then expand recurring events across it so a
        // standup shows on each day it's on, not as one row saying "every
        // weekday". `occurrences_between` returns owned, dated occurrences.
        // "Today" is your day: midnight on your clock, as a real moment. It
        // was UTC's day here, so after 5 pm in Pacific time "what's on today"
        // showed tomorrow (both chats found this; merged 26 Sep).
        let zone = self.home_zone();
        let lnow = zone.to_local(now as i64).max(0) as u64;
        // Midnight to midnight on your clock, each converted on its own: a
        // day the clocks change on is 23 or 25 hours, not 24 (28 Sep 2026).
        let local_day = |start_local: u64| {
            let s = zone.to_utc(start_local as i64).max(0) as u64;
            let e = zone.to_utc((start_local + 86_400) as i64).max(0) as u64;
            (s, e)
        };
        let (from, to) = if low.contains("today") || low.contains("tonight") {
            local_day(crate::calendar::start_of_day(lnow))
        } else if low.contains("tomorrow") {
            local_day(crate::calendar::start_of_day(crate::calendar::start_of_day(lnow) + 86_400))
        } else {
            (now, now + days as u64 * 86_400)
        };
        let mut events = self.calendar.occurrences_between(from, to);
        // Narrow to one side of the firewall when a business was named.
        if business {
            events.retain(|e| e.space == only);
        }
        if events.is_empty() {
            let scope = match &only {
                crate::earned::Space::Business(b) => format!(" for {b}"),
                crate::earned::Space::Personal => String::new(),
            };
            return format!("Nothing on your calendar{scope} for that stretch.");
        }
        let mut lines: Vec<String> =
            // `say_when` is on your clock (`localclock::zone`, the same home
            // zone as `home_zone`).
            events.iter().take(12).map(|e| format!("{} — {}", e.say_when(), e.title)).collect();
        let head = match events.len() {
            1 => "One thing:".to_string(),
            n => format!("{n} things:"),
        };
        lines.insert(0, head);
        lines.join("\n")
    }

    fn calendar_cfg(&self) -> crate::calendar::CalendarConfig {
        self.tools_ref().map(|t| t.calendar.clone()).unwrap_or_default()
    }

    /// Your calendar as busy slots, for judging a proposed time against.
    fn busy_slots(&self, from: u64, to: u64) -> Vec<crate::booking::Slot> {
        self.calendar
            .occurrences_between(from, to)
            .into_iter()
            .filter(|e| !e.all_day)
            .map(|e| crate::booking::Slot {
                start: e.start,
                mins: (e.end.saturating_sub(e.start) / 60) as u32,
            })
            .collect()
    }

    /// Read a proposal out of a request: who, what it's about, and the times
    /// they offered. `None` if it doesn't name a proposal or no time parses —
    /// Atlas asks rather than inventing one.
    fn parse_proposal(&self, text: &str, now: u64) -> Option<crate::booking::Proposal> {
        // ASCII-only lowering keeps every byte where it was, so an index found
        // in `low` cuts `text` at the same place. Full lowering changes lengths
        // ("İ" becomes two characters) and a cut could land inside one.
        let low = text.to_ascii_lowercase();
        const TRIGGERS: &[&str] = &[
            "proposed", "proposes", "wants a time", "wants to meet", "suggested",
            "asked for a time", "asked to meet", "offered a time", "log a proposal",
        ];
        let trigger = TRIGGERS.iter().find(|t| low.contains(**t))?;
        let t_idx = low.find(trigger)?;

        // Who: "from X" wins (the natural way to name them when the sentence
        // starts with a fixed phrase — "log a proposal from Sam …"); else the
        // words before the trigger ("Sam proposed …"); else "someone".
        let from = if let Some(i) = low.find("from ") {
            const STOP: &[&str] = &[
                "tomorrow", "today", "tonight", "monday", "tuesday", "wednesday", "thursday",
                "friday", "saturday", "sunday", "at", "next", "this", "for", "about",
            ];
            let name: Vec<&str> = text[i + 5..]
                .split_whitespace()
                .take_while(|w| {
                    let w = w.trim_matches(|c: char| !c.is_alphanumeric());
                    !w.is_empty()
                        && w.chars().all(|c| c.is_alphabetic())
                        && !STOP.contains(&w.to_lowercase().as_str())
                })
                .take(2)
                .collect();
            if name.is_empty() { "someone".to_string() } else { name.join(" ") }
        } else {
            let head = text[..t_idx].trim().trim_end_matches(',').trim();
            if head.is_empty() || head.eq_ignore_ascii_case("someone") {
                "someone".to_string()
            } else {
                head.to_string()
            }
        };

        // What it's about: after "for the"/"about the"/"for a"/"about", trimmed.
        let about = ["for the ", "about the ", "for a ", "for ", "about "]
            .iter()
            .find_map(|k| low.rfind(k).map(|i| text[i + k.len()..].trim().trim_end_matches('.').to_string()))
            .filter(|s| !s.is_empty() && s.split_whitespace().count() <= 6);

        // The times they offered: split the request on "or"/"and", read a time
        // from each chunk, keep the ones that resolve.
        let after = &text[t_idx..];
        let mut times = Vec::new();
        // Offered times are read on your clock — the one the offer was made
        // to — and kept in UTC like everything else.
        let zone = self.home_zone();
        let lnow = zone.to_local(now as i64).max(0) as u64;
        for chunk in after.split(" or ").flat_map(|c| c.split(" and ")) {
            if let Some(w) = crate::calendar::resolve_when(chunk, lnow) {
                if !w.all_day {
                    let mins = ((w.end.saturating_sub(w.start)) / 60) as u32;
                    times.push(crate::booking::Slot { start: zone.to_utc(w.start as i64).max(0) as u64, mins });
                }
            }
        }
        if times.is_empty() {
            return None;
        }
        Some(crate::booking::Proposal {
            id: now,
            from,
            about,
            their_words: text.to_string(),
            times,
            at: now,
            state: crate::booking::State::NeedsYou,
        })
    }

    /// A time someone proposed, or your answer to one. Atlas does the tedious
    /// half — reads it, checks it against your calendar, lays out what fits —
    /// and stops. Only your explicit accept writes it to your calendar, and
    /// nothing is ever sent on your behalf.
    fn booking(&mut self, what: &str) -> String {
        let cfg = self.tools_cfg().booking.clone();
        if !cfg.enabled {
            return "Working through times other people propose is switched off in your settings.".into();
        }
        let arg = what.trim();
        let now = self.now_acting();

        // An answer to a proposal already waiting on you?
        if let Some(state) = crate::booking::answered(arg) {
            if let Some(i) = self.proposals.iter().position(|p| p.state == crate::booking::State::NeedsYou) {
                return self.answer_proposal(i, state, now, &cfg);
            }
            // "yes"/"no" with nothing pending: fall through to the guidance below.
        }

        // A new proposal to log?
        if let Some(p) = self.parse_proposal(arg, now) {
            let busy = self.busy_slots(now, now + 30 * 86_400);
            let assessed = crate::booking::assess(&p, &busy, now, &cfg, &self.home_zone());
            let mins = p.times.first().map(|s| s.mins).unwrap_or(30);
            let alternatives = crate::booking::could_offer(&busy, now, mins, &cfg, 3, &self.home_zone());
            let line = crate::booking::to_decide(&p, &assessed, &alternatives);
            self.proposals.push(p);
            let _ = self.store.save("proposals", &self.proposals);
            return line;
        }

        // Nothing to log and nothing answered — re-present what's waiting, or say
        // there's nothing.
        match self.proposals.iter().find(|p| p.state == crate::booking::State::NeedsYou).cloned() {
            Some(p) => {
                let busy = self.busy_slots(now, now + 30 * 86_400);
                let assessed = crate::booking::assess(&p, &busy, now, &cfg, &self.home_zone());
                let mins = p.times.first().map(|s| s.mins).unwrap_or(30);
                let alternatives = crate::booking::could_offer(&busy, now, mins, &cfg, 3, &self.home_zone());
                crate::booking::to_decide(&p, &assessed, &alternatives)
            }
            None => "I don't have a proposed time to work through. Tell me who proposed what — \
                     \"Sam proposed Tuesday at 2pm or Wednesday at 10am for the review\" — and I'll \
                     check it against your calendar."
                .into(),
        }
    }

    /// Carry out your answer to the waiting proposal. Accept writes it to your
    /// calendar; decline marks it; a counter lays out times you could offer.
    /// Atlas never sends the reply — that stays with you.
    fn answer_proposal(
        &mut self,
        i: usize,
        state: crate::booking::State,
        now: u64,
        cfg: &crate::booking::BookingConfig,
    ) -> String {
        use crate::booking::{Fit, State};
        let p = self.proposals[i].clone();
        match state {
            State::Accepted => {
                let busy = self.busy_slots(now, now + 30 * 86_400);
                let assessed = crate::booking::assess(&p, &busy, now, cfg, &self.home_zone());
                let slot = assessed
                    .iter()
                    .find(|a| matches!(a.verdict, Fit::Free | Fit::Awkward))
                    .map(|a| a.slot);
                match slot {
                    Some(s) => {
                        let title = p
                            .about
                            .clone()
                            .unwrap_or_else(|| format!("meeting with {}", p.from));
                        let when = crate::calendar::When {
                            start: s.start,
                            end: s.start + s.mins as u64 * 60,
                            all_day: false,
                        };
                        let id = self.calendar.add_in(
                            &title,
                            when,
                            None,
                            crate::earned::Space::Personal,
                            now,
                        );
                        let whenn = self.calendar.event(id).map(|e| e.say_when_in(&self.home_zone())).unwrap_or_default();
                        self.proposals[i].state = State::Accepted;
                        let _ = self.calendar.save(&self.store);
                        let _ = self.store.save("proposals", &self.proposals);
                        format!(
                            "Done — \"{title}\" is on your calendar for {whenn}. I haven't replied to \
                             {}; say the word and I'll draft it, but I don't send on your behalf.",
                            p.from
                        )
                    }
                    None => "None of their times actually clear against your calendar, so accepting \
                             would book a clash. Better to offer another — say \"offer another time\"."
                        .into(),
                }
            }
            State::Declined => {
                self.proposals[i].state = State::Declined;
                let _ = self.store.save("proposals", &self.proposals);
                format!(
                    "Marked {}'s proposal declined. Nothing's been sent — that reply is yours to make.",
                    p.from
                )
            }
            State::CounterOffered => {
                let busy = self.busy_slots(now, now + 30 * 86_400);
                let mins = p.times.first().map(|s| s.mins).unwrap_or(30);
                let alternatives = crate::booking::could_offer(&busy, now, mins, cfg, 3, &self.home_zone());
                self.proposals[i].state = State::CounterOffered;
                let _ = self.store.save("proposals", &self.proposals);
                if alternatives.is_empty() {
                    "I couldn't find a clear slot inside your hours to offer instead — your next two \
                     weeks are full at those times.".into()
                } else {
                    let times: Vec<String> = alternatives
                        .iter()
                        .map(|s| {
                            let e = crate::calendar::Event {
                                id: 0,
                                title: String::new(),
                                start: s.start,
                                end: s.start + s.mins as u64 * 60,
                                all_day: false,
                                place: None,
                                note: None,
                                space: crate::earned::Space::Personal,
                                kind: crate::calendar::EventKind::Meeting,
                                repeat: crate::calendar::Repeat::Once,
                                remind_before_mins: None,
                                source: crate::calendar::Source::Atlas,
                                phone_key: None,
                                created: now,
                                except: Vec::new(),
                                zone: None,
                            };
                            e.say_when_in(&self.home_zone())
                        })
                        .collect();
                    format!(
                        "Here's what you could offer {} instead: {}. Nothing's sent — pick one and \
                         I'll draft the reply for you to send.",
                        p.from,
                        times.join("; ")
                    )
                }
            }
            State::NeedsYou | State::WentStale => {
                let opts = "\"accept the meeting\", \"decline the meeting\", or \"offer another time\"";
                format!("I couldn't tell if that was an accept, a decline, or a counter — say {opts}.")
            }
        }
    }

    /// Which registered project a request is about, if any — a registered name
    /// mentioned anywhere wins; otherwise a "project X"/"on X"/"in X" phrase
    /// names one (new or existing).
    /// Review a page's design against the house style.
    ///
    /// The honest half of taste, made usable: it reads the markup — a file you
    /// name, or HTML you paste — and reports where it's off the spacing scale,
    /// using typed-in colours instead of tokens, or failing accessibility. It
    /// never says whether the design is *good*; that's yours to judge or a
    /// stronger model's. A clean review means "consistent and accessible", not
    /// "right".
    fn design_review(&self, what: &str) -> String {
        let arg = what.trim();
        if arg.is_empty() {
            return "Point me at a page — \"review the design of index.html\" — or paste the markup."
                .into();
        }

        // Paste vs path: markup has a tag in it; anything else is treated as a
        // file to read.
        let (html, source) = if arg.contains('<') && arg.contains('>') {
            (arg.to_string(), "the markup you gave me".to_string())
        } else {
            match std::fs::read_to_string(arg) {
                Ok(text) => (text, format!("\"{arg}\"")),
                Err(e) => {
                    return format!(
                        "I couldn't read {arg}: {e}. Name an HTML file I can reach, or paste the \
                         markup."
                    )
                }
            }
        };

        let rules = self.tools_cfg().taste.clone();
        let findings = crate::taste::review(&html, &rules);
        let mut out = crate::taste::spoken(&findings);

        let blocking = crate::taste::blocking(&findings);
        if !blocking.is_empty() {
            out.push_str(&format!(" In {source}:"));
            for f in &blocking {
                out.push_str(&format!("\n  • {}", f.detail));
            }
        }
        let advisory: Vec<&crate::taste::Finding> =
            findings.iter().filter(|f| f.severity == crate::taste::Severity::Advisory).collect();
        if !advisory.is_empty() {
            out.push_str("\nWorth a look:");
            for f in &advisory {
                out.push_str(&format!("\n  • {}", f.detail));
            }
        }
        out
    }

    /// Learn a whole body of knowledge at once — a paste, or a file.
    ///
    /// This is how the knowledge base grows vast: a document, a page of notes,
    /// a reference sheet is broken into many discrete facts, each tagged and
    /// indexed and folded into the one book with merge-on-restate, so nothing
    /// duplicates and everything is recalled the same fast way. Reference
    /// knowledge — kept, never an evictable guess.
    fn learn_knowledge(&mut self, what: &str) -> String {
        let arg = what.trim().trim_start_matches(':').trim();
        if arg.is_empty() {
            return "Give me something to learn — paste some text after \"learn this\", or name a \
                    file with \"learn from …\"."
                .into();
        }
        let now = crate::store::now();
        // A whole folder of reference material, imported in one pass (bounded so
        // a large tree can't stall the machine) — the way a knowledge base grows
        // vast without pasting a hundred documents by hand.
        if std::fs::metadata(arg).map(|m| m.is_dir()).unwrap_or(false) {
            return self.learn_folder(arg, now);
        }
        // A readable file path, or the text itself.
        let (text, whence) = match std::fs::read_to_string(arg) {
            Ok(t) => (t, format!("\"{arg}\"")),
            Err(_) => (arg.to_string(), "what you gave me".to_string()),
        };
        let chunks = crate::facts::into_facts(&text);
        if chunks.is_empty() {
            return "There wasn't anything in that I could turn into facts to remember.".into();
        }
        let mut learned = 0u32;
        for chunk in &chunks {
            self.facts.learn(crate::facts::reference_fact(chunk, now), now);
            learned += 1;
        }
        self.facts.trim(crate::facts::MEMORY_BUDGET_BYTES, now);
        let _ = self.facts.save(&self.store);
        format!(
            "Learned {learned} thing{} from {whence}. Ask \"what do you know about …\" and I'll have it.",
            if learned == 1 { "" } else { "s" }
        )
    }

    /// Import every readable text file under a folder in one pass.
    ///
    /// Bounded on purpose — a cap on files read and a per-file size limit, and
    /// it skips the folders that are never knowledge (`.git`, `target`,
    /// `node_modules`) and any file it can't read as text. That is what lets you
    /// point Atlas at a whole notes folder and have it learn the lot without a
    /// deep tree stalling the machine or a binary being turned into nonsense
    /// facts.
    fn learn_folder(&mut self, dir: &str, now: u64) -> String {
        const MAX_FILES: usize = 200;
        const MAX_BYTES: u64 = 5 * 1024 * 1024;
        const SKIP_DIRS: &[&str] = &[".git", "target", "node_modules", ".obsidian", ".venv"];
        let mut files = 0usize;
        let mut learned = 0u32;
        let mut skipped = 0usize;
        let mut stack = vec![std::path::PathBuf::from(dir)];
        while let Some(p) = stack.pop() {
            if files >= MAX_FILES {
                break;
            }
            let Ok(entries) = std::fs::read_dir(&p) else { continue };
            for e in entries.flatten() {
                let path = e.path();
                if path.is_dir() {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if !SKIP_DIRS.contains(&name) {
                        stack.push(path);
                    }
                    continue;
                }
                if files >= MAX_FILES {
                    break;
                }
                if path.metadata().map(|m| m.len() > MAX_BYTES).unwrap_or(true) {
                    skipped += 1;
                    continue;
                }
                match std::fs::read_to_string(&path) {
                    Ok(t) => {
                        for chunk in crate::facts::into_facts(&t) {
                            self.facts.learn(crate::facts::reference_fact(&chunk, now), now);
                            learned += 1;
                        }
                        files += 1;
                    }
                    // Not text (binary, non-UTF8): left alone rather than
                    // turned into garbage facts.
                    Err(_) => skipped += 1,
                }
            }
        }
        if files == 0 {
            return "I couldn't read any text files in that folder.".into();
        }
        self.facts.trim(crate::facts::MEMORY_BUDGET_BYTES, now);
        let _ = self.facts.save(&self.store);
        let tail = if skipped > 0 {
            format!(" ({skipped} file{} weren't text and were left alone.)", if skipped == 1 { "" } else { "s" })
        } else {
            String::new()
        };
        format!(
            "Learned {learned} thing{} from {files} file{} in that folder.{tail} Ask \"what do you know about …\" and I'll have it.",
            if learned == 1 { "" } else { "s" },
            if files == 1 { "" } else { "s" }
        )
    }

    /// Explain the staged change as behaviour, not code.
    ///
    /// The self-fix path leads its confirmation with a one-line behaviour
    /// summary; this is the fuller form, for when you want it — what the change
    /// will now do, what it no longer promises, what it touched, and what's
    /// worth watching — read from the tests it adds and drops, with no diff. It
    /// only reads what's already staged; nothing here lands or discards.
    fn plain_change(&self) -> String {
        match &self.pending_change_effect {
            Some(effect) => {
                let mut out = crate::plainchange::written(effect);
                out.push_str(&format!("\n{}", crate::plainchange::ask(effect)));
                out
            }
            None => "Nothing of mine is staged right now, so there's no change to explain. Set me on \
                     a fix first and I'll tell you what it'll do before you land it."
                .into(),
        }
    }

    /// Make an animation: "animate a bouncing ball, 600x400, for 3 seconds".
    ///
    /// Draws it as a self-contained SVG with the local model, checks that it
    /// renders and matches the size/duration asked for, saves it where you can
    /// open it, and reports honestly. The check is the reliable half — it does
    /// not, and does not pretend to, judge whether the motion looks good; that
    /// is what opening the file is for. Nothing is sent anywhere; it's a file.
    fn animate(&mut self, what: &str) -> String {
        let idea = what.trim();
        if idea.is_empty() {
            return "What should I animate? Try \"animate a bouncing ball, 600x400, for 3 seconds\"."
                .into();
        }
        // "animate a bouncing ball in 3d" is a moving 3-D scene, not an SVG.
        let low = idea.to_lowercase();
        if ["3d", "3-d", "three d", "3 d ", "three-dimensional"].iter().any(|w| low.contains(w)) {
            return self.scene(idea);
        }
        let Some(llm) = self.llm.clone() else {
            return "I can draw an animation, but I need a model to draft it and none is configured."
                .into();
        };
        let spec = crate::motion::MotionSpec::from_words(idea);

        // Draw it and iterate against the check — the model drafts, the check
        // decides, up to the shared fix-round budget. Nothing here judges
        // whether it looks good; that's what opening the file is for.
        let rounds_budget = self.tools_cfg().build.max_fix_rounds;
        let outcome = crate::motion::draw_loop(&spec, llm.as_ref(), rounds_budget, |s| {
            crate::motion::check(s, &spec)
        });
        let (svg, findings, rounds, clean) = match outcome {
            crate::motion::Outcome::NoDraft(why) => {
                return format!("I tried, but {why}. Ask me to try again.")
            }
            crate::motion::Outcome::Drawn { svg, rounds, notes } => (svg, notes, rounds, true),
            crate::motion::Outcome::Struggled { svg, rounds, findings } => {
                (svg, findings, rounds, false)
            }
        };

        // Save it either way — even a flawed draft is worth opening — under a
        // name that says whether it passed the checks.
        let dir = crate::roots::data_sub("animations");
        let _ = std::fs::create_dir_all(&dir);
        let name = if clean { "animation" } else { "animation.draft" };
        let path = dir.join(format!("{name}.svg"));
        if let Err(e) = std::fs::write(&path, &svg) {
            return format!("I drew it, but couldn't save it: {e}.");
        }
        self.last_animation = Some((path.clone(), spec.clone(), crate::store::now()));

        let mut said = crate::motion::spoken(&findings);
        if clean && rounds > 0 {
            said.push_str(&format!(
                " Took {rounds} fix{} to get there.",
                if rounds == 1 { "" } else { "es" }
            ));
        }
        said.push_str(&format!(" Saved to {}.", path.display()));

        // If a rasteriser is configured and the SVG itself is sound, render a
        // PNG still next to it and check what came out. No rasteriser → the SVG
        // stands on its own (it renders in any browser), said plainly rather
        // than pretended.
        let render_cmd = self.tools_cfg().build.render_svg_command.clone();
        if clean && !render_cmd.trim().is_empty() {
            let png = dir.join("animation.png");
            let expect = crate::motion::Expect {
                kind: crate::motion::RenderKind::Png,
                width: spec.width,
                height: spec.height,
            };
            match crate::motion::render(&path, &png, &render_cmd, &expect) {
                Ok(render_findings) => {
                    if crate::motion::blocking(&render_findings).is_empty() {
                        said.push_str(&format!(" Rendered a PNG to {}.", png.display()));
                    } else {
                        said.push_str(" I rendered it, but the result didn't check out:");
                    }
                    for f in &render_findings {
                        said.push_str(&format!("\n  • {}", f.detail));
                    }
                }
                Err(why) => said.push_str(&format!(" (No PNG: {why}.)")),
            }
        }

        // Played and saved as a GIF (and an MP4 with ffmpeg) when there's a
        // browser to play it in — Edge on any Windows machine. `filmstrip`.
        if clean {
            let tc = self.tools_cfg();
            match crate::filmstrip::find_browser(tc.vars.get("browser").map(|s| s.as_str())) {
                Some(browser) => {
                    let plan = crate::filmstrip::Plan::for_svg(&svg, &spec, 12);
                    let ffmpeg = tc.vars.get("ffmpeg").cloned().unwrap_or_else(|| "ffmpeg".into());
                    match crate::filmstrip::film(&svg, &plan, &browser, Some(&ffmpeg), &dir, name) {
                        Ok(made) => said.push_str(&format!(" {}", made.say())),
                        Err(e) => said.push_str(&format!(" (No GIF: {e}.)")),
                    }
                }
                None => said.push_str(" (No GIF: that needs Edge or Chrome to play it in, and I didn't find either.)"),
            }
        }

        for f in crate::motion::blocking(&findings) {
            said.push_str(&format!("\n  • {}", f.detail));
        }
        said
    }

    /// "Make it faster", "make it red", "a bit slower and bigger" -- said
    /// within the hour after an animation was drawn, an edit to that one
    /// (`motion::refine`), done here with no model, checked the same way,
    /// and saved beside it as the next version.
    fn refine_animation(&mut self, said: &str, t: u64) -> Option<String> {
        let (path, spec, at) = self.last_animation.clone()?;
        if t.saturating_sub(at) > 3600 {
            return None;
        }
        let low = said.to_lowercase();
        let about_it = ["make it", "make the animation", "now make it", "can you make it", "and make it", "faster", "slower", "speed it up", "slow it down"]
            .iter()
            .any(|p| low.trim_start().starts_with(p));
        if !about_it {
            return None;
        }
        let svg = std::fs::read_to_string(&path).ok()?;
        let r = crate::motion::refine(&svg, said)?;
        let mut spec = spec;
        if let Some(d) = r.duration_secs {
            spec.duration_secs = d;
        }
        if let Some((w, h)) = r.size {
            spec.width = w;
            spec.height = h;
        }
        let findings = crate::motion::check(&r.svg, &spec);
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("animation");
        let base = stem.split(".v").next().unwrap_or(stem).to_string();
        let n = stem.rsplit(".v").next().and_then(|v| v.parse::<u32>().ok()).unwrap_or(1) + 1;
        let next = path.with_file_name(format!("{base}.v{n}.svg"));
        if let Err(e) = std::fs::write(&next, &r.svg) {
            return Some(format!("I changed it, but couldn't save it: {e}."));
        }
        self.last_animation = Some((next.clone(), spec, t));
        let mut out = format!("Made it {}. Saved to {} (the one before is still there).", r.changes.join(", "), next.display());
        let problems = crate::motion::blocking(&findings);
        if problems.is_empty() {
            out.push_str(" It still checks out: renders, right size, right length.");
        } else {
            for f in problems {
                out.push_str(&format!("\n  • {}", f.detail));
            }
        }
        Some(out)
    }

    /// Draw a 3-D scene: "draw a 3d scene of a red ball on a box".
    ///
    /// A model drafts the scene as JSON (shapes, colours, a sun, a camera);
    /// Atlas draws it itself — a still and a turntable GIF — and in Blender
    /// too when Blender is installed. Checked: it reads, it has something in
    /// view, the picture isn't just sky. Not checked: whether it looks right.
    fn scene(&mut self, what: &str) -> String {
        let idea = what.trim();
        if idea.is_empty() {
            return "What should I draw? Try \"draw a 3d scene of a red ball on a blue box\".".into();
        }
        let Some(llm) = self.llm.clone() else {
            return "I can draw a 3-D scene, but I need a model to describe it first and none is configured.".into();
        };
        let rounds = self.tools_cfg().build.max_fix_rounds;
        // Models Eric has put in the models folder can be placed by name.
        let models = crate::roots::data_sub("models");
        let known = crate::scene3d::model_files(&models);
        let ask = if known.is_empty() {
            idea.to_string()
        } else {
            format!("{idea}\n\nModel files you may place (shape \"mesh\"): {}", known.join(", "))
        };
        let scene = match crate::scene3d::draft_scene(&ask, llm.as_ref(), rounds, Some(&models)) {
            Ok(s) => s,
            Err(e) => return format!("I tried, but the scene never came together: {e}."),
        };

        let dir = crate::roots::data_sub("animations");
        let tc = self.tools_cfg();
        let blender = crate::scene3d::find_blender(tc.vars.get("blender").map(|s| s.as_str()));
        match crate::scene3d::make(&scene, &dir, "scene", 24, blender.as_deref()) {
            Ok(made) => made.say(),
            Err(e) => format!("I described it but couldn't draw it: {e}."),
        }
    }

    /// Explain code in plain English: "explain this: <code>" or "explain
    /// src/foo.rs".
    ///
    /// Writes a non-coder explanation with the local model and iterates it
    /// against the plain-language check — no leaked code, the right length,
    /// jargon flagged. The check is the reliable half: it never claims the
    /// explanation is *correct* about the code, only that it reads like an
    /// explanation. Reading only — it changes nothing.
    fn explain_code(&mut self, what: &str) -> String {
        let arg = what.trim();
        // Strip a lead-in like "this:" / "this code:" so the rest is the code.
        let arg = arg
            .strip_prefix("this code:")
            .or_else(|| arg.strip_prefix("this:"))
            .or_else(|| arg.strip_prefix("this code"))
            .or_else(|| arg.strip_prefix("this"))
            .unwrap_or(arg)
            .trim();
        if arg.is_empty() {
            return "What should I explain? Point me at a file — \"explain src/foo.rs\" — or paste \
                    the code."
                .into();
        }

        // What to explain, in order: something I built recently, a change
        // waiting to be implemented, a file you named, or code you pasted.
        let low = arg.to_lowercase();
        let (code, source) = if references_a_build(&low) {
            match self.latest_build_code() {
                Ok(p) => p,
                Err(e) => return e,
            }
        } else if references_a_queued_change(&low)
            || self.names_a_ready_change(&low)
        {
            match self.queued_change_code(&low) {
                Some(p) => p,
                None => return "There's nothing waiting to be implemented right now.".into(),
            }
        } else {
            // A single-line, path-shaped argument that names a real file is
            // read; anything else is treated as pasted code.
            let looks_like_path = !arg.contains('\n') && arg.split_whitespace().count() == 1;
            if looks_like_path && std::path::Path::new(arg).is_file() {
                match std::fs::read_to_string(arg) {
                    Ok(text) => (text, format!("\"{arg}\"")),
                    Err(e) => return format!("I couldn't read {arg}: {e}."),
                }
            } else {
                (arg.to_string(), "the code you gave me".to_string())
            }
        };

        let Some(llm) = self.llm.clone() else {
            return "I can explain it, but I need a model to write the explanation and none is \
                    configured."
                .into();
        };

        // The depth dial, read from the request ("like I'm five", "in detail").
        let depth = crate::explain::Depth::from_words(&low);
        let rounds_budget = self.tools_cfg().build.max_fix_rounds;
        let outcome = crate::explain::explain_loop(&code, llm.as_ref(), rounds_budget, depth, |t| {
            crate::explain::check_at(t, depth)
        });
        let (text, findings, rounds) = match outcome {
            crate::explain::Outcome::NoDraft(why) => {
                return format!("I tried to explain {source}, but {why}.")
            }
            crate::explain::Outcome::Explained { text, rounds, notes } => (text, notes, rounds),
            crate::explain::Outcome::Struggled { text, rounds, findings } => (text, findings, rounds),
        };

        let mut said = text;
        said.push_str("\n\n");
        said.push_str(&crate::explain::spoken(&findings));
        if rounds > 0 {
            said.push_str(&format!(
                " (Took {rounds} rewrite{} to read plainly.)",
                if rounds == 1 { "" } else { "s" }
            ));
        }
        for f in crate::explain::blocking(&findings) {
            said.push_str(&format!("\n  • {}", f.detail));
        }
        said
    }

    /// The most recent thing Atlas built, as code to explain.
    fn latest_build_code(&self) -> std::result::Result<(String, String), String> {
        let dir = crate::roots::data_sub("builds");
        let mut files: Vec<(std::path::PathBuf, std::time::SystemTime)> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                let m = e.metadata().ok()?.modified().ok()?;
                p.is_file().then_some((p, m))
            })
            .collect();
        files.sort_by_key(|(_, m)| *m);
        match files.last() {
            Some((p, _)) => {
                let code = std::fs::read_to_string(p)
                    .map_err(|e| format!("I found what I built but couldn't read it: {e}."))?;
                let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                Ok((code, format!("what I built ({name})")))
            }
            None => Err("I haven't built anything recently — ask me to build something first, \
                         then I'll explain it."
                .into()),
        }
    }

    /// Does the request name a change that's ready to implement?
    fn names_a_ready_change(&self, low: &str) -> bool {
        self.workshop.projects.iter().flat_map(|p| p.ready()).any(|c| {
            let t = c.title.to_lowercase();
            !t.is_empty() && low.contains(&t)
        })
    }

    /// A change waiting to be implemented, as code to explain: the one named in
    /// the request, or the most recent if none is named.
    fn queued_change_code(&self, low: &str) -> Option<(String, String)> {
        let ready: Vec<&crate::workshop::Change> =
            self.workshop.projects.iter().flat_map(|p| p.ready()).collect();
        let picked = ready
            .iter()
            .find(|c| {
                let t = c.title.to_lowercase();
                !t.is_empty() && low.contains(&t)
            })
            .copied()
            .or_else(|| ready.iter().max_by_key(|c| c.created).copied())?;
        let code = picked
            .files
            .iter()
            .map(|f| format!("// {}\n{}", f.path, f.content))
            .collect::<Vec<_>>()
            .join("\n\n");
        Some((code, format!("the \"{}\" change waiting to be implemented", picked.title)))
    }

    fn detect_project(&self, what: &str) -> Option<String> {
        let low = what.to_ascii_lowercase();
        // A registered project named anywhere in the request.
        for p in &self.workshop.projects {
            let n = p.name.to_ascii_lowercase();
            if !n.is_empty() && low.contains(&n) {
                return Some(p.name.clone());
            }
        }
        // "the X project" / "on X" / "in X" — take the word after the lead.
        for lead in ["the ", "on ", "in ", "for ", "project "] {
            if let Some(i) = low.find(lead) {
                let rest = &what[i + lead.len()..];
                let word: String =
                    rest.split_whitespace().next().unwrap_or("").chars().filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_').collect();
                // "the date parser" shouldn't name a project "date"; only take
                // it when the phrase is "<name> project" or the lead was
                // "project".
                let followed_by_project = rest.to_lowercase().contains("project");
                if !word.is_empty() && (lead == "project " || followed_by_project) {
                    return Some(word);
                }
            }
        }
        None
    }

    /// Write a fresh master document for a project — what it is, where it
    /// lives, and its recent implemented changes — and return where it landed.
    fn write_project_master_doc(&self, project: &str) -> Option<String> {
        let p = self.workshop.resolve(project)?;
        let mut doc = format!("# {} — master document\n\n", p.name);
        if !p.folder.is_empty() {
            doc.push_str(&format!("Folder: {}\n\n", p.folder));
        }
        doc.push_str("## Implemented changes\n\n");
        let mut any = false;
        for c in p.changes.iter().filter(|c| c.state == crate::workshop::State::Implemented) {
            any = true;
            doc.push_str(&format!("- **{}** — {}\n", c.title, c.what));
        }
        if !any {
            doc.push_str("- (none yet)\n");
        }
        if !p.outstanding().is_empty() {
            doc.push_str("\n## Outstanding\n\n");
            for t in p.outstanding() {
                doc.push_str(&format!("- {}\n", t.title));
            }
        }
        let dir = crate::roots::data_sub("projects");
        let _ = std::fs::create_dir_all(&dir);
        let slug: String =
            p.name.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect();
        let path = dir.join(format!("{slug}.master.md"));
        std::fs::write(&path, doc).ok().map(|_| path.display().to_string())
    }

    fn research(&mut self, topic: &str) -> String {
        let mut cfg = self.tools_ref()
            .map(|t| t.research.clone())
            .unwrap_or_default()
            .resolved(&self.store.install_root());
        if !cfg.enabled {
            return "Research is switched off in your settings.".into();
        }
        // Answered from what research already taught, before spending a
        // search on it. `self.known` was written by nothing and read by
        // nothing: `consolidate`'s decay and merge machinery ran on a
        // permanently empty store while every research answer was thrown
        // away the moment it was spoken. "again" anywhere in the ask
        // forces a fresh run — a cache must never argue with you.
        let now_check = crate::store::now();
        if !topic.to_lowercase().contains("again") {
            if let Some(c) = self
                .known
                .iter_mut()
                .find(|c| !c.corrected && crate::consolidate::same_claim(topic, &c.says))
            {
                if !c.worth_rechecking(now_check) {
                    c.asked_about += 1;
                    let says = c.says.clone();
                    let _ = self.store.save("known", &self.known);
                    return format!(
                        "From what I found before: {says} Say 'research it again' if you \
                         want it checked fresh."
                    );
                }
            }
        }
        // Checked before spending a search on it. `need_of` already routes an
        // offline research request to the backlog; this is the spoken half.
        // The sentence lives in `connectivity::deferral_message` -- the one
        // place that answers "the network was needed and isn't here" -- rather
        // than a second copy hand-written at the point of use. Research is the
        // only `Need::Internet` intent, so this is that function's real home.
        if self.connectivity.cached() == Reach::Offline {
            return crate::connectivity::deferral_message(&Intent::Research(topic.to_string()));
        }
        let Some(llm) = self.llm.clone() else {
            return format!(
                "I can search for {topic}, but I need a model to read the sources and write it up, and I haven't got one configured."
            );
        };
        // When online and Cloudflare is set up, the heavy read-and-write-up
        // step is delegated to a worker instead of grinding on the local 3B,
        // and the local model becomes the *checker* — it reads the delegated
        // write-up back and says whether it holds. Offline, or with no
        // provider, `worker` is None and the local model does the work as
        // before. This is the whole "delegate out, pull back, verify in the
        // background" flow, on the existing research errand.
        let cf_fetch = self.tools_cfg().cloudflare.fetch.clone();
        let worker = self.cloudflare_worker();
        // The vars the errand runs its tools with. When delegating, they
        // carry the Cloudflare account id and token so a Browser Rendering
        // fetch can authenticate; otherwise they are just the shared vars.
        let vars = match &worker {
            Some((_, cf_vars)) => cf_vars.clone(),
            None => self.tools_ref().map(|t| t.vars.clone()).unwrap_or_default(),
        };
        // Delegated page fetch (Browser Rendering) on Cloudflare's side, when
        // it is offered and we are delegating — lighter on this machine.
        if worker.is_some() {
            if let Some(browser_fetch) = cf_fetch {
                cfg.fetch = Some(browser_fetch);
            }
        }
        // Carried into the errand so the finding can be assessed where its
        // sources are known. Out here there is only the acknowledgement.
        let certainty = self.tools_cfg().certainty.clone();
        // Cloned out here for the same reason `certainty` is: the errand runs
        // on another thread and cannot reach back into the daemon.
        let judging = self.tools_cfg().judgment.clone();
        let cf_verify = self.tools_cfg().cloudflare.verify;
        let verifier = worker.as_ref().map(|_| llm.clone());
        let summariser: std::sync::Arc<dyn crate::brain::Llm> =
            worker.as_ref().map(|(w, _)| w.clone()).unwrap_or_else(|| llm.clone());
        let delegated = worker.is_some();
        // The headless browser the search step falls back on when curl's
        // copy of the results page had no links in it. Only when a tools
        // section exists -- no config, no browser to start.
        let browser = self.tools_ref().map(|t| t.browser.clone());
        let topic_owned = topic.to_string();
        let topic_for_errand = topic_owned.clone();
        let work: crew::Work = Box::new(move |ctl| {
            let r = crate::research::Research { cfg, vars, browser };
            let ran = r.run_checked(&topic_for_errand, summariser.as_ref(), &|| ctl.checkpoint());
            match ran {
                Ok(note) => {
                    // Saved automatically, same as before — just never
                    // narrated. Where it landed, or whether it landed, is
                    // mechanics you didn't ask about; if it matters later
                    // you can ask to see it or ask for it to be saved
                    // again. A save failure here is silent rather than
                    // spoken, on the same reasoning: the note itself,
                    // which is the thing you actually asked for, still
                    // came back fine.
                    let _ = r.save(&note);
                    // The one place in Atlas where grounding is known exactly
                    // rather than guessed at: the note carries the list of
                    // what it read. A write-up built on nothing gets said as
                    // one.
                    let g = crate::certainty::Grounding::from_sources(note.sources.len());
                    let (mut level, _, why) = crate::certainty::assess(&note.spoken, &g, &certainty);
                    // A figure in the answer that isn't in any page it read is
                    // said as unconfirmed, never as read (`figures_not_in`).
                    let spoken_ungrounded: Vec<&String> =
                        note.ungrounded.iter().filter(|f| note.spoken.contains(f.as_str())).collect();
                    let why = if !spoken_ungrounded.is_empty() && level != crate::certainty::Confidence::Withhold {
                        level = crate::certainty::Confidence::Qualify;
                        let list = spoken_ungrounded.iter().map(|f| f.as_str()).collect::<Vec<_>>().join(", ");
                        let mine = format!("{list}{}", crate::research::UNCONFIRMED);
                        if why.trim().is_empty() { mine } else { format!("{why}; {mine}") }
                    } else {
                        why
                    };
                    // If this was delegated, the local model checks the
                    // worker's write-up before it is spoken — accuracy
                    // analysed in the background, exactly as asked. The check
                    // can only hold or lower the confidence, never raise it.
                    let mut why = why;
                    if delegated && cf_verify {
                        if let Some(v) = &verifier {
                            let (l2, why2, _ok) = crate::online::verify_result(
                                &topic_for_errand,
                                &note.spoken,
                                Some(v.as_ref()),
                                level,
                                why,
                            );
                            level = l2;
                            why = why2;
                        }
                    }
                    let spoken = crate::certainty::phrase(&note.spoken, level, &why);
                    // How much is behind it, not just how many pages were
                    // opened. `Grounding::from_sources` is `sources > 0`, so
                    // one source and eight grade the same -- and `research`
                    // has recorded how much each page actually yielded since
                    // the day it was written, with nothing reading it. A page
                    // that gave back two hundred characters is a cookie
                    // banner.
                    //
                    // Said only when it is worth saying: thin, notably well
                    // read, or unreadable. A qualifier on every answer is a
                    // qualifier nobody reads.
                    let rests = crate::research::rests_on(&note, &judging);
                    Ok(format!(
                        "{} Read {} source{}.{}",
                        spoken,
                        note.sources.len(),
                        if note.sources.len() == 1 { "" } else { "s" },
                        rests.map(|r| format!(" {r}")).unwrap_or_default()
                    ))
                }
                Err(e) => Err(format!(
                    "I couldn't finish looking into {topic_for_errand}: {e}. It's on the outstanding list."
                )),
            }
        });
        let taken = self.hand_off(
            "research",
            crate::store::now(),
            work,
            Some(topic_owned.clone()),
            SpeakPolicy::Always,
        );
        if taken {
            if delegated {
                format!(
                    "Looking into {topic} — I've handed the heavy lifting to a worker online and \
                     I'll check what comes back before I bring it to you."
                )
            } else {
                format!("Looking into {topic}. I'll let you know what I find.")
            }
        } else {
            // The bounded waiting list is full -- vanishingly unlikely in
            // real use, but honest rather than silently dropping the ask.
            format!("I'm swamped with background work right now — ask me about {topic} again in a moment.")
        }
    }



    /// Read the writing in a photo.
    ///
    /// Atlas's own reader first, and the outside program only if it is both
    /// configured and installed. That order matters: for as long as this went
    /// to tesseract first, "I can't read photos yet" was the answer to every
    /// photo ever handed to Atlas, and the settings switch it pointed at could
    /// not fix it — there was no tesseract to switch on.
    fn read_photo(&self, item: &crate::tray::Item) -> std::result::Result<String, String> {
        let tools = self.tools_cfg();
        let path = item.stored_at.clone().unwrap_or_else(|| item.what.clone());
        let vars = self.tools_ref().map(|t| t.vars.clone()).unwrap_or_default();

        let models = std::path::PathBuf::from(&tools.models.dir);
        if crate::words::Reader::installed(&models) {
            return match crate::words::Reader::open(&models).and_then(|mut r| {
                crate::words::read_file(&mut r, &tools.video.ffmpeg, &vars, &path, &tools.words)
            }) {
                Ok(read) if read.worth_acting_on() => Ok(read.text()),
                Ok(read) => Err(read.spoken()),
                Err(e) => Err(format!("I couldn't read that photo: {e}")),
            };
        }

        // Without Atlas's reading models, the operating system's own
        // recognizer (Windows.Media.Ocr, on every Windows 10 and 11) reads it
        // -- capitals and punctuation too, which `words` can't (28 Sep 2026).
        if let Ok(Some(raw)) = self.plat.recognise_image_file(&path) {
            let text = crate::screentext::tidy_lines(&raw);
            if crate::screentext::plausible(&text) {
                return Ok(text);
            }
        }

        let cfg = tools.ocr.clone();
        if !cfg.enabled {
            return Err("I can't read photos yet — the two reading models aren't \
                        installed. They're an optional one-off download, and Atlas's \
                        window doesn't offer it yet."
                .to_string());
        }
        match crate::ocr::read_image(&cfg, &path, &vars) {
            Ok(reading) if reading.trustworthy() => Ok(reading.summary()),
            // A bad reading is worse than none: half-recognised words look
            // like a quotation and are not one.
            Ok(_) => Err("I looked, but the writing in it is too unclear for me \
                          to read honestly."
                .to_string()),
            Err(e) => Err(format!("I couldn't read that photo: {e}")),
        }
    }



    /// Shrink a frame down to something worth keeping.
    ///
    /// A thumbnail is not a screenshot, and the width is what makes the
    /// difference: at 160 pixels a frame is four to eight kilobytes, against a
    /// third of a megabyte at full size. Forty of them is a third of a
    /// megabyte in total — small enough that keeping them is not the cost that
    /// made screenshots the wrong answer, and big enough to recognise what you
    /// were looking at.
    ///
    /// Returns `None` on any failure. A missing picture is a smaller loss than
    /// a video that refuses to be watched because one frame would not scale.
    fn keep_thumbnail(
        &self,
        frame: &std::path::Path,
        into: &std::path::Path,
        width: u32,
        n: usize,
    ) -> Option<String> {
        std::fs::create_dir_all(into).ok()?;
        let out = into.join(format!("{n:03}.jpg"));
        let ok = crate::tools::command("ffmpeg")
            .args(["-y", "-i"])
            .arg(frame)
            .args(["-vf"])
            .arg(format!("scale={width}:-1"))
            .args(["-q:v", "6"])
            .arg(&out)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        ok.then(|| out.to_string_lossy().to_string())
    }

    /// Watch a video: what was on screen, next to what was being said.
    ///
    /// Listening alone loses the half of a video that is on the screen —
    /// someone says "you can see the problem here" over a screen holding the
    /// whole answer. Screenshots every few seconds would be the version that
    /// looks like it works: hundreds of near-identical pictures of a static
    /// slide, and still a missed frame at the one second something appeared.
    ///
    /// So: ask ffmpeg which frames actually differ, read those, and delete
    /// each frame once it has been read. What a frame is worth is the words on
    /// it, and those are a few hundred bytes against a few hundred kilobytes.
    fn watch(&self, item: &crate::tray::Item, path: &str) -> std::result::Result<String, String> {
        let tools = self.tools_cfg();
        let view = tools.viewing.clone();
        let scratch = std::path::Path::new(&tools.work_dir).join(format!("watch-{}", item.id));
        let _ = std::fs::create_dir_all(&scratch);

        // Two passes, because one is not enough. The first asks ffmpeg where
        // the picture changed; the second pulls frames at those moments *plus*
        // wherever the first left too long a gap.
        //
        // Measured against real footage: a 24-second handheld clip produced
        // zero scene changes at the old threshold. Scene detection assumes the
        // picture changes when the content does, which holds for a screen
        // recording and fails for most of what actually gets sent.
        let scan = crate::tools::command("ffmpeg")
            .args(["-y", "-i", path, "-vf"])
            .arg(format!("select='gt(scene,{})',showinfo", view.scene_change))
            .args(["-vsync", "vfr", "-f", "null", "-"])
            .output()
            .map_err(|e| format!("I couldn't watch that: {e}"))?;

        // showinfo writes to stderr. That is not an error; it is where ffmpeg
        // says what it did.
        let told = String::from_utf8_lossy(&scan.stderr).to_string();
        let scenes = crate::viewing::scene_times(&told, view.most_frames);
        let duration = seconds_of(&told);

        // `viewing.longest_minutes` -- "Longest video Atlas will start on" --
        // was read by nothing until 18 Sep 2026, so a three-hour file went
        // through the whole two-pass scan and the OCR behind it. The number
        // was already in hand one line above and never compared to anything.
        // Refused rather than truncated: watching the first ninety minutes of
        // something and reporting on it as if it were the whole is the
        // failure this module is most able to cause.
        if let Some(too_long) = crate::viewing::too_long(duration, &view) {
            let _ = std::fs::remove_dir_all(&scratch);
            return Err(too_long);
        }
        let times = crate::viewing::where_to_look(&scenes, duration, &view);

        // Pull exactly those moments. One `select` listing the timestamps
        // rather than one ffmpeg run per frame, which on a long video would be
        // forty process launches.
        let pattern = scratch.join("scene-%03d.png");
        let picks = times
            .iter()
            .map(|t| format!("between(t,{:.2},{:.2})", t, t + 0.05))
            .collect::<Vec<String>>()
            .join("+");
        // `showinfo` again on the way out, so Atlas knows the real time of
        // each frame it got rather than assuming it got exactly what it asked
        // for. A selection window has to be wider than one frame interval to
        // be sure of catching anything, which means it often catches two — on
        // this clip, six requested moments produced twelve frames.
        let pull = crate::tools::command("ffmpeg")
            .args(["-y", "-i", path, "-vf"])
            .arg(format!("select='{picks}',showinfo"))
            .args(["-vsync", "vfr", "-frames:v"])
            .arg((view.most_frames * 3).to_string())
            .arg(&pattern)
            .output()
            .map_err(|e| format!("I couldn't watch that: {e}"))?;
        let got = crate::viewing::frame_times(&String::from_utf8_lossy(&pull.stderr));

        let mut frames: Vec<std::path::PathBuf> = std::fs::read_dir(&scratch)
            .map_err(|e| format!("I couldn't watch that: {e}"))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "png"))
            .collect();
        frames.sort();

        // Drop the neighbours. Two frames a thirtieth of a second apart are
        // the same moment, and reading both costs a second pass of text
        // recognition to produce the same answer twice.
        let (frames, times) = crate::viewing::one_per_moment(frames, &got, &times);
        for extra in &times.1 {
            let _ = std::fs::remove_file(extra);
        }
        let times = times.0;

        let ocr = tools.ocr.clone();
        let vars = self.tools_ref().map(|t| t.vars.clone()).unwrap_or_default();
        let kept_dir = std::path::Path::new(self.store.root())
            .join(crate::tray::FOLDER)
            .join(format!("frames-{}", item.id));

        let mut screens: Vec<crate::viewing::Seen> = Vec::new();
        for (i, frame) in frames.iter().enumerate() {
            let at = times.get(i).copied().unwrap_or(i as f32);
            let mut text = String::new();
            if ocr.enabled {
                if let Ok(reading) = crate::ocr::read_image(&ocr, &frame.to_string_lossy(), &vars)
                {
                    // A half-read screen is worse than a skipped one: it looks
                    // like a quotation and is not.
                    if reading.trustworthy() {
                        text = reading.text.clone();
                    }
                }
            }

            // The two rules held together. Where the screen turned into words,
            // the words are what is worth keeping and the frame goes. Where it
            // didn't — a chart, a photo, someone pointing at something — the
            // words would be nothing at all, and deleting the frame there is
            // how "keep the reading" quietly loses everything that isn't text.
            let kept_frame = if text.trim().is_empty() {
                self.keep_thumbnail(frame, &kept_dir, view.thumbnail_width, i)
            } else {
                None
            };

            if !text.trim().is_empty() || kept_frame.is_some() {
                screens.push(crate::viewing::Seen { at, text, kept_frame });
            }

            // Read, then gone. The full frame never survives either way: this
            // is the line that keeps an hour of video costing about as much
            // disk as a long email.
            let _ = std::fs::remove_file(frame);
        }
        let _ = std::fs::remove_dir(&scratch);

        let spoken = self.transcribe_timed(item, path);
        if screens.is_empty() && spoken.is_empty() {
            return Err(if !ocr.enabled {
                "I can watch videos, but reading what's on screen needs text \
                 recognition turned on — it's in Settings, under what I can see."
                    .to_string()
            } else {
                "I watched it and couldn't read anything on screen or make out \
                 anything said."
                    .to_string()
            });
        }

        let moments = crate::viewing::weave_seen(&spoken, &screens);
        let cut_short = times.len() >= view.most_frames;
        let mut account = crate::viewing::retell(&moments, cut_short);
        if !ocr.enabled {
            // Watching still works with text recognition off -- you get the
            // moments that changed, as pictures. What you lose is being able
            // to search or quote any of it, which is worth saying rather than
            // leaving you to wonder why nothing is quoted.
            account.push_str(
                "\nText recognition is off, so I kept pictures of what changed \
                 rather than reading any of it. Settings, under what I can see.\n",
            );
        } else if screens.is_empty() {
            account.push_str(
                "\nI could hear it but couldn't read anything on screen.\n",
            );
        } else if spoken.is_empty() {
            account.push_str(
                "\nI could see it but couldn't line it up with anything said.\n",
            );
        }
        Ok(account)
    }

    /// The transcript, with timestamps, when a transcriber that writes them is
    /// configured. An empty list otherwise — Atlas still watches, and says so.
    // RECOVERED IN THE 17 SEP MERGE. This function and its two call sites were
    // added on this side on 16 Sep to wire the `language` capability, and they
    // lived in `daemon.rs` -- which the improvements side also rewrote. Taking
    // their daemon whole removed the wiring silently: the module compiled, the
    // `{task_opt}`/`{lang_opt}`/`{lang_val}` placeholders stayed in
    // `config/tools.yaml`, and nothing supplied them. `wiring.rs` caught it by
    // putting `language` back on the unreachable list, and `bug_sweep` caught
    // it by finding three placeholders defined nowhere.
    //
    // Worth recording because the merge notes flagged `main.rs` as the
    // overlap and not `daemon.rs`. Two sides editing the same 8,000-line file
    // is where a merge loses work without anything failing to compile.
    /// Add the `{language}` and `{task}` template variables a transcription
    /// command uses, computed from the language settings and which model is
    /// actually loaded. On the English-only default these are empty/transcribe
    /// (a no-op), so this is safe to call on every transcription; it only does
    /// something once a multilingual model is in place and multilingual is
    /// switched on. This is what wires the `language` capability: without it
    /// the language setting is read by nothing and every clip is transcribed
    /// as English.
    fn add_language_vars(&self, vars: &mut std::collections::BTreeMap<String, String>) {
        let tools = self.tools_cfg();
        if !vars.contains_key("stt_model") {
            if let Some(m) = tools.vars.get("stt_model") {
                vars.insert("stt_model".into(), m.clone());
            }
        }
        let model = crate::language::model_facts(vars.get("stt_model").map(String::as_str).unwrap_or(""));
        crate::language::insert_whisper_vars(&tools.language, &model, vars);
    }

    fn transcribe_timed(&self, item: &crate::tray::Item, path: &str) -> Vec<crate::viewing::Spoken> {
        let tools = self.tools_cfg();
        let Some(timed) = tools.stt_timed.as_ref() else {
            return Vec::new();
        };
        let scratch = std::path::Path::new(&tools.work_dir);
        let wav = scratch.join(format!("watch-{}.wav", item.id));
        // Removed however this ends, including the `return` two lines down.
        // It used to be a `remove_file` on the last line of the happy path,
        // so a video ffmpeg could not read left the extracted audio on disk
        // for good. See `retention::Recording`.
        let mut recording = crate::retention::Recording::new(&wav, &tools.retention);
        let ok = crate::tools::command("ffmpeg")
            .args(["-y", "-i", path, "-ar", "16000", "-ac", "1"])
            .arg(&wav)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !ok {
            return Vec::new();
        }
        let stem = wav.with_extension("");
        let srt = std::path::PathBuf::from(format!("{}.srt", stem.to_string_lossy()));
        // The subtitles are a transcript of the same speech, written beside
        // the audio and left there by the old cleanup.
        recording.and_also(&srt);
        let mut vars = tools.vars.clone();
        vars.insert("in_wav".into(), wav.to_string_lossy().to_string());
        vars.insert("stem".into(), stem.to_string_lossy().to_string());
        vars.insert("srt".into(), srt.to_string_lossy().to_string());
        self.add_language_vars(&mut vars);
        let text = timed.run(&vars, None).unwrap_or_default();
        crate::viewing::read_timed(&text)
    }

    /// Listen to a recording, or to the sound of a video.
    ///
    /// Both go through the same transcriber Atlas already uses for your voice.
    /// A video is converted to audio first, which is what ffmpeg is already
    /// installed for — no new dependency for a whole new kind of thing.
    fn listen_to(&self, item: &crate::tray::Item) -> std::result::Result<String, String> {
        let tools = self.tools_cfg();
        let path = item.stored_at.clone().unwrap_or_else(|| item.what.clone());
        let scratch = std::path::Path::new(&tools.work_dir);
        let _ = std::fs::create_dir_all(scratch);
        let wav = scratch.join(format!("handed-{}.wav", item.id));
        // Removed however this ends. Both of the early returns below —
        // "I couldn't get the sound out of that" and "There was nothing said
        // in that" — used to leave the extracted audio on disk, and those are
        // the common endings for a bad recording rather than the rare ones.
        let mut recording = crate::retention::Recording::new(&wav, &tools.retention);

        // Video first: pull the sound out. For audio that is already a wav
        // this still normalises the sample rate, which whisper is fussy about.
        let out = crate::tools::command("ffmpeg")
            .args(["-y", "-i", &path, "-ar", "16000", "-ac", "1"])
            .arg(&wav)
            .output();
        match out {
            Ok(o) if o.status.success() => {}
            Ok(_) => {
                return Err("I couldn't get the sound out of that — it may not \
                            have any."
                    .to_string())
            }
            Err(e) => return Err(format!("I couldn't get the sound out of that: {e}")),
        }

        let stem = wav.with_extension("");
        let mut vars = tools.vars.clone();
        vars.insert("in_wav".into(), wav.to_string_lossy().to_string());
        vars.insert("stem".into(), stem.to_string_lossy().to_string());
        let transcript = std::path::PathBuf::from(format!("{}.txt", stem.to_string_lossy()));
        // The transcript file is the same words in another form, and it was
        // left beside the audio by the old cleanup.
        recording.and_also(&transcript);
        vars.insert("transcript".into(), transcript.to_string_lossy().to_string());
        self.add_language_vars(&mut vars);
        let said = tools
            .stt
            .run(&vars, None)
            .map_err(|e| format!("I couldn't make out the words: {e}"))?;

        let said = crate::voice::clean_transcript(&said);
        if said.trim().is_empty() {
            return Err("There was nothing said in that.".to_string());
        }
        Ok(crate::research::first_sentences(&said, 5))
    }



    /// Start a sequence: one you saved, or an add-on's (`plugin`).
    fn start_flow(
        &mut self,
        f: crate::flow::Workflow,
        plugin: Option<String>,
        said: Option<&str>,
        t: u64,
    ) -> String {
        let w = f.name.clone();
        let steps: Vec<String> = f.steps.iter().map(|s| s.command.clone()).collect();
        let (mind_id, moved) = if self.mind.active().is_empty() {
            (self.mind.begin(&w, false, t), String::new())
        } else {
            // Something is already running; the new request takes
            // the floor and the old work carries on out of sight.
            let s = self.mind.take_on(&w, false, t);
            (s.id(), s.spoken())
        };
        if let Some(work) = self.mind.get_mut(mind_id) {
            work.plan(&steps);
        }
        self.flow_mind = mind_id;
        let mut reply = match &plugin {
            Some(id) => {
                self.current_flow = Some(crate::flow::Run::start_for_plugin(&f, id));
                format!("Running {w} (the {id} add-on), {} steps.", steps.len())
            }
            None => {
                self.current_flow = Some(crate::flow::Run::start(&f));
                format!("Running {w}, {} steps.", steps.len())
            }
        };
        // Which of them can't be taken back, said up front (G7, merged from
        // the third chat's inline copy of this path).
        if let Some(run) = self.current_flow.clone() {
            let chain = self.chain_of(&run, run.steps.len());
            let cant: Vec<&str> = chain.steps.iter().filter(|s| !s.reversible).map(|s| s.what.as_str()).collect();
            if !cant.is_empty() {
                reply.push_str(&format!(" These can't be undone once done: {}.", cant.join("; ")));
            }
        }
        if !moved.is_empty() {
            reply = format!("{moved} {reply}");
        }
        if let Some(more) = self.drive_flow(t) {
            reply = format!("{reply} {more}");
        }
        if let Some(said) = said {
            self.thread.append(said, &reply, Some(w), t);
        }
        self.persist();
        reply
    }

    /// Did you say this add-on step needn't ask? (`plugins::may_skip_question`)
    fn plugin_step_trusted(&self, run: &crate::flow::Run, cmd: &str) -> bool {
        let (Some(id), Some(step)) = (&run.plugin, run.current()) else { return false };
        let (_, name) = self.parser.parse_named(cmd);
        crate::plugins::may_skip_question(&self.store, &self.plugins_dir, id, &step.command, cmd, name.as_deref())
    }

    /// An add-on's step, checked against what you still allow it
    /// (`plugins::may_run`). `Err` is the sentence to stop the run with.
    fn plugin_step_allowed(&self, run: &crate::flow::Run, cmd: &str) -> std::result::Result<(), String> {
        let Some(id) = &run.plugin else { return Ok(()) };
        let (_, name) = self.parser.parse_named(cmd);
        // The step as the add-on wrote it, before `{name}` was filled in.
        let written = run.current().and_then(|s| self.parser.parse_named(&s.command).1);
        crate::plugins::may_run(&self.store, &self.plugins_dir, id, name.as_deref(), written.as_deref())
            .map_err(|why| format!("Stopped: {why}."))
    }

    /// Drive the workflow in flight until it needs something — your yes, or
    /// the end.
    ///
    /// Each step goes through the same parser and policy gate a queued
    /// command does, so a flow cannot sneak a consequential step past
    /// approval by being part of a chain: a step the policy would ask about
    /// pauses the whole run (`Run::needs_approval`) and asks. What happens
    /// then is `hear`'s flow-approval block — yes resumes, no abandons the
    /// rest rather than half-doing it.
    /// Run the one step a fresh yes just approved, without re-classifying it.
    fn approved_flow_step(&mut self, t: u64) {
        let Some(mut run) = self.current_flow.take() else { return };
        run.approve();
        if let crate::flow::Next::Run(cmd) = run.next() {
            let step_i = run.position;
            if let Some(w) = self.mind.get_mut(self.flow_mind) {
                w.think(crate::mind::Stage::Doing, &cmd, t);
            }
            // Your yes covers the question, not what the add-on may do: a
            // permission taken away while it waited still stops it here.
            if let Err(why) = self.plugin_step_allowed(&run, &cmd) {
                run.halt(&why);
                self.current_flow = Some(run);
                return;
            }
            let intent = self.parser.parse(&cmd);
            let result = self.execute(&intent);
            let ok = !result.starts_with("error");
            self.journal.record_at(Act::Scheduled, &cmd, ok, t);
            if let Some(w) = self.mind.get_mut(self.flow_mind) {
                w.finish_step(step_i, (!ok).then(|| result.clone()));
            }
            run.report(&result, ok);
        }
        self.current_flow = Some(run);
    }

    fn drive_flow(&mut self, t: u64) -> Option<String> {
        let mut run = self.current_flow.take()?;
        let mut said: Vec<String> = Vec::new();
        // Bounded. A flow cannot hold the turn forever, whatever is in it;
        // anything left keeps moving on the next tick.
        for _ in 0..64 {
            match run.next() {
                crate::flow::Next::Run(cmd) => {
                    let step_i = run.position;
                    if let Some(w) = self.mind.get_mut(self.flow_mind) {
                        w.think(crate::mind::Stage::Doing, &cmd, t);
                    }
                    // An add-on's step is checked before anything else --
                    // before the approval gate, so you are never asked to
                    // approve a step it isn't allowed to take.
                    if let Err(why) = self.plugin_step_allowed(&run, &cmd) {
                        run.halt(&why);
                        continue;
                    }
                    let intent = self.parser.parse(&cmd);
                    let call = crate::policy::classify_with_policy(
                        &intent,
                        &self.memory,
                        &self.cfg.policy,
                    );
                    if call.needs_consent() && !self.plugin_step_trusted(&run, &cmd) {
                        run.needs_approval();
                        let from = match &run.plugin {
                            Some(id) => format!(" (from the {id} add-on)"),
                            None => String::new(),
                        };
                        // Said once, where it helps: an add-on's step that can
                        // be trusted is offered "always", so the same question
                        // need not come back every run.
                        let trustable = run.plugin.is_some()
                            && run.current().is_some_and(|s| s.command == cmd)
                            && crate::plugins::why_always_asks(&cmd, self.parser.parse_named(&cmd).1.as_deref()).is_none();
                        let always = if trustable {
                            " Say \"always\" and I won't ask about this step again."
                        } else {
                            ""
                        };
                        let q = format!(
                            "Step {} of {}{from} is \"{cmd}\" — go ahead?{always}",
                            run.position + 1,
                            run.steps.len()
                        );
                        if let Some(w) = self.mind.get_mut(self.flow_mind) {
                            w.think(crate::mind::Stage::Waiting, &q, t);
                        }
                        self.session.ask(&q);
                        said.push(q);
                        break;
                    }
                    let result = self.execute(&intent);
                    let ok = !result.starts_with("error");
                    self.journal.record_at(Act::Scheduled, &cmd, ok, t);
                    if let Some(w) = self.mind.get_mut(self.flow_mind) {
                        w.finish_step(step_i, (!ok).then(|| result.clone()));
                    }
                    run.report(&result, ok);
                }
                crate::flow::Next::Approve(q) => {
                    // Already paused and already asked; nothing to do until
                    // the answer arrives.
                    let _ = q;
                    break;
                }
                crate::flow::Next::Finished => {
                    let progress = self
                        .mind
                        .get_mut(self.flow_mind)
                        .map(|w| {
                            let p = w.progress();
                            w.think(crate::mind::Stage::Done, "finished", t);
                            p
                        });
                    said.push(match progress {
                        Some((done, total)) if total > 0 => {
                            format!("{}: done, {done} of {total} steps.", run.workflow)
                        }
                        _ => format!("{}: done.", run.workflow),
                    });
                    // A completed sequence is what `record_workflow` was
                    // built to remember — repeats increment rather than
                    // duplicate, which is what makes "you always do X after
                    // Y" detectable by `habits()` later. The store had no
                    // production writer, so `workflows` stayed empty for
                    // the life of every install.
                    self.memory.record_workflow(
                        &run.workflow,
                        run.steps.iter().map(|s| s.command.clone()).collect(),
                    );
                    // Said once, at the moment a sequence crosses the bar —
                    // not re-announced on every run after.
                    if self
                        .memory
                        .habits(3)
                        .iter()
                        .any(|w| w.trigger == run.workflow.trim().to_lowercase() && w.times_used == 3)
                    {
                        said.push(
                            "That's the third time through this one — I'll treat it as a habit."
                                .into(),
                        );
                    }
                    let _ = self.memory.save(&self.store);
                    self.flow_mind = 0;
                    return Some(said.join(" "));
                }
                crate::flow::Next::Stopped(why) => {
                    if let Some(w) = self.mind.get_mut(self.flow_mind) {
                        w.think(crate::mind::Stage::Stuck, &why, t);
                    }
                    said.push(format!("{} stopped: {why}", run.workflow));
                    // What already happened and stands (G7).
                    let chain = self.chain_of(&run, run.position);
                    let stands = crate::chain::what_stands(&chain);
                    if !stands.is_empty() {
                        said.push(format!("Already done and can't be taken back: {}.", stands.join("; ")));
                    }
                    self.flow_mind = 0;
                    return Some(said.join(" "));
                }
            }
        }
        self.current_flow = Some(run);
        (!said.is_empty()).then(|| said.join(" "))
    }

    /// One line on what the mind is doing, plus what carries on out of sight.
    ///
    /// One function, two panel call sites — the daemon does not keep a
    /// second copy of this sentence to drift.
    fn mind_summary(&self) -> String {
        let mut s = self.mind.now();
        let behind = self.mind.background();
        if !behind.is_empty() {
            let names: Vec<String> = behind.iter().map(|w| w.asked.clone()).collect();
            s.push_str(&format!(" In the background: {}.", names.join(", ")));
        }
        s
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
