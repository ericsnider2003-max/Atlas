//! Looking after the laptop it lives on.
//!
//! Your machine isn't optimised, and on 15.7GB of shared memory that stops
//! being cosmetic. But "optimising Windows" is also the single most common
//! excuse for software to do something reckless, so this is deliberately
//! narrow:
//!
//! * It **finds** things — startup programs you never use, junk that can be
//!   deleted, memory being held by something you forgot was open.
//! * It **explains** each one with the actual number, so you can disagree.
//! * It only **acts** on things that are reversible or genuinely disposable.
//!
//! The named Windows mechanisms behind these findings live in `checks`, which
//! also holds the permanent no-list. Registry cleaners are still how people
//! break their machines, and they are on it.

use serde::{Deserialize, Serialize};

/// Something worth doing about the machine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    /// One plain line, with the number in it.
    pub what: String,
    /// Megabytes of memory freed, or disk reclaimed.
    pub frees_mb: u64,
    /// Seconds saved at startup.
    pub saves_boot_secs: f32,
    pub fix: Fix,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Fix {
    /// Atlas can do it and undo it.
    Reversible { action: String },
    /// Atlas can do it; you'd have to put it back yourself.
    OneWay { action: String },
    /// You have to do it — needs elevation, or it's a judgement call.
    Yours { action: String },
    /// Worth knowing, nothing to do.
    Nothing,
}

impl Fix {
    pub fn atlas_can_do_it(&self) -> bool {
        matches!(self, Fix::Reversible { .. } | Fix::OneWay { .. })
    }
}

/// What Atlas measured.
#[derive(Debug, Clone, Default)]
pub struct Survey {
    /// Program name to megabytes held, and whether you've used it today.
    pub memory_by_app: Vec<(String, u64, bool)>,
    /// Programs that start with Windows, and their measured startup cost.
    pub startup_items: Vec<(String, f32, bool)>,
    /// Folder to megabytes, for things safe to clear.
    pub disposable: Vec<(String, u64)>,
    pub disk_free_gb: f32,
    pub disk_total_gb: f32,
    pub ram_used_gb: f32,
    pub ram_total_gb: f32,
    /// Removable drives that could hold big files instead.
    pub other_drives: Vec<(String, f32)>,
    /// Space Atlas itself is using.
    pub atlas_mb: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct TuneConfig {
    pub enabled: bool,
    /// Ignore anything smaller than this.
    pub min_mb: u64,
    /// Startup items costing less than this aren't worth mentioning.
    pub min_boot_secs: f32,
    /// Programs never suggested for removal, whatever they cost.
    pub keep: Vec<String>,
}

impl Default for TuneConfig {
    fn default() -> Self {
        TuneConfig {
            // On (1 Oct 2026): it only finds and explains; nothing is done
            // without your yes.
            enabled: true,
            min_mb: 200,
            min_boot_secs: 0.5,
            keep: vec![
                "explorer".into(),
                "defender".into(),
                "security".into(),
                "audio".into(),
                "graphics".into(),
                "intel".into(),
                "nvidia".into(),
                "realtek".into(),
                "onedrive".into(),
            ],
        }
    }
}

/// The one everybody's PC has and nobody clears.
///
/// Windows writes temporary files constantly and never tidies them up. It's
/// the largest easy win on most machines and it needs no tool — the folder
/// opens from Run, and anything still in use simply refuses to delete, which
/// is why skipping those is safe.
pub const TEMP_FILES: (&str, &str) = (
    "%temp%",
    "Windows never clears this. Select all and delete — anything in use will refuse, and \
     skipping those is fine. Worth doing monthly.",
);

pub fn examine(s: &Survey, cfg: &TuneConfig) -> Vec<Finding> {
    let mut out = Vec::new();
    if !cfg.enabled {
        return out;
    }
    let protected = |name: &str| {
        let n = name.to_lowercase();
        cfg.keep.iter().any(|k| n.contains(&k.to_lowercase()))
    };

    // Memory held by things you aren't using.
    for (app, mb, used_today) in &s.memory_by_app {
        if *mb < cfg.min_mb || *used_today || protected(app) || !may_close(app, &cfg.keep) {
            continue;
        }
        out.push(Finding {
            id: format!("mem:{app}"),
            what: format!("{app} is holding {mb} MB and you haven't touched it today."),
            frees_mb: *mb,
            saves_boot_secs: 0.0,
            fix: Fix::Reversible { action: format!("close {app}") },
        });
    }

    // Startup programs.
    for (app, secs, used_this_week) in &s.startup_items {
        if *secs < cfg.min_boot_secs || protected(app) {
            continue;
        }
        let never_used = !*used_this_week;
        // Something you actually use is only worth mentioning if it's
        // genuinely expensive. Otherwise this becomes a list of things you
        // already decided you wanted.
        if !never_used && *secs < 3.0 {
            continue;
        }
        out.push(Finding {
            id: format!("boot:{app}"),
            what: if never_used {
                format!("{app} starts with Windows, costs {secs:.1}s, and you haven't opened it this week.")
            } else {
                format!("{app} adds {secs:.1}s to startup.")
            },
            frees_mb: 0,
            saves_boot_secs: *secs,
            // Reversible: it goes back on with one click.
            fix: Fix::Reversible { action: format!("stop {app} starting with Windows") },
        });
    }

    // Junk.
    for (folder, mb) in &s.disposable {
        if *mb < cfg.min_mb {
            continue;
        }
        out.push(Finding {
            id: format!("junk:{folder}"),
            what: format!("{mb} MB of temporary files in {folder}."),
            frees_mb: *mb,
            saves_boot_secs: 0.0,
            fix: Fix::OneWay { action: format!("clear {folder}") },
        });
    }

    // Atlas's own footprint, held to the same standard as everything else.
    if s.atlas_mb > 1000 {
        out.push(Finding {
            id: "atlas".into(),
            what: format!("I'm using {} MB myself, mostly models.", s.atlas_mb),
            frees_mb: 0,
            saves_boot_secs: 0.0,
            fix: Fix::Yours {
                action: "move my models to one of your other drives — see storage below".into(),
            },
        });
    }

    // Disk pressure, with a specific way out rather than "free up space".
    if s.disk_total_gb > 0.0 && s.disk_free_gb < 20.0 {
        let big_drive = s
            .other_drives
            .iter()
            .filter(|(_, free)| *free > 20.0)
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        out.push(Finding {
            id: "disk".into(),
            what: format!("Only {:.0} GB free on C.", s.disk_free_gb),
            frees_mb: 0,
            saves_boot_secs: 0.0,
            fix: match big_drive {
                Some((drive, free)) => Fix::Yours {
                    action: format!(
                        "point my models and captures at {drive}, which has {free:.0} GB free"
                    ),
                },
                None => Fix::Yours { action: "free up space on C".into() },
            },
        });
    }

    if s.ram_total_gb > 0.0 {
        let used = s.ram_used_gb / s.ram_total_gb;
        if used > 0.85 {
            let reclaimable: u64 = out.iter().map(|f| f.frees_mb).sum();
            if reclaimable > 500 {
                out.push(Finding {
                    id: "ram".into(),
                    what: format!(
                        "Memory is at {:.0}%, and about {:.1} GB of that is things you aren't using.",
                        used * 100.0,
                        reclaimable as f32 / 1024.0
                    ),
                    frees_mb: 0,
                    saves_boot_secs: 0.0,
                    fix: Fix::Nothing,
                });
            }
        }
    }

    // Biggest wins first, so the first thing said is the one worth doing.
    out.sort_by(|a, b| {
        let av = a.frees_mb as f32 + a.saves_boot_secs * 200.0;
        let bv = b.frees_mb as f32 + b.saves_boot_secs * 200.0;
        bv.partial_cmp(&av).unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

/// What Atlas can do without asking twice — reversible things only.
pub fn actionable(findings: &[Finding]) -> Vec<&Finding> {
    findings.iter().filter(|f| f.fix.atlas_can_do_it()).collect()
}

/// The total on offer, so you can decide whether it's worth bothering.
pub fn worth_it(findings: &[Finding]) -> (u64, f32) {
    (
        findings.iter().map(|f| f.frees_mb).sum(),
        findings.iter().map(|f| f.saves_boot_secs).sum(),
    )
}

pub fn summary(findings: &[Finding]) -> String {
    if findings.is_empty() {
        return "Nothing worth changing.".into();
    }
    let (mb, secs) = worth_it(findings);
    let mut s = format!("{}: {}", findings.len(), findings[0].what);
    if mb > 500 || secs > 2.0 {
        s.push_str(&format!(
            " All told, about {:.1} GB and {secs:.0}s off startup.",
            mb as f32 / 1024.0
        ));
    }
    s
}

// ---------- where the big files live ----------

/// Storage is the other constraint, and the answer isn't deleting things.
///
/// Models are the bulk of it and they never change once downloaded, which
/// makes them the ideal thing to keep on a second drive: read once at startup,
/// never written to. You have two removable drives sitting empty.
#[derive(Debug, Clone, PartialEq)]
pub struct StoragePlan {
    /// What would move, and to where.
    pub moves: Vec<(String, String, u64)>,
    pub frees_mb: u64,
    pub note: String,
}

pub fn storage_plan(s: &Survey, atlas_root: &str) -> Option<StoragePlan> {
    let target = s
        .other_drives
        .iter()
        .filter(|(_, free)| *free > 5.0)
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))?;

    // Only things that are large, static, and re-downloadable. Never state,
    // never notes — those stay on the fast drive and in the backups. Sizes
    // measured, not guessed: the figures that stood here (900, 300, 2000)
    // were invented, and an empty folder isn't worth moving at all.
    let movable: [&[&str]; 3] = [&["models"], &["data", "video"], &["data", "captures"]];
    let moves: Vec<(String, String, u64)> = movable
        .iter()
        .map(|parts| {
            let from = format!("{atlas_root}/{}", parts.join("/"));
            let mb = dir_mb(std::path::Path::new(&from));
            let leaf = parts.last().copied().unwrap_or("models");
            (from, format!("{}/atlas/{leaf}", target.0), mb)
        })
        .filter(|(_, _, mb)| *mb > 0)
        .collect();
    if moves.is_empty() {
        return None;
    }
    let frees_mb = moves.iter().map(|(_, _, mb)| mb).sum();

    Some(StoragePlan {
        moves,
        frees_mb,
        note: format!(
            "Models and captures are large, never change, and are re-downloadable — \
             exactly what belongs on {}. My notes and what I've learned stay on C, \
             where they're fast and backed up.",
            target.0
        ),
    })
}

/// Attach the actual mechanism to a finding, where one is known.
///
/// A finding that says "startup items are costing you four seconds" and stops
/// there is a complaint. The same finding with `Task Manager → Startup apps`
/// on it is something you can act on without a search engine.
pub fn mechanism_for(f: &Finding) -> Option<&'static crate::checks::Check> {
    let id = match f.id.split(':').next().unwrap_or("") {
        "startup" => "startup-items",
        "disk" | "temp" => "temp-files",
        "mem" => "process-priority",
        _ => return None,
    };
    crate::checks::by_id(id)
}

// ---------------------------------------------------------------------------
// Moving big files to another drive (Eric, 25 Sep 2026, G5: "as long as
// things stay findable and organised")
// ---------------------------------------------------------------------------

/// Megabytes under a folder, counted.
fn dir_mb(path: &std::path::Path) -> u64 {
    fn walk(p: &std::path::Path) -> u64 {
        let Ok(rd) = std::fs::read_dir(p) else { return 0 };
        rd.flatten()
            .map(|e| match e.file_type() {
                Ok(t) if t.is_dir() => walk(&e.path()),
                Ok(_) => e.metadata().map(|m| m.len()).unwrap_or(0),
                Err(_) => 0,
            })
            .sum()
    }
    walk(path) / 1_000_000
}

/// Drives other than the one Atlas runs from, with their free space in GB.
#[cfg(windows)]
pub fn other_drives() -> Vec<(String, f32)> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let here = std::env::current_dir()
        .ok()
        .and_then(|p| p.components().next().map(|c| c.as_os_str().to_string_lossy().to_uppercase()))
        .unwrap_or_else(|| "C:".into());
    let mut out = Vec::new();
    for letter in 'D'..='Z' {
        let root = format!("{letter}:\\");
        if root.starts_with(&here) || !std::path::Path::new(&root).exists() {
            continue;
        }
        let w: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
        let (mut free, mut total) = (0u64, 0u64);
        // SAFETY: `w` is NUL-terminated and outlives the call.
        if unsafe { GetDiskFreeSpaceExW(PCWSTR(w.as_ptr()), None, Some(&mut total), Some(&mut free)) }.is_ok() {
            out.push((format!("{letter}:"), free as f32 / 1_073_741_824.0));
        }
    }
    out
}

