//! Letting go of memory when the machine is short of it (20 Oct 2026).
//!
//! Until now low memory only produced a spoken warning ("Memory is nearly
//! full"): nothing Atlas held was ever given back, so the warning was about a
//! shortage Atlas could have eased itself. This is the relief, in a fixed
//! order, cheapest to reload first, each step only if it is idle:
//!
//! 1. the Kokoro voice (reloads on the next speech),
//! 2. the hand-tracking model, 3. the picture reader, 4. the meaning encoder
//!    (all three are loaded for a call and dropped after it today, so there is
//!    normally nothing to let go; they are named so a future resident cache
//!    has its place in the order and the log says what was checked),
//! 5. the talking model's server (starts again on the next question),
//! 6. the resident speech recogniser (`asr_server`) -- last, and only while
//!    memory is still critically short after everything above.
//!
//! The recogniser is the floor's ears: with the talking model gone, a spoken
//! command still has to be heard to reach the no-model command path, so it is
//! never let go before the model server, and not at all unless
//! [`Holders::still_short`] says the machine is critically short even then.
//!
//! The small essentials (the wake-word spotter, the hub, the microphone)
//! stay. Every relief logs why and what it measured: the process's private
//! bytes before and after, so the effect is a number and not a hope. For a
//! separate server (the model, the recogniser) it is the system's available
//! memory before and a few seconds after instead (21 Oct 2026).
//!
//! The order and the "idle only" rule are pure and tested here; what is
//! actually released lives behind [`Holders`], which the daemon implements.

use std::time::Duration;

/// What can be let go, in the order it is tried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Kokoro,
    HandTracking,
    Vision,
    MeaningEncoder,
    ModelServer,
    Recogniser,
}

/// The order relief tries things in. The recogniser is last, always after
/// the model server (tested).
pub const ORDER: [Step; 6] = [
    Step::Kokoro,
    Step::HandTracking,
    Step::Vision,
    Step::MeaningEncoder,
    Step::ModelServer,
    Step::Recogniser,
];

impl Step {
    /// Whether this is a separate process (a child server). Its memory is
    /// not in the daemon's own private bytes, so what relief frees there is
    /// read from the system's available memory instead (21 Oct 2026: "private
    /// bytes 762 MB -> 762 MB (freed 0 MB)" was logged for a model server
    /// that had in fact given back gigabytes).
    pub fn is_child_process(self) -> bool {
        matches!(self, Step::ModelServer | Step::Recogniser)
    }

    pub fn name(self) -> &'static str {
        match self {
            Step::Kokoro => "Kokoro voice",
            Step::HandTracking => "hand-tracking model",
            Step::Vision => "picture reader",
            Step::MeaningEncoder => "meaning encoder",
            Step::ModelServer => "talking model's server",
            Step::Recogniser => "resident speech recogniser",
        }
    }
}

/// What holds memory, and how to let it go. Implemented by the daemon.
pub trait Holders {
    /// Is this loaded right now? (A step that holds nothing is skipped.)
    fn holds(&self, step: Step) -> bool;
    /// Is it answering something right now? (Never let go while it is.)
    fn busy(&self, step: Step) -> bool;
    /// Let it go. True when it was.
    fn release(&mut self, step: Step) -> bool;
    /// Is memory still critically short, after what was let go so far? Only
    /// then is the recogniser let go. Defaults to no: it stays.
    fn still_short(&self) -> bool {
        false
    }
}

/// What a relief did.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub released: Vec<Step>,
    /// Held but answering, so left alone.
    pub kept_busy: Vec<Step>,
    pub before: Option<u64>,
    pub after: Option<u64>,
    /// System available memory in MB before, and a few seconds after, when a
    /// child process was let go (the effect of stopping a separate server).
    pub available_before_mb: Option<u64>,
    pub available_after_mb: Option<u64>,
}

