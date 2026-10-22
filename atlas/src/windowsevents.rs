//! What the Windows event logs say about why Atlas ended (12 Oct 2026).
//!
//! 13 of Atlas's last 30 runs ended with nothing written down, and ten of
//! those were Windows "Application Error" 1000 events for atlas.exe. Windows
//! knew; Atlas didn't ask. This is the asking, and nothing more: a bounded,
//! read-only query of the event logs through `wevtutil` (and one line of
//! PowerShell for the boot time), parsed by plain functions that tests feed
//! saved output to.
//!
//! Every query is optional. It has a hard time limit, it is skipped off
//! Windows and inside a test, and a failure is `None` -- "could not ask" --
//! which is different from `Some(vec![])`, "asked, and nothing was there".
//! `whystopped` keeps those two apart, because "nothing was there" is
//! evidence (nobody crashed it) and "could not ask" is not.

use std::time::{Duration, Instant};

/// How long one query may take before it is given up on.
pub const QUERY_LIMIT: Duration = Duration::from_secs(4);

/// The most output read from one query: an event log can be large.
const MOST_OUTPUT: u64 = 2 * 1024 * 1024;

/// One event, reduced to what is used here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// Unix seconds (UTC).
    pub at: u64,
    pub id: u32,
    pub provider: String,
    /// The event's own data items, in order.
    pub data: Vec<String>,
}

/// Windows saying atlas.exe crashed (Application Error, event 1000).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppError {
    pub at: u64,
    /// The module the crash was in, e.g. `openvino_intel_npu_compiler.dll`.
    pub module: String,
    /// e.g. `0xc0000005`.
    pub exception: String,
    /// The crashed process's id.
    pub pid: Option<u64>,
    /// When the crashed process started (Unix seconds).
    pub started: Option<u64>,
}

impl AppError {
    /// In a sentence, for the evidence list.
    pub fn plain(&self) -> String {
        format!(
            "Windows Application Error 1000 for atlas.exe at {}: exception {} in {}",
            iso_utc(self.at),
            self.exception,
            self.module
        )
    }
}

/// What the System log says about the computer starting and stopping.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SystemFacts {
    /// Times Windows logged the operating system starting.
    pub boots: Vec<u64>,
    /// Times Windows logged a shutdown or restart beginning.
    pub shutdowns: Vec<u64>,
    /// An unexpected restart or power loss (Kernel-Power 41, EventLog 6008).
    pub unexpected: Vec<u64>,
    /// `LastBootUpTime`, when it could be read.
    pub last_boot_up_time: Option<u64>,
}

// ------------------------------------------------------------------ parsing

/// Every event in `wevtutil ... /f:xml` output.
pub fn parse_events(xml: &str) -> Vec<Event> {
    xml.split("<Event ")
        .skip(1)
        .filter_map(|chunk| {
            let system = chunk.split("</System>").next()?;
            let id: u32 = tag_text(system, "EventID")?.trim().parse().ok()?;
            let provider = attr_of(system, "<Provider", "Name").unwrap_or_default();
            let at = parse_iso(&attr_of(system, "<TimeCreated", "SystemTime")?)?;
            let body = chunk.split("<EventData").nth(1).unwrap_or("");
            Some(Event {
                at,
                id,
                provider,
                data: data_items(body),
            })
        })
        .collect()
}

/// atlas.exe crashes among `events`, newest first as given.
pub fn app_errors_in(events: &[Event]) -> Vec<AppError> {
    events
        .iter()
        .filter(|e| e.id == 1000 && e.provider.eq_ignore_ascii_case("Application Error"))
        .filter(|e| {
            e.data
                .first()
                .is_some_and(|app| app.eq_ignore_ascii_case("atlas.exe"))
        })
        .map(|e| AppError {
            at: e.at,
            module: e.data.get(3).cloned().unwrap_or_default(),
            exception: e.data.get(6).cloned().unwrap_or_default(),
            pid: e.data.get(8).and_then(|s| parse_hex(s)),
            started: e
                .data
                .get(9)
                .and_then(|s| parse_hex(s))
                .and_then(unix_from_filetime),
        })
        .collect()
}