/// Other drives on anything that isn't Windows: what's mounted under
/// /media, /mnt, /run/media or /Volumes, from `df`.
#[cfg(not(windows))]
pub fn other_drives() -> Vec<(String, f32)> {
    let Ok(o) = crate::tools::command("df").args(["-k"]).output() else { return Vec::new() };
    String::from_utf8_lossy(&o.stdout)
        .lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            let mount = f.last()?;
            let avail: f32 = f.get(3)?.parse().ok()?;
            ["/media/", "/mnt/", "/run/media/", "/Volumes/"]
                .iter()
                .any(|p| mount.starts_with(p))
                .then(|| (mount.to_string(), avail / (1024.0 * 1024.0)))
        })
        .collect()
}

/// A move that happened, kept so you can always ask where something went.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Moved {
    pub from: String,
    pub to: String,
    pub mb: u64,
    pub at: u64,
}

pub const MOVED_RECORD: &str = "moved_folders";

/// Copy a folder to its new home, check every byte arrived, then remove the
/// original and leave a note where it was saying where it went. Nothing is
/// removed unless the copy matches.
pub fn move_folder(from: &std::path::Path, to: &std::path::Path, now: u64) -> Result<Moved, String> {
    fn copy(a: &std::path::Path, b: &std::path::Path) -> std::io::Result<u64> {
        std::fs::create_dir_all(b)?;
        let mut n = 0;
        for e in std::fs::read_dir(a)? {
            let e = e?;
            let (src, dst) = (e.path(), b.join(e.file_name()));
            if e.file_type()?.is_dir() {
                n += copy(&src, &dst)?;
            } else {
                n += std::fs::copy(&src, &dst)?;
            }
        }
        Ok(n)
    }
    fn bytes(p: &std::path::Path) -> u64 {
        let Ok(rd) = std::fs::read_dir(p) else { return 0 };
        rd.flatten()
            .map(|e| if e.path().is_dir() { bytes(&e.path()) } else { e.metadata().map(|m| m.len()).unwrap_or(0) })
            .sum()
    }
    if !from.is_dir() {
        return Err(format!("{} isn't there to move", from.display()));
    }
    let before = bytes(from);
    copy(from, to).map_err(|e| format!("the copy to {} failed: {e}", to.display()))?;
    let after = bytes(to);
    if after != before {
        return Err(format!(
            "the copy came to {after} bytes, not {before}, so I've left the original where it was"
        ));
    }
    std::fs::remove_dir_all(from).map_err(|e| format!("copied, but I couldn't clear the old folder: {e}"))?;
    let note = format!(
        "This folder was moved by Atlas to {} ({} MB), to make room on this drive.\n\
         Atlas's settings point at the new place. Ask Atlas \"where did you move\" to hear it again.\n",
        to.display(),
        before / 1_000_000
    );
    crate::kept!(std::fs::write(from.with_extension("MOVED.txt"), note));
    Ok(Moved { from: from.display().to_string(), to: to.display().to_string(), mb: before / 1_000_000, at: now })
}


// ---------------------------------------------------------------------------
// Measuring, so there is something to find (1 Oct 2026: "optimise my PC"
// answered only "276 GB free, memory at 90 percent" -- the survey was handed
// in empty, and `tune` was off, so nothing could ever be found).
// ---------------------------------------------------------------------------

/// Memory by program, from Windows' `tasklist /FO CSV /NH`: each program's
/// processes added together, largest first.
pub fn parse_tasklist(csv: &str) -> Vec<(String, u64)> {
    let mut by: std::collections::BTreeMap<String, u64> = Default::default();
    for line in csv.lines() {
        let cells: Vec<&str> = line.split("\",\"").map(|c| c.trim_matches('"')).collect();
        if cells.len() < 5 {
            continue;
        }
        let name = cells[0].trim_end_matches(".exe").trim_end_matches(".EXE").to_string();
        let kb: u64 = cells[4].chars().filter(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(0);
        *by.entry(name).or_default() += kb / 1024;
    }
    let mut v: Vec<(String, u64)> = by.into_iter().collect();
    v.sort_by_key(|b| std::cmp::Reverse(b.1));
    v
}

/// The same from `ps -eo comm=,rss=` elsewhere.
pub fn parse_ps(text: &str) -> Vec<(String, u64)> {
    let mut by: std::collections::BTreeMap<String, u64> = Default::default();
    for line in text.lines() {
        let mut parts = line.split_whitespace().collect::<Vec<_>>();
        let Some(kb) = parts.pop().and_then(|k| k.parse::<u64>().ok()) else { continue };
        if parts.is_empty() {
            continue;
        }
        *by.entry(parts.join(" ")).or_default() += kb / 1024;
    }
    let mut v: Vec<(String, u64)> = by.into_iter().collect();
    v.sort_by_key(|b| std::cmp::Reverse(b.1));
    v
}

/// What's holding memory right now, by program.
pub fn memory_by_app() -> Vec<(String, u64)> {
    if cfg!(windows) {
        crate::tools::command("tasklist")
            .args(["/FO", "CSV", "/NH"])
            .output()
            .map(|o| parse_tasklist(&String::from_utf8_lossy(&o.stdout)))
            .unwrap_or_default()
    } else {
        crate::tools::command("ps")
            .args(["-eo", "comm=,rss="])
            .output()
            .map(|o| parse_ps(&String::from_utf8_lossy(&o.stdout)))
            .unwrap_or_default()
    }
}

/// Megabytes in a folder and everything under it, counted for at most
/// `budget` so a huge folder can't hold the turn.
pub fn folder_mb(dir: &std::path::Path, budget: std::time::Duration) -> u64 {
    let until = std::time::Instant::now() + budget;
    let mut bytes = 0u64;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        if std::time::Instant::now() > until {
            break;
        }
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let Ok(m) = e.metadata() else { continue };
            if m.is_dir() {
                stack.push(e.path());
            } else {
                bytes += m.len();
            }
        }
    }
    bytes / (1024 * 1024)
}

