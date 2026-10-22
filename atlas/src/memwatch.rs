//! Low-overhead memory history for diagnosing growth that a single snapshot
//! cannot show. The daemon writes one bounded JSONL series every 30 seconds.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::{OnceLock, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

const SAMPLE_SECONDS: u64 = 30;
const MAX_LINES: usize = 2_880;
const TREND_SAMPLES: usize = 720;

static TREND_HISTORY: OnceLock<RwLock<VecDeque<Sample>>> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sample {
    pub record_type: String,
    pub at_unix: u64,
    pub process_id: u32,
    pub private_bytes: Option<u64>,
    pub working_set_bytes: Option<u64>,
    pub peak_working_set_bytes: Option<u64>,
    pub pagefile_bytes: Option<u64>,
    pub handles: Option<u32>,
    pub threads: Option<u32>,
    pub gdi_objects: Option<u32>,
    pub user_objects: Option<u32>,
    pub system_commit_bytes: Option<u64>,
    pub system_available_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related_processes: Vec<RelatedProcess>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedProcess {
    pub process_id: u32,
    pub name: String,
    pub private_bytes: Option<u64>,
    pub working_set_bytes: Option<u64>,
    pub handles: Option<u32>,
    pub threads: u32,
    pub gdi_objects: Option<u32>,
    pub user_objects: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Event {
    pub record_type: &'static str,
    pub at_unix: u64,
    pub process_id: u32,
    pub event: &'static str,
}

/// Start the daemon's best-effort memory sampler. A failed sample or disk
/// write is intentionally non-fatal; observability must never stop Atlas.
pub fn start() {
    let path = crate::roots::logs_dir().join("memory.jsonl");
    let mut history = read_history(&path);
    history.sort_by_key(|sample| sample.at_unix);
    let mut history: VecDeque<Sample> = history.into_iter().map(trend_sample).collect();
    while history.len() > TREND_SAMPLES {
        history.pop_front();
    }
    let _ = TREND_HISTORY.set(RwLock::new(history)); // unheard-ok: the daemon initializes this singleton once
    let sampler = std::thread::Builder::new()
        .name("atlas-memory-sampler".into())
        .spawn(|| loop {
            std::thread::sleep(std::time::Duration::from_secs(SAMPLE_SECONDS));
            if let Some(sample) = sample() {
                if append(&crate::roots::logs_dir().join("memory.jsonl"), &sample).is_ok() {
                    remember_for_trend(&sample);
                }
            }
        });
    drop(crate::heard!(sampler));
}

/// Return a compact in-memory copy for Status rendering; no file I/O occurs
/// on the daemon's request path.
pub fn trend_history() -> Vec<Sample> {
    TREND_HISTORY
        .get()
        .and_then(|history| history.read().ok())
        .map(|history| history.iter().cloned().collect())
        .unwrap_or_default()
}

fn remember_for_trend(sample: &Sample) {
    let Some(history) = TREND_HISTORY.get() else {
        return;
    };
    let Ok(mut history) = history.write() else {
        return;
    };
    history.push_back(trend_sample(sample.clone()));
    while history.len() > TREND_SAMPLES {
        history.pop_front();
    }
}

fn trend_sample(sample: Sample) -> Sample {
    Sample {
        record_type: "sample".into(),
        at_unix: sample.at_unix,
        process_id: sample.process_id,
        private_bytes: sample.private_bytes,
        working_set_bytes: None,
        peak_working_set_bytes: None,
        pagefile_bytes: None,
        handles: None,
        threads: None,
        gdi_objects: None,
        user_objects: None,
        system_commit_bytes: None,
        system_available_bytes: None,
        related_processes: Vec::new(),
    }
}

/// Mark a lifecycle transition in the same time series as memory samples.
pub fn record_event(event: &'static str) {
    let Some(at_unix) = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
    else {
        return;
    };
    let record = Event {
        record_type: "event",
        at_unix,
        process_id: std::process::id(),
        event,
    };
    if let Some(()) = crate::heard!(append(
        &crate::roots::logs_dir().join("memory.jsonl"),
        &record
    )) {}
}

pub fn sample() -> Option<Sample> {
    let at_unix = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    #[cfg(windows)]
    let (
        private_bytes,
        working_set_bytes,
        peak_working_set_bytes,
        pagefile_bytes,
        handles,
        threads,
        gdi_objects,
        user_objects,
        system_commit_bytes,
        system_available_bytes,
    ) = windows_sample();
    #[cfg(not(windows))]
    let (
        private_bytes,
        working_set_bytes,
        peak_working_set_bytes,
        pagefile_bytes,
        handles,
        threads,
        gdi_objects,
        user_objects,
        system_commit_bytes,
        system_available_bytes,
    ) = (None, None, None, None, None, None, None, None, None, None);
    #[cfg(windows)]
    let related_processes = related_processes();
    #[cfg(not(windows))]
    let related_processes = Vec::new();

    Some(Sample {
        record_type: "sample".into(),
        at_unix,
        process_id: std::process::id(),
        private_bytes,
        working_set_bytes,
        peak_working_set_bytes,
        pagefile_bytes,
        handles,
        threads,
        gdi_objects,
        user_objects,
        system_commit_bytes,
        system_available_bytes,
        related_processes,
    })
}

/// Read valid sample records from the mixed sample/event JSONL stream.
pub fn read_history(path: &std::path::Path) -> Vec<Sample> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let value: serde_json::Value = serde_json::from_str(line).ok()?;
            (value.get("record_type")?.as_str()? == "sample")
                .then(|| serde_json::from_value(value).ok())
                .flatten()
        })
        .collect()
}