impl Report {
    /// Whether a separate process was let go, so the daemon's own private
    /// bytes say nothing about the effect.
    pub fn let_go_a_child(&self) -> bool {
        self.released.iter().any(|s| s.is_child_process())
    }

    /// Megabytes of system memory that came back, when both readings exist.
    pub fn available_gain_mb(&self) -> Option<u64> {
        Some(self.available_after_mb?.saturating_sub(self.available_before_mb?))
    }

    /// Bytes given back, when both readings exist.
    pub fn freed(&self) -> Option<u64> {
        Some(self.before?.saturating_sub(self.after?))
    }

    /// One log line: why, what went, and the measured effect.
    pub fn line(&self, why: &str) -> String {
        let mb = |b: u64| b / (1024 * 1024);
        let went = if self.released.is_empty() {
            "nothing idle to let go".to_string()
        } else {
            let names: Vec<&str> = self.released.iter().map(|s| s.name()).collect();
            format!("let go of {}", names.join(", "))
        };
        let busy = if self.kept_busy.is_empty() {
            String::new()
        } else {
            let names: Vec<&str> = self.kept_busy.iter().map(|s| s.name()).collect();
            format!("; kept (in use): {}", names.join(", "))
        };
        let measured = match (self.before, self.after) {
            _ if self.let_go_a_child() => match (self.available_before_mb, self.available_after_mb)
            {
                (Some(b), Some(a)) => format!(
                    "; system available memory {b} MB -> {a} MB (freed {} MB; a separate server's \
                     memory is not in the daemon's own private bytes)",
                    self.available_gain_mb().unwrap_or(0)
                ),
                _ => "; system available memory not readable".to_string(),
            },
            (Some(b), Some(a)) => format!(
                "; private bytes {} MB -> {} MB (freed {} MB)",
                mb(b),
                mb(a),
                mb(self.freed().unwrap_or(0))
            ),
            _ => "; private bytes not readable".to_string(),
        };
        format!("memory relief ({why}): {went}{busy}{measured}")
    }
}

/// How long to wait after stopping a child process before reading what came
/// back: a stopped process gives its memory up within a moment, not at once.
pub const CHILD_SETTLE: Duration = Duration::from_secs(3);

/// Walk the order once. `measure` reads the process's private bytes (called
/// before and after, so the effect is measured, not assumed); `available`
/// reads the system's available memory in MB, sampled before and, when a
/// child process was let go, `settle` after it.
pub fn relieve(
    holders: &mut dyn Holders,
    measure: &mut dyn FnMut() -> Option<u64>,
    available: &mut dyn FnMut() -> Option<u64>,
    settle: Duration,
) -> Report {
    let before = measure();
    let available_before_mb = available();
    let mut released = Vec::new();
    let mut kept_busy = Vec::new();
    for step in ORDER {
        if !holders.holds(step) {
            continue;
        }
        if holders.busy(step) {
            kept_busy.push(step);
            continue;
        }
        // The floor's ears stay unless memory is still critical after every
        // step before it.
        if step == Step::Recogniser && !holders.still_short() {
            continue;
        }
        if holders.release(step) {
            released.push(step);
        }
    }
    if released.iter().any(|s| s.is_child_process()) && !settle.is_zero() {
        std::thread::sleep(settle);
    }
    let after = measure();
    let available_after_mb = available();
    Report {
        released,
        kept_busy,
        before,
        after,
        available_before_mb,
        available_after_mb,
    }
}

/// How often relief may run while pressure lasts: letting go of something
/// that reloads in seconds, every tick, would be churn, not relief.
pub const MIN_GAP: Duration = Duration::from_secs(120);

/// Whether to run a relief now: under pressure, and not within `MIN_GAP` of
/// the last one (`since_last` is None before the first).
pub fn time_to_relieve(pressure: bool, since_last: Option<Duration>) -> bool {
    pressure && since_last.is_none_or(|d| d >= MIN_GAP)
}

static LAST_RAN: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

/// Note that a relief ran just now.
pub fn note_ran() {
    *LAST_RAN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(std::time::Instant::now());
}