/// What clearing temporary files did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cleared {
    pub files: u64,
    pub mb: u64,
    /// In use, or not ours to delete: left, as `TEMP_FILES` says is fine.
    pub skipped: u64,
}

/// Delete what in `dir` hasn't been touched for `older_than` seconds. Anything
/// in use refuses and is skipped; folders left empty are removed. Only ever
/// called on the temporary folder, and only on your yes.
pub fn clear_old_files(dir: &std::path::Path, older_than: u64) -> Cleared {
    let mut c = Cleared::default();
    let cutoff = std::time::SystemTime::now() - std::time::Duration::from_secs(older_than);
    let mut dirs = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let Ok(m) = e.metadata() else { continue };
            if m.file_type().is_symlink() {
                continue;
            }
            if m.is_dir() {
                stack.push(e.path());
                dirs.push(e.path());
                continue;
            }
            if m.modified().map(|t| t > cutoff).unwrap_or(true) {
                c.skipped += 1;
                continue;
            }
            match std::fs::remove_file(e.path()) {
                Ok(()) => {
                    c.files += 1;
                    c.mb += m.len();
                }
                Err(_) => c.skipped += 1,
            }
        }
    }
    // Deepest first, and only the empty ones (`remove_dir` refuses the rest).
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    for d in dirs {
        crate::heard!(std::fs::remove_dir(&d));
    }
    c.mb /= 1024 * 1024;
    c
}


/// Programs with a window of their own, from `tasklist /V /FO CSV /NH` (its
/// last column is the window title, "N/A" for background processes). Only
/// these are ever offered for closing: a program you can see is one you
/// opened; a background process may be part of Windows.
pub fn parse_windowed(csv: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in csv.lines() {
        let cells: Vec<&str> = line.split("\",\"").map(|c| c.trim_matches('"')).collect();
        if cells.len() < 9 {
            continue;
        }
        let title = cells[cells.len() - 1].trim();
        if title.is_empty() || title == "N/A" || title.eq_ignore_ascii_case("OleMainThreadWndName") {
            continue;
        }
        let name = cells[0].trim_end_matches(".exe").trim_end_matches(".EXE").to_string();
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

fn windowed_programs() -> Vec<String> {
    if !cfg!(windows) {
        return Vec::new();
    }
    crate::tools::command("tasklist")
        .args(["/V", "/FO", "CSV", "/NH"])
        .output()
        .map(|o| parse_windowed(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

/// Never closed, never stopped from starting, whatever they hold: Windows
/// itself, security, the shell, sync, and the assistants in use.
///
/// Widened 2 Oct 2026, when closing stopped being limited to programs with a
/// window: the parts of Windows that run in your own session without one
/// (indexing, updates, installers mid-install, the print spooler, driver
/// hosts) are here by name, so no reading of a process list can offer them.
pub const NEVER_CLOSE: &[&str] = &[
    "explorer", "dwm", "csrss", "winlogon", "svchost", "sihost", "ctfmon", "fontdrvhost", "runtimebroker",
    "searchhost", "startmenuexperiencehost", "textinputhost", "shellexperiencehost", "applicationframehost",
    "securityhealth", "msmpeng", "audiodg", "memory compression", "system", "registry", "lsass", "smss",
    "wininit", "services", "taskhostw", "dllhost", "conhost", "atlas", "llama-server", "claude", "taskmgr",
    "systemsettings", "lockapp", "widgets", "nissrv", "mpdefendercoreservice",
    "idle", "secure system", "lsaiso", "userinit", "wudfhost", "spoolsv", "searchindexer", "searchprotocolhost",
    "searchfilterhost", "wmiprvse", "backgroundtaskhost", "smartscreen", "sgrmbroker", "dashost", "unsecapp",
    "wlanext", "msiexec", "trustedinstaller", "tiworker", "mousocoreworker", "usocoreworker", "wuauclt",
    "vmmem", "vmcompute", "wslservice", "ntoskrnl", "searchapp", "securityhealthsystray",
    "openconsole", "windowsterminal", "powershell", "pwsh", "cmd",
];

/// Atlas's own helpers, by name, for when one has outlived the process that
/// started it (so it is no longer found as Atlas's child): the language
/// model's server, speech in and out, the video tools, mail, Tor.
pub const ATLAS_HELPERS: &[&str] =
    &["atlas", "llama-server", "whisper", "piper", "ffmpeg", "ffprobe", "ffplay", "himalaya", "tor", "tesseract"];

pub fn may_close(name: &str, keep: &[String]) -> bool {
    let n = name.to_lowercase();
    !NEVER_CLOSE.iter().any(|k| n == *k || n.starts_with(k))
        && !keep.iter().filter(|k| !k.trim().is_empty()).any(|k| n.contains(&k.to_lowercase()))
}

/// Every value `reg query` printed: (name, data). The data is what follows
/// the type -- the command line for a Run entry, hex for a binary one.
/// (It replaced `parse_reg_run`, which kept only the names, on 2 Oct 2026:
/// what an entry runs is how it's matched to the programs you use.)
pub fn parse_reg_values(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            let mut parts = l.splitn(3, "    ");
            let name = parts.next()?.trim();
            let kind = parts.next()?.trim();
            let data = parts.next().unwrap_or("").trim();
            kind.starts_with("REG_").then(|| (name.to_string(), data.to_string()))
        })
        .filter(|(n, _)| !n.is_empty() && !n.eq_ignore_ascii_case("(Default)"))
        .collect()
}

const RUN: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_MACHINE: &str = r"HKLM\Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_MACHINE_32: &str = r"HKLM\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run";
const APPROVED: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
const APPROVED_FOLDER: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder";
const APPROVED_MACHINE: &str = r"HKLM\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
const APPROVED_MACHINE_32: &str = r"HKLM\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run32";
const APPROVED_MACHINE_FOLDER: &str =
    r"HKLM\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder";

/// What an optimization run offers to do, all of it on one yes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    /// Programs to close, as measured: heavy, not in front, not in use, not
    /// Windows and not Atlas (`pick_to_close`).
    pub close: Vec<Load>,
    /// Startup entries to switch off, each reversible (`pick_startup_to_stop`).
    pub stop_starting: Vec<StartupEntry>,
    /// The temporary folder and its size.
    pub temp: Option<(std::path::PathBuf, u64)>,
    /// Files to move, and the folder you named for them.
    pub moves: Option<(Vec<std::path::PathBuf>, std::path::PathBuf)>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.close.is_empty() && self.stop_starting.is_empty() && self.temp.is_none() && self.moves.is_none()
    }

    /// The offer, said with the numbers.
    pub fn offer(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if !self.close.is_empty() {
            let mb: u64 = self.close.iter().map(|c| c.mem_mb).sum();
            let names: Vec<String> = self.close.iter().map(|c| c.plain()).collect();
            parts.push(format!(
                "close {} -- not in front of you and not used in the last hour, about {:.1} GB back",
                names.join(", "),
                mb as f32 / 1024.0
            ));
        }
        if !self.stop_starting.is_empty() {
            let names: Vec<String> = self.stop_starting.iter().map(|e| e.name.clone()).collect();
            parts.push(format!(
                "stop {} starting with Windows -- not opened this week, and \"undo\" turns {} back on",
                names.join(", "),
                if names.len() == 1 { "it" } else { "them" }
            ));
        }
        if let Some((_, mb)) = &self.temp {
            parts.push(format!("clear {mb} MB of temporary files, leaving anything in use"));
        }
        if let Some((files, to)) = &self.moves {
            let mb: u64 = files.iter().filter_map(|f| std::fs::metadata(f).ok()).map(|m| m.len() / 1_048_576).sum();
            parts.push(format!(
                "move {} file{} ({}) into {} -- \"undo\" moves them back",
                files.len(),
                if files.len() == 1 { "" } else { "s" },
                mb_words(mb),
                to.display()
            ));
        }
        format!("I can {}. Go ahead?", parts.join("; "))
    }
}

fn mb_words(mb: u64) -> String {
    if mb >= 1024 {
        format!("{:.1} GB", mb as f32 / 1024.0)
    } else {
        format!("{mb} MB")
    }
}

// ---------------------------------------------------------------------------
// The deeper look, and acting on it (2 Oct 2026, Eric: "can't properly
// optimize my PC ... closing things in the task manager that aren't needed,
// moving files, doing deeper dives and actually making it run well").
//
// Before this, "optimise my PC" read memory once from `tasklist`, offered to
// close only programs with a window that held 200 MB and weren't used today,
// asked them to close by name and never followed up, looked only at your own
// Run key for startup programs, and recorded none of it where "undo" could
// find it. What's here: CPU measured over a few seconds rather than guessed
// from one look, every place Windows starts things from, a closing that
// follows through, and every change written into the history "undo" reads.
// ---------------------------------------------------------------------------

/// One process, as Windows reported it at one moment.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Proc {
    pub pid: u32,
    pub parent: u32,
    /// Without ".exe".
    pub name: String,
    /// 0 is where Windows' services run; your programs are in yours.
    pub session: u32,
    pub mem_mb: u64,
    /// CPU time used so far, all threads, milliseconds. Only the change
    /// between two readings means anything.
    pub cpu_ms: u64,
    /// Has a visible window of its own.
    pub windowed: bool,
}