/// The boot, shutdown and unexpected-restart times in `events`.
pub fn system_facts_in(events: &[Event]) -> SystemFacts {
    let mut f = SystemFacts::default();
    for e in events {
        let p = e.provider.as_str();
        match (p, e.id) {
            ("Microsoft-Windows-Kernel-General", 12) | ("EventLog", 6005) => f.boots.push(e.at),
            ("Microsoft-Windows-Kernel-General", 13) | ("EventLog", 6006) | ("User32", 1074) => {
                f.shutdowns.push(e.at)
            }
            ("Microsoft-Windows-Kernel-Power", 41) | ("EventLog", 6008) => f.unexpected.push(e.at),
            _ => {}
        }
    }
    f
}

/// The events nearest `at`, as short lines for the record: how far away,
/// who logged it, which event, and its first piece of data (an installing
/// package's name, say). At most `most` of them.
pub fn nearest_lines(events: &[Event], at: u64, most: usize) -> Vec<String> {
    let mut v: Vec<&Event> = events.iter().collect();
    v.sort_by_key(|e| e.at.abs_diff(at));
    v.truncate(most);
    v.sort_by_key(|e| e.at);
    v.iter()
        .map(|e| {
            let off = e.at as i64 - at as i64;
            let first = e
                .data
                .iter()
                .find(|d| !d.trim().is_empty())
                .map(|d| format!(" {}", clip(d.trim(), 60)))
                .unwrap_or_default();
            format!("{off:+}s {} #{}{first}", e.provider, e.id)
        })
        .collect()
}

fn clip(s: &str, n: usize) -> String {
    let mut out: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        out.push('~');
    }
    out
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// The text between `<tag ...>` and `</tag>`.
fn tag_text(s: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}");
    let at = s.find(&open)?;
    let rest = &s[at + open.len()..];
    let gt = rest.find('>')?;
    // `<Tag/>` has no text.
    if rest[..gt].ends_with('/') {
        return None;
    }
    let body = &rest[gt + 1..];
    let end = body.find(&format!("</{tag}>"))?;
    Some(unescape(&body[..end]))
}

/// `attr` of the first `<element ...>`, with either quote.
fn attr_of(s: &str, element: &str, attr: &str) -> Option<String> {
    let at = s.find(element)?;
    let rest = &s[at..];
    let end = rest.find('>')?;
    let tag = &rest[..end];
    for quote in ['\'', '"'] {
        let key = format!("{attr}={quote}");
        if let Some(i) = tag.find(&key) {
            let v = &tag[i + key.len()..];
            return v.find(quote).map(|j| unescape(&v[..j]));
        }
    }
    None
}

/// Each `<Data>` item of an event's data, in order.
fn data_items(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(i) = rest.find("<Data") {
        let after = &rest[i + 5..];
        // `<DataSomething` is not `<Data`.
        if !after.starts_with(['>', ' ', '/']) {
            rest = after;
            continue;
        }
        let Some(gt) = after.find('>') else { break };
        if after[..gt].ends_with('/') {
            out.push(String::new());
            rest = &after[gt + 1..];
            continue;
        }
        let text = &after[gt + 1..];
        let Some(end) = text.find("</Data>") else {
            break;
        };
        out.push(unescape(&text[..end]));
        rest = &text[end + 7..];
    }
    out
}

fn parse_hex(s: &str) -> Option<u64> {
    let t = s.trim();
    let t = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t);
    u64::from_str_radix(t, 16).ok()
}

/// A Windows FILETIME (100 ns ticks since 1601) as Unix seconds.
pub fn unix_from_filetime(ft: u64) -> Option<u64> {
    (ft / 10_000_000).checked_sub(11_644_473_600)
}

/// `2026-10-12T15:01:02.1234567Z` as Unix seconds.
pub fn parse_iso(s: &str) -> Option<u64> {
    let s = s.trim();
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-');
    let (y, m, day): (i64, u32, u32) = (
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
    );
    let time = time.trim_end_matches('Z');
    let mut t = time.split(':');
    let h: u64 = t.next()?.parse().ok()?;
    let mi: u64 = t.next()?.parse().ok()?;
    let sec: u64 = t.next()?.split('.').next()?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) || h > 23 || mi > 59 || sec > 60 {
        return None;
    }
    let days = crate::civil::days_from_civil(y, m, day);
    u64::try_from(days)
        .ok()?
        .checked_mul(86_400)?
        .checked_add(h * 3600 + mi * 60 + sec)
}

