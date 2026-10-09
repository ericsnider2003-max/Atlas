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
use crate::safety::{due_for_backup, prune_backups, BackupConfig, Trash};
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
mod camera;
pub use hands::{phone_retry_wait, PHONE_RETRY_EVERY_SECS, PHONE_RETRY_MOST, SAID_FOR_APPS_KEPT};
use hands::PHONE_RETRY_FILE;
pub use errands::CONNECTED_ACCOUNTS;
pub use tick::moment_clock;
mod away;
mod brief_prep;
mod file_moves;
// The workday collector needs this bounded snapshot entry point outside the
// daemon's child modules; its implementation remains internal.
pub(crate) fn brief_snapshot_value<T: serde::Serialize>(value: &T) -> std::result::Result<serde_json::Value, String> {
    brief_prep::snapshot_value(value)
}
mod tick;
pub use tick::AUTO_SYNC_EVERY_SECS;
mod inbox;
pub use inbox::{one_message_asked, OneMessage};
mod making;
mod calendar_delivery;
mod reading;
mod execute;
mod turn;
mod tasks;
mod operating;
mod learning;
mod askthelaptop;
mod onethread;

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
    // Mail accounts Atlas connected itself (`errands::CONNECTED_ACCOUNTS`),
    // after the ones you listed; one you listed by the same address wins.
    let connected: Vec<crate::mail::Account> = store.load(errands::CONNECTED_ACCOUNTS);
    for a in connected {
        if !t.mail.accounts.iter().any(|b| b.address.eq_ignore_ascii_case(&a.address)) {
            t.mail.accounts.push(a);
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
        // A picture being made keeps the graphics and every core busy.
        "make-picture" => TheWholeMachine,
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
            crate::heard!(std::fs::remove_dir_all(&dir));
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
        crate::heard!(std::fs::remove_dir_all(&dir));
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
    /// The file index being written on its own thread, and what it was
    /// when the copy was taken.
    index_saving: Option<((usize, u64), std::thread::JoinHandle<crate::error::Result<()>>)>,
    /// This save is the tick's sweep: the index may be written behind it.
    index_behind: bool,
    /// Atlas testing itself (`selftest`): background work isn't started,
    /// and commands that would touch real files, the network or the install
    /// are reported as what they'd do (`selftest::tier`).
    pub rehearsal: bool,
    /// What a rehearsal held back, since the last look.
    pub rehearsed: Vec<String>,
    /// The last command carried out, by kind: where a sentence actually went.
    last_executed: Option<String>,
    /// Typed result for the exact intent just executed. Handlers opt in as
    /// their real completion boundary is established; words are not proof.
    execution_receipt: Option<(Intent, crate::taskloop::Outcome)>,
    /// A job being worked in an app, step by step (`operate`).
    pub operating: Option<crate::operate::Job>,
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
    /// The camera kept open between the looks Atlas takes on its own, and
    /// when it was last asked for a frame (`one_frame`, 2 Oct 2026).
    kept_camera: Option<(crate::frames::Latest, std::time::Instant)>,
    /// A look running on its own thread (2 Oct 2026): the camera, the models
    /// and the last sight go with it and come back with the answer, so the
    /// loop never waits on a look.
    look_in_flight: Option<(crate::gaze::Reason, std::time::Instant, std::sync::mpsc::Receiver<LookDone>)>,
    /// The last look's thumbnail, when, and what it saw: a room that hasn't
    /// changed isn't run through the models again (`see_you`).
    last_sight: Option<(Vec<u8>, std::time::Instant, crate::vision::Sight)>,
    /// A watch in progress ("watch me for five minutes", `camwatch`).
    pub cam_watch: Option<crate::camwatch::Watcher>,
    /// A watch asked for before the camera was allowed: started on the yes.
    pub watch_after_allow: Option<(u64, bool)>,
    /// How a coding agent's program is looked for: this machine's PATH, or
    /// the tests' stand-in (`coding_agent`, 2 Oct 2026).
    pub(crate) agent_lookup: Option<fn(&str) -> Option<String>>,
    /// What Atlas last asked your yes for that only a yes may start -- a
    /// hand-over to a coding agent, running a build -- exactly as it will
    /// come back. A request carrying the marker any other way (a model's
    /// tool call, say) is asked about again, never taken as the yes.
    pub(crate) offered_for_yes: Option<String>,
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
    announcements: std::sync::OnceLock<crate::speakthread::Announcements>,
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
    /// A self-fix drafted and proven on the crew, waiting to be taken in
    /// when its errand ends (`on_itself::finish_own_fix`).
    self_fix_done: std::sync::Arc<std::sync::Mutex<Option<on_itself::SelfFixDone>>>,
    /// The proving test's run, from the crew (`finish_proof_check`).
    proof_check_done: std::sync::Arc<std::sync::Mutex<Option<(String, crate::selfwork::ProofToday)>>>,
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
    last_brief_attempt_at: u64,
    pending_brief_delivery: Option<(String, u64)>,
    brief_preparation: brief_prep::Live,
    file_moves: file_moves::Live,
    brief_requested_explicitly: bool,
    brief_requested_panel: bool,
    /// When Atlas last said hello of any kind (`returning::hello_now`),
    /// kept across a restart.
    last_greeted_at: u64,
    /// Rough turns in a row (`persona::spiral_line`).
    rough_in_a_row: u32,
    /// A "let's work" session under way (`worksession`), kept across a restart.
    work_session: Option<crate::worksession::Session>,
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
    /// When to list the sound devices again after a listing failed.
    audio_devices_retry_at: u64,
    /// The listing in progress, off the loop (6 Oct 2026: ffmpeg's device
    /// list held the first tick 2.4 s on Windows, and again every five
    /// minutes while it failed).
    audio_probe: Option<std::thread::JoinHandle<std::result::Result<Vec<crate::audio::Device>, String>>>,
    /// Where a message from another Atlas would arrive, if one is open.
    /// `None` for the overwhelming majority of installs, which will never
    /// have another Atlas to hear from -- see `with_signal_listener`.
    signal_listener: Option<crate::server::SignalListener>,
    /// The friends' door that couldn't be opened at start (its port busy),
    /// tried again once a minute: (port, who may come in, next try).
    signal_retry: Option<(u16, Vec<crate::kin::Peer>, u64)>,
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
    // A daemon constructed through the library retains the same restore read
    // lease as a launched command; snapshots still use the separate short lock.
    _state_access: crate::store::AccessGuard,
    pub autonomy: Autonomy,
    /// One record of everything Atlas did, across every area, so "what did
    /// you do" and "undo that" have something to answer from.
    pub history: crate::undo::History,
    approved_undo: Option<late::ApprovedUndo>,
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
    /// The last reminder said when it came due, and when (`keeping`: "snooze").
    pub last_reminder_fired: Option<(String, u64)>,
    pending_reminder_edit: Option<helping::PendingReminderEdit>,
    /// Notes a push to your phone failed for, tried again every few minutes
    /// while you're still away (`tick`). 30 Sep 2026: one failed push -- a
    /// phone on a lift, the push server restarting -- meant the note waited
    /// until you were back at the desk, which is what the phone was for.
    pub phone_to_retry: Vec<crate::notify::Note>,
    /// What the background said, numbered, for an app on the phone to show
    /// as notifications (`/hub/live.json`'s `said`). The phone has no
    /// speaker loop: its tick output was dropped (30 Sep 2026), so a
    /// reminder set on the phone never appeared.
    pub said_for_apps: Vec<(u64, String)>,
    pub phone_retry_at: u64,
    /// Failed tries in a row, for the wait before the next (`retry_phone`).
    phone_retry_tries: u32,
    /// The last reminder or timer set ("cancel that reminder").
    pub last_reminder_set: Option<u64>,
    /// "Remind me to X" with no time: X, until you say when.
    pub reminder_waiting_for_a_time: Option<String>,
    /// The town the weather was last given for, kept for the session.
    pub weather_place: Option<crate::weather::Place>,
    /// Whose draft was last read out ("send it").
    pub draft_last_read: Option<String>,
    pub draft_last_read_identity: Option<crate::outbox::PendingReply>,
    /// Notes routed to speaking (`reach_you`), said on the next tick.
    pub to_say_aloud: Vec<String>,
    /// Where the vault is kept (`vault_home_for`).
    pub vault_home: crate::store::Store,
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
    proposals_recovery: Option<Vec<crate::booking::Proposal>>,
    calendar_cancel_pending: bool,
    calendar_delivery_bindings: Vec<(String, String, bool)>,
    calendar_active_delivery_ids: Vec<String>,
    calendar_booking_problem: Option<String>,
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
    /// What arrived from your other devices, so it isn't carried straight
    /// back out (`onethread`, item 16).
    pub(crate) arrived_by_sync: onethread::ArrivedBySync,
    /// A turn added to the sync log and it isn't written out yet.
    pub(crate) synclog_unsaved: bool,
    /// Other devices' reminders cancelled here, to be told on the next carry
    /// (`onethread::cancel_elsewhere`).
    pub(crate) reminders_cancelled_elsewhere: Vec<String>,
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
    /// How you talk: wordings learned from being corrected (`phrasebook`,
    /// 2 Oct 2026).
    pub phrasebook: crate::phrasebook::Phrasebook,
    /// What Atlas got wrong, and what it misheard (`misses`).
    pub misses: crate::misses::MissLog,
    /// The last turn, as it ended: what a correction, a rephrase or an undo
    /// is about (`learning`).
    last_outcome: Option<learning::Outcome>,
    /// A turn is being watched for how it ends; one inside it (a correction
    /// running the words it meant) is not watched twice.
    watching_turn: bool,
    /// The words this turn came in by voice as, when it did.
    spoken_turn: Option<String>,
    /// This turn was routed by a learned wording: which, and whether it is
    /// sure yet.
    routed_by_phrase: Option<(String, bool)>,
    /// The connection lines last written to the log, so each is written once.
    connections_logged: Vec<String>,
    /// The tier-mix line last logged, so it is logged once each time it changes.
    tier_mix_logged: Option<String>,
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
    /// When typing-only last looked again at whether the voice tools work.
    audio_look_at: u64,
    /// When to list the sound devices again and see whether a different
    /// microphone should be recorded from (29 Sep 2026).
    mic_look_at: u64,
    /// Push-to-talk because the microphone failed: how many recordings had
    /// given sound when it dropped (`back_to_the_wake_word`).
    mic_heard_before_ptt: Option<u64>,
    /// When to try the microphone again, while that lasts.
    mic_probe_at: u64,
    /// That listing, running off the loop.
    mic_listing: Option<std::sync::mpsc::Receiver<std::result::Result<Vec<crate::audio::Device>, String>>>,
    /// The voice tools weren't there at start (not a failing microphone).
    audio_tools_missing: bool,
    /// What was typed or said on the hub's Talk page, waiting for its turn:
    /// (words, said aloud). Answered on the next tick rather than inside the
    /// request, so sending never hangs the page while the model thinks.
    pub(crate) talk_queue: Vec<(String, bool)>,
    /// A turn whose model call is running on a worker thread, so the daemon
    /// loop -- the hub, the ticks -- carries on while the model thinks
    /// (27 Sep 2026: the whole hub hung for as long as a Talk reply took).
    pending_turn: Option<PendingTurn>,
    /// The deep model for background work, beside the talking one
    /// (`deepbrain`, 30 Sep 2026): started when such work comes, stopped
    /// when idle, giving way to every turn.
    pub(crate) deep: crate::deepbrain::DeepBrain,
    /// The model code is written with, swapped in for a build (`coder`).
    pub(crate) coder: crate::coder::Coder,
    /// When to look again for the coding model's file.
    coder_look_at: u64,
    /// When to look for the deep model's file again.
    deep_look_at: u64,
    /// A request of several steps being worked through on a worker
    /// (`tasks::work_through`, 30 Sep 2026): its steps are carried out and
    /// said by the tick.
    task_loop: Option<tasks::TaskLoop>,
    /// Exact job selected by the most recent handoff, including joining a run.
    last_crew_handoff: Option<u64>,
    /// Whether what a restart cut off (`LEFT_WAITING`) has been read back
    /// yet; until then it isn't written over.
    left_waiting_read: bool,
    /// Atlas's own CPU and where the loop's time goes, by window (`cpuuse`).
    cpu_meter: crate::cpuuse::Meter,
    /// `models.talk` as last followed (`follow_the_talk_setting`), and the
    /// model the talking server was started on.
    talk_setting_seen: Option<String>,
    model_running_id: Option<String>,
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
    /// When Atlas last started its model server, while that start is still
    /// being watched for dying young.
    model_started: Option<std::time::Instant>,
    /// Model servers that ended soon after being started, in a row: each
    /// doubles the pause before the next try (29 Sep 2026).
    model_deaths: u32,
    /// The model server was let go after its idle time (Phase 0.2, 1 Oct
    /// 2026): it is started again by the next thing you say, not by the
    /// loop's next pass. On 30 Sep it was stopped "idle" and started again
    /// within seconds, thirteen times, with nobody talking -- each start
    /// losing the warm prompt the last one had read.
    pub(crate) model_rested: bool,
    /// The record of runs (`whystopped`, item 33), kept only by the Atlas that
    /// runs the loop: a test or a typed prompt never writes it.
    pub(crate) runs: Option<crate::whystopped::Runs>,
    /// When the model's answer came back this turn (`TurnNews::Done`), so
    /// the turn's "doing" stops there rather than after the reply has been
    /// played (Phase 0.2).
    pub(crate) model_done_at: std::cell::Cell<Option<std::time::Instant>>,
    /// A model call failed while our model server was running: it is asked
    /// whether it is still answering, and restarted if not.
    model_suspect: bool,
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
    /// The few of them a sentence needs (`router`, 30 Sep 2026).
    router: crate::router::Router,
    /// The meaning encoder, resident, and every tool's vector (`meaningroute`).
    /// Started once from the tick when an encoder is installed.
    meaning_route: Option<crate::meaningroute::Route>,
    meaning_route_tried: bool,
    /// Where the last tick spent its time (`timing::Laps`).
    tick_laps: crate::timing::Laps,
    /// Which abilities requests have used (`used`), and when it was last saved.
    pub(crate) used: crate::used::Used,
    used_saved: u64,
    /// The meaning model was asked for: start the encoder once it lands.
    meaning_route_retry: bool,
    /// The parts of the last request of several parts, and where each
    /// stands (`streams`), for "what are you working on".
    streams: Vec<crate::streams::Stream>,
    /// How long after the model was asked the first sentence of this turn's
    /// reply went to be spoken (30 Sep 2026): the wait you actually hear,
    /// for the turn's timing line. Taken by `log_turn_timing`.
    first_words_ms: std::cell::Cell<Option<u64>>,
    /// End of speech to first sound for the turn just said (Phase 0.2),
    /// for the timing line.
    silence_ms: std::cell::Cell<Option<u64>>,
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
    pub(crate) signing_pending: Option<crate::hublive::SigningPending>,
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
    publication_jobs: std::collections::HashMap<u64, (u64, u64)>,
    /// The automatic sync's sends to your other devices, made off the loop
    /// (5 Oct 2026, Q5): each waited up to 4 s for a device that was asleep
    /// or away, every quarter hour, with Atlas frozen meanwhile. Set while
    /// the automatic pass runs; the sends it would have made are collected
    /// in `dials` and made on a thread, and their answers come back through
    /// `dial_answers` to the next pass.
    pub(crate) dial_later: bool,
    pub(crate) dials: Vec<tick::Dial>,
    pub(crate) dial_answers: Option<std::sync::mpsc::Receiver<(tick::Dial, std::result::Result<Vec<u8>, String>)>>,
    /// A button that can't be undone, waiting for your yes: (window, name, app).
    pending_press: Option<(u64, String, String)>,
    /// A storage plan shown and waiting for your yes.
    pending_storage: Option<crate::tune::StoragePlan>,
    /// What an optimization run offered to do, waiting on your yes.
    pending_optimize: Option<crate::tune::Plan>,
    /// A folder's sorting plan, said and waiting for your yes ("tidy my
    /// desktop", 29 Sep 2026; since 2 Oct 2026 any folder, by kind, with
    /// copies and old installers to "To review" -- `organize::SortPlan`).
    pending_desktop: Option<crate::organize::SortPlan>,
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
    /// When the last automatic sync pass ran (`sync.automatic`).
    pub last_auto_sync: u64,
    /// When Atlas last looked for missing downloads to fetch itself
    /// (`keep_everything_here`); 0 until it has.
    pub last_top_up: u64,
    /// "Get to know me", in progress (`getknow`).
    pub interview: Option<crate::getknow::Interview>,
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
    scheduled_anchors_pending: std::collections::BTreeMap<String, u64>,
    scheduled_notices: std::collections::BTreeSet<String>,
    last_backup: u64,
    last_backup_attempt: u64,
    /// When the backup schedule and the self-repair sweep are next looked
    /// at (2 Oct 2026: both were looked at every tick, from disk).
    next_backup_look: u64,
    next_sweep_look: u64,
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
        Self::try_new(cfg, plat, llm, store, proactive).unwrap_or_else(|error| panic!("Atlas could not safely open its configured output roots: {error}"))
    }

    pub fn try_new(
        cfg: &'a Config,
        plat: &'a dyn Platform,
        llm: Option<std::sync::Arc<dyn Llm>>,
        store: Store,
        proactive: Proactive,
    ) -> crate::error::Result<Self> {
        let state_access = crate::store::begin_state_command(store.root(), false)?;
        let notes: std::path::PathBuf = cfg.tools.as_ref().map(|tools| tools.research.clone()).unwrap_or_default().resolved(&store.install_root()).notes_dir.into();
        let trash = cfg.tools.as_ref().map(|tools| tools.trash.clone()).unwrap_or_default().resolved(&store.install_root());
        crate::safety::check_configured_scope(&store, &notes, std::path::Path::new(&trash.dir))?;
        let store_for_load = store.clone();
        let store_for_vault = store.clone();
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
        let keep_resident = crate::fit::plan_for(&here).keep_model_warm;
        crate::brain::set_keep_warm(keep_resident);
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
            index_saving: None,
            index_behind: false,
            rehearsal: false,
            rehearsed: Vec::new(),
            last_executed: None,
            execution_receipt: None,
            operating: None,
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
            kept_camera: None,
            look_in_flight: None,
            last_sight: None,
            cam_watch: None,
            watch_after_allow: None,
            agent_lookup: None,
            offered_for_yes: None,
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
            announcements: std::sync::OnceLock::new(),
            library: crate::recall::Library::default(),
            // Loaded before `reload_library` runs below, so the rebuilt
            // library rehydrates vectors made in an earlier session rather
            // than starting every note over as unembedded.
            meaning: crate::meaning::Remembered::load(&store_for_load),
            search_check_due: store_for_load.load::<Vec<crate::recall::SearchCheck>>("search_checks").is_empty(),
            contents: crate::contents::Contents::default(),
            trace: crate::trace::Trace::default(),
            last_brief: brief_prep::restored_receipt(&store_for_load).map(|(source, _)| source).unwrap_or_else(|| store_for_load.load::<Option<u64>>("last_brief_source_at").unwrap_or_else(|| store_for_load.load("last_brief_at"))),
            // Loaded rather than zeroed: a restart would otherwise give you
            // the day's run a second time, and a brief said twice is the
            // failure `morning_brief` already documents.
            last_brief_at: brief_prep::restored_receipt(&store_for_load).map(|(_, delivered)| delivered).unwrap_or_else(|| store_for_load.load::<u64>("last_brief_at")),
            last_brief_attempt_at: brief_prep::restored_attempt(&store_for_load),
            pending_brief_delivery: brief_prep::restored_pending(&store_for_load),
            brief_preparation: brief_prep::Live::load(&store_for_load),
            file_moves: file_moves::Live::default(),
            brief_requested_explicitly: false,
            brief_requested_panel: false,
            last_greeted_at: store_for_load.load::<u64>("last_greeted_at"),
            rough_in_a_row: 0,
            work_session: store_for_load.load("work_session"),
            // Loaded too, and for the opposite reason. Zeroed, a restart
            // looks like an infinite gap, so every restart would read as you
            // arriving -- and restarts happen in the middle of the night you
            // are working through.
            last_turn_of_yours: store_for_load.load::<u64>("last_turn_of_yours"),
            mending: store_for_load.load("mending"),
            selfwork: store_for_load.load("selfwork"),
            pending_landing: Vec::new(),
            self_fix_done: Default::default(),
            proof_check_done: Default::default(),
            pending_correction: None,
            last_said: String::new(),
            pending_edit: None,
            outbox: crate::notify::Outbox::load(&store_for_load2),
            audio_devices: None,
            audio_devices_retry_at: 0,
            audio_probe: None,
            signal_listener: None,
            signal_retry: None,
            sync_server: None,
            tier_mix: crate::tier::Mix::default(),
            nudger: {
                // Your goals, kept across a restart (F4): the nudges toward
                // them had nothing to nudge about while this list started
                // empty every time.
                let mut n = crate::nudge::Nudger::new(crate::nudge::NudgeConfig::default());
                n.goals = store_for_load.load(crate::nudge::GOALS);
                n.set_last_daypart(store_for_load.load("greeted_part"));
                n
            },
            store,
            _state_access: state_access,
            // Kept across a restart: "undo" the morning after is still undo.
            history: store_for_load.load_checked("undo_history")?.unwrap_or_default(),
            approved_undo: None,
            decisions: crate::why::Record::default(),
            offered_handover: false,
            asks_quietly: None,
            // From the install's own state, not the active profile's --
            // see `vault::Vault::FILE`. Loaded rather than defaulted:
            // a `default()` vault has no salt and no check value, so it
            // held nothing across a restart and its passphrase was set
            // afresh by the first unlock of every run.
            vault: if keeps_the_install_vault(&store_for_vault) {
                crate::vault::Vault::load(&crate::roots::install_state())
            } else {
                crate::vault::Vault::load(&store_for_vault)
            },
            last_reminder_fired: None,
            pending_reminder_edit: None,
            // Kept across a restart (research report, Stage 1 item 10).
            phone_to_retry: store_for_load.load(PHONE_RETRY_FILE),
            said_for_apps: Vec::new(),
            phone_retry_at: 0,
            phone_retry_tries: 0,
            last_reminder_set: None,
            reminder_waiting_for_a_time: None,
            weather_place: None,
            draft_last_read: None,
            draft_last_read_identity: None,
            to_say_aloud: Vec::new(),
            vault_home: if keeps_the_install_vault(&store_for_vault) { crate::roots::install_state() } else { store_for_vault.clone() },
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
            // Loaded, so an item taken off Outstanding stays off after a
            // restart (2 Oct 2026). Nothing was ever kept here before, so the
            // first start after this finds nothing and begins empty, as it
            // always did.
            workspace: store_for_load.load("workspace"),
            layout: crate::layout_prefs::Layout::default_layout(),
            last_seen: 0,
            carried: Vec::new(),
            proposals,
            proposals_recovery: None,
            calendar_cancel_pending: false,
            calendar_delivery_bindings: Vec::new(),
            calendar_active_delivery_ids: Vec::new(),
            calendar_booking_problem: None,
            calendar: crate::calendar::Calendar::load(&store_for_load),
            daily_history: store_for_load.load("daily_history"),
            rhythm: store_for_load2.load("rhythm"),
            dropped: store_for_load.load("dropped"),
            wants_seen: store_for_load.load("wants_seen"),
            interrupted: Vec::new(),
            signals: Vec::new(),
            run: crate::channel::Run::default(),
            facts: crate::facts::Book::load_checked(&store_for_load)?,
            arrived_by_sync: Default::default(),
            synclog_unsaved: false,
            reminders_cancelled_elsewhere: Vec::new(),
            known,
            stones: store_for_load.load("known_stones"),
            vocab: store_for_load.load("vocabulary"),
            folding: false,
            running: crate::awake::Running::Awake,
            heard_note: None,
            phrasebook: crate::phrasebook::Phrasebook::load(&store_for_load),
            misses: crate::misses::MissLog::load(&store_for_load),
            last_outcome: None,
            watching_turn: false,
            spoken_turn: None,
            routed_by_phrase: None,
            connections_logged: Vec::new(),
            tier_mix_logged: None,
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
            audio_look_at: 0,
            mic_look_at: crate::store::now() + MIC_LOOK_EVERY_SECS,
            mic_heard_before_ptt: None,
            mic_probe_at: 0,
            mic_listing: None,
            audio_tools_missing: false,
            talk_queue: Vec::new(),
            pending_turn: None,
            deep: crate::deepbrain::DeepBrain::none(),
            deep_look_at: 0,
            coder: crate::coder::Coder::none(),
            coder_look_at: 0,
            task_loop: None,
            last_crew_handoff: None,
            left_waiting_read: false,
            cpu_meter: Default::default(),
            talk_setting_seen: None,
            model_running_id: None,
            pending_seq: 0,
            also_asked: Vec::new(),
            rephrase_ok: false,
            rephrase_ask: None,
            acting_at: None,
            model_server_seen: Default::default(),
            model_probe_busy: Default::default(),
            model_warmed: Default::default(),
            model_start_tried: None,
            model_started: None,
            model_deaths: 0,
            model_rested: false,
            runs: None,
            model_done_at: std::cell::Cell::new(None),
            model_suspect: false,
            defer_turns: false,
            may_defer: false,
            decided_already: None,
            decided_in_ms: None,
            talk_partial: Default::default(),
            pending_stamp: None,
            last_spoke_at: 0,
            router: crate::router::Router::new(&crate::intent::ToolBook::new(&cfg.commands)),
            meaning_route: None,
            meaning_route_tried: false,
            tick_laps: crate::timing::Laps::start(),
            used: store_for_load.load(crate::used::KEY),
            used_saved: 0,
            meaning_route_retry: false,
            streams: Vec::new(),
            first_words_ms: std::cell::Cell::new(None),
            silence_ms: std::cell::Cell::new(None),
            by_chat: false,
            model_server_trouble: None,
            helpers: crate::lifecycle::Helpers::new(crate::lifecycle::model_stays_when_it_fits(
                cfg.tools.as_ref().map(|t| t.lifecycle.clone()).unwrap_or_default(),
                keep_resident,
            )),
            crew: Crew::with_limits(crew_limits).with_room(Box::new(plat.crew_room())),
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
            publication_jobs: Default::default(),
            dial_later: false,
            dials: Vec::new(),
            dial_answers: None,
            pending_press: None,
            pending_storage: None,
            pending_optimize: None,
            pending_desktop: None,
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
            last_auto_sync: 0,
            last_top_up: 0,
            interview: None,
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
            queue: Queue::load_checked(&store_for_load2)?,
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
            scheduled_anchors_pending: std::collections::BTreeMap::new(),
            scheduled_notices: std::collections::BTreeSet::new(),
            signing_pending: None,
            last_backup: 0,
            last_backup_attempt: 0,
            next_backup_look: 0,
            next_sweep_look: 0,
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
        Ok(d)
    }

    // ---------- one spoken turn ----------

}