/// One program's share of the machine over a sampled stretch: its processes
/// added together, the way Task Manager groups them.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Load {
    pub name: String,
    pub pids: Vec<u32>,
    /// Share of the whole machine (every core), 0-100, as Task Manager shows it.
    pub cpu_pct: f32,
    pub mem_mb: u64,
    pub windowed: bool,
    /// Started by Windows' services, or running in their session: never ours.
    pub system: bool,
    /// Stands on its own: it has a window, or it was started by the shell (a
    /// tray program), or whatever started it is gone. A helper of another
    /// program is not -- closing it would break that program, not free it.
    pub alone: bool,
}

impl Load {
    /// "Discord (410 MB)", "Steam (620 MB, 12% CPU, in the background)".
    pub fn plain(&self) -> String {
        let mut bits = vec![mb_words(self.mem_mb)];
        if self.cpu_pct >= 1.0 {
            bits.push(format!("{:.0}% CPU", self.cpu_pct));
        }
        if !self.windowed {
            bits.push("in the background".into());
        }
        format!("{} ({})", self.name, bits.join(", "))
    }
}

/// The share of the whole machine a stretch of CPU time is: `used_ms` of
/// processor time over `wall_ms` of clock time on `cores` cores. Two cores
/// busy for one second out of a one-second window on an eight-core machine
/// is 25%, which is what Task Manager says too.
pub fn cpu_share(used_ms: u64, wall_ms: u64, cores: u32) -> f32 {
    if wall_ms == 0 || cores == 0 {
        return 0.0;
    }
    (used_ms as f32 / (wall_ms as f32 * cores as f32) * 100.0).clamp(0.0, 100.0)
}

/// Names whose children are Windows' own, whatever they're called.
const SYSTEM_PARENTS: &[&str] = &["services", "svchost", "wininit", "winlogon", "smss", "csrss", "lsass", "system"];

/// Two readings of the process list, `wall_ms` apart, made into each
/// program's load. A process in both readings counts the CPU it used in
/// between; one that started in between counts all of its CPU time (it was
/// all used in the window); one that ended is left out. A process is only
/// matched to itself if its name is the same in both, so a number Windows
/// reused for something else isn't read as one process's work.
pub fn sample_load(before: &[Proc], after: &[Proc], wall_ms: u64, cores: u32) -> Vec<Load> {
    let earlier: std::collections::HashMap<u32, &Proc> = before.iter().map(|p| (p.pid, p)).collect();
    let by_pid: std::collections::HashMap<u32, &Proc> = after.iter().map(|p| (p.pid, p)).collect();
    let mut by: Vec<(String, Load, u64)> = Vec::new();
    for p in after {
        let used = match earlier.get(&p.pid) {
            Some(b) if b.name.eq_ignore_ascii_case(&p.name) => p.cpu_ms.saturating_sub(b.cpu_ms),
            _ => p.cpu_ms,
        };
        let parent = by_pid.get(&p.parent).filter(|q| q.pid != p.pid);
        let parent_name = parent.map(|q| q.name.to_lowercase());
        let system = p.session == 0
            || p.pid <= 4
            || parent_name.as_deref().map(|n| SYSTEM_PARENTS.contains(&n)).unwrap_or(false);
        let alone = p.windowed
            || match parent_name.as_deref() {
                None => true,
                Some("explorer") => true,
                Some(n) => n.eq_ignore_ascii_case(&p.name),
            };
        let key = p.name.to_lowercase();
        match by.iter_mut().find(|(k, _, _)| *k == key) {
            Some((_, l, ms)) => {
                l.pids.push(p.pid);
                l.mem_mb += p.mem_mb;
                l.windowed |= p.windowed;
                l.system |= system;
                l.alone &= alone;
                *ms += used;
            }
            None => by.push((
                key,
                Load { name: p.name.clone(), pids: vec![p.pid], cpu_pct: 0.0, mem_mb: p.mem_mb, windowed: p.windowed, system, alone },
                used,
            )),
        }
    }
    let mut out: Vec<Load> = by
        .into_iter()
        .map(|(_, mut l, ms)| {
            // A program with a window stands on its own whatever its helpers
            // are doing: closing the window closes them.
            l.alone |= l.windowed;
            l.cpu_pct = cpu_share(ms, wall_ms, cores);
            l
        })
        .collect();
    out.sort_by(|a, b| {
        b.cpu_pct.partial_cmp(&a.cpu_pct).unwrap_or(std::cmp::Ordering::Equal).then(b.mem_mb.cmp(&a.mem_mb))
    });
    out
}

/// Atlas and everything it started, and everything those started: never
/// offered for closing, whatever they're called.
pub fn atlas_family(procs: &[Proc], me: u32) -> Vec<u32> {
    let mut family = vec![me];
    // Bounded: a parent number Windows has reused can make a loop.
    for _ in 0..16 {
        let more: Vec<u32> = procs
            .iter()
            .filter(|p| family.contains(&p.parent) && !family.contains(&p.pid) && p.pid != p.parent)
            .map(|p| p.pid)
            .collect();
        if more.is_empty() {
            break;
        }
        family.extend(more);
    }
    family
}

/// Whether two names are the same program: "Discord", "discord.exe" and
/// "Discord.exe" are; a name of fewer than three letters matches nothing,
/// so an empty or one-letter entry in the work log can't match everything.
fn same_program(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.trim().trim_end_matches(".exe").trim_end_matches(".EXE").to_lowercase();
    let (a, b) = (norm(a), norm(b));
    if a.len() < 3 || b.len() < 3 {
        return false;
    }
    a == b || a.contains(&b) || b.contains(&a)
}

/// What a closing must leave alone, measured on this machine at this moment.
#[derive(Debug, Clone, Default)]
pub struct Spare {
    /// Atlas and its children (`atlas_family`).
    pub own_pids: Vec<u32>,
    /// Atlas's own program name on this machine (it may have been renamed),
    /// and its helpers.
    pub own_names: Vec<String>,
    /// The program in front of you.
    pub foreground: Option<String>,
    pub foreground_pid: Option<u32>,
    /// Programs you've had in front of you recently (the work log).
    pub in_use: Vec<String>,
    /// Your own never-close list (`tune.keep`).
    pub keep: Vec<String>,
    /// Heavy means at least this much memory...
    pub min_mb: u64,
    /// ...or at least this share of the machine's CPU.
    pub min_cpu: f32,
}

/// The programs worth closing, heaviest first, at most six. Every rule is a
/// refusal: Windows' hard-coded list and your own keep list, Atlas and its
/// helpers (by process and by name), the program in front of you, anything
/// started by Windows' services, a helper of another program, anything
/// you've used recently, and anything too light to be worth it.
pub fn pick_to_close(load: &[Load], spare: &Spare) -> Vec<Load> {
    let mut out: Vec<Load> = load
        .iter()
        .filter(|l| may_close(&l.name, &spare.keep))
        .filter(|l| {
            !l.name.to_lowercase().starts_with("atlas")
                && !spare.own_names.iter().any(|n| same_program(n, &l.name))
                && !ATLAS_HELPERS.iter().any(|n| l.name.eq_ignore_ascii_case(n))
                && !l.pids.iter().any(|p| spare.own_pids.contains(p))
        })
        .filter(|l| {
            !spare.foreground.as_deref().map(|f| same_program(f, &l.name)).unwrap_or(false)
                && !spare.foreground_pid.map(|p| l.pids.contains(&p)).unwrap_or(false)
        })
        .filter(|l| !l.system && l.alone)
        .filter(|l| !spare.in_use.iter().any(|u| same_program(u, &l.name)))
        .filter(|l| l.mem_mb >= spare.min_mb || l.cpu_pct >= spare.min_cpu)
        .cloned()
        .collect();
    out.sort_by(|a, b| {
        let w = |l: &Load| l.mem_mb as f32 + l.cpu_pct * 50.0;
        w(b).partial_cmp(&w(a)).unwrap_or(std::cmp::Ordering::Equal)
    });
    out.truncate(6);
    out
}

