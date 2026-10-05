//! Where this install lives.
//!
//! Everything Atlas remembers — `data/state`, `data/backups`, `data/logs`,
//! `config/` — used to hang off the *current working directory*, because the
//! call sites said `Store::new("data/state")` and `Config::load("config")`.
//! `ATLAS.bat` hid that, because its first real line is `cd /d "%~dp0"`. The
//! day the `.bat` is not the launcher — a Start Menu shortcut with the wrong
//! "Start in", Task Scheduler (whose working directory is `system32`), a
//! terminal that happens to be somewhere else, `atlas update` run from
//! anywhere — Atlas came up with an *empty* `data/state`, silently: no
//! memory, no pairings, no backups, and a second `data/` tree written
//! wherever it was launched from. Nothing errored.
//!
//! `Store::install_root()` did not save us either. It climbs two levels when
//! the root's last two components are literally `data`/`state`, so for the
//! relative `"data/state"` the two parents are `"data"` and `""` — it
//! returned the **empty path**, and every `install_root().join("data/logs")`
//! downstream was cwd-relative again. The rule was stated at the leaf and
//! broken at the trunk.
//!
//! So: one place decides, once, and everything derives from it. The rule is
//! *the install is where the executable is*, not where you happened to be
//! standing. `ATLAS_HOME` overrides it for anyone who wants the program and
//! its data apart.
//!
//! `tests/one_install_root.rs` asserts that no literal `"data/…"` or
//! `"config/…"` path survives anywhere else in `src`, so a future call site
//! cannot reintroduce this one file at a time — which is how it happened
//! four times already (`BackupConfig.dir`, `data/index.md`, the trace log,
//! and then the trunk itself).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Why a particular folder was chosen as the install root.
///
/// Kept because "Atlas started with no memory" and "Atlas started with your
/// memory" look identical from the outside, and the whole failure this
/// module exists to kill was a silent one. `atlas doctor` prints this, and
/// `first_run_here()` lets the daemon say it out loud the first time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chosen {
    /// `ATLAS_HOME` was set. You said where; we did not guess.
    Told,
    /// The folder holding `atlas.exe` already looks like an install.
    BesideTheProgram,
    /// An ancestor of the program's folder looks like an install. This is
    /// the `cargo run` case — the binary is in `target/debug`, the install
    /// is the crate root four levels up.
    AboveTheProgram,
    /// The working directory looks like an install and the program's own
    /// folder does not. Someone is running a copied-out binary against a
    /// real install; trust the install.
    WhereYouAreStanding,
    /// Nothing looks like an install yet. A fresh unpack: the files get
    /// created beside the program, which is the only answer that puts them
    /// somewhere you will find them again.
    FreshBesideTheProgram,
}

impl Chosen {
    pub fn plain(&self) -> &'static str {
        match self {
            Chosen::Told => "ATLAS_HOME told me where to look",
            Chosen::BesideTheProgram => "the folder the program is in",
            Chosen::AboveTheProgram => "a folder above the program — a build tree, not an install",
            Chosen::WhereYouAreStanding => {
                "the folder you ran me from, because the program's own folder holds no install"
            }
            Chosen::FreshBesideTheProgram => {
                "nothing here looks like an install yet, so I am starting one beside the program"
            }
        }
    }
}

static ROOT: OnceLock<(PathBuf, Chosen)> = OnceLock::new();

/// Does this folder already hold an Atlas install?
///
/// Deliberately generous: either half of the pair counts. A user who has
/// unpacked the zip but never started Atlas has `config/` and no `data/`; a
/// user who deleted `config/` to start over has `data/` and no `config/`.
/// Requiring both would send the second one somewhere new and lose
/// everything, which is the failure this module is here to prevent.
pub fn looks_like_an_install(dir: &Path) -> bool {
    dir.join("config").join("tools.yaml").is_file()
        || dir.join("data").join("state").is_dir()
        || (dir.join("config").is_dir() && dir.join("data").is_dir())
}