/// Atlas descendants include its job-owned helpers and WebView2 processes.
/// Snapshot them by parent chain; unrelated browser processes are excluded.
#[cfg(windows)]
fn related_processes() -> Vec<RelatedProcess> {
    use std::collections::{HashMap, HashSet};
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows::Win32::System::Threading::{
        GetCurrentProcessId, GetGuiResources, GetProcessHandleCount, OpenProcess, GR_GDIOBJECTS,
        GR_USEROBJECTS, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
    };

    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    unsafe {
        let mut more = Process32FirstW(snapshot, &mut entry).is_ok();
        while more {
            let end = entry
                .szExeFile
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(entry.szExeFile.len());
            entries.push((
                entry.th32ProcessID,
                entry.th32ParentProcessID,
                String::from_utf16_lossy(&entry.szExeFile[..end]),
            ));
            more = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
    }

    let root = unsafe { GetCurrentProcessId() };
    let mut selected = HashSet::new();
    loop {
        let before = selected.len();
        for (pid, parent, _) in &entries {
            if *parent == root || selected.contains(parent) {
                selected.insert(*pid);
            }
        }
        if selected.len() == before {
            break;
        }
    }

    let mut thread_counts = HashMap::<u32, u32>::new();
    if let Ok(snapshot) = unsafe {
        CreateToolhelp32Snapshot(
            windows::Win32::System::Diagnostics::ToolHelp::TH32CS_SNAPTHREAD,
            0,
        )
    } {
        use windows::Win32::System::Diagnostics::ToolHelp::{
            Thread32First, Thread32Next, THREADENTRY32,
        };
        let mut thread = THREADENTRY32 {
            dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        unsafe {
            let mut more = Thread32First(snapshot, &mut thread).is_ok();
            while more {
                if selected.contains(&thread.th32OwnerProcessID) {
                    *thread_counts.entry(thread.th32OwnerProcessID).or_default() += 1;
                }
                more = Thread32Next(snapshot, &mut thread).is_ok();
            }
            let _ = CloseHandle(snapshot);
        }
    }

    let mut result = Vec::new();
    for (pid, _, name) in entries
        .into_iter()
        .filter(|(pid, _, _)| selected.contains(pid))
    {
        let Ok(handle) = (unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
                false,
                pid,
            )
        }) else {
            continue;
        };
        let mut counters = PROCESS_MEMORY_COUNTERS_EX {
            cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
            ..Default::default()
        };
        let memory_ok = unsafe {
            K32GetProcessMemoryInfo(
                handle,
                (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX).cast(),
                counters.cb,
            )
            .as_bool()
        };
        let mut handle_count = 0;
        let handles = unsafe { GetProcessHandleCount(handle, &mut handle_count) }
            .is_ok()
            .then_some(handle_count);
        result.push(RelatedProcess {
            process_id: pid,
            name,
            private_bytes: memory_ok.then_some(counters.PrivateUsage as u64),
            working_set_bytes: memory_ok.then_some(counters.WorkingSetSize as u64),
            handles,
            threads: thread_counts.get(&pid).copied().unwrap_or(0),
            gdi_objects: Some(unsafe { GetGuiResources(handle, GR_GDIOBJECTS) }),
            user_objects: Some(unsafe { GetGuiResources(handle, GR_USEROBJECTS) }),
        });
        unsafe {
            let _ = CloseHandle(handle);
        }
    }
    result
}

