//! Finding space on your disk, without ever being the reason you lost something.
//!
//! Everything else Atlas deletes lives under `data/` — its own folder, its own
//! mess, capped at 500MB, with `retention::out_of_bounds` refusing anything
//! outside and logging the attempt as a bug upstream. That boundary is why
//! Atlas is safe to leave running.
//!
//! This crosses it, deliberately and narrowly, because running out of disk
//! stops Atlas working at all and "I could see the problem and wasn't allowed
//! to mention it" is a poor answer.
//!
//! ## Four rules, and the reasoning for each
//!
//! **1. An allowlist of places, never a blocklist.** A blocklist means Atlas
//! deletes anything nobody thought to exclude, and the first thing nobody
//! thought of is the thing you cannot replace. Every location here is named,
//! and anything unnamed is invisible to this module.
//!
//! **2. Nothing is deleted. Things are moved to Atlas's trash**, which keeps
//! them 30 days. Every reclaim is reversible for a month. If Atlas is ever
//! wrong about a file, you get it back.
//!
//! **3. Nothing recent.** A cache written this morning is a cache in use. Age
//! thresholds are per-category and deliberately generous.
//!
//! **4. Atlas proposes; you decide.** `survey` returns candidates and nothing
//! else. There is no code path in this module that removes a file on Atlas's
//! own initiative — `reclaim` takes an explicit list, which has to come from a
//! person saying yes.
//!
//! ## What is deliberately not here
//!
//! No `Documents`, `Desktop`, `Pictures`, source folders, or anything you
//! made. No "large files you haven't opened lately" — that heuristic finds
//! your archives and your backups. No emptying the system Recycle Bin: that is
//! the undo you already have, and a tool that empties it has removed your
//! safety net to save you space.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// What kind of thing this is, which decides how old it must be and what you
/// lose by dropping it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Files a program wrote to a temp folder and never cleaned up.
    Temp,
    /// A package manager's download cache. Rebuilt on demand.
    PackageCache,
    /// Build output — `target`, `node_modules`, `__pycache__`.
    BuildOutput,
    /// An installer you already ran.
    Installer,
    /// A crash dump or an old log.
    Debris,

    // --- below here: reported, never moved by Atlas ----------------------
    //
    // These are things only you can judge. A four-gigabyte video from 2019 is
    // either a wedding or a download you forgot; Atlas cannot tell, and the
    // cost of guessing wrong is absolute. So it says where the space went and
    // stops.
    /// A large file nothing has opened in a long time.
    BigAndOld,
    /// Screenshots. Almost always disposable, occasionally not.
    Screenshots,
    /// An installed application, with how long since it last ran.
    App,
    /// A folder that is simply large, so you know where the space went.
    BigFolder,
}

impl Kind {
    /// How old, in days, before this is worth offering.
    ///
    /// Generous on purpose. The cost of waiting another week is a few hundred
    /// megabytes; the cost of being wrong is your afternoon.
    pub fn min_age_days(&self) -> u64 {
        match self {
            Kind::Temp => 7,
            Kind::PackageCache => 30,
            // The longest, because a build folder belongs to a project, and a
            // project you have not touched in two months is not necessarily a
            // project you have finished.
            Kind::BuildOutput => 60,
            Kind::Installer => 30,
            Kind::Debris => 14,
            Kind::BigAndOld => 180,
            Kind::Screenshots => 90,
            Kind::App => 180,
            Kind::BigFolder => 0,
        }
    }

    /// May Atlas move this itself, or is it only reporting?
    ///
    /// **This is the line the whole module turns on.** Looking is safe;
    /// removing is not. Atlas surveys your whole disk so it can tell you where
    /// the space went, and confines what it will *touch* to things that
    /// rebuild themselves. Everything else it names and leaves alone.
    ///
    /// The first version of this module conflated the two, and the result was
    /// a scanner that could only see caches — safe, and close to useless,
    /// because the space is rarely in the caches.
    pub fn atlas_may_move(&self) -> bool {
        matches!(
            self,
            Kind::Temp | Kind::PackageCache | Kind::BuildOutput | Kind::Installer | Kind::Debris
        )
    }

    /// What dropping it actually costs you, in plain words.
    pub fn costs_you(&self) -> &'static str {
        match self {
            Kind::Temp => "nothing — these are leftovers no program is waiting for",
            Kind::PackageCache => "the next install of that package downloads again",
            Kind::BuildOutput => "the next build of that project takes longer once",
            Kind::Installer => "nothing — you already ran it. Re-download if you need it again",
            Kind::Debris => "nothing, unless you are mid-way through diagnosing a crash",
            Kind::BigAndOld => "only you know — Atlas won't touch this, it's just telling you",
            Kind::Screenshots => "the screenshots themselves. Look before you clear them",
            Kind::App => "the application. Uninstall it properly rather than deleting the folder",
            Kind::BigFolder => "nothing yet — this is where your space went, not a suggestion",
        }
    }
}