fn decide() -> (PathBuf, Chosen) {
    // 1. You said so.
    if let Some(home) = std::env::var_os("ATLAS_HOME") {
        let p = PathBuf::from(home);
        if !p.as_os_str().is_empty() {
            return (p, Chosen::Told);
        }
    }

    let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf()));

    // 2. Beside the program.
    if let Some(dir) = exe_dir.as_ref() {
        if looks_like_an_install(dir) {
            return (dir.clone(), Chosen::BesideTheProgram);
        }
    }

    // 3. Above the program. `cargo run` puts the binary in
    //    `<crate>/target/debug/atlas`, and the integration-test harness puts
    //    it deeper still, in `target/debug/deps/`. Four levels reaches the
    //    crate root from either. Bounded rather than unbounded: walking to
    //    `/` would let a stray `config/` folder in a home directory capture
    //    every Atlas on the machine.
    if let Some(dir) = exe_dir.as_ref() {
        let mut up = dir.as_path();
        for _ in 0..4 {
            let Some(parent) = up.parent() else { break };
            up = parent;
            if looks_like_an_install(up) {
                return (up.to_path_buf(), Chosen::AboveTheProgram);
            }
        }
    }

    // 4. Where you are standing — but only if there is really an install
    //    there. This is the *only* remaining cwd-sensitive path, and it can
    //    no longer produce the silent-empty-start failure, because an empty
    //    folder does not pass `looks_like_an_install`.
    if let Ok(cwd) = std::env::current_dir() {
        if looks_like_an_install(&cwd) {
            return (cwd, Chosen::WhereYouAreStanding);
        }
    }

    // 5. Nothing exists yet. Create it beside the program, never in
    //    `system32` and never in whatever folder a shortcut happened to name.
    match exe_dir {
        Some(dir) => (dir, Chosen::FreshBesideTheProgram),
        // `current_exe` failing is close to impossible on Windows and Linux;
        // if it does, the working directory is the only thing left to say.
        None => (
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            Chosen::FreshBesideTheProgram,
        ),
    }
}

fn resolved() -> &'static (PathBuf, Chosen) {
    ROOT.get_or_init(decide)
}

/// The install's own folder. Everything below is derived from this.
pub fn install_root() -> PathBuf {
    resolved().0.clone()
}

/// How the root was chosen — for `atlas doctor` and the first-run line.
pub fn how() -> Chosen {
    resolved().1
}

/// One sentence naming the folder and the reason, for a human.
pub fn where_and_why() -> String {
    let (p, c) = resolved();
    format!("{} — {}", p.display(), c.plain())
}

/// True when Atlas is about to start an install rather than open one. The
/// caller says so out loud; a fresh start that looks like a lost memory is
/// the exact failure this module exists to make impossible to miss.
pub fn first_run_here() -> bool {
    matches!(how(), Chosen::FreshBesideTheProgram)
}

/// `data/` — everything Atlas generates.
pub fn data_dir() -> PathBuf {
    data_home().join("data")
}

/// Where `data/` lives: the install root, except under the test harness.
///
/// The checkout is the install root for its own tests (config is read from
/// it), but its data folder is not theirs to write: the vault, logs, builds
/// and handover state landed in `atlas/data/` on every run, hidden by
/// .gitignore, and the next run read them back (5 Oct 2026 audit, Q18). One
/// folder per test process instead, shaped like an install (`<it>/data`), so
/// a `Store`'s own idea of its install root still agrees with this one.
pub fn data_home() -> PathBuf {
    if under_the_test_harness() {
        static SWEPT: std::sync::Once = std::sync::Once::new();
        SWEPT.call_once(|| {
            sweep_old_test_scratch(&std::env::temp_dir(), std::time::SystemTime::now());
        });
        return std::env::temp_dir().join(format!("atlas-test-{}", std::process::id()));
    }
    install_root()
}