/// Beyond the follow-up window itself, how long the words may take to be
/// made out before the loop stops waiting (`Daemon::follow_up`).
const FOLLOW_UP_HEARING_SECS: u64 = 30;

/// Where what cutting in by voice learned about the speakers and the
/// microphone is kept between runs (`micthread::Learned`).
const CUT_IN_STATE: &str = "cut_in";


/// The permission gate's answer for acting on an app: go ahead, or ask this
/// first (and the action is parked as a pending approval).
enum AppGate {
    Go,
    Ask(String),
}


// ---------- reminders and opportunities: free helpers ----------
// Used by `Daemon::remind_help` and `Daemon::spot_opportunity`. Free functions
// rather than methods because they touch no daemon state — just words in, a
// value out — which keeps them unit-testable without a whole daemon.


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


// ---------------------------------------------------------------------------
// Dropping a task, and bringing back what you dropped (Eric's ruling H8)


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


// ---------------------------------------------------------------------------
// The keys, changed by saying them (Eric, 26 Sep 2026: "it needs to be
// customizable. I don't have an Alt button")


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
/// Kept at six (30 Sep 2026, the prompt diet): the window's start steps
/// forward three exchanges at a time, so the model server can reuse the
/// conversation it already read on two turns in three
/// (`speed_measured::a_conversation_rereads_only_what_is_new`); what keeps
/// the prompt small is `HISTORY_TOKENS`, and past replies going back as
/// their first sentences.
pub const HISTORY_EXCHANGES: usize = 6;
/// How long nothing has been said before the conversation is summarised
/// (`fold_if_due`): the summary is a model call, and shares the model with
/// the conversation.
pub const FOLD_WHEN_QUIET_SECS: u64 = 90;
/// How many of Atlas's last replies a new one is checked against for
/// saying the same again (`brain::Turn::recent_replies`).
pub const REPLIES_CHECKED: usize = 4;
/// The most sentences a spoken reply is asked for, unless more was asked
/// for (`persona::asks_for_more`).
pub const SPOKEN_SENTENCES: usize = 3;
/// Said to the model when a sentence reads as a request (`doing::looks_like_an_action`).
pub const ACTION_OR_SAY_SO: &str = "This is a request: if a tool does it, call it now. If none does, say in one \
sentence what you can do instead. Don't chat around it.";
/// 30 Sep 2026: 1200 -> 350, with `HISTORY_EXCHANGES` (the prompt diet).
pub const HISTORY_TOKENS: usize = 350;