/// One thing Atlas could move to the trash, if you said so.
///
/// Serialisable so a survey run on the crew can be kept in the store and
/// shown on the Status page, where the choosing happens (27 Sep 2026).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub path: PathBuf,
    pub size_mb: u64,
    pub kind: Kind,
    pub age_days: u64,
}

impl Candidate {
    /// One line, with the number and the consequence together.
    ///
    /// Never a bare size. "4.2 GB" invites a yes; "4.2 GB, and the next build
    /// takes longer once" is a decision.
    pub fn line(&self) -> String {
        format!(
            "{} — {} MB, {} days old. {} Costs you: {}",
            self.path.display(),
            self.size_mb,
            self.age_days,
            if self.kind.atlas_may_move() { "I can move this." } else { "Yours to decide." },
            self.kind.costs_you()
        )
    }
}

/// Folder names that are safe to offer, and what they are.
///
/// The allowlist. Anything not named here is invisible to this module, which
/// is the whole safety property — a name nobody added cannot be deleted by
/// something nobody reviewed.
pub const KNOWN: &[(&str, Kind)] = &[
    ("Temp", Kind::Temp),
    ("tmp", Kind::Temp),
    ("Crash Reports", Kind::Debris),
    ("CrashDumps", Kind::Debris),
    ("node_modules", Kind::BuildOutput),
    ("__pycache__", Kind::BuildOutput),
    ("target", Kind::BuildOutput),
    (".gradle", Kind::PackageCache),
    (".nuget", Kind::PackageCache),
    ("pip", Kind::PackageCache),
    ("npm-cache", Kind::PackageCache),
    ("_cacache", Kind::PackageCache),
];

/// Names that stop a walk dead, wherever they appear.
///
/// A second belt, not the primary safety mechanism — the allowlist above is
/// that. This exists because directory walks follow links and nesting, and a
/// `node_modules` inside `Documents` is still inside `Documents`.
pub const NEVER: &[&str] = &[
    "Documents",
    "Desktop",
    "Pictures",
    "Videos",
    "Music",
    "OneDrive",
    "Dropbox",
    "iCloud",
    "Google Drive",
    ".git",
    ".ssh",
    "Recycle.Bin",
    "$Recycle.Bin",
    "Trash",
    "System32",
    "Program Files",
    "Windows",
];

/// The places Atlas may look: your home folder and the temp folder, from the
/// environment. Named here rather than discovered, so how far it can reach is
/// a decision written down in one place — `atlas reclaim` and the Status
/// page's "Look for space" both start from it. Empty when neither is known.
pub fn roots_from_env() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(home) = crate::doctor::lookup_env("USERPROFILE").or_else(|| crate::doctor::lookup_env("HOME")) {
        roots.push(PathBuf::from(&home));
    }
    if let Some(t) = crate::doctor::lookup_env("TEMP") {
        roots.push(PathBuf::from(t));
    }
    roots
}

/// Is any part of this path something Atlas must never touch?
pub fn forbidden(path: &Path) -> bool {
    // Split on both separators by hand rather than using `components()`.
    //
    // `components()` only knows the separator of the platform it is compiled
    // for, so on Linux a Windows path arrives as a *single* component --
    // `C:\$Recycle.Bin\S-1-5-21` -- and matches nothing on the list. Caught
    // by a test that expected the recycle bin to be refused and watched it be
    // allowed. It matters beyond the test: a path can reach this from a
    // config file, a sync folder, or a mounted disk, and be parsed by a
    // platform that did not write it.
    path.to_string_lossy()
        .split(['/', '\\'])
        .any(|part| NEVER.iter().any(|n| part.eq_ignore_ascii_case(n)))
}

/// What kind of thing this folder is, if it is anything Atlas knows.
pub fn classify(name: &str) -> Option<Kind> {
    KNOWN.iter().find(|(n, _)| name.eq_ignore_ascii_case(n)).map(|(_, k)| *k)
}

/// Look for space, and report. **Never removes anything.**
///
/// `roots` are the places to look — passed in rather than discovered, so the
/// caller decides how far Atlas may look and the answer is testable.
pub fn survey(roots: &[PathBuf], now: u64) -> Vec<Candidate> {
    let mut found = Vec::new();
    for root in roots {
        walk(root, now, 0, &mut found);
    }
    // Biggest first: the decision is usually made on the first two lines.
    found.sort_by_key(|b| std::cmp::Reverse(b.size_mb));
    found
}