/// "What's slowing it down": the busiest programs by CPU over the sample,
/// then the ones holding the most memory, with the numbers.
pub fn slowest_words(load: &[Load]) -> String {
    let busy: Vec<String> = load
        .iter()
        .filter(|l| l.cpu_pct >= 1.0 && !l.name.eq_ignore_ascii_case("idle") && !l.name.eq_ignore_ascii_case("system idle process"))
        .take(4)
        .map(|l| format!("{} {:.0}%", l.name, l.cpu_pct))
        .collect();
    let mut by_mem: Vec<&Load> = load.iter().collect();
    by_mem.sort_by_key(|b| std::cmp::Reverse(b.mem_mb));
    let heavy: Vec<String> = by_mem.iter().take(4).map(|l| format!("{} {}", l.name, mb_words(l.mem_mb))).collect();
    let total: f32 = load
        .iter()
        .filter(|l| !l.name.eq_ignore_ascii_case("idle") && !l.name.eq_ignore_ascii_case("system idle process"))
        .map(|l| l.cpu_pct)
        .sum();
    let mut s = if busy.is_empty() {
        format!("Over the last few seconds the processor was barely used ({total:.0}% in all).")
    } else {
        format!("Over the last few seconds, busiest: {} ({total:.0}% of the processor in all).", busy.join(", "))
    };
    if !heavy.is_empty() {
        s.push_str(&format!(" Holding the most memory: {}.", heavy.join(", ")));
    }
    s
}

/// The process list now: every process, its parent, session, memory, CPU
/// time so far and whether it has a window. Windows only; elsewhere empty.
#[cfg(windows)]
fn snapshot() -> Vec<Proc> {
    use windows::Win32::Foundation::{CloseHandle, BOOL, FILETIME, HWND, LPARAM, TRUE};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
    use windows::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindow, GetWindowTextLengthW, GetWindowThreadProcessId, IsWindowVisible, GW_OWNER,
    };

    unsafe extern "system" fn each(hwnd: HWND, lp: LPARAM) -> BOOL {
        let pids = &mut *(lp.0 as *mut Vec<u32>);
        // A visible, unowned window with a title: one you could click on.
        let owned = GetWindow(hwnd, GW_OWNER).map(|o| !o.0.is_null()).unwrap_or(false);
        if IsWindowVisible(hwnd).as_bool() && !owned && GetWindowTextLengthW(hwnd) > 0 {
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid != 0 {
                pids.push(pid);
            }
        }
        TRUE
    }

    let mut windowed: Vec<u32> = Vec::new();
    // SAFETY: the callback only touches the Vec behind the pointer, which
    // outlives the call.
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut windowed as *mut Vec<u32> as isize));
    }
    let mut out = Vec::new();
    // SAFETY: the snapshot handle is closed below; the entry is sized as
    // the API requires.
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut more = Process32FirstW(snap, &mut e).is_ok();
        while more {
            let end = e.szExeFile.iter().position(|c| *c == 0).unwrap_or(e.szExeFile.len());
            let exe = String::from_utf16_lossy(&e.szExeFile[..end]);
            let name = exe.strip_suffix(".exe").or_else(|| exe.strip_suffix(".EXE")).unwrap_or(&exe).to_string();
            let pid = e.th32ProcessID;
            let mut session = 0u32;
            let _ = ProcessIdToSessionId(pid, &mut session);
            let (mut mem_mb, mut cpu_ms) = (0u64, 0u64);
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, false, pid)
                .or_else(|_| OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid));
            if let Ok(h) = h {
                let cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
                let mut m = PROCESS_MEMORY_COUNTERS { cb, ..Default::default() };
                if K32GetProcessMemoryInfo(h, &mut m, cb).as_bool() {
                    mem_mb = m.WorkingSetSize as u64 / 1_048_576;
                }
                let (mut c, mut x, mut k, mut u) =
                    (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
                if GetProcessTimes(h, &mut c, &mut x, &mut k, &mut u).is_ok() {
                    let t = |f: FILETIME| ((f.dwHighDateTime as u64) << 32 | f.dwLowDateTime as u64) / 10_000;
                    cpu_ms = t(k) + t(u);
                }
                let _ = CloseHandle(h);
            }
            out.push(Proc { pid, parent: e.th32ParentProcessID, name, session, mem_mb, cpu_ms, windowed: windowed.contains(&pid) });
            more = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    out
}

#[cfg(not(windows))]
fn snapshot() -> Vec<Proc> {
    Vec::new()
}

/// What a sample of the machine came to: each program's load, the whole
/// process list it was read from (for `atlas_family`), and the program in
/// front of you by process number.
#[derive(Debug, Clone, Default)]
pub struct Sampled {
    pub load: Vec<Load>,
    pub procs: Vec<Proc>,
    pub foreground_pid: Option<u32>,
}

/// Read every process, wait `wait`, read again: each program's CPU over that
/// stretch and its memory now. `None` where it can't be done -- anything
/// that isn't Windows, where Atlas doesn't close programs either.
pub fn sample_machine(wait: std::time::Duration) -> Option<Sampled> {
    if !cfg!(windows) {
        return None;
    }
    let started = std::time::Instant::now();
    let before = snapshot();
    std::thread::sleep(wait);
    let after = snapshot();
    let wall_ms = started.elapsed().as_millis() as u64;
    if after.is_empty() {
        // The native list couldn't be read: memory and windows from
        // `tasklist` instead, without CPU, so there is still an answer.
        let windowed = windowed_programs();
        let load = memory_by_app()
            .into_iter()
            .map(|(name, mem_mb)| Load {
                windowed: windowed.iter().any(|w| w.eq_ignore_ascii_case(&name)),
                alone: windowed.iter().any(|w| w.eq_ignore_ascii_case(&name)),
                name,
                mem_mb,
                ..Default::default()
            })
            .collect();
        return Some(Sampled { load, procs: Vec::new(), foreground_pid: foreground_pid() });
    }
    let cores = std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(1);
    Some(Sampled { load: sample_load(&before, &after, wall_ms, cores), procs: after, foreground_pid: foreground_pid() })
}

#[cfg(windows)]
fn foreground_pid() -> Option<u32> {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    // SAFETY: plain reads of the window in front.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        (pid != 0).then_some(pid)
    }
}

#[cfg(not(windows))]
fn foreground_pid() -> Option<u32> {
    None
}

/// How a closing went.
#[derive(Debug, Clone, PartialEq)]
pub enum Closed {
    /// It closed when asked, the way its X does.
    Asked,
    /// It didn't answer being asked, and was ended.
    Ended,
    /// Asked, still open, and left: a program with a window that won't close
    /// is nearly always asking whether to save, and ending it loses that work.
    LeftOpen,
    /// It had already gone.
    Gone,
    Failed(String),
}

/// Close programs: every process of each that is still the same program
/// (checked again now -- the list may be minutes old by the time you say
/// yes, and Windows reuses numbers). All are asked at once, then waited on
/// together for at most `wait`, so six programs cost one wait, not six; any
/// still running with no window of its own is then ended.
pub fn close_loads(loads: &[Load], wait: std::time::Duration) -> Vec<Closed> {
    if !cfg!(windows) {
        return loads.iter().map(|_| Closed::Failed("closing programs is only done on Windows".into())).collect();
    }
    let still = |l: &Load, now: &[Proc]| -> Vec<u32> {
        if l.pids.is_empty() {
            // Measured without process numbers (`sample_machine`'s
            // fallback): every process of that name.
            return now.iter().filter(|q| q.name.eq_ignore_ascii_case(&l.name)).map(|q| q.pid).collect();
        }
        l.pids
            .iter()
            .copied()
            .filter(|p| now.iter().any(|q| q.pid == *p && q.name.eq_ignore_ascii_case(&l.name)))
            .collect()
    };
    let taskkill = |force: bool, l: &Load, pids: &[u32]| -> Result<(), String> {
        let mut args: Vec<String> = Vec::new();
        if force {
            args.push("/F".into());
        }
        args.push("/T".into());
        if pids.is_empty() {
            args.extend(["/IM".into(), format!("{}.exe", l.name)]);
        } else {
            for p in pids {
                args.extend(["/PID".into(), p.to_string()]);
            }
        }
        let out = crate::tools::command("taskkill").args(&args).output().map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    };
    let now = snapshot();
    let mut result: Vec<Option<Closed>> = vec![None; loads.len()];
    for (i, l) in loads.iter().enumerate() {
        let pids = still(l, &now);
        if pids.is_empty() && !now.is_empty() {
            result[i] = Some(Closed::Gone);
            continue;
        }
        // A background process refuses a polite request ("can only be
        // terminated forcefully"); that refusal is expected, not a failure.
        crate::heard!(taskkill(false, l, &pids));
    }
    let until = std::time::Instant::now() + wait;
    loop {
        let now = snapshot();
        if now.is_empty() {
            // Nothing to check against: said, not assumed closed.
            for r in result.iter_mut().filter(|r| r.is_none()) {
                *r = Some(Closed::Failed("I asked it to close but couldn't read the process list to check".into()));
            }
            break;
        }
        let mut waiting = false;
        for (i, l) in loads.iter().enumerate() {
            if result[i].is_some() {
                continue;
            }
            let left = still(l, &now);
            if left.is_empty() {
                result[i] = Some(Closed::Asked);
            } else if std::time::Instant::now() >= until {
                result[i] = Some(if l.windowed {
                    Closed::LeftOpen
                } else {
                    match taskkill(true, l, &left) {
                        Ok(()) => Closed::Ended,
                        Err(e) => Closed::Failed(e),
                    }
                });
            } else {
                waiting = true;
            }
        }
        if !waiting {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    result.into_iter().map(|r| r.unwrap_or(Closed::Gone)).collect()
}

// ---------- what starts with Windows, from everywhere it can ----------

/// Where a startup entry lives, which decides who may change it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StartupFrom {
    /// Your Run key: Atlas can switch it off and on, as Task Manager does.
    YourRunKey,
    /// Your Startup folder: the same.
    YourStartupFolder,
    /// A scheduled task that runs when you sign in, not one of Windows' own.
    LogonTask,
    /// Machine-wide: an administrator's to change, in Task Manager.
    MachineRunKey,
    MachineStartupFolder,
}

impl StartupFrom {
    fn atlas_may_change(&self) -> bool {
        matches!(self, StartupFrom::YourRunKey | StartupFrom::YourStartupFolder | StartupFrom::LogonTask)
    }

    fn plain(&self) -> &'static str {
        match self {
            StartupFrom::YourRunKey => "your startup list",
            StartupFrom::YourStartupFolder => "your Startup folder",
            StartupFrom::LogonTask => "a task that runs when you sign in",
            StartupFrom::MachineRunKey => "the machine-wide startup list",
            StartupFrom::MachineStartupFolder => "the machine-wide Startup folder",
        }
    }
}

