//! What Atlas can actually check and change on this machine.
//!
//! `tune` measures — how much memory something is holding, how long a startup
//! item costs. This is the other half: the specific, named Windows mechanisms
//! behind those measurements, so a finding can say *run this* rather than
//! *something is wrong*.
//!
//! Every entry came out of watching what people actually do to Windows and
//! then checking it against the mechanism underneath. Most of it is Microsoft's
//! own tooling that ships turned off or buried.
//!
//! The organising idea is the same one `system` already uses: **reversibility
//! decides the gate, not how impressive the action sounds.** A keyboard repeat
//! rate is nothing. Deleting the component store is one-way. Turning off a
//! security feature to gain frames is a trade you make, not one Atlas makes.
//!
//! Three things this deliberately will not do, listed in `NEVER` below with
//! reasons. They are not configurable, and `tests/guards.rs` fails the build
//! if the list stops existing.

use serde::{Deserialize, Serialize};

/// How hard the change is to put back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Undo {
    /// Reads only. Nothing to undo.
    ReadOnly,
    /// Atlas records the old value and can restore it.
    Reversible,
    /// Atlas can do it; putting it back is manual or impossible.
    OneWay,
    /// Yours to decide. Atlas surfaces it and stops.
    Yours,
}

/// Why a check exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    /// Tells you the state of something. Changes nothing.
    Diagnose,
    /// Frees disk.
    Reclaim,
    /// Makes the machine faster or quieter.
    Speed,
    /// Reduces what leaves the machine.
    Privacy,
    /// Stops a future problem.
    Resilience,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Check {
    pub id: &'static str,
    /// One plain line: what it does for you.
    pub what: &'static str,
    /// The mechanism. A command, or a settings path.
    pub how: &'static str,
    pub kind: Kind,
    pub undo: Undo,
    pub needs_admin: bool,
    /// What you give up, where anything is given up at all. Present on every
    /// entry that trades one thing for another — an optimisation with no
    /// stated cost is usually one whose cost nobody looked for.
    pub cost: Option<&'static str>,
}

/// Things Atlas will not do to this machine, and why.
///
/// Enforced as a `const` rather than a config field, so there is no setting to
/// flip. `tests/guards.rs` fails the build if this list or `is_refused` stops
/// existing.
///
/// The first is the important one. The rest of this module is a list of ways
/// to make a machine better; a way to read every password it has ever stored
/// is a different kind of thing wearing the same clothes, and an assistant
/// that can sign you into your bank has no business also being able to dump
/// your credentials to a log.
pub const NEVER: &[(&str, &str)] = &[
    (
        "netsh wlan show profile key=clear",
        "dumps every saved Wi-Fi password in plain text — a credential dump is not an optimisation",
    ),
    (
        "delete prefetch",
        "Prefetch is Windows remembering how your apps load; clearing it makes them slower until it \
         is rebuilt, and it self-caps at 128 entries so it was never going to grow",
    ),
    (
        "registry cleaners",
        "no measurable gain and an unbounded blast radius — this is how machines get broken",
    ),
    (
        "disable memory integrity",
        "trades a kernel security boundary for frames; Atlas can tell you it exists, it cannot \
         decide that for you",
    ),
];

/// True if this is on the permanent no-list.
pub fn is_refused(action: &str) -> bool {
    let a = action.to_lowercase();
    NEVER.iter().any(|(k, _)| a.contains(k) || k.contains(a.as_str()))
}