/// Lines of the summary of older talk, at most, that go in when they bear
/// on what was said.
pub const SUMMARY_LINES: usize = 2;

/// Lines from the capability catalogue a question about Atlas gets.
pub const ABILITY_LINES: usize = 2;

/// The standing rules learned from corrections, at most this many characters.
pub const LEARNED_CHARS: usize = 300;

/// How many facts you told Atlas go in front of the model every turn.
/// 30 Sep 2026: 10 -> 4, each cut to 100 characters (the prompt diet);
/// the rest come in as hints when they bear on what was said.
pub const FACTS_IN_PROMPT: usize = 8;

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


/// How long a finding that another model server is (or isn't) up holds.
const MODEL_SERVER_RECHECK: std::time::Duration = std::time::Duration::from_secs(60);

/// How long a spoken question waits in silence before Atlas says its model is
/// still loading, when it is.
pub const STILL_LOADING_AFTER: std::time::Duration = std::time::Duration::from_secs(4);

/// What it says then.
pub const STILL_LOADING_WORDS: &str = "One moment -- my language model is still loading. The first answer takes about a minute.";

/// A model server this young may still be loading its model: not judged
/// stuck for not answering yet.
const MODEL_SERVER_LOADING: std::time::Duration = std::time::Duration::from_secs(150);

