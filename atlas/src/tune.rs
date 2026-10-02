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
    let _ = std::fs::write(from.with_extension("MOVED.txt"), note);
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
    v.sort_by(|a, b| b.1.cmp(&a.1));
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
    v.sort_by(|a, b| b.1.cmp(&a.1));
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
        let _ = std::fs::remove_dir(&d);
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

pub fn windowed_programs() -> Vec<String> {
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
pub const NEVER_CLOSE: &[&str] = &[
    "explorer", "dwm", "csrss", "winlogon", "svchost", "sihost", "ctfmon", "fontdrvhost", "runtimebroker",
    "searchhost", "startmenuexperiencehost", "textinputhost", "shellexperiencehost", "applicationframehost",
    "securityhealth", "msmpeng", "audiodg", "memory compression", "system", "registry", "lsass", "smss",
    "wininit", "services", "taskhostw", "dllhost", "conhost", "atlas", "llama-server", "claude", "taskmgr",
    "systemsettings", "lockapp", "widgets", "nissrv", "mpdefendercoreservice",
];

pub fn may_close(name: &str, keep: &[String]) -> bool {
    let n = name.to_lowercase();
    !NEVER_CLOSE.iter().any(|k| n == *k || n.starts_with(k)) && !keep.iter().any(|k| n.contains(&k.to_lowercase()))
}

/// Ask a program to close, the way its window's X does: it can save first.
/// Never forced.
pub fn close_program(name: &str) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("closing programs is only done on Windows".into());
    }
    let out = crate::tools::command("taskkill")
        .args(["/IM", &format!("{name}.exe")])
        .output()
        .map_err(|e| format!("couldn't ask {name} to close: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// What starts with Windows for you (`reg query` of your Run key): names.
pub fn parse_reg_run(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            let mut parts = l.splitn(3, "    ");
            let name = parts.next()?.trim();
            let kind = parts.next()?.trim();
            kind.starts_with("REG_").then(|| name.to_string())
        })
        .filter(|n| !n.is_empty() && !n.eq_ignore_ascii_case("(Default)"))
        .collect()
}

const RUN: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const APPROVED: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

/// Your startup programs. (Only your own: the machine-wide ones need an
/// administrator, and are yours to change in Task Manager.)
pub fn startup_programs() -> Vec<String> {
    if !cfg!(windows) {
        return Vec::new();
    }
    crate::tools::command("reg")
        .args(["query", RUN])
        .output()
        .map(|o| parse_reg_run(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

/// Stop one starting with Windows, the way Task Manager's "Disable" does:
/// the entry stays, marked off, and "Enable" there puts it back.
pub fn stop_starting(name: &str) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("startup programs are only changed on Windows".into());
    }
    let out = crate::tools::command("reg")
        .args(["add", APPROVED, "/v", name, "/t", "REG_BINARY", "/d", "030000000000000000000000", "/f"])
        .output()
        .map_err(|e| format!("couldn't change {name}'s startup: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// What an optimization run offers to do, all of it on one yes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    /// Programs open, holding memory, unused today: (name, MB).
    pub close: Vec<(String, u64)>,
    /// Startup programs you haven't opened this week.
    pub stop_starting: Vec<String>,
    /// The temporary folder and its size.
    pub temp: Option<(std::path::PathBuf, u64)>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.close.is_empty() && self.stop_starting.is_empty() && self.temp.is_none()
    }

    /// The offer, said with the numbers.
    pub fn offer(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if !self.close.is_empty() {
            let mb: u64 = self.close.iter().map(|c| c.1).sum();
            let names: Vec<String> = self.close.iter().map(|c| format!("{} ({} MB)", c.0, c.1)).collect();
            parts.push(format!("close {} -- open but not used today, about {:.1} GB back", names.join(", "), mb as f32 / 1024.0));
        }
        if !self.stop_starting.is_empty() {
            parts.push(format!(
                "stop {} starting with Windows -- not opened this week, and Task Manager can turn {} back on",
                self.stop_starting.join(", "),
                if self.stop_starting.len() == 1 { "it" } else { "them" }
            ));
        }
        if let Some((_, mb)) = &self.temp {
            parts.push(format!("clear {mb} MB of temporary files, leaving anything in use"));
        }
        format!("I can {}. Go ahead?", parts.join("; "))
    }
}
