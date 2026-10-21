//! Heavy self-work only when the computer is yours to give up (21 Oct 2026,
//! owner decision).
//!
//! In a real-profile run the weekly self-audit ("You've told me not to ask, so
//! I'm having a go at it now") started a `cargo` build as a child of Atlas in
//! the foreground, while someone was using the machine. Acting without asking
//! stays on; making the PC unusable while doing it does not. A build, a test
//! suite or a mutation sweep run for self-repair goes through here and:
//!
//! 1. starts only when the owner has been away for a while (no keyboard or
//!    mouse for `idle_minutes`), the machine is on mains power, and there is
//!    memory to spare on top of what the job may use
//!    ([`heavy_work_may_run`]);
//! 2. runs in a Windows job object with a hard CPU cap and a memory limit,
//!    at idle priority and in efficiency mode, with cargo told to use half
//!    the cores ([`Caps`], `childjob::Scope::spawn_with`);
//! 3. is stopped within one poll (about two seconds) of the owner touching
//!    the machine, of the power being unplugged, or of memory running low.
//!    The build is killed rather than frozen: cargo picks up where it left
//!    off, cheaply, so "paused" means "will start again when you're away"
//!    ([`Controller`]);
//! 4. shows in the Hub as one durable job -- running, paused (waiting for you
//!    to be away), done or failed -- and says nothing more aloud on any pause
//!    or resume.
//!
//! The decision is a pure function and the cycle is a small state machine, so
//! both are tested without a compiler or a stopwatch.

use crate::tools::{ProcessEnd, ProcessRun};
use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

/// The start of every heavy self-work job's id, so the Hub can tell it from an
/// errand's record.
pub const JOB_PREFIX: &str = "heavy:";

/// How often the owner's presence, power and memory are looked at while heavy
/// work waits or runs.
pub const POLL: Duration = Duration::from_secs(2);

/// How often an unchanged job record is rewritten, so a long wait is not read
/// as a job that has gone quiet (`hublive`: quiet for 15 minutes is Blocked).
const RECORD_EVERY: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HeavyConfig {
    /// Minutes with no keyboard or mouse before heavy work may start.
    pub idle_minutes: u64,
    /// Memory that must stay free, in megabytes, on top of the job's own.
    pub min_free_mb: u64,
    /// The most processor time the work may take, in percent of the machine.
    pub cpu_percent: u32,
    /// The most memory the work may hold, in megabytes (also capped to a
    /// quarter of the machine's).
    pub memory_cap_mb: u64,
}

impl Default for HeavyConfig {
    fn default() -> Self {
        HeavyConfig {
            idle_minutes: 10,
            min_free_mb: 2048,
            cpu_percent: 50,
            memory_cap_mb: 4096,
        }
    }
}

/// Why heavy work is not starting, or keeps being stopped.
#[derive(Debug, Clone, PartialEq)]
pub enum Reason {
    UserActive { idle_secs: u64, need_secs: u64 },
    OnBattery,
    LowMemory { free_mb: u64, need_mb: u64 },
}