/// Everything Atlas knows how to check, in rough order of what it buys you.
pub const CHECKS: &[Check] = &[
    // ---------- read-only: always safe, never gated ----------
    Check {
        id: "sfc",
        what: "Finds corrupted Windows system files, which is the usual cause of unexplained \
               crashes and freezes",
        how: "sfc /scannow",
        kind: Kind::Diagnose,
        undo: Undo::ReadOnly,
        needs_admin: true,
        cost: Some("takes several minutes and pins a core while it runs"),
    },
    Check {
        id: "dism-scan",
        what: "Checks the component store for corruption that sfc alone cannot repair",
        how: "DISM /Online /Cleanup-Image /ScanHealth",
        kind: Kind::Diagnose,
        undo: Undo::ReadOnly,
        needs_admin: true,
        cost: None,
    },
    Check {
        id: "perfmon-report",
        what: "A full 60-second system report: memory pressure, driver problems, startup cost, \
               all colour-coded",
        how: "perfmon /report",
        kind: Kind::Diagnose,
        undo: Undo::ReadOnly,
        needs_admin: true,
        cost: Some("the machine is busy for the minute it runs"),
    },
    Check {
        id: "reliability",
        what: "The dated timeline of every crash, freeze and failed update this machine has had",
        how: "perfmon /rel",
        kind: Kind::Diagnose,
        undo: Undo::ReadOnly,
        needs_admin: false,
        cost: None,
    },
    Check {
        id: "netstat",
        what: "Every open network connection with the process behind it — what is talking to the \
               internet that you did not start",
        how: "netstat -ano",
        kind: Kind::Diagnose,
        undo: Undo::ReadOnly,
        needs_admin: false,
        cost: None,
    },
    Check {
        id: "bandwidth-by-process",
        what: "Live per-process bandwidth, so a background updater eating the connection is \
               visible rather than guessed at",
        how: "resmon → Network tab, sort by Total",
        kind: Kind::Diagnose,
        undo: Undo::ReadOnly,
        needs_admin: false,
        cost: None,
    },
    Check {
        id: "unsigned-drivers",
        what: "Lists drivers that are unsigned or unverifiable, which is worth knowing before \
               blaming software for instability",
        how: "sigverif → Start, then read the results tab",
        kind: Kind::Diagnose,
        undo: Undo::ReadOnly,
        needs_admin: false,
        cost: None,
    },
    Check {
        id: "component-store-size",
        what: "How much dead weight Windows Update backups are holding on C:",
        how: "DISM /Online /Cleanup-Image /AnalyzeComponentStore",
        kind: Kind::Diagnose,
        undo: Undo::ReadOnly,
        needs_admin: true,
        cost: None,
    },
    Check {
        id: "uptime",
        what: "How long since a real shutdown — fast startup means 'shut down' usually is not one, \
               and a machine that never truly restarts accumulates every leak",
        how: "Task Manager → Performance → Up time",
        kind: Kind::Diagnose,
        undo: Undo::ReadOnly,
        needs_admin: false,
        cost: None,
    },
    // ---------- reversible: Atlas can do and undo ----------
    Check {
        id: "startup-items",
        what: "Disables startup programs you never use. Disabling is not uninstalling, so it is \
               reversible in one click",
        how: "Task Manager → Startup apps → Disable, by Startup impact",
        kind: Kind::Speed,
        undo: Undo::Reversible,
        needs_admin: false,
        cost: None,
    },
    Check {
        id: "visual-effects",
        what: "Turns off window and menu animations. The machine is not slow, it is waiting for \
               the animation to finish",
        how: "System Properties → Performance → Adjust for best performance (keep font smoothing)",
        kind: Kind::Speed,
        undo: Undo::Reversible,
        needs_admin: false,
        cost: Some("everything looks blunter; font smoothing must stay on or text degrades"),
    },
    Check {
        id: "delivery-optimisation",
        what: "Stops Windows using your upload bandwidth to serve updates to strangers",
        how: "Settings → Windows Update → Advanced → Delivery Optimization → off",
        kind: Kind::Speed,
        undo: Undo::Reversible,
        needs_admin: false,
        cost: None,
    },
    Check {
        id: "keyboard-repeat",
        what: "Repeat delay to short and repeat rate to fast. Ships on medium for no reason and \
               makes every held key feel laggy",
        how: "control keyboard",
        kind: Kind::Speed,
        undo: Undo::Reversible,
        needs_admin: false,
        cost: None,
    },
    Check {
        id: "comms-ducking",
        what: "Stops Windows dropping all other audio by 80% whenever it decides you are on a call",
        how: "mmsys.cpl → Communications → Do nothing",
        kind: Kind::Speed,
        undo: Undo::Reversible,
        needs_admin: false,
        cost: None,
    },
    Check {
        id: "windowed-optimisations",
        what: "Gives borderless windows the same low-latency present path as full screen",
        how: "Settings → System → Display → Graphics → Optimizations for windowed games",
        kind: Kind::Speed,
        undo: Undo::Reversible,
        needs_admin: false,
        cost: None,
    },
    Check {
        id: "usb-selective-suspend",
        what: "Stops Windows powering down USB ports, which is why a mouse or keyboard sometimes \
               stutters after idle",
        how: "Power Options → advanced → USB selective suspend → Disabled",
        kind: Kind::Speed,
        undo: Undo::Reversible,
        needs_admin: false,
        cost: Some("marginally more idle draw on battery"),
    },
    Check {
        id: "storage-sense",
        what: "Windows clears temp files, the recycle bin and stale downloads on its own, forever",
        how: "Settings → System → Storage → Storage Sense → on",
        kind: Kind::Reclaim,
        undo: Undo::Reversible,
        needs_admin: false,
        cost: Some("deletes downloads untouched past the threshold — check the window first"),
    },
    Check {
        id: "clipboard-history",
        what: "Keeps the last 25 copied items instead of one",
        how: "Win+V → enable",
        kind: Kind::Resilience,
        undo: Undo::Reversible,
        needs_admin: false,
        cost: Some("copied text lives in memory longer; do not use it for passwords"),
    },
    Check {
        id: "end-task-taskbar",
        what: "Adds End task to the taskbar right-click, so a frozen window dies in one click",
        how: "Settings → System → For developers → End Task",
        kind: Kind::Resilience,
        undo: Undo::Reversible,
        needs_admin: false,
        cost: None,
    },
    Check {
        id: "advertising-id",
        what: "Turns off the per-machine advertising ID that lets apps correlate you across each \
               other",
        how: "Settings → Privacy & security → General → all toggles off",
        kind: Kind::Privacy,
        undo: Undo::Reversible,
        needs_admin: false,
        cost: None,
    },
    Check {
        id: "start-recommendations",
        what: "Removes Microsoft's promoted apps from Start and the lock screen",
        how: "Settings → Personalization → Start → Recommendations off; Lock screen → fun facts off",
        kind: Kind::Privacy,
        undo: Undo::Reversible,
        needs_admin: false,
        cost: None,
    },
    Check {
        id: "location-history",
        what: "Shows which apps pulled your location and when, with per-app toggles",
        how: "Settings → Privacy & security → Location → Recent activity",
        kind: Kind::Privacy,
        undo: Undo::Reversible,
        needs_admin: false,
        cost: None,
    },
    Check {
        id: "system-restore",
        what: "Turns on restore points, which most Windows 11 machines ship without. This is the \
               thing that makes every other change on this list safe to try",
        how: "SystemPropertiesProtection → Configure → on, ~5% of C:",
        kind: Kind::Resilience,
        undo: Undo::Reversible,
        needs_admin: true,
        cost: Some("spends a few percent of C: — which is the point"),
    },
    Check {
        id: "restart-gpu-driver",
        what: "Restarts the graphics driver without rebooting, which clears most black screens \
               and display freezes with nothing lost",
        how: "Ctrl+Shift+Win+B",
        kind: Kind::Resilience,
        undo: Undo::Reversible,
        needs_admin: false,
        cost: None,
    },
    // ---------- one-way: Atlas can do it, you cannot take it back ----------
    Check {
        id: "component-cleanup",
        what: "Deletes superseded Windows Update backups. Usually gigabytes",
        how: "DISM /Online /Cleanup-Image /StartComponentCleanup",
        kind: Kind::Reclaim,
        undo: Undo::OneWay,
        needs_admin: true,
        cost: Some("you can no longer uninstall the updates it cleans up"),
    },
    Check {
        id: "temp-files",
        what: "Clears %temp% and Windows Update Cleanup. Anything in use refuses to delete, which \
               is what makes it safe",
        how: "Settings → System → Storage → Temporary files",
        kind: Kind::Reclaim,
        undo: Undo::OneWay,
        needs_admin: false,
        cost: None,
    },
    Check {
        id: "winget-upgrade",
        what: "Updates every installed program in one pass instead of one at a time",
        how: "winget upgrade --all",
        kind: Kind::Resilience,
        undo: Undo::OneWay,
        needs_admin: true,
        cost: Some("a version bump can break something that was working; do it when you have time \
                    to notice"),
    },
    Check {
        id: "dism-restore",
        what: "Repairs component-store corruption that sfc could not fix",
        how: "DISM /Online /Cleanup-Image /RestoreHealth",
        kind: Kind::Resilience,
        undo: Undo::OneWay,
        needs_admin: true,
        cost: Some("pulls replacement files from Windows Update; needs a connection"),
    },
    Check {
        id: "flush-dns",
        what: "Clears the resolver cache, which also happens to be a plain-text log of every \
               domain looked up since boot — private browsing does not touch it",
        how: "ipconfig /flushdns",
        kind: Kind::Privacy,
        undo: Undo::OneWay,
        needs_admin: true,
        cost: Some("the next lookup for every cleared domain goes back out to the network, so \
                    the first page loads after it are slightly slower"),
    },
    // ---------- yours: surfaced, never done ----------
    Check {
        id: "fast-startup",
        what: "Fast startup means shutdown is not a shutdown. Turning it off gives you a real \
               cold boot and clears anything a long uptime accumulated",
        how: "Power Options → Choose what the power buttons do → uncheck Turn on fast startup",
        kind: Kind::Resilience,
        undo: Undo::Yours,
        needs_admin: true,
        cost: Some("boots several seconds slower, every time"),
    },
    Check {
        id: "dns-override",
        what: "Setting a public resolver can be faster than the one your ISP handed you",
        how: "Settings → Network → DNS → Manual",
        kind: Kind::Speed,
        undo: Undo::Yours,
        needs_admin: false,
        cost: Some("moves your entire query history to whoever runs that resolver, and breaks \
                    split-horizon DNS on networks that use it"),
    },
    Check {
        id: "memory-integrity",
        what: "Core isolation costs measurable frames. It is also a kernel security boundary",
        how: "Windows Security → Device security → Core isolation",
        kind: Kind::Speed,
        undo: Undo::Yours,
        needs_admin: true,
        cost: Some("this is a security downgrade for performance — Atlas will not make that trade \
                    on your behalf"),
    },
    Check {
        id: "xmp",
        what: "Memory usually runs below its rated speed until the profile is enabled in firmware",
        how: "BIOS → XMP / EXPO / DOCP → Profile 1",
        kind: Kind::Speed,
        undo: Undo::Yours,
        needs_admin: false,
        cost: Some("firmware, outside the OS entirely, and a bad profile means the machine does \
                    not post"),
    },
    Check {
        id: "process-priority",
        what: "Raising a foreground process to above-normal stops it competing with background junk",
        how: "Task Manager → Details → Set priority → Above normal",
        kind: Kind::Speed,
        undo: Undo::Yours,
        needs_admin: false,
        cost: Some("does not persist across restarts, and High or Realtime can starve the system"),
    },
];