/// A model server that ends within this long of being started died young.
const MODEL_SERVER_YOUNG: std::time::Duration = std::time::Duration::from_secs(300);

/// The longest a turn waits to learn whether one is up.
const MODEL_PROBE_WAIT: std::time::Duration = std::time::Duration::from_millis(800);

/// How long `speak_while_thinking` waits on the model before answering the
/// hub and checking for a pause again.
const THINKING_SLICE_MS: u64 = 50;

/// How many tools beyond the core ones a sentence is offered.
pub const RETRIEVED_TOOLS: usize = 6;


/// How often the microphones are listed again.
pub const MIC_LOOK_EVERY_SECS: u64 = 180;

/// Push-to-talk after the microphone failed: how often it is tried again.
pub const MIC_PROBE_EVERY_SECS: u64 = 20;

/// Your name on its own: how long Atlas waits for the rest after "Yes?".
pub const NAME_ALONE_WAIT_SECS: u32 = 8;


/// What a look on its own thread hands back: its answer, and everything it
/// took with it (`Daemon::look_in_flight`).
pub(crate) struct LookDone {
    pub printed: String,
    /// The look's own work, not the wait for the tick to collect it.
    pub spent_ms: u64,
    pub camera: Option<crate::frames::Latest>,
    pub opened_camera: bool,
    pub looking: Option<crate::vision::Looking>,
    pub last_sight: Option<(Vec<u8>, std::time::Instant, crate::vision::Sight)>,
}

// The rest of this module, by what it does (audit Q6, 6 Oct 2026).
mod said_words;
pub use said_words::*;
mod project_checks;
use project_checks::*;
mod small_words;
pub use small_words::*;
mod mail_out;
use mail_out::*;
mod time_words;
use time_words::*;
mod seats_and_signin;
pub use seats_and_signin::*;
mod files_off_the_loop;
use files_off_the_loop::*;
mod turn_words;
pub use turn_words::*;