impl Reason {
    pub fn plain(&self) -> String {
        match self {
            Reason::UserActive { .. } => "you're using the computer".into(),
            Reason::OnBattery => "it's running on battery".into(),
            Reason::LowMemory { free_mb, need_mb } => {
                format!("memory is short ({free_mb} MB free, {need_mb} MB needed)")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    Run,
    Wait(Reason),
}

impl Verdict {
    pub fn is_run(&self) -> bool {
        matches!(self, Verdict::Run)
    }
}

/// May heavy work run now? Only when the owner has been idle long enough, the
/// machine is on mains, and free memory covers the job's estimate plus the
/// headroom to keep.
pub fn heavy_work_may_run(
    idle_secs: u64,
    on_ac: bool,
    free_mb: u64,
    need_mb: u64,
    cfg: &HeavyConfig,
) -> Verdict {
    let need_secs = cfg.idle_minutes.saturating_mul(60);
    if idle_secs < need_secs {
        return Verdict::Wait(Reason::UserActive {
            idle_secs,
            need_secs,
        });
    }
    if !on_ac {
        return Verdict::Wait(Reason::OnBattery);
    }
    let need = need_mb.saturating_add(cfg.min_free_mb);
    if free_mb < need {
        return Verdict::Wait(Reason::LowMemory {
            free_mb,
            need_mb: need,
        });
    }
    Verdict::Run
}

/// The limits a heavy child runs under.
#[derive(Debug, Clone, PartialEq)]
pub struct Caps {
    /// Hard cap on processor time, percent of the whole machine (1..=100).
    pub cpu_percent: u32,
    /// Memory limit for the whole job, megabytes.
    pub memory_mb: u64,
    /// Parallel build jobs handed to cargo.
    pub jobs: u32,
}

/// The caps for this machine: half the cores for cargo, the configured CPU
/// share, and the smaller of the configured memory and a quarter of the
/// machine's (never under 512 MB, which a compiler cannot work in).
pub fn caps_for(cfg: &HeavyConfig, cores: u32, total_mb: u64) -> Caps {
    let cores = cores.max(1);
    let by_machine = if total_mb == 0 {
        cfg.memory_cap_mb
    } else {
        total_mb / 4
    };
    Caps {
        cpu_percent: cfg.cpu_percent.clamp(5, 100),
        memory_mb: cfg.memory_cap_mb.min(by_machine).max(512),
        jobs: (cores / 2).max(1),
    }
}

/// Where one heavy run is in its cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Not started; waiting for the owner to be away.
    Waiting,
    Running,
    /// Was running, stopped because the owner is back (or power or memory
    /// went); will start again.
    Paused,
    Done,
    Failed,
}

/// What to do about the child after a look at the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Start,
    Stop,
    Hold,
}

/// Waiting -> Running -> Paused -> Running -> Done or Failed.
#[derive(Debug, Clone, PartialEq)]
pub struct Controller {
    pub phase: Phase,
    pub pauses: u32,
}

impl Default for Controller {
    fn default() -> Self {
        Controller::new()
    }
}

impl Controller {
    pub fn new() -> Self {
        Controller {
            phase: Phase::Waiting,
            pauses: 0,
        }
    }

    /// One look at the machine: may heavy work run right now?
    pub fn poll(&mut self, may_run: bool) -> Action {
        match (self.phase, may_run) {
            (Phase::Waiting | Phase::Paused, true) => {
                self.phase = Phase::Running;
                Action::Start
            }
            (Phase::Running, false) => {
                self.phase = Phase::Paused;
                self.pauses += 1;
                Action::Stop
            }
            _ => Action::Hold,
        }
    }

    /// The child ended by itself (whatever its exit code said about the
    /// tests): the cycle is over.
    pub fn exited(&mut self, ran_to_the_end: bool) {
        if self.phase == Phase::Running {
            self.phase = if ran_to_the_end {
                Phase::Done
            } else {
                Phase::Failed
            };
        }
    }
}

/// One look at the machine.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub idle_secs: u64,
    pub on_ac: bool,
    pub free_mb: u64,
    pub total_mb: u64,
}

/// The real machine: how long since you last touched it, mains or battery,
/// and memory. Anything unreadable reads as "not safe to start": no idle
/// reading is zero seconds idle, no memory reading is zero free.
fn real_sense() -> impl Fn() -> Reading {
    let plat = crate::platform::here();
    move || {
        let r = crate::health::read_machine();
        let total = (r.ram_total_gb * 1024.0) as u64;
        let used = (r.ram_used_gb * 1024.0) as u64;
        Reading {
            idle_secs: plat.input_idle_secs().unwrap_or(0),
            on_ac: !r.on_battery,
            free_mb: total.saturating_sub(used),
            total_mb: total,
        }
    }
}

fn cores() -> u32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(2)
}

fn verdict_of(r: &Reading, caps: &Caps, cfg: &HeavyConfig) -> Verdict {
    heavy_work_may_run(r.idle_secs, r.on_ac, r.free_mb, caps.memory_mb, cfg)
}

fn nothing(end: ProcessEnd) -> ProcessRun {
    ProcessRun {
        end,
        stdout: Vec::new(),
        stderr: Vec::new(),
        truncated: false,
    }
}

fn is_cargo(command: &std::process::Command) -> bool {
    std::path::Path::new(command.get_program())
        .file_stem()
        .is_some_and(|s| s.to_string_lossy().eq_ignore_ascii_case("cargo"))
}