/// Time since the last relief, None if none has run.
pub fn since_last() -> Option<Duration> {
    LAST_RAN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .map(|t| t.elapsed())
}

/// The system's available memory in MB right now (None where unreadable).
pub fn available_mb() -> Option<u64> {
    let (total, free) = crate::floor::memory_now();
    (total > 0).then_some(free)
}

/// This process's private bytes (committed memory only it uses), from
/// Windows. None where it can't be read (and on other systems).
pub fn private_bytes() -> Option<u64> {
    #[cfg(windows)]
    {
        use windows::Win32::System::ProcessStatus::{
            K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX,
        };
        use windows::Win32::System::Threading::GetCurrentProcess;
        let mut m = PROCESS_MEMORY_COUNTERS_EX {
            cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
            ..Default::default()
        };
        // SAFETY: `m` is a correctly sized, initialised counters struct that
        // outlives the call; the pseudo-handle needs no closing.
        let ok = unsafe {
            K32GetProcessMemoryInfo(
                GetCurrentProcess(),
                &mut m as *mut PROCESS_MEMORY_COUNTERS_EX as *mut _,
                m.cb,
            )
        }
        .as_bool();
        ok.then_some(m.PrivateUsage as u64)
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        loaded: Vec<Step>,
        busy: Vec<Step>,
        order_seen: Vec<Step>,
        bytes: u64,
    }

    impl Holders for Fake {
        fn holds(&self, s: Step) -> bool {
            self.loaded.contains(&s)
        }
        fn busy(&self, s: Step) -> bool {
            self.busy.contains(&s)
        }
        fn release(&mut self, s: Step) -> bool {
            self.order_seen.push(s);
            self.loaded.retain(|x| *x != s);
            self.bytes -= 100 * 1024 * 1024;
            true
        }
    }

    #[test]
    fn lets_go_in_order_skips_busy_and_measures() {
        let mut f = Fake {
            loaded: vec![Step::ModelServer, Step::Kokoro, Step::Vision],
            busy: vec![Step::Vision],
            order_seen: vec![],
            bytes: 1000 * 1024 * 1024,
        };
        // The measure closure can't borrow `f`; feed it the two readings.
        let mut readings = vec![Some(700u64 << 20), Some(1000u64 << 20)];
        let r = relieve(
            &mut f,
            &mut || readings.pop().unwrap(),
            &mut || None,
            Duration::ZERO,
        );
        assert_eq!(f.order_seen, vec![Step::Kokoro, Step::ModelServer]);
        assert_eq!(r.released, vec![Step::Kokoro, Step::ModelServer]);
        assert_eq!(r.kept_busy, vec![Step::Vision]);
        assert_eq!(r.freed(), Some(300 << 20));
        let line = r.line("memory at 93 percent");
        assert!(
            line.contains("Kokoro voice, talking model's server"),
            "{line}"
        );
        // The server is a child process: no private-bytes figure for it.
        assert!(!line.contains("freed 300 MB"), "{line}");
        assert!(line.contains("not readable"), "{line}");
        assert!(line.contains("kept (in use): picture reader"), "{line}");
    }

    struct Ears {
        short: bool,
        released: Vec<Step>,
    }
    impl Holders for Ears {
        fn holds(&self, s: Step) -> bool {
            matches!(s, Step::Recogniser | Step::ModelServer)
        }
        fn busy(&self, _: Step) -> bool {
            false
        }
        fn release(&mut self, s: Step) -> bool {
            self.released.push(s);
            true
        }
        fn still_short(&self) -> bool {
            self.short
        }
    }

    #[test]
    fn the_recogniser_is_last_and_stays_unless_still_critically_short() {
        assert_eq!(ORDER.last(), Some(&Step::Recogniser));
        let at = |s: Step| ORDER.iter().position(|x| *x == s).unwrap();
        assert!(at(Step::Recogniser) > at(Step::ModelServer));
        let mut calm = Ears {
            short: false,
            released: vec![],
        };
        relieve(&mut calm, &mut || None, &mut || None, Duration::ZERO);
        assert_eq!(calm.released, vec![Step::ModelServer], "ears must stay");
        let mut dire = Ears {
            short: true,
            released: vec![],
        };
        relieve(&mut dire, &mut || None, &mut || None, Duration::ZERO);
        assert_eq!(dire.released, vec![Step::ModelServer, Step::Recogniser]);
    }

    /// 21 Oct 2026: letting go of the model server logged "private bytes 762
    /// MB -> 762 MB (freed 0 MB)", the daemon's own, for a child process. The
    /// effect of a child is the system's available memory before and after.
    #[test]
    fn a_child_server_is_measured_by_available_memory_not_the_daemons_bytes() {
        let mut f = Fake {
            loaded: vec![Step::ModelServer],
            busy: vec![],
            order_seen: vec![],
            bytes: 1000 * 1024 * 1024,
        };
        let mut avail = vec![Some(3_600u64), Some(2_100u64)];
        let r = relieve(
            &mut f,
            &mut || Some(762u64 << 20),
            &mut || avail.pop().unwrap(),
            Duration::ZERO,
        );
        assert!(r.let_go_a_child());
        assert!(Step::Recogniser.is_child_process() && !Step::Kokoro.is_child_process());
        assert_eq!(r.freed(), Some(0), "the daemon's own bytes did not move");
        assert_eq!(r.available_gain_mb(), Some(1_500));
        let line = r.line("memory is short");
        assert!(
            line.contains("system available memory 2100 MB -> 3600 MB (freed 1500 MB"),
            "{line}"
        );
        assert!(!line.contains("private bytes 762"), "{line}");
        // An in-process holder (the voice) is still measured by private bytes.
        let mut v = Fake {
            loaded: vec![Step::Kokoro],
            busy: vec![],
            order_seen: vec![],
            bytes: 1000 * 1024 * 1024,
        };
        let mut readings = vec![Some(500u64 << 20), Some(800u64 << 20)];
        let r = relieve(
            &mut v,
            &mut || readings.pop().unwrap(),
            &mut || None,
            Duration::ZERO,
        );
        assert!(!r.let_go_a_child());
        assert!(r.line("x").contains("freed 300 MB"));
    }

    #[test]
    fn nothing_loaded_is_said_plainly() {
        let mut f = Fake {
            loaded: vec![],
            busy: vec![],
            order_seen: vec![],
            bytes: 0,
        };
        let r = relieve(&mut f, &mut || None, &mut || None, Duration::ZERO);
        assert!(r.released.is_empty());
        assert_eq!(r.freed(), None);
        assert!(r.line("x").contains("nothing idle to let go"));
        assert!(r.line("x").contains("not readable"));
    }

    #[test]
    fn relief_is_rate_limited_and_needs_pressure() {
        assert!(!time_to_relieve(false, None));
        assert!(time_to_relieve(true, None));
        assert!(!time_to_relieve(true, Some(Duration::from_secs(30))));
        assert!(time_to_relieve(true, Some(MIN_GAP)));
    }

    #[test]
    fn kokoro_goes_when_idle_long_or_short_of_memory_never_when_speaking() {
        let five = crate::kokoro::IDLE_UNLOAD;
        let s = Duration::from_secs;
        assert!(!crate::kokoro::should_unload(1, s(10), five, false));
        assert!(crate::kokoro::should_unload(1, five, five, false));
        assert!(crate::kokoro::should_unload(1, s(1), five, true));
        // A reply in flight holds a second reference: never, even pressed.
        assert!(!crate::kokoro::should_unload(2, s(9999), five, true));
    }

    #[test]
    fn private_bytes_reads_on_windows() {
        if cfg!(windows) {
            assert!(private_bytes().is_some_and(|b| b > 1024 * 1024));
        }
    }
}
