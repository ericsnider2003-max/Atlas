//! Watching your machine.
//!
//! Jarvis constantly reports suit status — power, damage, what's failing. The
//! useful version is Atlas watching *this laptop*, because the things that
//! actually interrupt your work are boring and predictable: a full disk, RAM
//! pressure, a backup that stopped running, a battery that stopped holding
//! charge.
//!
//! The hard part isn't reading the numbers. It's saying something **once**,
//! at a moment worth interrupting for, and then shutting up about it.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Fine,
    /// Worth a mention next time you're between things.
    Notice,
    /// Worth interrupting for.
    Urgent,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Readings {
    pub disk_free_gb: f32,
    pub disk_total_gb: f32,
    pub ram_used_gb: f32,
    pub ram_total_gb: f32,
    /// None on a desktop.
    pub battery_percent: Option<u8>,
    pub on_battery: bool,
    /// Battery capacity now versus when new.
    pub battery_health_percent: Option<u8>,
    /// Days since the state folder was last backed up.
    pub days_since_backup: Option<u32>,
    pub uptime_days: u32,
    pub pending_updates: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    /// Stable id, so the same problem is not reported twice.
    pub id: String,
    pub severity: Severity,
    /// One spoken line. No units nobody says out loud.
    pub say: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct HealthConfig {
    pub disk_notice_gb: f32,
    pub disk_urgent_gb: f32,
    /// Fraction of RAM in use before it's worth mentioning.
    pub ram_notice: f32,
    pub ram_urgent: f32,
    pub battery_notice: u8,
    pub battery_urgent: u8,
    pub battery_health_notice: u8,
    pub backup_notice_days: u32,
    pub uptime_notice_days: u32,
    /// Don't repeat the same finding within this many seconds.
    pub repeat_after_secs: u64,
}

impl Default for HealthConfig {
    fn default() -> Self {
        HealthConfig {
            disk_notice_gb: 20.0,
            disk_urgent_gb: 5.0,
            ram_notice: 0.90,
            ram_urgent: 0.96,
            battery_notice: 20,
            battery_urgent: 8,
            battery_health_notice: 70,
            backup_notice_days: 14,
            uptime_notice_days: 14,
            repeat_after_secs: 7 * 86_400,
        }
    }
}

pub fn assess(r: &Readings, cfg: &HealthConfig) -> Vec<Finding> {
    let mut out = Vec::new();

    if r.disk_total_gb > 0.0 {
        if r.disk_free_gb <= cfg.disk_urgent_gb {
            out.push(Finding {
                id: "disk".into(),
                severity: Severity::Urgent,
                // Named with the thing that helps, not just the number. A
                // warning you can't act on is a warning you learn to ignore,
                // and `reclaim` is the one Atlas can genuinely offer here.
                say: format!(
                    "You're down to {:.0} gigabytes of disk. Say \"free up space on my drive\" \
                     and I'll show you what's safe to clear.",
                    r.disk_free_gb
                ),
            });
        } else if r.disk_free_gb <= cfg.disk_notice_gb {
            out.push(Finding {
                id: "disk".into(),
                severity: Severity::Notice,
                say: format!(
                    "Disk is getting tight, {:.0} gigabytes left. Say \"free up space on my drive\" \
                     and I'll show you what's safe to clear.",
                    r.disk_free_gb
                ),
            });
        }
    }

    if r.ram_total_gb > 0.0 {
        let used = r.ram_used_gb / r.ram_total_gb;
        if used >= cfg.ram_urgent {
            out.push(Finding {
                id: "ram".into(),
                severity: Severity::Urgent,
                say: "Memory is nearly full — things will start swapping.".into(),
            });
        } else if used >= cfg.ram_notice {
            out.push(Finding {
                id: "ram".into(),
                severity: Severity::Notice,
                say: format!("Memory is at {:.0} percent.", used * 100.0),
            });
        }
    }

    if let Some(p) = r.battery_percent {
        if r.on_battery && p <= cfg.battery_urgent {
            out.push(Finding {
                id: "battery".into(),
                severity: Severity::Urgent,
                say: format!("Battery is at {p} percent."),
            });
        } else if r.on_battery && p <= cfg.battery_notice {
            out.push(Finding {
                id: "battery".into(),
                severity: Severity::Notice,
                say: format!("Battery is down to {p} percent."),
            });
        }
    }

    if let Some(h) = r.battery_health_percent {
        if h <= cfg.battery_health_notice {
            out.push(Finding {
                id: "battery_health".into(),
                severity: Severity::Notice,
                say: format!("The battery is holding about {h} percent of its original charge."),
            });
        }
    }

    match r.days_since_backup {
        // Never backed up is the worse case, and the easy one to overlook.
        None => out.push(Finding {
            id: "backup".into(),
            severity: Severity::Notice,
            say: "What I have learned has never been backed up.".into(),
        }),
        Some(d) if d >= cfg.backup_notice_days => out.push(Finding {
            id: "backup".into(),
            severity: Severity::Notice,
            say: format!("Last backup was {d} days ago."),
        }),
        _ => {}
    }

    if r.uptime_days >= cfg.uptime_notice_days {
        out.push(Finding {
            id: "uptime".into(),
            severity: Severity::Notice,
            say: format!("This machine hasn't restarted in {} days.", r.uptime_days),
        });
    }

    if r.pending_updates > 0 && r.uptime_days >= 3 {
        out.push(Finding {
            id: "updates".into(),
            severity: Severity::Notice,
            say: format!("{} updates are waiting.", r.pending_updates),
        });
    }

    out.sort_by(|a, b| b.severity.cmp(&a.severity));
    out
}

/// Decides what actually gets said, and when.
///
/// The whole value is restraint. A monitor that mentions a full disk every
/// hour gets ignored, and then the genuinely urgent one is ignored too.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Reporter {
    /// Finding id to when it was last mentioned.
    said: std::collections::BTreeMap<String, u64>,
    /// Finding id to when it was first seen gone (`reconcile_at`).
    #[serde(default)]
    gone_since: std::collections::BTreeMap<String, u64>,
}