/// How old a test run's scratch has to be before the next run clears it: no
/// run of the suite takes this long, so nothing still in use is touched.
pub const TEST_SCRATCH_KEPT_FOR: std::time::Duration = std::time::Duration::from_secs(2 * 3600);

/// Clear what earlier test runs left in `dir` (5 Oct 2026, ledger Q21).
///
/// Hundreds of tests make a folder under the temp dir and few remove it; 18
/// runs had left 11,452 folders, 12 GB. `.cargo/config.toml` points the
/// tests' temp dir at `target/tmp`, and the first test in each run to reach
/// `data_home` clears entries there older than `TEST_SCRATCH_KEPT_FOR`.
///
/// Only ever inside a folder named `tmp` under one named `target` -- a
/// build's own scratch. Called anywhere else (the system temp dir, when the
/// tests were started without cargo's config) it does nothing, so it can
/// never reach a file of yours or a running Atlas's. Answers how many went.
pub fn sweep_old_test_scratch(dir: &Path, now: std::time::SystemTime) -> usize {
    let is_build_scratch = dir.file_name().is_some_and(|n| n == "tmp")
        && dir.parent().and_then(|p| p.file_name()).is_some_and(|n| n == "target");
    if !is_build_scratch {
        return 0;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    let mut gone = 0;
    for e in rd.flatten() {
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|m| now.duration_since(m).ok())
            .is_some_and(|age| age > TEST_SCRATCH_KEPT_FOR);
        if !old {
            continue;
        }
        let p = e.path();
        let removed = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
        if removed.is_ok() {
            gone += 1;
        }
    }
    gone
}

/// A cargo test binary (`target/<profile>/deps/<name>-<hash>`) that found its
/// install root by climbing out of `target/`. Never true for an installed
/// Atlas or for `ATLAS_HOME`.
fn under_the_test_harness() -> bool {
    how() == Chosen::AboveTheProgram
        && std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().and_then(|d| d.file_name()).map(|n| n == "deps"))
            .unwrap_or(false)
}

/// `data/state` — the store root: notes, the tray, the vault, peer tokens.
pub fn state_dir() -> PathBuf {
    data_dir().join("state")
}

/// The install's own state — shared by everyone who uses this copy.
///
/// The profile registry, the pairing tokens and the single-instance lock live
/// here. Anything about a *person* does not: that is `store()`.
pub fn install_state() -> crate::store::Store {
    crate::store::Store::new(state_dir())
}

/// The store every command should open — the active person's.
///
/// Resolved once, at first use, and cached for the life of the process.
///
/// **Why once, rather than switchable.** `profiles.rs` promised "a separate
/// state directory, enforced by construction" and `Profiles::state_dir` was
/// never used to open a `Store` — every profile read and wrote the same
/// `data/state`. The reason it was never wired is visible in `Daemon::new`:
/// the store is cloned into thirteen subsystems at construction, so a live
/// switch would leave twelve of them pointing at the previous person. That is
/// exactly what `Switch::must_forget()` was trying to describe, and what
/// nothing did.
///
/// Resolving at startup makes the isolation real without the half-swap that
/// would be worse than none. Switching says plainly that it takes effect next
/// start, which is honest and is also the safer failure: a switch that did not
/// take is visible, a switch that half-took is not.
pub fn store() -> crate::store::Store {
    static ACTIVE: OnceLock<PathBuf> = OnceLock::new();
    let root = ACTIVE.get_or_init(|| {
        let install = state_dir();
        match crate::profiles::active_dir(&install) {
            Some(d) => d,
            None => install,
        }
    });
    crate::store::Store::new(root.clone())
}

/// `config/`, unless `ATLAS_CONFIG` says otherwise.
///
/// `ATLAS_CONFIG` predates this module and was the only override in the
/// tree. It is kept, and now it is honoured by *every* config read rather
/// than only the one in `main`'s first few lines — four other call sites
/// hardcoded `"config"` and silently ignored it.
pub fn config_dir() -> PathBuf {
    match std::env::var_os("ATLAS_CONFIG") {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => install_root().join("config"),
    }
}

