//! Internet is an enhancement, never a dependency.
//!
//! Atlas runs on your laptop. Everything that makes it useful day to day —
//! hearing you, speaking, controlling the workspace, searching your files,
//! remembering, scheduling, reasoning — happens locally with no network at
//! all. The internet only adds *reach*: web research, and optionally a hosted
//! model if you choose one over the local default.
//!
//! Two rules enforced here:
//!   1. A local capability must never be blocked by a network check.
//!   2. Something that genuinely needs the internet is deferred and retried,
//!      not failed silently and forgotten.

use crate::intent::Intent;
use serde::Deserialize;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    Online,
    Offline,
    /// Not checked recently enough to say. Treated as online for *preference*
    /// decisions and as offline for *requirement* decisions — optimism where
    /// it is free, caution where it costs a failed job.
    Unknown,
}

/// What a piece of work needs to succeed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// Works with the network cable pulled out. Most of Atlas.
    Local,
    /// Cannot happen offline. Defer it.
    Internet,
    /// Better online, still works offline in reduced form.
    PrefersInternet,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ConnectivityConfig {
    /// Host:port for the reachability probe. A raw TCP connect, so it does not
    /// depend on DNS, HTTP, or any particular service being up.
    pub probe: String,
    pub timeout_ms: u64,
    /// How long a result is trusted before re-probing.
    pub cache_secs: u64,
    /// Never probe at all — assume offline. For a machine you keep air-gapped.
    pub assume_offline: bool,
}

impl Default for ConnectivityConfig {
    fn default() -> Self {
        ConnectivityConfig {
            probe: SHIPPED_PROBE.into(),
            timeout_ms: 800,
            cache_secs: 30,
            assume_offline: false,
        }
    }
}

pub struct Connectivity {
    pub cfg: ConnectivityConfig,
    last_checked: u64,
    last_result: Reach,
    /// An override that survives cache expiry. Set when you tell Atlas to
    /// treat the connection as up or down regardless of what it can probe —
    /// useful on a captive-portal network where the probe lies.
    pinned: Option<Reach>,
    /// A re-probe running on a thread of its own (`status_now`).
    refreshing: Option<std::sync::mpsc::Receiver<Reach>>,
}

impl Default for Connectivity {
    fn default() -> Self {
        Connectivity::new(ConnectivityConfig::default())
    }
}

impl Connectivity {
    pub fn new(cfg: ConnectivityConfig) -> Self {
        Connectivity { cfg, last_checked: 0, last_result: Reach::Unknown, pinned: None, refreshing: None }
    }

    /// Cached reachability. Cheap to call every tick.
    ///
    /// Note this is never called on the path to a local capability — it exists
    /// only to decide whether an internet-needing job can run yet.
    pub fn status(&mut self, t: u64) -> Reach {
        if let Some(p) = self.pinned {
            return p;
        }
        if self.cfg.assume_offline {
            // Must record it, not just return it — callers read `cached()`,
            // and leaving it Unknown meant blocked work was never filed.
            self.last_result = Reach::Offline;
            self.last_checked = t;
            return Reach::Offline;
        }
        if self.last_result != Reach::Unknown && t.saturating_sub(self.last_checked) < self.cfg.cache_secs {
            return self.last_result;
        }
        self.last_checked = t;
        self.last_result = if probe(&self.cfg.probe, self.cfg.timeout_ms) {
            Reach::Online
        } else {
            Reach::Offline
        };
        self.last_result
    }