/// How long a finding must stay gone before it counts as fixed. Memory at
/// 89.9% for one tick between 90s was "fixed" and "back" a minute apart,
/// and "Memory is at 90 percent." / "91 percent." reached Eric 18 times in
/// an hour and a half (5 Oct 2026, his held notes).
pub const FIXED_AFTER_SECS: u64 = 3600;

impl Reporter {
    /// The one thing worth saying now, or nothing.
    ///
    /// Urgent findings interrupt. Notices wait for a quiet moment. Either way
    /// one at a time — a list of five problems read aloud is noise.
    pub fn next(
        &mut self,
        findings: &[Finding],
        quiet_moment: bool,
        cfg: &HealthConfig,
        t: u64,
    ) -> Option<Finding> {
        for f in findings {
            if f.severity == Severity::Notice && !quiet_moment {
                continue;
            }
            let due = match self.said.get(&f.id) {
                None => true,
                Some(last) => t.saturating_sub(*last) >= cfg.repeat_after_secs,
            };
            if due {
                self.said.insert(f.id.clone(), t);
                return Some(f.clone());
            }
        }
        None
    }

    /// A problem that resolved should be able to be reported again if it
    /// comes back.
    pub fn clear(&mut self, id: &str) {
        self.said.remove(id);
    }

    /// Reconcile against current findings, so fixing something resets it --
    /// for a value read every tick: a finding counts as fixed
    /// only once it has stayed gone for `FIXED_AFTER_SECS`, so a reading
    /// that wobbles across its threshold is said once, not every wobble.
    pub fn reconcile_at(&mut self, findings: &[Finding], t: u64) {
        let live: Vec<&String> = findings.iter().map(|f| &f.id).collect();
        let mut fixed = Vec::new();
        for k in self.said.keys() {
            if live.contains(&k) {
                continue;
            }
            let since = *self.gone_since.entry(k.clone()).or_insert(t);
            if t.saturating_sub(since) >= FIXED_AFTER_SECS {
                fixed.push(k.clone());
            }
        }
        for k in fixed {
            self.said.remove(&k);
            self.gone_since.remove(&k);
        }
        self.gone_since.retain(|k, _| !live.contains(&k));
    }
}