/// One thing that starts with Windows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StartupEntry {
    pub name: String,
    pub from: StartupFrom,
    /// What it runs.
    pub command: String,
    /// What it is known by where it lives: the value's name, the file's name,
    /// or the task's full path.
    pub key: String,
    /// Switched on (Task Manager's "Enabled").
    pub on: bool,
}

/// The entries `StartupApproved` marks off. Task Manager's "Disable" writes a
/// binary value whose first byte is odd (03); "Enable" an even one (02). An
/// entry with no value there is on.
pub fn startup_switched_off(reg_text: &str) -> Vec<String> {
    parse_reg_values(reg_text)
        .into_iter()
        .filter(|(_, data)| {
            data.get(..2).and_then(|b| u8::from_str_radix(b, 16).ok()).map(|b| b & 1 == 1).unwrap_or(false)
        })
        .map(|(name, _)| name)
        .collect()
}

/// Sign-in tasks, from the PowerShell line in `startup_entries`: one per line,
/// path, name, state and program separated by tabs. Windows' own (under
/// `\Microsoft\`) are left out there and here.
pub fn parse_logon_tasks(text: &str) -> Vec<StartupEntry> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.trim_end_matches('\r').split('\t').collect();
            if f.len() < 3 {
                return None;
            }
            let (path, name, state) = (f[0].trim(), f[1].trim(), f[2].trim());
            if name.is_empty() || path.to_lowercase().starts_with("\\microsoft\\") {
                return None;
            }
            let key = format!("{}{name}", if path.ends_with('\\') { path.to_string() } else { format!("{path}\\") });
            Some(StartupEntry {
                name: name.to_string(),
                from: StartupFrom::LogonTask,
                command: f.get(3).map(|s| s.trim().to_string()).unwrap_or_default(),
                key,
                on: !state.eq_ignore_ascii_case("disabled"),
            })
        })
        .collect()
}

/// The program a startup command runs, by name: `"C:\x\Spotify.exe" /bg`
/// and `C:\Program Files\x\Steam.exe -silent` are Spotify and Steam.
fn startup_exe(command: &str) -> String {
    let c = command.trim();
    let path = if let Some(rest) = c.strip_prefix('"') {
        rest.split('"').next().unwrap_or("")
    } else {
        match c.to_ascii_lowercase().find(".exe") {
            Some(i) => &c[..i + 4],
            None => c.split_whitespace().next().unwrap_or(""),
        }
    };
    let leaf = path.rsplit(['\\', '/']).next().unwrap_or(path);
    leaf.trim_end_matches(".exe").trim_end_matches(".EXE").trim_end_matches(".lnk").to_string()
}

/// The startup entries worth switching off, at most five: on, changeable by
/// Atlas without an administrator, not on either never-close list, not Atlas
/// itself (by name, or by running Atlas's own program), and not opened this
/// week by its name or by the program it runs.
pub fn pick_startup_to_stop(entries: &[StartupEntry], used_this_week: &[String], keep: &[String], own_exe: &str) -> Vec<StartupEntry> {
    entries
        .iter()
        .filter(|e| e.on && e.from.atlas_may_change())
        .filter(|e| {
            let exe = startup_exe(&e.command);
            may_close(&e.name, keep)
                && (exe.is_empty() || may_close(&exe, keep))
                && !used_this_week.iter().any(|u| same_program(u, &e.name) || (!exe.is_empty() && same_program(u, &exe)))
                && !(own_exe.len() > 2 && e.command.to_lowercase().contains(&own_exe.to_lowercase()))
        })
        .take(5)
        .cloned()
        .collect()
}

/// Everything that starts with Windows on this machine, for you and for
/// everyone, each marked on or off the way Task Manager shows it. Sign-in
/// tasks need PowerShell, about a second, so they're read only when asked.
type KeptStartup = Option<(std::time::Instant, bool, Vec<StartupEntry>)>;
static KEPT_STARTUP: std::sync::Mutex<KeptStartup> = std::sync::Mutex::new(None);

/// `startup_entries`, kept for half a minute: the health answer reads it
/// on a thread while it samples the CPU, and the plan then takes it from
/// here rather than reading every key again (4 Oct 2026: "how's the
/// machine" took 8 seconds on the laptop, most of it waiting in turn).
pub fn startup_entries_kept(with_tasks: bool) -> Vec<StartupEntry> {
    if let Some((at, tasks, v)) = KEPT_STARTUP.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
        if at.elapsed() < std::time::Duration::from_secs(30) && *tasks == with_tasks {
            return v.clone();
        }
    }
    let v = startup_entries(with_tasks);
    *KEPT_STARTUP.lock().unwrap_or_else(|p| p.into_inner()) = Some((std::time::Instant::now(), with_tasks, v.clone()));
    v
}

pub fn startup_entries(with_tasks: bool) -> Vec<StartupEntry> {
    if !cfg!(windows) {
        return Vec::new();
    }
    let reg = |key: &str| -> String {
        crate::tools::command("reg")
            .args(["query", key])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default()
    };
    let mut out = Vec::new();
    let from_key = |key: &str, approved: &[&str], from: StartupFrom, out: &mut Vec<StartupEntry>| {
        let off: Vec<String> = approved.iter().flat_map(|a| startup_switched_off(&reg(a))).collect();
        for (name, command) in parse_reg_values(&reg(key)) {
            let on = !off.iter().any(|o| o.eq_ignore_ascii_case(&name));
            out.push(StartupEntry { name: name.clone(), from, command, key: name, on });
        }
    };
    from_key(RUN, &[APPROVED], StartupFrom::YourRunKey, &mut out);
    from_key(RUN_MACHINE, &[APPROVED_MACHINE], StartupFrom::MachineRunKey, &mut out);
    from_key(RUN_MACHINE_32, &[APPROVED_MACHINE_32], StartupFrom::MachineRunKey, &mut out);
    let folder = |dir: Option<std::path::PathBuf>, approved: &str, from: StartupFrom, out: &mut Vec<StartupEntry>| {
        let Some(dir) = dir else { return };
        let Ok(rd) = std::fs::read_dir(&dir) else { return };
        let off = startup_switched_off(&reg(approved));
        for e in rd.flatten() {
            let file = e.file_name().to_string_lossy().to_string();
            if file.eq_ignore_ascii_case("desktop.ini") || e.path().is_dir() {
                continue;
            }
            let name = std::path::Path::new(&file).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or(file.clone());
            let on = !off.iter().any(|o| o.eq_ignore_ascii_case(&file));
            out.push(StartupEntry { name, from, command: e.path().display().to_string(), key: file, on });
        }
    };
    let under = |var: &str, rest: &str| crate::doctor::lookup_env(var).map(|v| std::path::PathBuf::from(v).join(rest));
    folder(under("APPDATA", r"Microsoft\Windows\Start Menu\Programs\Startup"), APPROVED_FOLDER, StartupFrom::YourStartupFolder, &mut out);
    folder(under("ProgramData", r"Microsoft\Windows\Start Menu\Programs\StartUp"), APPROVED_MACHINE_FOLDER, StartupFrom::MachineStartupFolder, &mut out);
    if with_tasks {
        let ps = "Get-ScheduledTask | Where-Object { $_.TaskPath -notlike '\\Microsoft\\*' -and ($_.Triggers | Where-Object { $_.CimClass.CimClassName -eq 'MSFT_TaskLogonTrigger' }) } | ForEach-Object { \"{0}`t{1}`t{2}`t{3}\" -f $_.TaskPath, $_.TaskName, $_.State, (($_.Actions | Select-Object -First 1).Execute) }";
        if let Ok(o) = crate::tools::command("powershell").args(["-NoProfile", "-NonInteractive", "-Command", ps]).output() {
            out.extend(parse_logon_tasks(&String::from_utf8_lossy(&o.stdout)));
        }
    }
    out
}