/// Unix seconds as `2026-10-12T15:01:02Z`.
pub fn iso_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

// ------------------------------------------------------------------ asking

/// Run a program and read what it prints, giving up (and ending it) after
/// `limit`. `None` when it can't be started, runs out of time, or fails.
fn run_bounded(mut cmd: std::process::Command, limit: Duration) -> Option<String> {
    use std::io::Read;
    if cfg!(test) || !cfg!(windows) || crate::selftest::in_a_test() {
        return None;
    }
    let mut child = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let mut out = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        // unheard-ok: a short read is a short answer; the caller judges it
        let _ = Read::by_ref(&mut out).take(MOST_OUTPUT).read_to_end(&mut bytes);
        bytes
    });
    let until = Instant::now() + limit;
    let ok = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(25)),
            _ => {
                crate::heard!(child.kill());
                crate::heard!(child.wait());
                break false;
            }
        }
    };
    let bytes = reader.join().ok()?;
    ok.then(|| String::from_utf8_lossy(&bytes).into_owned())
}

fn wevtutil(channel: &str, query: &str, count: u32, newest_first: bool) -> Option<Vec<Event>> {
    let mut cmd = crate::tools::command("wevtutil");
    cmd.args([
        "qe",
        channel,
        &format!("/q:{query}"),
        &format!("/c:{count}"),
        &format!("/rd:{newest_first}"),
        "/f:xml",
    ]);
    run_bounded(cmd, QUERY_LIMIT).map(|text| parse_events(&text))
}

/// atlas.exe crashes Windows has recorded, newest first. `None` when the
/// log could not be asked.
pub fn recent_app_errors() -> Option<Vec<AppError>> {
    let events = wevtutil("Application", "*[System[(EventID=1000)]]", 60, true)?;
    Some(app_errors_in(&events))
}

/// Starts, stops and unexpected restarts of the computer since `since`.
/// `None` when the log could not be asked.
pub fn system_facts(since: u64) -> Option<SystemFacts> {
    let q = format!(
        "*[System[(EventID=12 or EventID=13 or EventID=41 or EventID=1074 or EventID=6005 \
         or EventID=6006 or EventID=6008) and TimeCreated[@SystemTime>='{}']]]",
        iso_utc(since.saturating_sub(60))
    );
    let events = wevtutil("System", &q, 100, false)?;
    let mut facts = system_facts_in(&events);
    // Only when the log gave no answer worth having: starting PowerShell
    // costs a second or two.
    if facts.boots.is_empty() && facts.shutdowns.is_empty() {
        facts.last_boot_up_time = last_boot_up_time();
    }
    Some(facts)
}

/// `LastBootUpTime` from Windows management, as Unix seconds.
pub fn last_boot_up_time() -> Option<u64> {
    let mut cmd = crate::tools::command("powershell");
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "[DateTimeOffset]::new((Get-CimInstance Win32_OperatingSystem).LastBootUpTime).ToUnixTimeSeconds()",
    ]);
    run_bounded(cmd, QUERY_LIMIT * 2)?.trim().parse().ok()
}

/// What else Windows logged within `radius` seconds of `at`, nearest first:
/// the System and Application logs and app-package servicing. Context for an
/// ending nobody explains; never a verdict.
pub fn events_near(at: u64, radius: u64) -> Option<Vec<Event>> {
    let q = format!(
        "*[System[TimeCreated[@SystemTime>='{}' and @SystemTime<='{}']]]",
        iso_utc(at.saturating_sub(radius)),
        iso_utc(at + radius)
    );
    let mut all = Vec::new();
    let mut any = false;
    for channel in [
        "System",
        "Application",
        "Microsoft-Windows-AppXDeploymentServer/Operational",
    ] {
        if let Some(mut v) = wevtutil(channel, &q, 40, false) {
            any = true;
            all.append(&mut v);
        }
    }
    any.then_some(all)
}