    /// `status` for the daemon's loop (28 Sep 2026): once there is an
    /// answer, a stale one is kept while the probe runs again on a thread of
    /// its own, and the new answer is taken on a later call.
    ///
    /// The tick asked `status` every pass, and every thirty seconds that was
    /// a TCP connect on the loop's thread -- up to `timeout_ms` (800ms) with
    /// the hub and the typing box waiting, on any network that drops the
    /// probe rather than refusing it (measured here: 808ms, every 30s).
    /// With no answer yet (at start, or after `invalidate`) it answers
    /// `Unknown` until the first probe is back -- at start that probe held
    /// the first tick, and so the hub's first page, for as long as it took.
    /// `Unknown` is already "not online" to everything that needs the
    /// network, and the tick decides nothing online-or-not on it.
    pub fn status_now(&mut self, t: u64) -> Reach {
        if self.pinned.is_some() || self.cfg.assume_offline {
            self.refreshing = None;
            return self.status(t);
        }
        if let Some(rx) = &self.refreshing {
            match rx.try_recv() {
                Ok(r) => {
                    self.last_result = r;
                    self.last_checked = t;
                    self.refreshing = None;
                    return r;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return self.last_result,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.refreshing = None,
            }
        }
        if self.last_result != Reach::Unknown && t.saturating_sub(self.last_checked) < self.cfg.cache_secs {
            return self.last_result;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let (target, ms) = (self.cfg.probe.clone(), self.cfg.timeout_ms);
        let started = std::thread::Builder::new().name("atlas-online-probe".into()).spawn(move || {
            let _ = tx.send(if probe(&target, ms) { Reach::Online } else { Reach::Offline });
        });
        match started {
            Ok(_) => {
                self.refreshing = Some(rx);
                self.last_result
            }
            // No thread to spare: asked here, as before.
            Err(_) => self.status(t),
        }
    }

    /// Force a re-probe on the next call — e.g. after a fetch fails.
    pub fn invalidate(&mut self) {
        if self.pinned.is_none() {
            self.last_result = Reach::Unknown;
            self.refreshing = None;
        }
    }

    pub fn cached(&self) -> Reach {
        self.last_result
    }

    /// Pin the connection state, overriding the probe until unpinned. For a
    /// captive-portal network where a TCP probe succeeds but nothing works,
    /// and for deterministic tests.
    pub fn set(&mut self, reach: Reach, t: u64) {
        self.pinned = Some(reach);
        self.last_result = reach;
        self.last_checked = t;
    }

    pub fn unpin(&mut self) {
        self.pinned = None;
        self.last_result = Reach::Unknown;
    }

    /// Can this run right now?
    pub fn allows(&self, need: Need) -> bool {
        match need {
            Need::Local | Need::PrefersInternet => true,
            Need::Internet => self.last_result == Reach::Online,
        }
    }
}

/// Raw TCP connect. No DNS lookup when given a literal address, no HTTP, no
/// dependency on any service staying up.
///
/// Every address in `target` (a comma-separated list) and then
/// [`ALSO_TRIED`], all at once: online if any answers (29 Sep 2026: the one
/// address was 1.1.1.1 on port 53, which plenty of home routers, providers
/// and security suites block for TCP. Eric's laptop, online, read "the
/// internet -- failing: no route out" all day, and everything that needs the
/// web held back).
fn probe(target: &str, timeout_ms: u64) -> bool {
    let mut targets: Vec<String> = probe_targets(target);
    // Only with the shipped setting: an address you chose is asked alone.
    let extras: &[&str] = if target.trim() == SHIPPED_PROBE { &ALSO_TRIED } else { &[] };
    for extra in extras {
        if !targets.iter().any(|t| t == extra) {
            targets.push(extra.to_string());
        }
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let mut asked = 0;
    for t in targets {
        let tx = tx.clone();
        let spawned = std::thread::Builder::new().name("atlas-online-try".into()).spawn(move || {
            let _ = tx.send(connects(&t, timeout_ms));
        });
        if spawned.is_ok() {
            asked += 1;
        }
    }
    drop(tx);
    let until = std::time::Instant::now() + Duration::from_millis(timeout_ms + 200);
    for _ in 0..asked {
        let left = until.saturating_duration_since(std::time::Instant::now());
        match rx.recv_timeout(left) {
            Ok(true) => return true,
            Ok(false) => {}
            Err(_) => return false,
        }
    }
    false
}

/// The probe setting Atlas ships with.
pub const SHIPPED_PROBE: &str = "1.1.1.1:53";

/// Addresses tried as well as the shipped one: ordinary HTTPS, which a
/// network that lets anything out lets out.
pub const ALSO_TRIED: [&str; 3] = ["1.1.1.1:443", "8.8.8.8:443", "9.9.9.9:443"];

/// The addresses in a `probe` setting: comma- or space-separated.
pub fn probe_targets(target: &str) -> Vec<String> {
    target.split(|c: char| c == ',' || c.is_whitespace()).map(str::trim).filter(|t| !t.is_empty()).map(String::from).collect()
}

fn connects(target: &str, timeout_ms: u64) -> bool {
    let addrs: Vec<SocketAddr> = match target.to_socket_addrs() {
        Ok(a) => a.collect(),
        Err(_) => return false,
    };
    addrs
        .iter()
        .any(|a| TcpStream::connect_timeout(a, Duration::from_millis(timeout_ms)).is_ok())
}

/// What each intent needs.
///
/// The important property of this table is how much of it says `Local`. If a
/// new capability lands here as `Internet` it should be because it genuinely
/// cannot work any other way.
pub fn need_of(intent: &Intent) -> Need {
    match intent {
        // Another program on this machine (`mcp`). Whether *it* needs the
        // internet is its own business, and it says so when it fails.
        Intent::McpTool(_) => Need::Local,
        Intent::Gestures(_) => Need::Local,
        // Typing into a window on this desk. The transcription that produced
        // the words happened before this intent existed, and is the STT
        // tool's need, not dictation's.
        Intent::Dictate(_) => Need::Local,
        // The models are compiled in and the camera is on this desk. Seeing
        // is one of the few things that works identically with the router
        // unplugged.
        Intent::WhatsThere | Intent::WhatsThis | Intent::NameThis(_) => Need::Local,
        Intent::CallNotes(_) => Need::Local,
        Intent::Recommend => Need::Local,
        // The procedures ship compiled into Atlas, so matching a symptom
        // against their snags never reaches a network -- the whole reason
        // knowhow exists is to answer when the internet doesn't.
        Intent::Diagnose(_) => Need::Local,
        // Same reason as `Diagnose`: the procedures are compiled in, so
        // reading the steps of one back never reaches a network.
        Intent::WalkThrough(_) => Need::Local,
        Intent::AddressAs(_) => Need::Local,
        // The notes are a folder on this disk and the index describes that
        // folder. Neither reading nor rebuilding it touches a network.
        Intent::RebuildIndex | Intent::WhatIHave(_) => Need::Local,
        // Reading the log of model calls never makes one.
        Intent::ModelTrace => Need::Local,
        // Five model calls. Local when the model is local, which is the
        // ordinary case here — the same need the model itself has.
        Intent::AskTheRoom(_) => Need::Local,
        Intent::GotItWrong(_) | Intent::ApplyLesson | Intent::HowAmIDoing => Need::Local,
        // The work log never leaves the machine.
        Intent::TimeSpent(_) => Need::Local,
        // Round 11: everything but feeds works offline; a feed is read when
        // the network is there and waits when it isn't -- the list of what's
        // already fetched still answers.
        Intent::ClipHistory(_)
        | Intent::ScreenText(_)
        | Intent::MarketDay(_)
        | Intent::WaitingFor(_)
        | Intent::NoteReview(_)
        | Intent::Launch(_)
        | Intent::TradeDay(_)
        | Intent::MeetingPrep(_)
        | Intent::Snippet(_)
        | Intent::FindFile(_)
        | Intent::Pdf(_)
        | Intent::People(_)
        | Intent::Feeds(_)
        | Intent::Receipt(_)
        | Intent::Habit(_)
        | Intent::Cards(_)
        | Intent::Translate(_) => Need::Local,
        // Reading model files off this disk. The whole point is no network.
        Intent::WhichModel => Need::Local,
        // Everything that makes Atlas an assistant rather than a search box.
        Intent::WorkspaceOn
        | Intent::WorkspaceOff
        | Intent::OpenApp(_)
        | Intent::CloseApp(_)
        | Intent::FocusApp(_)
        | Intent::ViewDisplay
        | Intent::CaptureWebcam
        | Intent::Ask(_)
        | Intent::Delegate(_)
        | Intent::AfterMe
        | Intent::Pause
        | Intent::Resume
        | Intent::Outstanding
        | Intent::Queued
        // Writing a post is local. Sending it is not, and that is a
        // separate step with its own gate.
        | Intent::DraftPost(_)
        | Intent::Undo
        | Intent::BackUp
        | Intent::SetMode(_)
        | Intent::MachineHealth
        | Intent::SelfCheck
        | Intent::Shakedown
        | Intent::UseClipboard(_)
        | Intent::Rehearse(_)
        | Intent::Show(_)
        | Intent::Dismiss
        | Intent::Ready
        | Intent::Capabilities(_)
        | Intent::History(_)
        // The turns of this session are held in memory on this machine.
        // Reading them back touches no network.
        | Intent::Recap
        // The autonomy ledger lives in the local confidence store. Reading
        // what Atlas may do on its own touches no network.
        | Intent::ActAlone
        // The knowledge store lives on this machine. Reading how big it is
        // touches no network.
        | Intent::KnowledgeSize
        | Intent::CreateAccount(_)
        | Intent::SignIn(_)
        // Typing a code into a window here. Finding it in email needs the
        // connection, and that errand says so itself when it fails.
        | Intent::TypeCode(_)
        | Intent::TwoFactor(_)
        // The compiler and the local model, on this machine.
        | Intent::KeepAtIt
        // Your goals are kept on this machine.
        | Intent::Goals(_)
        | Intent::Later(_)
        // Building or decoding an invite block is text manipulation against
        // a file already on disk -- sending it is a separate, out-of-band
        // step Atlas doesn't perform itself.
        | Intent::Pair(_)
        | Intent::AcceptPairing(_)
        | Intent::ForgetPeer(_)
        | Intent::WorkOnYourself(_)
        | Intent::Unlock(_)
        // A passphrase prompt on this machine's own keyboard.
        | Intent::TakeItBack
        // Saying somebody else has the laptop. A fact about this room.
        | Intent::HandOver(_)
        // Setup, muting and learning a face all happen on this desk.
        | Intent::FinishSetup
        | Intent::MuteTopic(_)
        | Intent::ThisIsMe
        // Writing one is local. Getting it to somebody is the transport's
        // problem, and it is allowed to take until tomorrow -- which is the
        // whole reason writing does not wait on a connection.
        | Intent::Message(_)
        | Intent::Messages
        | Intent::WhoIsIn(_)
        | Intent::NameGroup(_)
        | Intent::LeaveGroup(_)
        | Intent::ChangeGroup(_)
        // Adding a friend is kept and finished by itself when their Atlas is
        // reachable; nothing waits on a connection to say it.
        | Intent::Friend(_)
        // Both are kept and delivered by themselves (the courier, the
        // feedback outbox); an update already here installs offline.
        | Intent::Updates(_)
        | Intent::Feedback(_)
        // The download needs the internet and says so itself when it can't;
        // the status is local.
        | Intent::PhoneModel(_)
        | Intent::Capture(_)
        // Correcting how a note was filed rewrites the local notebook on this
        // machine. Same shape as capturing one -- nothing leaves the desk.
        | Intent::Refile(_)
        | Intent::Mail(_)
        | Intent::SortMail(_)
        // Setting a time is local; the send itself checks the connection.
        | Intent::SchedulePost(_)
        // A button in a window on this desk.
        | Intent::PressButton(_)
        | Intent::MoveBigFiles(_)
        // ffmpeg and the local model.
        | Intent::EditMedia(_)
        // This machine's own clock.
        | Intent::Clock
        // Your settings are on this machine.
        | Intent::SetKey(_)
        // Read from the speech model's name and your settings.
        | Intent::Languages(_)
        // The camera and the hand models are on this machine.
        | Intent::TeachGesture(_)
        // General knowledge compiled into Atlas.
        | Intent::MoneyAdvice(_)
        // Knowledge compiled into Atlas; nothing leaves the machine.
        | Intent::CreatorAdvice(_)
        // Read back from this machine.
        | Intent::Overnight
        // Your notes are on this machine.
        | Intent::Dangling
        // The suggestions are rules kept on this machine.
        | Intent::Suggestions(_)
        // Your list is kept on this machine.
        | Intent::DropTask(_)
        // Unpacking a zip on this disk, and scanning what came out.
        | Intent::Unzip(_)
        // Reading a file on this disk, scanned first by the scanner on this machine.
        | Intent::ReadDocument(_)
        | Intent::Sync(_)
        // Asking another Atlas how it is means reaching it.
        | Intent::BriefOn(_)
        | Intent::ReviewPost(_)
        | Intent::TravelPrep
        | Intent::Files(_)
        | Intent::Why(_)
        | Intent::Unknown(_) => Need::Local,

        // A local model answers offline; a hosted one would answer better.
        Intent::Say(_) => Need::PrefersInternet,

        // Web research is the one thing that genuinely cannot happen offline.
        Intent::Research(_) => Need::Internet,
        // Prefers a worker online, works with the local model offline.
        Intent::Build(_) => Need::PrefersInternet,
        Intent::Improve(_) => Need::PrefersInternet,
        Intent::Implement(_) => Need::Local,
        Intent::DesignReview(_) => Need::Local,
        // Drawing runs on the local model; online only helps it draft faster.
        Intent::Animate(_) => Need::PrefersInternet,
        Intent::Scene(_) => Need::PrefersInternet,
        Intent::Explain(_) => Need::PrefersInternet,
        // Reads a change already staged in memory — no model, no network.
        Intent::PlainChange(_) => Need::Local,
        Intent::Booking(_) => Need::Local,
        Intent::Learn(_) => Need::Local,
        // Your calendar lives here, offline. Neither reading it nor adding to
        // it wants the internet — the phone sync is the phone's job.
        Intent::Schedule(_) | Intent::Agenda(_) => Need::Local,
    }
}

/// What Atlas says when you ask for something the network is needed for.
/// Names the reason and what happens next — never a bare failure.
pub fn deferral_message(intent: &Intent) -> String {
    match intent {
        Intent::Research(topic) => {
            format!("No connection, so I can't research {topic} yet. I'll do it when we're back online.")
        }
        _ => "No connection for that. I'll pick it up when we're back online.".into(),
    }
}