/// Switch a startup entry off or back on. Run key and Startup folder: the
/// mark Task Manager uses, so Task Manager shows the same and can undo it
/// too. Sign-in task: Task Scheduler's own disable and enable. Machine-wide
/// entries need an administrator and are refused here, plainly.
pub fn set_startup(e: &StartupEntry, on: bool) -> Result<(), String> {
    // What was kept is out of date the moment one is changed.
    *KEPT_STARTUP.lock().unwrap_or_else(|p| p.into_inner()) = None;
    if !cfg!(windows) {
        return Err("startup programs are only changed on Windows".into());
    }
    let mark = if on { "020000000000000000000000" } else { "030000000000000000000000" };
    let out = match e.from {
        StartupFrom::YourRunKey => crate::tools::command("reg")
            .args(["add", APPROVED, "/v", e.key.as_str(), "/t", "REG_BINARY", "/d", mark, "/f"])
            .output(),
        StartupFrom::YourStartupFolder => crate::tools::command("reg")
            .args(["add", APPROVED_FOLDER, "/v", e.key.as_str(), "/t", "REG_BINARY", "/d", mark, "/f"])
            .output(),
        StartupFrom::LogonTask => crate::tools::command("schtasks")
            .args(["/change", "/tn", e.key.as_str(), if on { "/enable" } else { "/disable" }])
            .output(),
        StartupFrom::MachineRunKey | StartupFrom::MachineStartupFolder => {
            return Err(format!(
                "{} is in {}, which needs an administrator -- Task Manager's Startup apps page changes it",
                e.name,
                e.from.plain()
            ))
        }
    }
    .map_err(|err| format!("couldn't change {}'s startup: {err}", e.name))?;
    if out.status.success() {
        Ok(())
    } else {
        let why = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if why.is_empty() { format!("Windows refused to change {}", e.name) } else { why })
    }
}

/// "What starts with Windows": every entry, on ones first, each with where
/// it lives, and which of them are an administrator's to change.
pub fn startup_words(entries: &[StartupEntry]) -> String {
    if entries.is_empty() {
        return "Nothing I can see starts with Windows for you.".into();
    }
    let on: Vec<&StartupEntry> = entries.iter().filter(|e| e.on).collect();
    let off = entries.len() - on.len();
    let mut groups: Vec<String> = Vec::new();
    for from in [
        StartupFrom::YourRunKey,
        StartupFrom::YourStartupFolder,
        StartupFrom::LogonTask,
        StartupFrom::MachineRunKey,
        StartupFrom::MachineStartupFolder,
    ] {
        let names: Vec<String> = on.iter().filter(|e| e.from == from).map(|e| e.name.clone()).collect();
        if !names.is_empty() {
            groups.push(format!("from {}: {}", from.plain(), names.join(", ")));
        }
    }
    let mut s = format!("{} start{} with Windows -- {}.", on.len(), if on.len() == 1 { "s" } else { "" }, groups.join("; "));
    if off > 0 {
        s.push_str(&format!(" {off} more {} switched off already.", if off == 1 { "is" } else { "are" }));
    }
    if on.iter().any(|e| !e.from.atlas_may_change()) {
        s.push_str(" The machine-wide ones need an administrator, so those are yours to change in Task Manager.");
    }
    s
}

// ---------- where the space went ----------

/// Downloads and the temporary folder, looked at more closely: how big, the
/// biggest files, and copies of the same file kept twice.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SpaceLook {
    pub downloads_mb: u64,
    /// The biggest files in Downloads, 50 MB and over, largest first.
    pub biggest: Vec<(std::path::PathBuf, u64)>,
    /// Each set of identical files, oldest first (the first is the original).
    pub duplicates: Vec<Vec<std::path::PathBuf>>,
    /// What the extra copies take up.
    pub duplicate_mb: u64,
    pub temp_mb: u64,
    /// False when the time ran out before every file was looked at.
    pub complete: bool,
}

impl SpaceLook {
    /// Every copy after the first, for moving somewhere.
    pub fn extra_copies(&self) -> Vec<std::path::PathBuf> {
        self.duplicates.iter().flat_map(|set| set.iter().skip(1).cloned()).collect()
    }
}

/// A content fingerprint for telling copies apart: the whole file when it's
/// under 64 MB, otherwise its first and last 4 MB with its length -- two
/// different files that size agreeing at both ends is not a real case.
fn content_print(path: &std::path::Path, len: u64) -> Option<u64> {
    use std::io::{Read, Seek, SeekFrom};
    use std::hash::Hasher;
    let mut f = std::fs::File::open(path).ok()?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    h.write_u64(len);
    let mut buf = vec![0u8; 1 << 20];
    let mut read_some = |f: &mut std::fs::File, limit: u64, h: &mut std::collections::hash_map::DefaultHasher| -> Option<()> {
        let mut left = limit;
        while left > 0 {
            let n = f.read(&mut buf[..(left.min(1 << 20)) as usize]).ok()?;
            if n == 0 {
                break;
            }
            h.write(&buf[..n]);
            left -= n as u64;
        }
        Some(())
    };
    const WHOLE: u64 = 64 << 20;
    const END: u64 = 4 << 20;
    if len <= WHOLE {
        read_some(&mut f, len, &mut h)?;
    } else {
        read_some(&mut f, END, &mut h)?;
        f.seek(SeekFrom::End(-(END as i64))).ok()?;
        read_some(&mut f, END, &mut h)?;
    }
    Some(h.finish())
}

/// Look through `downloads` (and size `temp`) for at most `budget`. Copies
/// are only compared between files of exactly the same size, a megabyte or
/// more, so the cost is reading the few that could be copies.
pub fn look_at_space(downloads: &std::path::Path, temp: &std::path::Path, budget: std::time::Duration) -> SpaceLook {
    let until = std::time::Instant::now() + budget;
    let mut files: Vec<(std::path::PathBuf, u64, std::time::SystemTime)> = Vec::new();
    let mut complete = true;
    let mut stack = vec![downloads.to_path_buf()];
    while let Some(d) = stack.pop() {
        if std::time::Instant::now() > until {
            complete = false;
            break;
        }
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let Ok(m) = std::fs::symlink_metadata(e.path()) else { continue };
            if m.file_type().is_symlink() {
                continue;
            }
            if m.is_dir() {
                stack.push(e.path());
            } else {
                files.push((e.path(), m.len(), m.modified().unwrap_or(std::time::UNIX_EPOCH)));
            }
        }
    }
    let downloads_mb = files.iter().map(|f| f.1).sum::<u64>() / 1_048_576;
    let mut biggest: Vec<(std::path::PathBuf, u64)> =
        files.iter().filter(|f| f.1 >= 50 * 1_048_576).map(|f| (f.0.clone(), f.1 / 1_048_576)).collect();
    biggest.sort_by_key(|b| std::cmp::Reverse(b.1));
    biggest.truncate(10);

    let mut by_size: std::collections::BTreeMap<u64, Vec<usize>> = Default::default();
    for (i, f) in files.iter().enumerate() {
        if f.1 >= 1_048_576 {
            by_size.entry(f.1).or_default().push(i);
        }
    }
    let mut duplicates = Vec::new();
    let mut duplicate_bytes = 0u64;
    for (len, idx) in by_size.iter().rev().filter(|(_, v)| v.len() > 1) {
        if std::time::Instant::now() > until {
            complete = false;
            break;
        }
        let mut by_print: std::collections::HashMap<u64, Vec<usize>> = Default::default();
        for i in idx {
            if let Some(fp) = content_print(&files[*i].0, *len) {
                by_print.entry(fp).or_default().push(*i);
            }
        }
        for (_, mut same) in by_print.into_iter().filter(|(_, v)| v.len() > 1) {
            same.sort_by_key(|i| files[*i].2);
            duplicate_bytes += len * (same.len() as u64 - 1);
            duplicates.push(same.into_iter().map(|i| files[i].0.clone()).collect::<Vec<_>>());
        }
    }
    let left = until.saturating_duration_since(std::time::Instant::now()).max(std::time::Duration::from_millis(200));
    SpaceLook {
        downloads_mb,
        biggest,
        duplicates,
        duplicate_mb: duplicate_bytes / 1_048_576,
        temp_mb: folder_mb(temp, left),
        complete,
    }
}

/// The space look, said.
pub fn space_words(s: &SpaceLook) -> String {
    let mut out = format!("Downloads holds {}.", mb_words(s.downloads_mb));
    if !s.biggest.is_empty() {
        let names: Vec<String> = s
            .biggest
            .iter()
            .take(5)
            .map(|(p, mb)| format!("{} {}", p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), mb_words(*mb)))
            .collect();
        out.push_str(&format!(" The biggest: {}.", names.join(", ")));
    }
    if !s.duplicates.is_empty() {
        let copies: usize = s.duplicates.iter().map(|d| d.len() - 1).sum();
        out.push_str(&format!(
            " {copies} file{} there {} a copy of another, {} in extra copies.",
            if copies == 1 { "" } else { "s" },
            if copies == 1 { "is" } else { "are" },
            mb_words(s.duplicate_mb)
        ));
    }
    out.push_str(&format!(" Temporary files: {}.", mb_words(s.temp_mb)));
    if !s.complete {
        out.push_str(" (I ran out of time before looking at all of it.)");
    }
    out
}

// ---------- moving files, only where you say ----------