/// Depth is capped. An unbounded walk of a whole disk is slow enough to be
/// noticed, and being noticed is how a background tidy becomes a thing you
/// switch off.
const MAX_DEPTH: usize = 6;

fn walk(dir: &Path, now: u64, depth: usize, out: &mut Vec<Candidate>) {
    if depth > MAX_DEPTH || forbidden(dir) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let path = e.path();
        let Ok(meta) = e.metadata() else { continue };
        // Symlinks are not followed. A link into `Documents` is a way into
        // `Documents`, and the allowlist would never see it coming.
        if meta.file_type().is_symlink() || !meta.is_dir() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        if forbidden(&path) {
            continue;
        }
        match classify(&name) {
            Some(kind) => {
                let age = age_days(&meta, now);
                if age < kind.min_age_days() {
                    continue;
                }
                let mut bytes = 0u64;
                size_of(&path, &mut bytes);
                let mb = bytes / (1024 * 1024);
                // Below this the noise costs more than the space saves.
                if mb >= 50 {
                    out.push(Candidate { path, size_mb: mb, kind, age_days: age });
                }
            }
            // Not something Atlas knows: keep looking inside, but never offer
            // the folder itself.
            None => walk(&path, now, depth + 1, out),
        }
    }
}

fn age_days(meta: &std::fs::Metadata, now: u64) -> u64 {
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(now);
    now.saturating_sub(modified) / 86_400
}

fn size_of(dir: &Path, total: &mut u64) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let Ok(meta) = e.metadata() else { continue };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            size_of(&e.path(), total);
        } else {
            *total += meta.len();
        }
    }
}

/// Move chosen candidates to Atlas's trash, where they stay for 30 days.
///
/// Takes an explicit list. There is deliberately no path through this module
/// that decides for itself what to remove — the list has to come from a person
/// saying yes to specific lines.
///
/// Returns what was moved and what refused, because a partial reclaim reported
/// as a whole one is how you go looking for space that was never freed.
pub fn reclaim(
    chosen: &[Candidate],
    trash: &crate::safety::Trash,
) -> (Vec<PathBuf>, Vec<(PathBuf, String)>) {
    let mut moved = Vec::new();
    let mut refused = Vec::new();
    for c in chosen {
        // Checked again here rather than trusting the caller. The survey and
        // the removal are separated by a person reading a list, and this is
        // the last point at which a mistake is still cheap.
        if forbidden(&c.path) {
            refused.push((c.path.clone(), "that path is one Atlas never touches".into()));
            continue;
        }
        match trash.take(&c.path, &format!("reclaimed {:?}, {} MB", c.kind, c.size_mb)) {
            Ok(_) => moved.push(c.path.clone()),
            Err(e) => refused.push((c.path.clone(), e.to_string())),
        }
    }
    (moved, refused)
}

/// What Atlas says about a survey.
pub fn spoken(found: &[Candidate]) -> String {
    if found.is_empty() {
        return "Nothing worth clearing — I looked in the caches and build folders and there's \
                nothing old enough to be safe to move."
            .into();
    }
    let total: u64 = found.iter().map(|c| c.size_mb).sum();
    format!(
        "About {} GB across {} places. Everything I'd move goes to the trash for 30 days, so \
         it's all reversible. Say the word and I'll show you the list.",
        total / 1024,
        found.len()
    )
}

// ---------------------------------------------------------------------------
// Looking at the whole disk
//
// Separate from `survey` above, and the separation is the point. `survey` finds
// what Atlas may *move*; this finds where your space actually went. Looking is
// read-only and therefore safe anywhere; removing is not, and stays confined
// to things that rebuild themselves.
//
// Conflating the two produced a scanner that could only see caches — safe, and
// close to useless, because the space is rarely in the caches. It is in a
// folder of video from 2019, an application you stopped using, and four years
// of screenshots.
// ---------------------------------------------------------------------------

/// Anything at least this big is worth a line of your attention.
const BIG_FILE_MB: u64 = 200;

/// A folder worth naming, so you can see where the weight is.
const BIG_FOLDER_MB: u64 = 500;

/// Report on the whole disk: where the space went, what is old, what you have
/// stopped using. **Removes nothing, and cannot.**
///
/// Unlike `survey`, this is not limited to the allowlist — the allowlist
/// governs what Atlas may touch, not what it may look at. It still refuses to
/// descend into `NEVER` paths, because reading someone's `.ssh` folder is not
/// something Atlas should do even to count bytes.
pub fn whole_disk(roots: &[PathBuf], now: u64) -> Vec<Candidate> {
    let mut found = Vec::new();
    for root in roots {
        look(root, now, 0, &mut found);
    }
    found.sort_by_key(|b| std::cmp::Reverse(b.size_mb));
    found
}