/// Everything at once, for "how's the machine?"
pub fn summary(r: &Readings, findings: &[Finding]) -> String {
    // Unread instruments are checked before anything else, and deliberately
    // so. `assess()` only produces a finding when a value is *above* zero, so
    // a machine nothing could read produces no findings at all — and this
    // function's happy path then answered "All fine. 0 gigabytes free, memory
    // at 0 percent."
    //
    // The `unread_instruments` check below existed already, but only in the
    // branch taken when there *is* a finding, which is the one branch an
    // unread machine can never reach. The guard was real and unreachable.
    // This mattered the moment Atlas targeted more than Windows: `read_disk`
    // was an empty stub off Windows and `read_memory` needed `/proc`, so on
    // macOS every field was zero and the answer was "All fine."
    let unread = crate::hollow::unread_instruments(r);
    if !unread.is_empty() {
        let mut lines = vec![format!(
            "I could not read {} on this machine, so I can't tell you it's fine.",
            unread.join(" or ")
        )];
        lines.extend(findings.iter().take(3).map(|f| f.say.clone()));
        return lines.join(" ");
    }
    if findings.is_empty() {
        return format!(
            "All fine. {:.0} gigabytes free, memory at {:.0} percent.",
            r.disk_free_gb,
            if r.ram_total_gb > 0.0 { r.ram_used_gb / r.ram_total_gb * 100.0 } else { 0.0 }
        );
    }
    // The numbers come first whether or not anything is wrong. The old
    // version dropped them the moment a single finding appeared, so one minor
    // note about backups hid the memory and disk readings entirely — which is
    // how a machine that had never been read looked the same as one that had.
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!(
        "{:.0} gigabytes free, memory at {:.0} percent.",
        r.disk_free_gb,
        r.ram_used_gb / r.ram_total_gb * 100.0
    ));
    lines.extend(findings.iter().take(3).map(|f| f.say.clone()));
    lines.join(" ")
}

// ---------------------------------------------------------------------------
// Actually reading the machine.
//
// `Daemon::readings` returned `Readings::default()` — every field zero — and
// had done since the module was written. Nothing was broken by it, because
// `assess` only reports on values above zero, so a machine that read as all
// zeros looked like a machine with nothing wrong.
//
// It surfaced the moment `faithful` started saying what it could *not* read
// instead of summarising only what it could: "2 things didn't work: memory,
// disk". The honest reporting did not cause the gap, it revealed it.
//
// Anything unreadable stays zero rather than being guessed at, because a
// plausible number is worse than an admitted blank — `assess` skips zero and
// `faithful` names it.
// ---------------------------------------------------------------------------

#[cfg(windows)]
const BYTES_PER_GB: f32 = 1_073_741_824.0;

/// Read what this machine will tell us. Never fails; unreadable stays zero.
pub fn read_machine() -> Readings {
    let mut r = Readings::default();
    read_memory(&mut r);
    read_disk(&mut r);
    read_power(&mut r);
    r
}

/// Mains or battery, and the charge.
///
/// Nothing read this before 26 Sep 2026: `on_battery` was always false and
/// `battery_percent` always `None`, so the overnight run's "don't start an
/// hour of work on a dying battery" check could never fire, and the crew's
/// battery floor would have been a rule with nothing to measure.
#[cfg(windows)]
fn read_power(r: &mut Readings) {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut s = SYSTEM_POWER_STATUS::default();
    // SAFETY: `s` is a plain struct that outlives the call.
    if unsafe { GetSystemPowerStatus(&mut s) }.is_err() {
        return;
    }
    let (on_battery, percent) = power_from_status(s.ACLineStatus, s.BatteryFlag, s.BatteryLifePercent);
    r.on_battery = on_battery;
    r.battery_percent = percent;
}