/// The folder named after the last " to " or " into " in what was said,
/// quotes and closing punctuation taken off. Only a full path counts:
/// "to my archive" names no folder, and guessing one is how files get lost.
pub fn move_destination(said: &str) -> Option<std::path::PathBuf> {
    let lower = said.to_ascii_lowercase();
    let at = [" into ", " to "].iter().filter_map(|w| lower.rfind(w).map(|i| i + w.len())).max()?;
    let raw = said[at..].trim().trim_end_matches(['.', '?', '!', ',']).trim().trim_matches(['"', '\'']).trim();
    if raw.is_empty() {
        return None;
    }
    let p = std::path::PathBuf::from(raw);
    // "D:\Archive" is absolute on Windows only; it's recognised everywhere,
    // so a test on any machine reads it the same way.
    let windows_abs = raw.len() >= 3 && raw.as_bytes()[1] == b':' && (raw.as_bytes()[2] == b'\\' || raw.as_bytes()[2] == b'/');
    // And "/mnt/big/archive" the other way round: Windows doesn't call it
    // absolute (no drive), but it is a folder named in full (6 Oct 2026).
    let unix_abs = raw.starts_with('/');
    (p.is_absolute() || windows_abs || unix_abs).then_some(p)
}

/// Whether files may go into `dest`: never into Windows, the program
/// folders, anyone's AppData or the system's own folders, and never into
/// the folder they came from (that moves nothing).
pub fn may_move_into(dest: &std::path::Path, from_dir: &std::path::Path) -> Result<(), String> {
    let d = dest.display().to_string().replace('/', "\\").to_lowercase();
    let d = d.trim_end_matches('\\');
    let parts: Vec<&str> = d.split('\\').filter(|p| !p.is_empty()).collect();
    let second = parts.get(1).copied().unwrap_or("");
    let windows_system = ["windows", "program files", "program files (x86)", "programdata", "$recycle.bin", "system volume information"];
    let unix_system = ["etc", "usr", "bin", "sbin", "lib", "lib64", "boot", "proc", "sys", "dev", "var", "opt", "root"];
    let first = parts.first().copied().unwrap_or("");
    let drive_rooted = first.len() == 2 && first.ends_with(':');
    if (drive_rooted && windows_system.contains(&second)) || (!drive_rooted && unix_system.contains(&first)) {
        return Err(format!("{} is where the system keeps its own files, so nothing of yours goes there", dest.display()));
    }
    if parts.contains(&"appdata") {
        return Err(format!("{} is inside a program's settings folder, so nothing of yours goes there", dest.display()));
    }
    let from = from_dir.display().to_string().replace('/', "\\").to_lowercase();
    if d == from.trim_end_matches('\\') {
        return Err("that's the folder they're already in".into());
    }
    Ok(())
}

/// Move one file, never over another: a name already taken gets " (2)".
/// Across drives a rename can't work, so it's copied, the copy's size
/// checked, and only then the original removed.
fn move_one(from: &std::path::Path, dest_dir: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let name = from.file_name().ok_or_else(|| format!("{} has no name", from.display()))?;
    let mut to = dest_dir.join(name);
    let mut n = 2;
    while to.exists() {
        let stem = std::path::Path::new(name).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let ext = std::path::Path::new(name).extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
        to = dest_dir.join(format!("{stem} ({n}){ext}"));
        n += 1;
    }
    move_exact(from, &to)?;
    Ok(to)
}

fn move_exact(from: &std::path::Path, to: &std::path::Path) -> Result<(), String> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    let len = std::fs::metadata(from).map_err(|e| format!("{}: {e}", from.display()))?.len();
    let copied = std::fs::copy(from, to).map_err(|e| format!("{} couldn't be copied: {e}", from.display()))?;
    if copied != len {
        crate::heard!(std::fs::remove_file(to));
        return Err(format!("{} didn't copy whole, so the original stays", from.display()));
    }
    std::fs::remove_file(from).map_err(|e| format!("{} was copied but the original couldn't be removed: {e}", from.display()))
}

/// Move each file into `dest` (made if it isn't there). Returns each move
/// that happened, as (where it was, where it is), and why each other didn't.
pub fn move_files_into(files: &[std::path::PathBuf], dest: &std::path::Path) -> (Vec<(std::path::PathBuf, std::path::PathBuf)>, Vec<String>) {
    let mut done = Vec::new();
    let mut not = Vec::new();
    if let Err(e) = std::fs::create_dir_all(dest) {
        return (done, vec![format!("{} couldn't be made: {e}", dest.display())]);
    }
    for f in files {
        match move_one(f, dest) {
            Ok(to) => done.push((f.clone(), to)),
            Err(e) => not.push(e),
        }
    }
    (done, not)
}

/// Put moved files back where they were. A file whose old place has
/// something new in it is left where it is, and said.
pub fn move_back(moves: &[(std::path::PathBuf, std::path::PathBuf)]) -> (usize, Vec<String>) {
    let mut back = 0;
    let mut not = Vec::new();
    for (was, is) in moves {
        if was.exists() {
            not.push(format!("{} has something new in its old place", was.display()));
            continue;
        }
        if !is.exists() {
            not.push(format!("{} isn't where I moved it any more", is.display()));
            continue;
        }
        if let Some(dir) = was.parent() {
            crate::heard!(std::fs::create_dir_all(dir));
        }
        match move_exact(is, was) {
            Ok(()) => back += 1,
            Err(e) => not.push(e),
        }
    }
    (back, not)
}

// ---------- reading the request, and taking things back ----------

/// Which part of looking after the machine was asked for.
#[derive(Debug, Clone, PartialEq)]
pub enum TuneAsk {
    /// "Close what I don't need."
    Close,
    /// "What's slowing my computer down?"
    Slowing,
    /// "What starts with Windows?"
    Startup,
    /// "What's taking up my space?"
    Space,
    /// "Clear my temp files."
    ClearTemp,
    /// "Move my big downloads to D:\Archive" (or the duplicates).
    MoveInto { to: std::path::PathBuf, duplicates: bool },
    /// A move with no folder named: asked for, never guessed.
    MoveWhere,
}

/// What was asked, from the whole sentence.
pub fn tune_ask(said: &str) -> TuneAsk {
    let t = said.to_lowercase();
    let has = |w: &[&str]| w.iter().any(|x| t.contains(x));
    if t.starts_with("move") || has(&[" move "]) {
        let duplicates = has(&["duplicate", "copies", "copy of"]);
        return match move_destination(said) {
            Some(to) => TuneAsk::MoveInto { to, duplicates },
            None => TuneAsk::MoveWhere,
        };
    }
    if has(&["startup", "start up", "starts with windows", "start with windows", "starts up with", "runs at startup", "run at startup", "launch at startup", "boot"]) {
        return TuneAsk::Startup;
    }
    if has(&["temp file", "temporary file", "temp folder", "temporary folder"]) {
        return TuneAsk::ClearTemp;
    }
    if has(&["space", "storage", "duplicate", "big files", "biggest files", "downloads folder", "disk"]) {
        return TuneAsk::Space;
    }
    if has(&["close", "kill", "end task", "shut down what", "quit what"]) {
        return TuneAsk::Close;
    }
    TuneAsk::Slowing
}

/// How to take back something Atlas did to the machine, kept beside the
/// history entry it belongs to (`TUNE_UNDO_RECORD`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TuneUndo {
    /// Switch this startup entry back on.
    Startup(StartupEntry),
    /// Move these files back: (where it was, where it is).
    Moves(Vec<(std::path::PathBuf, std::path::PathBuf)>),
    /// A folder sorted (`organize`, 2 Oct 2026): each file moved back, then
    /// the folders the sorting made taken away again, innermost first, but
    /// only those that are empty once the files are back -- anything you've
    /// put in one since keeps it.
    Organized { moves: Vec<(std::path::PathBuf, std::path::PathBuf)>, made: Vec<std::path::PathBuf> },
}

pub const TUNE_UNDO_RECORD: &str = "tune_undo";

/// Take one back: what happened, or why it couldn't be (and then it stays
/// in the record, to try again).
pub fn undo_tune_change(u: &TuneUndo) -> Result<String, String> {
    match u {
        TuneUndo::Startup(e) => match set_startup(e, true) {
            Ok(()) => Ok(format!("{} starts with Windows again.", e.name)),
            Err(err) => Err(format!("I couldn't switch {} back on: {err}.", e.name)),
        },
        TuneUndo::Moves(m) => {
            let (back, not) = move_back(m);
            let mut s = format!("Moved {back} of {} back.", m.len());
            if !not.is_empty() {
                s.push_str(&format!(" Not moved: {}.", not.join("; ")));
            }
            if back == 0 {
                Err(s)
            } else {
                Ok(s)
            }
        }
        TuneUndo::Organized { moves, made } => {
            let (back, not) = move_back(moves);
            // `remove_dir` only ever removes an empty folder.
            for d in made.iter().rev() {
                crate::heard!(std::fs::remove_dir(d));
            }
            let mut s = format!("Put {back} of {} back where {} were.", moves.len(), if back == 1 { "it" } else { "they" });
            if !not.is_empty() {
                s.push_str(&format!(" Not put back: {}.", not.join("; ")));
            }
            if back == 0 && !moves.is_empty() {
                Err(s)
            } else {
                Ok(s)
            }
        }
    }
}