/// Sleep for `gap`, in slices, ending early when asked to stop.
fn nap(gap: Duration, stop: Option<&dyn Fn() -> bool>) {
    let until = Instant::now() + gap;
    while Instant::now() < until {
        if stop.is_some_and(|s| s()) || crate::goodbye::asked_to_stop() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25).min(gap));
    }
}

/// The whole cycle with the machine and the clock passed in, so a test can
/// play the owner coming and going. `track` hears each phase with a plain
/// line; it never speaks.
/// The most wall-clock time heavy work may spend waiting for you to be away
/// (21 Oct 2026). The run limit counts only time spent running, so without
/// this a job on a computer that is never idle held its crew slot for ever.
/// After it the job ends Cancelled and is tried again another day.
pub const MAX_WAIT: Duration = Duration::from_secs(24 * 3600);

/// What a job that gave up waiting says.
const GAVE_UP: &str = "It never got a chance to start (you were using the computer, it was on battery, or memory was short for 24 hours), so it will retry when you're away.";

/// `run_with` that can feed the child its standard input (a coding agent is
/// handed its task that way, and is handed it again when it is started anew
/// after a pause) and that gives up waiting after `max_wait`. The flag says
/// it gave up.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_gated(
    sense: &dyn Fn() -> Reading,
    poll: Duration,
    cfg: &HeavyConfig,
    track: &mut dyn FnMut(Phase, &str),
    command: &mut std::process::Command,
    limit: Duration,
    cap: usize,
    feed: Option<String>,
    stop: Option<&dyn Fn() -> bool>,
    max_wait: Duration,
) -> (ProcessRun, bool) {
    let caps = caps_for(cfg, cores(), sense().total_mb);
    let entered = Instant::now();
    if is_cargo(command) {
        command.env("CARGO_BUILD_JOBS", caps.jobs.to_string());
    }
    let ctl = RefCell::new(Controller::new());
    let track = RefCell::new(track);
    let mut ran = Duration::ZERO;
    loop {
        if stop.is_some_and(|s| s()) || crate::goodbye::asked_to_stop() {
            return (nothing(ProcessEnd::Stopped), false);
        }
        let verdict = verdict_of(&sense(), &caps, cfg);
        let action = ctl.borrow_mut().poll(verdict.is_run());
        if action != Action::Start {
            if entered.elapsed().saturating_sub(ran) >= max_wait {
                (track.borrow_mut())(Phase::Waiting, GAVE_UP);
                return (nothing(ProcessEnd::Stopped), true);
            }
            let (phase, why) = match &verdict {
                Verdict::Wait(r) => (ctl.borrow().phase, r.plain()),
                Verdict::Run => (ctl.borrow().phase, String::new()),
            };
            let line = if phase == Phase::Paused {
                format!("Paused -- will resume when you're away ({why}).")
            } else {
                format!("Waiting for you to be away ({why}) before it starts.")
            };
            (track.borrow_mut())(phase, &line);
            nap(poll, stop);
            continue;
        }
        let left = limit.saturating_sub(ran);
        if left.is_zero() {
            ctl.borrow_mut().exited(false);
            return (nothing(ProcessEnd::TimedOut), false);
        }
        (track.borrow_mut())(Phase::Running, "Running while you're away.");
        let paused = Cell::new(false);
        let last = Cell::new(Instant::now());
        let began = Instant::now();
        let watch = || {
            if stop.is_some_and(|s| s()) {
                return true;
            }
            if last.get().elapsed() >= poll {
                last.set(Instant::now());
                let v = verdict_of(&sense(), &caps, cfg);
                if ctl.borrow_mut().poll(v.is_run()) == Action::Stop {
                    paused.set(true);
                    return true;
                }
                (track.borrow_mut())(Phase::Running, "Running while you're away.");
            }
            false
        };
        let run =
            crate::tools::run_scoped_capped(command, left, cap, feed.clone(), Some(&watch), &caps);
        ran += began.elapsed();
        if paused.get() && matches!(run.end, ProcessEnd::Stopped) {
            let why = match verdict_of(&sense(), &caps, cfg) {
                Verdict::Wait(r) => r.plain(),
                Verdict::Run => "you were using the computer".into(),
            };
            (track.borrow_mut())(
                Phase::Paused,
                &format!("Paused -- will resume when you're away ({why})."),
            );
            continue;
        }
        ctl.borrow_mut()
            .exited(matches!(run.end, ProcessEnd::Exited(_)));
        return (run, false);
    }
}