/// `SYSTEM_POWER_STATUS` read: AC line 0 is battery, 1 is mains, 255 unknown;
/// battery flag 128 is "no battery"; percent 255 is unknown. Unknown is never
/// read as "on battery" — a desktop that can't say must not have its work held.
pub fn power_from_status(ac_line: u8, battery_flag: u8, percent: u8) -> (bool, Option<u8>) {
    let no_battery = battery_flag == 128 || battery_flag == 255;
    let pct = (!no_battery && percent <= 100).then_some(percent);
    (ac_line == 0 && !no_battery, pct)
}

#[cfg(not(windows))]
fn read_power(r: &mut Readings) {
    let base = std::path::Path::new("/sys/class/power_supply");
    if let Ok(dir) = std::fs::read_dir(base) {
        let (on_battery, percent) = power_from_sysfs(dir.flatten().filter_map(|e| {
            let p = e.path();
            let kind = std::fs::read_to_string(p.join("type")).ok()?;
            let status = std::fs::read_to_string(p.join("status")).unwrap_or_default();
            let online = std::fs::read_to_string(p.join("online")).unwrap_or_default();
            let cap = std::fs::read_to_string(p.join("capacity")).unwrap_or_default();
            Some((kind.trim().to_string(), status.trim().to_string(), online.trim().to_string(), cap.trim().to_string()))
        }));
        r.on_battery = on_battery;
        r.battery_percent = percent;
        return;
    }
    // macOS: no /sys. `pmset -g batt` says "'Battery Power'" or "'AC Power'"
    // and a percentage.
    if let Ok(o) = crate::tools::command("pmset").args(["-g", "batt"]).output() {
        let text = String::from_utf8_lossy(&o.stdout);
        r.on_battery = text.contains("'Battery Power'");
        r.battery_percent = text
            .split('%')
            .next()
            .and_then(|l| l.rsplit(|c: char| !c.is_ascii_digit()).next())
            .and_then(|n| n.parse().ok());
    }
}

/// Linux power supplies, read: on battery when a battery says
/// "Discharging" and no mains supply says it's online.
pub fn power_from_sysfs(supplies: impl Iterator<Item = (String, String, String, String)>) -> (bool, Option<u8>) {
    let mut discharging = false;
    let mut mains = false;
    let mut percent = None;
    for (kind, status, online, cap) in supplies {
        match kind.as_str() {
            "Battery" => {
                discharging |= status == "Discharging";
                if percent.is_none() {
                    percent = cap.parse::<u8>().ok().filter(|p| *p <= 100);
                }
            }
            "Mains" | "USB" => mains |= online == "1",
            _ => {}
        }
    }
    (discharging && !mains, percent)
}

#[cfg(windows)]
fn read_memory(r: &mut Readings) {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut m = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: dwLength is set as the API requires and `m` outlives the call.
    if unsafe { GlobalMemoryStatusEx(&mut m) }.is_ok() {
        r.ram_total_gb = m.ullTotalPhys as f32 / BYTES_PER_GB;
        r.ram_used_gb = (m.ullTotalPhys.saturating_sub(m.ullAvailPhys)) as f32 / BYTES_PER_GB;
    }
}

#[cfg(windows)]
fn read_disk(r: &mut Readings) {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    // The drive Atlas is installed on, not C: by assumption — someone running
    // this from a second drive should get numbers about that drive.
    let root: Vec<u16> = std::env::current_dir()
        .ok()
        .and_then(|p| p.components().next().map(|c| c.as_os_str().to_string_lossy().to_string()))
        .map(|c| format!("{c}\\"))
        .unwrap_or_else(|| "C:\\".into())
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let (mut free, mut total) = (0u64, 0u64);
    // SAFETY: `root` is NUL-terminated and outlives the call.
    if unsafe { GetDiskFreeSpaceExW(PCWSTR(root.as_ptr()), None, Some(&mut total), Some(&mut free)) }
        .is_ok()
    {
        r.disk_total_gb = total as f32 / BYTES_PER_GB;
        r.disk_free_gb = free as f32 / BYTES_PER_GB;
    }
}