/// A named file under `config/`.
pub fn config_file(name: &str) -> PathBuf {
    config_dir().join(name)
}

/// A named folder under `data/`.
pub fn data_sub(name: &str) -> PathBuf {
    data_dir().join(name)
}

pub fn logs_dir() -> PathBuf {
    data_sub("logs")
}

pub fn notes_dir() -> PathBuf {
    data_sub("notes")
}

pub fn backups_dir() -> PathBuf {
    data_sub("backups")
}

pub fn trash_dir() -> PathBuf {
    data_sub("trash")
}

pub fn tmp_dir() -> PathBuf {
    data_sub("tmp")
}

/// `models/` — what the installer downloads model files into.
///
/// There is no `tools_dir()` beside it, deliberately. One was written and
/// nothing called it — `tests/dead_capabilities.rs` said so within the hour —
/// because every tool path in the tree arrives as an install-relative string
/// from the config (`tools/whisper/whisper-cli.exe`) and goes through
/// `under_install`. A second way to say the same thing is how two answers to
/// one question get into a codebase.
pub fn models_dir() -> PathBuf {
    install_root().join("models")
}

/// Resolve an install-relative path that may already be absolute.
///
/// Config files carry paths like `models/ggml-base.en.bin`, and a user may
/// reasonably write an absolute one instead. Joining an absolute path onto
/// the root is a no-op in Rust's `Path::join`, but stating it here means the
/// call sites do not each have to remember that.
pub fn under_install(p: impl AsRef<Path>) -> PathBuf {
    let p = p.as_ref();
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        install_root().join(p)
    }
}

/// A folder in the system's temp area for one run of a command, removed
/// when it goes out of scope (28 Sep 2026: `atlas-notes-<pid>` was never
/// removed, and a recording's segments piled up in the temp folder).
pub struct RunScratch {
    path: PathBuf,
}