// ============ the scope a self-repair job runs in ============

struct Tracked {
    id: String,
    last: Instant,
    phase: Option<Phase>,
}

struct ScopeState {
    label: String,
    cfg: HeavyConfig,
    store: Option<crate::store::Store>,
    job: Option<Tracked>,
    runs: u32,
    trouble: Option<(bool, String)>,
    /// Run each time a child is killed because you are back, to clear what
    /// it left behind (a mutation sweep's multi-gigabyte copies of the tree).
    on_pause: Option<std::rc::Rc<dyn Fn()>>,
}

thread_local! {
    static SCOPE: RefCell<Option<ScopeState>> = const { RefCell::new(None) };
}

static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Held by the thread doing self-repair for as long as the work lasts. Every
/// build or test that thread starts through `sandbox` or `selfwork` goes
/// through the gate, and the Hub shows the lot as one job. Dropping it closes
/// the job.
pub struct SelfWork {
    outer: bool,
}

impl SelfWork {
    /// `label` is what the Hub shows, e.g. "Self-repair: rebuilding to test a
    /// fix for <symptom>". A scope inside a scope joins the outer one.
    pub fn enter(label: &str, cfg: &HeavyConfig, store: Option<crate::store::Store>) -> SelfWork {
        let outer = SCOPE.with(|s| {
            let mut s = s.borrow_mut();
            if s.is_some() {
                return false;
            }
            *s = Some(ScopeState {
                label: label.to_string(),
                cfg: cfg.clone(),
                store,
                job: None,
                runs: 0,
                trouble: None,
                on_pause: None,
            });
            true
        });
        SelfWork { outer }
    }

    /// Run `clear` each time the work is killed because you are back, once
    /// the process tree is gone.
    pub fn on_pause(&self, clear: impl Fn() + 'static) {
        if !self.outer {
            return;
        }
        SCOPE.with(|s| {
            if let Some(state) = s.borrow_mut().as_mut() {
                state.on_pause = Some(std::rc::Rc::new(clear));
            }
        });
    }

    /// The work did not get done: the job ends Failed with `why` (unless it
    /// already ended for a reason of its own, which is kept). Without this the
    /// job closed as "Finished." whatever happened (21 Oct 2026).
    pub fn fail(&self, why: &str) {
        self.trouble(false, why);
    }

    /// The work was stopped or never started, through no fault of its own:
    /// the job ends Cancelled with `why`.
    pub fn cancel(&self, why: &str) {
        self.trouble(true, why);
    }

    fn trouble(&self, cancelled: bool, why: &str) {
        if !self.outer {
            return;
        }
        SCOPE.with(|s| {
            if let Some(state) = s.borrow_mut().as_mut() {
                if state.trouble.is_none() {
                    state.trouble = Some((cancelled, why.to_string()));
                }
            }
        });
    }
}

impl Drop for SelfWork {
    fn drop(&mut self) {
        if !self.outer {
            return;
        }
        let Some(state) = SCOPE.with(|s| s.borrow_mut().take()) else {
            return;
        };
        let (Some(store), Some(job)) = (state.store, state.job) else {
            return;
        };
        use crate::job::State;
        let (end, said) = match state.trouble {
            Some((true, why)) => (State::Cancelled, why),
            Some((false, why)) => (State::Failed, why),
            // Nothing was run and nobody said why: it was stopped while it
            // waited, or ended early. Not "Finished".
            None if state.runs == 0 => (
                State::Cancelled,
                "Nothing was run: it ended before it got a chance to start.".to_string(),
            ),
            None => (
                State::Succeeded,
                format!(
                    "Finished: {} build or test run{}.",
                    state.runs,
                    if state.runs == 1 { "" } else { "s" }
                ),
            ),
        };
        crate::heard!(crate::job::end(&store, &job.id, &state.label, end, &said));
    }
}

fn scope_config() -> Option<HeavyConfig> {
    SCOPE.with(|s| s.borrow().as_ref().map(|s| s.cfg.clone()))
}