#[cfg(not(windows))]
fn read_memory(r: &mut Readings) {
    // /proc/meminfo is in kB. MemAvailable is the honest one — MemFree
    // excludes cache that would be handed back under pressure and makes every
    // healthy Linux box look like it is out of memory.
    //
    // macOS has no /proc, so this used to fall straight through and leave
    // every field zero — which `assess()` then reported as nothing wrong,
    // because it only speaks when values are above zero. That is the readings
    // stub again, on a platform Atlas is built for.
    let Ok(text) = std::fs::read_to_string("/proc/meminfo") else {
        read_memory_bsd(r);
        return;
    };
    let kb = |key: &str| -> Option<f32> {
        text.lines()
            .find(|l| l.starts_with(key))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|n| n.parse::<f32>().ok())
    };
    if let (Some(total), Some(avail)) = (kb("MemTotal:"), kb("MemAvailable:")) {
        r.ram_total_gb = total / 1_048_576.0;
        r.ram_used_gb = (total - avail) / 1_048_576.0;
    }
}

/// Memory on systems with no `/proc` — macOS and the BSDs.
///
/// Shelling out rather than adding `libc` for one `sysctl`. `hw.memsize` is
/// total bytes; `vm_stat` reports free and inactive pages, and inactive pages
/// are reclaimable, so counting them as available is the same judgement
/// `MemAvailable` makes on Linux.
#[cfg(not(windows))]
fn read_memory_bsd(r: &mut Readings) {
    let Ok(o) = crate::tools::command("sysctl").args(["-n", "hw.memsize"]).output() else { return };
    let Ok(total) = String::from_utf8_lossy(&o.stdout).trim().parse::<f64>() else { return };
    const B_PER_GB: f64 = 1024.0 * 1024.0 * 1024.0;
    r.ram_total_gb = (total / B_PER_GB) as f32;

    let Ok(o) = crate::tools::command("vm_stat").output() else { return };
    let text = String::from_utf8_lossy(&o.stdout);
    let page = text
        .lines()
        .next()
        .and_then(|l| l.split("page size of ").nth(1))
        .and_then(|s| s.split_whitespace().next())
        .and_then(|n| n.parse::<f64>().ok())
        .unwrap_or(4096.0);
    let pages = |key: &str| -> f64 {
        text.lines()
            .find(|l| l.starts_with(key))
            .and_then(|l| l.rsplit(':').next())
            .map(|v| v.trim().trim_end_matches('.'))
            .and_then(|n| n.parse::<f64>().ok())
            .unwrap_or(0.0)
    };
    let avail = (pages("Pages free") + pages("Pages inactive")) * page;
    if total > 0.0 {
        r.ram_used_gb = ((total - avail) / (1024.0 * 1024.0 * 1024.0)) as f32;
    }
}

/// Disk on anything that is not Windows.
///
/// The previous version was an empty stub, with a comment saying statvfs
/// "is not worth adding for a platform Atlas does not target". Atlas targets
/// every platform, so the reading has to exist. `df -k` is on macOS, Linux
/// and the BSDs, needs no new dependency, and reports the filesystem the
/// program is actually running from.
#[cfg(not(windows))]
fn read_disk(r: &mut Readings) {
    let Ok(o) = crate::tools::command("df").args(["-k", "."]).output() else { return };
    let text = String::from_utf8_lossy(&o.stdout);
    // Skip the header; the figures are the second line. Long device names wrap
    // on some systems, so take the last line with enough columns rather than
    // assuming line two.
    let Some(f) = text
        .lines()
        .skip(1)
        .map(|l| l.split_whitespace().collect::<Vec<_>>())
        .find(|f| f.len() >= 4)
    else {
        return;
    };
    const KB_PER_GB: f32 = 1024.0 * 1024.0;
    if let (Ok(total), Ok(avail)) = (f[1].parse::<f32>(), f[3].parse::<f32>()) {
        r.disk_total_gb = total / KB_PER_GB;
        r.disk_free_gb = avail / KB_PER_GB;
    }
}