/// Names that mean "these are screenshots".
const SHOT_FOLDERS: &[&str] = &["Screenshots", "Screen Shots", "Captures", "Screen Recordings"];

/// Where applications live, so they can be reported with their size.
const APP_FOLDERS: &[&str] = &["Program Files", "Program Files (x86)", "Applications"];

fn look(dir: &Path, now: u64, depth: usize, out: &mut Vec<Candidate>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let path = e.path();
        let Ok(meta) = e.metadata() else { continue };
        if meta.file_type().is_symlink() {
            // Not followed, or the same gigabyte gets counted five times and
            // the total becomes a number you cannot trust.
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();

        if meta.is_file() {
            let mb = meta.len() / (1024 * 1024);
            let age = age_days(&meta, now);
            if mb >= BIG_FILE_MB && age >= Kind::BigAndOld.min_age_days() {
                out.push(Candidate { path, size_mb: mb, kind: Kind::BigAndOld, age_days: age });
            }
            continue;
        }

        // A screenshots folder is worth naming as a whole rather than as four
        // thousand files.
        if SHOT_FOLDERS.iter().any(|s| name.eq_ignore_ascii_case(s)) {
            let mut bytes = 0;
            size_of(&path, &mut bytes);
            let mb = bytes / (1024 * 1024);
            if mb >= 100 {
                out.push(Candidate {
                    path,
                    size_mb: mb,
                    kind: Kind::Screenshots,
                    age_days: age_days(&meta, now),
                });
            }
            continue;
        }

        // Reading inside somebody's private folders to count bytes is still
        // reading inside them. Their size is reported; their contents are not
        // examined.
        if forbidden(&path) {
            let mut bytes = 0;
            size_of(&path, &mut bytes);
            let mb = bytes / (1024 * 1024);
            if mb >= BIG_FOLDER_MB {
                out.push(Candidate {
                    path,
                    size_mb: mb,
                    kind: Kind::BigFolder,
                    age_days: age_days(&meta, now),
                });
            }
            continue;
        }

        look(&path, now, depth + 1, out);
    }
}

/// Installed applications, largest first, with how long since anything in them
/// changed.
///
/// **Reported only.** Deleting an application folder leaves its registry
/// entries, its services and its scheduled tasks behind — a broken uninstall
/// that is harder to fix than the space was worth. Atlas names them and tells
/// you to uninstall properly.
///
/// Age here is the newest file in the folder, which is a proxy for "when did
/// this last get used or updated" and not a precise one. It is honest about
/// being a proxy in `line()`.
pub fn installed_apps(roots: &[PathBuf], now: u64) -> Vec<Candidate> {
    let mut out = Vec::new();
    for root in roots {
        for folder in APP_FOLDERS {
            let dir = root.join(folder);
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for e in entries.flatten() {
                let path = e.path();
                let Ok(meta) = e.metadata() else { continue };
                if !meta.is_dir() || meta.file_type().is_symlink() {
                    continue;
                }
                let mut bytes = 0;
                size_of(&path, &mut bytes);
                let mb = bytes / (1024 * 1024);
                if mb < 100 {
                    continue;
                }
                let mut newest = 0u64;
                newest_in(&path, &mut newest);
                let age = now.saturating_sub(newest) / 86_400;
                out.push(Candidate { path, size_mb: mb, kind: Kind::App, age_days: age });
            }
        }
    }
    out.sort_by_key(|b| std::cmp::Reverse(b.size_mb));
    out
}

fn newest_in(dir: &Path, newest: &mut u64) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let Ok(meta) = e.metadata() else { continue };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            newest_in(&e.path(), newest);
        } else if let Some(t) = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
        {
            if t > *newest {
                *newest = t;
            }
        }
    }
}

/// What a whole-disk report says, split by what Atlas can and cannot do.
pub fn report(found: &[Candidate]) -> String {
    let (mine, yours): (Vec<&Candidate>, Vec<&Candidate>) =
        found.iter().partition(|c| c.kind.atlas_may_move());
    let mine_mb: u64 = mine.iter().map(|c| c.size_mb).sum();
    let yours_mb: u64 = yours.iter().map(|c| c.size_mb).sum();
    format!(
        "I can clear {} MB myself — caches and build folders that rebuild themselves.\n\
         Another {} MB is yours to judge: big old files, screenshots, applications. I've \
         listed those but I won't touch them.",
        mine_mb, yours_mb
    )
}