/// Write the phase to the job record: when it changed, or once a minute so a
/// long wait does not look like a job that went quiet.
fn scope_track(phase: Phase, detail: &str) {
    SCOPE.with(|s| {
        let mut s = s.borrow_mut();
        let Some(state) = s.as_mut() else { return };
        let Some(store) = state.store.clone() else {
            return;
        };
        let job = state.job.get_or_insert_with(|| Tracked {
            id: format!(
                "{JOB_PREFIX}{}:{}",
                crate::store::now(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ),
            last: Instant::now() - RECORD_EVERY,
            phase: None,
        });
        if job.phase == Some(phase) && job.last.elapsed() < RECORD_EVERY {
            return;
        }
        job.phase = Some(phase);
        job.last = Instant::now();
        write_job(&store, &job.id, &state.label, phase, detail);
    });
}

/// Running while it runs; Checkpointed (still open, in progress in the Hub)
/// while it waits or is paused, with the reason in its progress line.
fn write_job(store: &crate::store::Store, id: &str, label: &str, phase: Phase, detail: &str) {
    use crate::job::{Record, State};
    let Ok(records) = crate::job::load_checked(store) else {
        return;
    };
    let mut record = records
        .into_iter()
        .find(|r| r.job_id == id)
        .unwrap_or_else(|| Record::planned(id, label));
    if record.state == State::Planned {
        // unheard-ok: Planned to Running is always allowed
        let _ = record.transition(State::Running, "started");
    }
    let next = if phase == Phase::Running {
        State::Running
    } else {
        State::Checkpointed
    };
    if record.transition(next, detail).is_ok() {
        record.heartbeat(detail);
        crate::heard!(crate::job::upsert(store, record));
    }
}

/// Run a build or test command. Outside a [`SelfWork`] scope this is
/// `tools::run_scoped`, unchanged; inside, it is gated, capped and recorded.
pub(crate) fn run(
    command: &mut std::process::Command,
    limit: Duration,
    cap: usize,
    stop: Option<&dyn Fn() -> bool>,
) -> ProcessRun {
    run_feed(command, limit, cap, None, stop)
}

/// `run` for a child that is handed text on its standard input (a coding
/// agent's task). Inside a scope it is held to the same gate and caps, killed
/// the moment you are back and started again when you are away.
pub(crate) fn run_feed(
    command: &mut std::process::Command,
    limit: Duration,
    cap: usize,
    feed: Option<String>,
    stop: Option<&dyn Fn() -> bool>,
) -> ProcessRun {
    // Not under a test binary: a test run would otherwise wait on whether the
    // machine it happens to run on is idle. The gate itself is tested through
    // `run_with`, with the machine passed in.
    let Some(cfg) = scope_config().filter(|_| !crate::roots::under_test()) else {
        return crate::tools::run_scoped(command, limit, cap, feed, stop);
    };
    let sense = real_sense();
    let mut cleared = false;
    let mut track = |phase: Phase, detail: &str| {
        scope_track(phase, detail);
        match phase {
            Phase::Paused if !cleared => {
                cleared = true;
                let hook = SCOPE.with(|s| s.borrow().as_ref().and_then(|s| s.on_pause.clone()));
                if let Some(hook) = hook {
                    hook();
                }
            }
            Phase::Running => cleared = false,
            _ => {}
        }
    };
    let (run, gave_up) = run_gated(
        &sense, POLL, &cfg, &mut track, command, limit, cap, feed, stop, MAX_WAIT,
    );
    SCOPE.with(|s| {
        if let Some(state) = s.borrow_mut().as_mut() {
            if !matches!(run.end, ProcessEnd::Stopped) || !gave_up {
                state.runs += 1;
            }
            let trouble = match &run.end {
                ProcessEnd::Exited(_) => None,
                ProcessEnd::Stopped if gave_up => Some((true, GAVE_UP.into())),
                ProcessEnd::Stopped => Some((true, "Stopped because you asked me to.".into())),
                ProcessEnd::TimedOut => {
                    Some((false, "It was still going when its time ran out.".into()))
                }
                ProcessEnd::Failed(why) => Some((false, why.clone())),
            };
            if state.trouble.is_none() {
                state.trouble = trouble;
            }
        }
    });
    run
}

#[cfg(test)]
mod tests {
    use super::*;

    // Only the tests drive the controller with a scripted machine.
    #[allow(clippy::too_many_arguments)]
    fn run_with(
        sense: &dyn Fn() -> Reading,
        poll: Duration,
        cfg: &HeavyConfig,
        track: &mut dyn FnMut(Phase, &str),
        command: &mut std::process::Command,
        limit: Duration,
        cap: usize,
        stop: Option<&dyn Fn() -> bool>,
    ) -> ProcessRun {
        run_gated(
            sense, poll, cfg, track, command, limit, cap, None, stop, MAX_WAIT,
        )
        .0
    }

    #[test]
    fn an_active_user_never_starts_it() {
        for idle in [0, 5, 599] {
            assert_eq!(
                heavy_work_may_run(idle, true, 64_000, 4096, &HeavyConfig::default()),
                Verdict::Wait(Reason::UserActive {
                    idle_secs: idle,
                    need_secs: 600
                })
            );
        }
    }

    #[test]
    fn battery_and_low_memory_wait() {
        assert_eq!(
            heavy_work_may_run(3600, false, 64_000, 4096, &HeavyConfig::default()),
            Verdict::Wait(Reason::OnBattery)
        );
        // 4096 for the job + 2048 headroom: 6143 is one short.
        assert_eq!(
            heavy_work_may_run(3600, true, 6143, 4096, &HeavyConfig::default()),
            Verdict::Wait(Reason::LowMemory {
                free_mb: 6143,
                need_mb: 6144
            })
        );
        assert_eq!(
            heavy_work_may_run(3600, true, 6144, 4096, &HeavyConfig::default()),
            Verdict::Run
        );
    }

    #[test]
    fn idle_ac_and_memory_runs_and_the_idle_time_is_configurable() {
        assert_eq!(
            heavy_work_may_run(600, true, 16_000, 4096, &HeavyConfig::default()),
            Verdict::Run
        );
        let quick = HeavyConfig {
            idle_minutes: 1,
            ..HeavyConfig::default()
        };
        assert!(heavy_work_may_run(60, true, 16_000, 4096, &quick).is_run());
        assert!(!heavy_work_may_run(59, true, 16_000, 4096, &quick).is_run());
    }

    #[test]
    fn the_caps_follow_the_machine() {
        let c = caps_for(&HeavyConfig::default(), 8, 32_768);
        assert_eq!(
            c,
            Caps {
                cpu_percent: 50,
                memory_mb: 4096,
                jobs: 4
            }
        );
        // A quarter of an 8 GB machine, not 4 GB.
        assert_eq!(caps_for(&HeavyConfig::default(), 8, 8192).memory_mb, 2048);
        // Never under what a compiler can work in; one core still builds.
        assert_eq!(caps_for(&HeavyConfig::default(), 1, 1024).memory_mb, 512);
        assert_eq!(caps_for(&HeavyConfig::default(), 1, 1024).jobs, 1);
        // Unreadable memory falls back to the configured cap.
        assert_eq!(caps_for(&HeavyConfig::default(), 4, 0).memory_mb, 4096);
        let wild = HeavyConfig {
            cpu_percent: 0,
            ..HeavyConfig::default()
        };
        assert_eq!(caps_for(&wild, 4, 16_384).cpu_percent, 5);
    }

    #[test]
    fn the_cycle_pauses_within_one_poll_and_resumes() {
        let mut c = Controller::new();
        assert_eq!(c.poll(false), Action::Hold);
        assert_eq!(c.phase, Phase::Waiting);
        assert_eq!(c.poll(true), Action::Start);
        assert_eq!(c.phase, Phase::Running);
        assert_eq!(c.poll(true), Action::Hold);
        // The owner is back: the very next poll stops it.
        assert_eq!(c.poll(false), Action::Stop);
        assert_eq!(c.phase, Phase::Paused);
        assert_eq!(c.poll(false), Action::Hold);
        assert_eq!(c.poll(true), Action::Start);
        assert_eq!(c.pauses, 1);
        c.exited(true);
        assert_eq!(c.phase, Phase::Done);
        let mut f = Controller::new();
        f.poll(true);
        f.exited(false);
        assert_eq!(f.phase, Phase::Failed);
        // A finished cycle does not start again.
        assert_eq!(f.poll(true), Action::Hold);
    }

    fn quick_command(secs: u32) -> std::process::Command {
        if cfg!(windows) {
            let mut c = std::process::Command::new("cmd");
            c.args(["/C", &format!("ping -n {} 127.0.0.1 >nul", secs + 1)]);
            c
        } else {
            let mut c = std::process::Command::new("sleep");
            c.arg(secs.to_string());
            c
        }
    }

    fn reading(idle_secs: u64, on_ac: bool) -> Reading {
        Reading {
            idle_secs,
            on_ac,
            free_mb: 64_000,
            total_mb: 65_536,
        }
    }

    #[test]
    fn it_does_not_start_while_you_are_active_and_starts_when_away() {
        let looks = Cell::new(0u32);
        // Busy for the first three looks, then away.
        let sense = || {
            looks.set(looks.get() + 1);
            if looks.get() <= 3 {
                reading(2, true)
            } else {
                reading(3600, true)
            }
        };
        let mut phases: Vec<Phase> = Vec::new();
        let mut track = |p: Phase, _: &str| phases.push(p);
        let mut c = quick_command(0);
        let run = run_with(
            &sense,
            Duration::from_millis(20),
            &HeavyConfig::default(),
            &mut track,
            &mut c,
            Duration::from_secs(30),
            1 << 20,
            None,
        );
        assert!(matches!(run.end, ProcessEnd::Exited(_)), "{:?}", run.end);
        let first_run = phases.iter().position(|p| *p == Phase::Running).unwrap();
        assert!(first_run > 0 && phases[..first_run].iter().all(|p| *p == Phase::Waiting));
    }

    #[test]
    fn it_pauses_when_you_come_back_and_runs_again_when_you_leave() {
        let looks = Cell::new(0u32);
        // away for the start, back after a few looks, away again later.
        let sense = || {
            looks.set(looks.get() + 1);
            match looks.get() {
                1..=4 => reading(3600, true),
                5..=8 => reading(1, true),
                _ => reading(3600, true),
            }
        };
        let mut phases: Vec<Phase> = Vec::new();
        let mut track = |p: Phase, _: &str| phases.push(p);
        let mut c = quick_command(2);
        let run = run_with(
            &sense,
            Duration::from_millis(100),
            &HeavyConfig::default(),
            &mut track,
            &mut c,
            Duration::from_secs(60),
            1 << 20,
            None,
        );
        assert!(matches!(run.end, ProcessEnd::Exited(_)), "{:?}", run.end);
        assert!(phases.contains(&Phase::Paused), "{phases:?}");
        assert_eq!(phases.last(), Some(&Phase::Running), "{phases:?}");
    }

    #[test]
    fn on_battery_it_waits_until_stopped() {
        let sense = || reading(3600, false);
        let mut phases: Vec<Phase> = Vec::new();
        let mut track = |p: Phase, _: &str| phases.push(p);
        let stops = Cell::new(0u32);
        let stop = || {
            stops.set(stops.get() + 1);
            stops.get() > 20
        };
        let mut c = quick_command(0);
        let run = run_with(
            &sense,
            Duration::from_millis(20),
            &HeavyConfig::default(),
            &mut track,
            &mut c,
            Duration::from_secs(30),
            1 << 20,
            Some(&stop),
        );
        assert!(matches!(run.end, ProcessEnd::Stopped));
        assert!(!phases.is_empty() && phases.iter().all(|p| *p == Phase::Waiting));
    }

    #[test]
    fn outside_a_scope_it_is_the_plain_runner() {
        let mut c = quick_command(0);
        let run = run(&mut c, Duration::from_secs(30), 1 << 20, None);
        assert!(matches!(run.end, ProcessEnd::Exited(_)));
    }

    #[test]
    fn a_scope_records_one_job_through_its_life() {
        let root = std::env::temp_dir().join(format!(
            "atlas-heavywork-{}-{}",
            std::process::id(),
            crate::store::now()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(&root);
        {
            let _g = SelfWork::enter(
                "Self-repair: test",
                &HeavyConfig::default(),
                Some(store.clone()),
            );
            scope_track(
                Phase::Waiting,
                "Waiting for you to be away (x) before it starts.",
            );
            let open = crate::job::load_checked(&store).unwrap();
            assert_eq!(open.len(), 1);
            assert!(open[0].job_id.starts_with(JOB_PREFIX));
            assert_eq!(open[0].state, crate::job::State::Checkpointed);
            scope_track(Phase::Running, "Running while you're away.");
            assert_eq!(
                crate::job::load_checked(&store).unwrap()[0].state,
                crate::job::State::Running
            );
        }
        let closed = crate::job::load_checked(&store).unwrap();
        // Nothing ran in that scope, so it is not "Finished" (21 Oct 2026).
        assert_eq!(closed[0].state, crate::job::State::Cancelled);
        crate::heard!(std::fs::remove_dir_all(root));
    }

    /// Open a scope, let it write its job, apply `act`, close it, and say how
    /// the job ended and what it said.
    fn ended(act: impl Fn(&SelfWork)) -> (crate::job::State, String) {
        let root = std::env::temp_dir().join(format!(
            "atlas-heavywork-end-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(&root);
        {
            let g = SelfWork::enter(
                "Self-repair: test",
                &HeavyConfig::default(),
                Some(store.clone()),
            );
            scope_track(
                Phase::Waiting,
                "Waiting for you to be away (x) before it starts.",
            );
            act(&g);
        }
        let job = crate::job::load_checked(&store).unwrap().remove(0);
        let said = format!("{job:?}");
        crate::heard!(std::fs::remove_dir_all(root));
        (job.state, said)
    }

    #[test]
    fn a_job_ends_as_truthfully_as_it_went() {
        use crate::job::State;
        let (s, said) = ended(|g| g.fail("the agent didn't finish"));
        assert_eq!(s, State::Failed);
        assert!(said.contains("the agent didn't finish"), "{said}");
        let (s, said) = ended(|g| g.cancel("stopped while waiting"));
        assert_eq!(s, State::Cancelled);
        assert!(said.contains("stopped while waiting"), "{said}");
        // The first reason stands.
        let (s, _) = ended(|g| {
            g.cancel("first");
            g.fail("second");
        });
        assert_eq!(s, State::Cancelled);
        // A real run that ended well is the one thing that is Succeeded.
        let (s, _) = ended(|_| SCOPE.with(|s| s.borrow_mut().as_mut().unwrap().runs = 1));
        assert_eq!(s, State::Succeeded);
        // And an early exit with nothing run and nothing said is not.
        let (s, _) = ended(|_| {});
        assert_ne!(s, State::Succeeded);
    }

    #[test]
    fn a_fed_child_is_killed_when_you_return_and_started_again_with_its_input() {
        let looks = Cell::new(0u32);
        let sense = || {
            looks.set(looks.get() + 1);
            match looks.get() {
                1..=4 => reading(3600, true),
                5..=8 => reading(1, true),
                _ => reading(3600, true),
            }
        };
        let mut phases: Vec<Phase> = Vec::new();
        let mut track = |p: Phase, _: &str| phases.push(p);
        let mut c = quick_command(2);
        let (run, gave_up) = run_gated(
            &sense,
            Duration::from_millis(100),
            &HeavyConfig::default(),
            &mut track,
            &mut c,
            Duration::from_secs(60),
            1 << 20,
            Some("the task".into()),
            None,
            MAX_WAIT,
        );
        assert!(!gave_up);
        assert!(matches!(run.end, ProcessEnd::Exited(_)), "{:?}", run.end);
        assert!(phases.contains(&Phase::Paused), "{phases:?}");
    }

    #[test]
    fn waiting_is_bounded_in_wall_clock_time() {
        let sense = || reading(1, true);
        let mut phases: Vec<Phase> = Vec::new();
        let mut track = |p: Phase, _: &str| phases.push(p);
        let mut c = quick_command(0);
        let (run, gave_up) = run_gated(
            &sense,
            Duration::from_millis(20),
            &HeavyConfig::default(),
            &mut track,
            &mut c,
            Duration::from_secs(30),
            1 << 20,
            None,
            None,
            Duration::from_millis(150),
        );
        assert!(gave_up);
        assert!(matches!(run.end, ProcessEnd::Stopped));
        assert!(phases.iter().all(|p| *p == Phase::Waiting));
    }
}
