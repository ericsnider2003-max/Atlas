//! How much of the computer Atlas uses while nobody is talking to it, and
//! what for.
//!
//! Research report, 30 Sep 2026, Stage 2 item 16: a slow tick is written
//! down with its three slowest parts (`timing::Laps`), but nothing measured
//! Atlas's own CPU while idle, or added up where the loop's time goes over
//! an hour rather than one tick. A laptop that runs warm with Atlas "doing
//! nothing" had no answer to "doing what?".
//!
//! `Meter` adds every tick's parts up, and every `WINDOW_SECS` reads the
//! process's own CPU time: the share of one core used, and the parts that
//! took the loop's time, largest first.

use serde::{Deserialize, Serialize};

/// How often the meter reads and starts again.
pub const WINDOW_SECS: u64 = 15 * 60;
/// Past this share of one core while idle, it's said in the log as a warning.
pub const WARN_PERCENT: f32 = 10.0;

/// This process's own CPU time so far, in milliseconds (user + kernel, all
/// threads). `None` where it can't be read.
pub fn own_cpu_ms() -> Option<u64> {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::FILETIME;
        use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
        let (mut c, mut e, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
        unsafe { GetProcessTimes(GetCurrentProcess(), &mut c, &mut e, &mut k, &mut u) }.ok()?;
        let t = |f: FILETIME| ((f.dwHighDateTime as u64) << 32 | f.dwLowDateTime as u64) / 10_000;
        Some(t(k) + t(u))
    }
    #[cfg(not(windows))]
    {
        // utime and stime, fields 14 and 15, in clock ticks (100 a second
        // on Linux and Android).
        let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
        let after = stat.rsplit_once(')')?.1;
        let f: Vec<&str> = after.split_whitespace().collect();
        let ticks = f.get(11)?.parse::<u64>().ok()? + f.get(12)?.parse::<u64>().ok()?;
        Some(ticks * 10)
    }
}

/// One window's reading.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reading {
    pub at: u64,
    pub secs: u64,
    /// Share of one core, 0-100+.
    pub percent: f32,
    /// Whether anyone talked to Atlas in the window.
    pub idle: bool,
    /// The loop's time by part, largest first, milliseconds.
    pub parts: Vec<(String, u64)>,
    /// The same parts by this thread's CPU (2 Oct 2026): where the loop
    /// *worked*, rather than where it waited.
    #[serde(default)]
    pub cpu_parts: Vec<(String, u64)>,
    /// The whole process's CPU over the window, in ms: what the loop's parts
    /// don't add up to was the other threads (the microphone, hand
    /// tracking, the crew, the hub's connections).
    #[serde(default)]
    pub process_ms: u64,
}

impl Reading {
    /// "4.2% of one core over 15 min, idle; the loop's CPU: observing 61%, ...; its time: ...".
    pub fn plain(&self) -> String {
        let total: u64 = self.parts.iter().map(|p| p.1).sum::<u64>().max(1);
        let parts: Vec<String> = self.parts.iter().take(3).map(|(n, ms)| format!("{n} {}%", ms * 100 / total)).collect();
        let cpu_total: u64 = self.cpu_parts.iter().map(|p| p.1).sum::<u64>().max(1);
        let cpu: Vec<String> = self.cpu_parts.iter().take(3).map(|(n, us)| format!("{n} {}%", us * 100 / cpu_total)).collect();
        let loop_ms = self.cpu_parts.iter().map(|p| p.1).sum::<u64>() / 1000;
        let loop_share = (loop_ms * 100).checked_div(self.process_ms).map_or(0, |s| s.min(100));
        format!(
            "{:.1}% of one core over {} min{}; the main loop {}% of that, its CPU: {}; its time: {}",
            self.percent,
            self.secs / 60,
            if self.idle { ", idle" } else { "" },
            loop_share,
            if cpu.is_empty() { "nothing measurable".into() } else { cpu.join(", ") },
            if parts.is_empty() { "nothing measurable".into() } else { parts.join(", ") }
        )
    }
}

#[derive(Debug, Default)]
pub struct Meter {
    started: Option<(u64, u64)>,
    parts: std::collections::BTreeMap<&'static str, u64>,
    cpu_parts: std::collections::BTreeMap<&'static str, u64>,
    talked: bool,
}

impl Meter {
    /// One tick's parts.
    pub fn add(&mut self, parts: &[(&'static str, u32)]) {
        for (n, ms) in parts {
            *self.parts.entry(n).or_insert(0) += u64::from(*ms);
        }
    }

    /// Someone talked to Atlas in this window.
    /// One pass's parts by CPU (microseconds).
    pub fn add_cpu(&mut self, parts: &[(&'static str, u64)]) {
        for (n, us) in parts {
            *self.cpu_parts.entry(n).or_insert(0) += *us;
        }
    }

    pub fn talked(&mut self) {
        self.talked = true;
    }

    /// Read when a window has passed (`cpu_ms` is `own_cpu_ms()`); `None`
    /// before then, and on the first call, which only starts the clock.
    pub fn read(&mut self, t: u64, cpu_ms: Option<u64>) -> Option<Reading> {
        let cpu = cpu_ms?;
        let Some((t0, c0)) = self.started else {
            self.started = Some((t, cpu));
            return None;
        };
        let secs = t.saturating_sub(t0);
        if secs < WINDOW_SECS {
            return None;
        }
        let percent = cpu.saturating_sub(c0) as f32 / (secs as f32 * 10.0);
        let mut parts: Vec<(String, u64)> = self.parts.iter().map(|(n, ms)| (n.to_string(), *ms)).filter(|p| p.1 > 0).collect();
        parts.sort_by_key(|b| std::cmp::Reverse(b.1));
        let mut cpu_parts: Vec<(String, u64)> = self.cpu_parts.iter().map(|(n, us)| (n.to_string(), *us)).filter(|p| p.1 > 0).collect();
        cpu_parts.sort_by_key(|b| std::cmp::Reverse(b.1));
        let r = Reading { at: t, secs, percent, idle: !self.talked, parts, cpu_parts, process_ms: cpu.saturating_sub(c0) };
        *self = Meter { started: Some((t, cpu)), ..Default::default() };
        Some(r)
    }
}

/// Where the last few readings are kept.
pub const KEPT: &str = "cpu_readings";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_reads_the_share_of_one_core_and_the_parts() {
        let mut m = Meter::default();
        assert!(m.read(1000, Some(5_000)).is_none(), "the first call starts the clock");
        m.add(&[("machine health", 300), ("observing", 100)]);
        m.add(&[("machine health", 300)]);
        m.add_cpu(&[("observing", 3_000), ("machine health", 1_000)]);
        assert!(m.read(1000 + 60, Some(6_000)).is_none(), "not a whole window yet");
        let r = m.read(1000 + WINDOW_SECS, Some(5_000 + 90_000)).unwrap();
        assert!((r.percent - 10.0).abs() < 0.01, "{}", r.percent);
        assert!(r.idle);
        assert_eq!(r.parts[0], ("machine health".to_string(), 600));
        // By CPU, where it worked, first; then where its time went.
        assert!(
            r.plain().starts_with("10.0% of one core over 15 min, idle; the main loop 0% of that, its CPU: observing 75%, machine health 25%; its time: machine health 85%"),
            "{}",
            r.plain()
        );
    }

    #[test]
    fn this_process_can_read_its_own_cpu_time() {
        assert!(own_cpu_ms().is_some());
    }
}