#[cfg(test)]
mod tests {
    use super::*;

    const APP_ERROR: &str = "<Event xmlns='http://schemas.microsoft.com/win/2004/08/events/event'>\
<System><Provider Name='Application Error'/><EventID Qualifiers='0'>1000</EventID>\
<TimeCreated SystemTime='2026-10-11T03:14:15.1234567Z'/></System>\
<EventData><Data>atlas.exe</Data><Data>0.1.0.0</Data><Data>66f0a1b2</Data>\
<Data>openvino_intel_npu_compiler.dll</Data><Data>0.0.0.0</Data><Data>66f0a1b3</Data>\
<Data>c0000005</Data><Data>00000000012a4f10</Data><Data>0x1ab4</Data>\
<Data>0x1DC3A9B8E2F4A10</Data><Data>C:\\Atlas\\atlas.exe</Data></EventData></Event>";

    #[test]
    fn an_application_error_is_read_with_its_module_pid_and_start_time() {
        let events = parse_events(APP_ERROR);
        assert_eq!(events.len(), 1);
        let errs = app_errors_in(&events);
        assert_eq!(errs.len(), 1);
        let e = &errs[0];
        assert_eq!(e.module, "openvino_intel_npu_compiler.dll");
        assert_eq!(e.exception, "c0000005");
        assert_eq!(e.pid, Some(0x1ab4));
        assert_eq!(e.at, parse_iso("2026-10-11T03:14:15Z").unwrap());
        // 0x1DC3A9B8E2F4A10 ticks since 1601: a date in 2024-25, never zero.
        assert!(e.started.is_some_and(|s| s > 1_700_000_000));
    }

    #[test]
    fn other_programs_crashes_are_not_atlas() {
        let other = APP_ERROR.replace("<Data>atlas.exe</Data>", "<Data>chrome.exe</Data>");
        assert!(app_errors_in(&parse_events(&other)).is_empty());
    }

    #[test]
    fn boots_shutdowns_and_unexpected_restarts_are_told_apart() {
        let events = vec![
            Event { at: 10, id: 12, provider: "Microsoft-Windows-Kernel-General".into(), data: vec![] },
            Event { at: 20, id: 13, provider: "Microsoft-Windows-Kernel-General".into(), data: vec![] },
            Event { at: 30, id: 41, provider: "Microsoft-Windows-Kernel-Power".into(), data: vec![] },
            Event { at: 40, id: 12, provider: "Some-Other-Provider".into(), data: vec![] },
            Event { at: 50, id: 1074, provider: "User32".into(), data: vec![] },
        ];
        let f = system_facts_in(&events);
        assert_eq!(f.boots, vec![10]);
        assert_eq!(f.shutdowns, vec![20, 50]);
        assert_eq!(f.unexpected, vec![30]);
    }

    #[test]
    fn nearest_events_are_listed_by_distance_with_their_first_detail() {
        let events = vec![
            Event { at: 1000, id: 854, provider: "AppX".into(), data: vec!["Claude_1.2.3_x64".into()] },
            Event { at: 5000, id: 1, provider: "Far".into(), data: vec![] },
            Event { at: 1010, id: 7, provider: "Near".into(), data: vec![] },
        ];
        let lines = nearest_lines(&events, 1005, 2);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("-5s AppX #854 Claude_1.2.3_x64"), "{lines:?}");
        assert!(lines[1].starts_with("+5s Near #7"), "{lines:?}");
    }

    #[test]
    fn time_round_trips_and_filetimes_convert() {
        for t in [0u64, 86_399, 1_791_000_000, 1_800_000_123] {
            assert_eq!(parse_iso(&iso_utc(t)), Some(t), "{t}");
        }
        assert_eq!(unix_from_filetime(116_444_736_000_000_000), Some(0));
        assert_eq!(unix_from_filetime(1), None);
        assert_eq!(parse_iso("not a time"), None);
    }

    #[test]
    fn asking_is_always_none_inside_a_test() {
        assert!(recent_app_errors().is_none());
        assert!(system_facts(0).is_none());
        assert!(events_near(1_000, 60).is_none());
    }
}