impl RunScratch {
    /// `<temp>/<prefix>-<pid>`, made now. Folders of the same prefix left by
    /// runs that ended without cleaning up (killed, crashed) are cleared
    /// first (`sweep_run_scratch`).
    pub fn new(prefix: &str) -> RunScratch {
        sweep_run_scratch(&std::env::temp_dir(), prefix, std::process::id(), RUN_SCRATCH_STALE_SECS);
        let path = std::env::temp_dir().join(format!("{prefix}-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&path);
        RunScratch { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for RunScratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// How old another run's folder must be to be cleared.
pub const RUN_SCRATCH_STALE_SECS: u64 = 3600;

/// Remove `<prefix>-<pid>` folders under `temp` that aren't this process's
/// (`mine`) and weren't touched for `stale_secs`. Returns how many went.
pub fn sweep_run_scratch(temp: &Path, prefix: &str, mine: u32, stale_secs: u64) -> usize {
    let Ok(entries) = std::fs::read_dir(temp) else { return 0 };
    let lead = format!("{prefix}-");
    let mut gone = 0;
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let Some(pid) = name.strip_prefix(&lead) else { continue };
        if pid.parse::<u32>().ok().is_none_or(|p| p == mine) {
            continue;
        }
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|m| m.elapsed().ok())
            .is_some_and(|age| age.as_secs() >= stale_secs);
        if old && e.path().is_dir() && std::fs::remove_dir_all(e.path()).is_ok() {
            gone += 1;
        }
    }
    gone
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_folder_is_not_an_install() {
        // The whole failure in one assertion: a folder with nothing in it
        // must never be mistaken for somewhere Atlas already lives, or
        // Atlas starts clean and says nothing.
        let d = std::env::temp_dir().join(format!("atlas-roots-empty-{}", crate::store::now()));
        std::fs::create_dir_all(&d).unwrap();
        assert!(!looks_like_an_install(&d));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_config_with_tools_yaml_is_an_install() {
        let d = std::env::temp_dir().join(format!("atlas-roots-cfg-{}", crate::store::now()));
        std::fs::create_dir_all(d.join("config")).unwrap();
        std::fs::write(d.join("config").join("tools.yaml"), "enabled: true\n").unwrap();
        assert!(looks_like_an_install(&d));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_data_state_folder_alone_is_an_install() {
        // Someone who deleted `config/` to start it over still has months of
        // state. Sending them somewhere new would lose it.
        let d = std::env::temp_dir().join(format!("atlas-roots-data-{}", crate::store::now()));
        std::fs::create_dir_all(d.join("data").join("state")).unwrap();
        assert!(looks_like_an_install(&d));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn everything_hangs_off_the_one_root() {
        let r = install_root();
        let d = data_home();
        assert_eq!(data_dir(), d.join("data"));
        assert_eq!(state_dir(), d.join("data").join("state"));
        assert_eq!(logs_dir(), d.join("data").join("logs"));
        assert_eq!(backups_dir(), d.join("data").join("backups"));
        assert_eq!(models_dir(), r.join("models"));
        assert_eq!(notes_dir(), d.join("data").join("notes"));
        assert_eq!(trash_dir(), d.join("data").join("trash"));
        assert_eq!(tmp_dir(), d.join("data").join("tmp"));
        assert_eq!(data_sub("finance"), d.join("data").join("finance"));
    }

    #[test]
    fn the_root_is_absolute_so_nothing_downstream_depends_on_where_you_stood() {
        // The property that actually matters. A relative root is what made
        // `install_root()` return "" and put `data/` wherever the shortcut
        // pointed.
        assert!(
            install_root().is_absolute(),
            "install root resolved to {:?}, which is relative — the cwd bug is back",
            install_root()
        );
    }

    #[test]
    fn the_store_root_is_data_state_under_the_install_and_climbs_back_to_it() {
        // The round trip: `roots::store()` must produce a `Store` whose own
        // `install_root()` agrees with this module. That agreement is what
        // `upgrade::YOURS` depends on when `atlas update` goes looking for
        // `data/backups`.
        let s = store();
        assert_eq!(s.root(), state_dir().as_path());
        assert_eq!(s.install_root(), data_home());
        // Outside the test harness these are the same folder.
        assert!(data_home() == install_root() || under_the_test_harness());
    }

    #[test]
    fn the_config_folder_and_the_files_in_it_agree() {
        assert_eq!(config_file("tools.yaml"), config_dir().join("tools.yaml"));
    }

    #[test]
    fn how_and_why_agree_with_each_other() {
        // `where_and_why` is what `atlas doctor` prints and what the startup
        // line is built from; if it ever stops naming the folder, the silent
        // failure this module exists to kill is silent again.
        let said = where_and_why();
        assert!(said.contains(&install_root().display().to_string()));
        assert!(said.contains(how().plain()));
        assert_eq!(first_run_here(), matches!(how(), Chosen::FreshBesideTheProgram));
    }

    #[test]
    fn every_reason_has_something_to_say() {
        for c in [
            Chosen::Told,
            Chosen::BesideTheProgram,
            Chosen::AboveTheProgram,
            Chosen::WhereYouAreStanding,
            Chosen::FreshBesideTheProgram,
        ] {
            assert!(c.plain().len() > 10, "{c:?} has no explanation");
        }
    }

    #[test]
    fn an_absolute_path_is_left_alone() {
        let abs = if cfg!(windows) { "C:\\models\\x.gguf" } else { "/models/x.gguf" };
        assert_eq!(under_install(abs), PathBuf::from(abs));
    }

    #[test]
    fn a_relative_path_is_anchored_to_the_install() {
        assert_eq!(under_install("models/x.gguf"), install_root().join("models/x.gguf"));
    }
}