pub fn by_id(id: &str) -> Option<&'static Check> {
    CHECKS.iter().find(|c| c.id == id)
}

/// Everything Atlas may run without asking, because it changes nothing.
pub fn read_only() -> Vec<&'static Check> {
    CHECKS.iter().filter(|c| c.undo == Undo::ReadOnly).collect()
}

/// Everything Atlas may do and put back.
pub fn reversible() -> Vec<&'static Check> {
    CHECKS.iter().filter(|c| c.undo == Undo::Reversible).collect()
}

/// Needs your say-so before anything happens: one-way changes and judgement
/// calls both, because "Atlas can technically do this" and "Atlas should"
/// are different questions.
pub fn needs_approval() -> Vec<&'static Check> {
    CHECKS
        .iter()
        .filter(|c| matches!(c.undo, Undo::OneWay | Undo::Yours))
        .collect()
}

/// Ordered for a first pass on a machine nobody has looked after: prove there
/// is a way back before changing anything, then diagnose, then act.
pub fn first_pass() -> Vec<&'static Check> {
    let mut out = Vec::new();
    if let Some(c) = by_id("system-restore") {
        out.push(c);
    }
    out.extend(read_only());
    out.extend(reversible().into_iter().filter(|c| c.id != "system-restore"));
    out
}