#[cfg(windows)]
fn windows_sample() -> (
    Option<u64>,
    Option<u64>,
    Option<u64>,
    Option<u64>,
    Option<u32>,
    Option<u32>,
    Option<u32>,
    Option<u32>,
    Option<u64>,
    Option<u64>,
) {
    use windows::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows::Win32::System::Threading::{
        GetCurrentProcess, GetGuiResources, GetProcessHandleCount, GR_GDIOBJECTS, GR_USEROBJECTS,
    };

    let mut counters = PROCESS_MEMORY_COUNTERS_EX {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    let process = unsafe { GetCurrentProcess() };
    let counters_ok = unsafe {
        K32GetProcessMemoryInfo(
            process,
            (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX).cast(),
            counters.cb,
        )
        .as_bool()
    };
    let mut handle_count = 0u32;
    let handles = unsafe { GetProcessHandleCount(process, &mut handle_count) }
        .is_ok()
        .then_some(handle_count);
    let gdi_objects = unsafe { GetGuiResources(process, GR_GDIOBJECTS) };
    let user_objects = unsafe { GetGuiResources(process, GR_USEROBJECTS) };
    let mut memory = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    let system_ok = unsafe { GlobalMemoryStatusEx(&mut memory) }.is_ok();

    (
        counters_ok.then_some(counters.PrivateUsage as u64),
        counters_ok.then_some(counters.WorkingSetSize as u64),
        counters_ok.then_some(counters.PeakWorkingSetSize as u64),
        counters_ok.then_some(counters.PagefileUsage as u64),
        handles,
        current_thread_count(),
        Some(gdi_objects),
        Some(user_objects),
        system_ok.then_some(
            memory
                .ullTotalPageFile
                .saturating_sub(memory.ullAvailPageFile),
        ),
        system_ok.then_some(memory.ullAvailPhys),
    )
}

#[cfg(windows)]
fn current_thread_count() -> Option<u32> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    };
    use windows::Win32::System::Threading::GetCurrentProcessId;
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }.ok()?;
    let pid = unsafe { GetCurrentProcessId() };
    let mut entry = THREADENTRY32 {
        dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    let mut count = 0u32;
    unsafe {
        let mut more = Thread32First(snapshot, &mut entry).is_ok();
        while more {
            if entry.th32OwnerProcessID == pid {
                count += 1;
            }
            more = Thread32Next(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
    }
    Some(count)
}

fn append(path: &std::path::Path, record: &impl Serialize) -> std::io::Result<()> {
    use std::io::Write;
    static LINE_COUNT: std::sync::OnceLock<std::sync::Mutex<usize>> = std::sync::OnceLock::new();
    let mut line_count = LINE_COUNT
        .get_or_init(|| std::sync::Mutex::new(0))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let line = serde_json::to_string(record).map_err(std::io::Error::other)?;
    if *line_count == 0 {
        *line_count = std::fs::read_to_string(path)
            .ok()
            .map(|text| text.lines().count())
            .unwrap_or(0);
    }
    if *line_count >= MAX_LINES {
        let lines: Vec<String> = std::fs::read_to_string(path)?
            .lines()
            .map(str::to_owned)
            .collect();
        let remove = lines.len().saturating_sub(MAX_LINES - 1);
        let retained = lines.into_iter().skip(remove).chain(std::iter::once(line));
        let mut file = std::fs::File::create(path)?;
        for line in retained {
            writeln!(file, "{line}")?;
        }
        *line_count = MAX_LINES;
    } else {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        writeln!(file, "{line}")?;
        *line_count += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn series_is_bounded_to_two_days() {
        assert_eq!(MAX_LINES, 2_880);
        assert_eq!(SAMPLE_SECONDS * MAX_LINES as u64, 86_400);
    }

    #[test]
    fn sample_is_serializable_and_has_process_identity() {
        let sample = sample().unwrap();
        assert_eq!(sample.process_id, std::process::id());
        assert!(serde_json::to_string(&sample)
            .unwrap()
            .contains("private_bytes"));
    }

    #[test]
    fn lifecycle_event_has_a_distinct_record_type_and_cause() {
        let event = Event {
            record_type: "event",
            at_unix: 42,
            process_id: 7,
            event: "camera_opened_for_look",
        };
        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value["record_type"], "event");
        assert_eq!(value["at_unix"], 42);
        assert_eq!(value["process_id"], 7);
        assert_eq!(value["event"], "camera_opened_for_look");
    }

    #[test]
    fn append_keeps_the_newest_two_days_of_records() {
        let path = std::env::temp_dir().join(format!(
            "atlas-memwatch-{}-{}.jsonl",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        for at_unix in 0..=MAX_LINES as u64 {
            append(
                &path,
                &Event {
                    record_type: "event",
                    at_unix,
                    process_id: 1,
                    event: "test",
                },
            )
            .unwrap();
        }
        let contents = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<_> = contents.lines().collect();
        assert_eq!(lines.len(), MAX_LINES);
        assert!(lines[0].contains("\"at_unix\":1,"));
        assert!(lines[MAX_LINES - 1].contains(&format!("\"at_unix\":{MAX_LINES},")));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn history_reader_ignores_events_and_malformed_lines() {
        let path = std::env::temp_dir().join(format!(
            "atlas-memwatch-read-{}-{}.jsonl",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let sample = sample().unwrap();
        let event = Event {
            record_type: "event",
            at_unix: sample.at_unix,
            process_id: sample.process_id,
            event: "camera_opened_for_look",
        };
        std::fs::write(
            &path,
            format!(
                "{}\n{}\nnot-json\n",
                serde_json::to_string(&sample).unwrap(),
                serde_json::to_string(&event).unwrap()
            ),
        )
        .unwrap();
        let history = read_history(&path);
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].process_id, sample.process_id);
        std::fs::remove_file(path).unwrap();
    }
}
