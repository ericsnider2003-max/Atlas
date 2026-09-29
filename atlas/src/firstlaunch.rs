//! Double-click and it's set up.
//!
//! Eric, 23 Sep 2026: *"I have to ensure Atlas is in a very specific folder …
//! I have to have a file directly next to another, hard to explain to people
//! who don't have Claude or myself present for set up."* Both were true:
//! `atlas.exe` refused to start without a `config/` folder beside it, and the
//! only way to fetch its voice pieces was a batch file in the same folder.
//!
//! This module removes both:
//!
//! - **The settings travel inside the program.** Every `config/*.yaml` is built
//!   into `atlas.exe`. A copy that finds no settings beside it writes the
//!   defaults out and carries on — it never overwrites a file that is there.
//! - **Atlas picks its own home.** Double-clicked from anywhere that isn't
//!   already an install (Downloads, the desktop, a USB stick), it moves itself
//!   to the one standard per-user place — `%LOCALAPPDATA%\Atlas` on Windows —
//!   puts itself in the Start menu and on the desktop, and opens its own
//!   setup window from there. No folder to choose, nothing to keep next to
//!   anything, no administrator rights.
//! - **An install that already works is left where it is.** A folder that
//!   already holds Atlas's settings (a developer's tree, an unzipped copy, an
//!   `ATLAS_HOME`) is used in place, exactly as before — `roots` decides that,
//!   and nothing here second-guesses it.

use std::path::{Path, PathBuf};

/// The shipped settings, built into the program. Written out only where a
/// file is missing.
pub const DEFAULT_CONFIG: &[(&str, &str)] = &[
    ("apps.yaml", include_str!("../config/apps.yaml")),
    ("commands.yaml", include_str!("../config/commands.yaml")),
    ("indexing.yaml", include_str!("../config/indexing.yaml")),
    ("layouts.yaml", include_str!("../config/layouts.yaml")),
    ("policy.yaml", include_str!("../config/policy.yaml")),
    ("tools.yaml", include_str!("../config/tools.yaml")),
    // Not the two label lists (`config/labels/*.txt`). They are part of the
    // vision models, compiled in by `vision.rs` and never read from disk, so a
    // copy written here was a file that looked editable and did nothing --
    // and an update could not have kept an edit to it either. Anything listed
    // here is a file Atlas reads *and* keeps your edits to (`yourchanges`).
];

/// Write whichever shipped settings files are missing. Returns how many were
/// written. Never touches a file that exists — your edits are yours.
pub fn write_default_config(config_dir: &Path) -> std::io::Result<usize> {
    let mut wrote = 0;
    for (name, text) in DEFAULT_CONFIG {
        let path = config_dir.join(name);
        if path.exists() {
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, text)?;
        wrote += 1;
    }
    Ok(wrote)
}

/// Are Atlas's settings missing from this folder? The three files
/// `Config::load` cannot start without.
pub fn settings_missing(config_dir: &Path) -> bool {
    ["apps.yaml", "layouts.yaml", "commands.yaml"].iter().any(|f| !config_dir.join(f).is_file())
}

/// The one standard place Atlas lives when you didn't choose one:
/// `%LOCALAPPDATA%\Atlas` on Windows, `~/.local/share/atlas` elsewhere.
pub fn standard_home() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Atlas"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")))
            .map(|d| d.join("atlas"))
    }
}

/// Where this copy of Atlas should run from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Where {
    /// Here is fine: it's already an install, it's already the standard home,
    /// or you named a home yourself.
    Here,
    /// Move into the standard home first.
    MoveTo(PathBuf),
}

/// Decide, without touching anything. `already_an_install` is `roots`'s own
/// judgement of the folder the program is in.
pub fn where_to_live(exe_dir: &Path, already_an_install: bool, told: bool, home: Option<&Path>) -> Where {
    if told || already_an_install {
        return Where::Here;
    }
    match home {
        Some(h) if !same_place(h, exe_dir) => Where::MoveTo(h.to_path_buf()),
        _ => Where::Here,
    }
}

fn same_place(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        std::fs::canonicalize(p)
            .unwrap_or_else(|_| p.to_path_buf())
            .to_string_lossy()
            .trim_end_matches(['/', '\\'])
            .to_lowercase()
    };
    norm(a) == norm(b)
}

/// What the program is called once it's installed.
pub const INSTALLED_NAME: &str = if cfg!(windows) { "atlas.exe" } else { "atlas" };

/// Move into `home`: the program, then its settings. Returns the program's new
/// path. Copied rather than moved, so the one you double-clicked is still
/// there if anything goes wrong; a newer copy replaces an older one, which is
/// how an update arrives.
///
/// The installed program is always `atlas.exe` (`atlas` elsewhere), whatever
/// the download was called: it's handed out as one file, "Atlas Setup.exe"
/// (27 Sep 2026), and the shortcuts, the start-with-Windows task and the
/// updates all expect the one name.
///
/// When an Atlas is already installed and running there, a plain copy fails:
/// Windows won't write over a running program. Until 28 Sep 2026 that failure
/// went to a console a double-clicked Atlas doesn't have, so running Atlas
/// Setup.exe again did nothing at all, silently. Now: the running Atlas is
/// asked to stop first (`ask_atlas_to_stop`, up to `stop_wait`), and if the
/// file is still held, it is renamed aside -- Windows allows renaming a
/// running program -- and the copy goes where it was. The same bytes already
/// there are left alone.
///
/// `stop_first`: ask an Atlas running from `home` to stop first. (Until 28
/// Sep 2026 this was `move_in`, which never asked; and later that day it
/// took the hub's port, which was the wrong question -- see
/// `atlas_running`.)
pub fn move_in_over(exe: &Path, home: &Path, stop_first: bool, stop_wait: std::time::Duration) -> Result<PathBuf, String> {
    std::fs::create_dir_all(home).map_err(|e| format!("I couldn't make {}: {e}", home.display()))?;
    let target = home.join(INSTALLED_NAME);
    tidy_set_aside(home);
    let same_bytes = target.is_file()
        && crate::upgrade::sha256_of(exe).is_some_and(|a| Some(a) == crate::upgrade::sha256_of(&target));
    if !same_place(exe, &target) && !same_bytes {
        if target.is_file() {
            if stop_first {
                // Best effort: it finishes what it's saving on the way out.
                let _ = ask_atlas_to_stop(home, stop_wait);
            }
        }
        copy_over(exe, &target).map_err(|e| format!("I couldn't copy myself into {}: {e}", home.display()))?;
    }
    // A copy keeps the "downloaded from the internet" mark, which would make
    // Windows ask again every time the shortcut is used. Asked once, at the
    // download, is enough.
    forget_download_mark(&target);
    write_default_config(&home.join("config")).map_err(|e| format!("I couldn't write my settings: {e}"))?;
    Ok(target)
}

/// What a program set aside by `copy_over` is called.
const SET_ASIDE: &str = ".set-aside-";

/// Copy `from` over `to`. If `to` can't be written (a running program), move
/// it aside first and copy into its place; put it back if the copy fails.
fn copy_over(from: &Path, to: &Path) -> std::io::Result<()> {
    match std::fs::copy(from, to) {
        Ok(_) => Ok(()),
        Err(first) if to.is_file() => {
            let name = to.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let aside = to.with_file_name(format!("{name}{SET_ASIDE}{}", std::process::id()));
            let _ = std::fs::remove_file(&aside);
            std::fs::rename(to, &aside).map_err(|e| std::io::Error::new(e.kind(), format!("{first}; and it couldn't be moved aside: {e}")))?;
            match std::fs::copy(from, to) {
                Ok(_) => Ok(()),
                Err(e) => {
                    let _ = std::fs::remove_file(to);
                    let _ = std::fs::rename(&aside, to);
                    Err(e)
                }
            }
        }
        Err(e) => Err(e),
    }
}

/// Remove programs set aside by an earlier `copy_over` that are no longer
/// running (a running one can't be removed, and is left for next time).
pub fn tidy_set_aside(home: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(home) else { return 0 };
    entries
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(SET_ASIDE))
        .filter(|e| std::fs::remove_file(e.path()).is_ok())
        .count()
}

/// What to do about the Atlas already installed where this copy would move in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Replacing {
    /// Nothing there yet, or an older (or unreadable) build: put this one in.
    Install,
    /// The very same build is there already: open it.
    Same,
    /// A newer version is there: ask before putting an older one over it.
    Older { installed: String, mine: String },
}

/// Decide, from the two builds' versions and fingerprints. An installed
/// build whose version can't be read is replaced (it's what a broken
/// install looks like), but a readable newer one is never silently
/// downgraded.
pub fn replacing(mine: (&str, &str), installed: Option<(Result<String, String>, String)>) -> Replacing {
    let Some((installed_version, installed_sha)) = installed else { return Replacing::Install };
    if !mine.1.is_empty() && mine.1 == installed_sha {
        return Replacing::Same;
    }
    match installed_version {
        Ok(v) if crate::upgrade::older_version(mine.0, &v) => Replacing::Older { installed: v, mine: mine.0.to_string() },
        _ => Replacing::Install,
    }
}

/// Look at what's installed in `home` and decide (`replacing`).
pub fn replacing_in(exe: &Path, home: &Path) -> Replacing {
    let target = home.join(INSTALLED_NAME);
    let installed = target
        .is_file()
        .then(|| (crate::upgrade::version_of(&target), crate::upgrade::sha256_of(&target).unwrap_or_default()));
    replacing((crate::upgrade::version(), &crate::upgrade::sha256_of(exe).unwrap_or_default()), installed)
}

/// Tell the person something went wrong: a message box when Atlas was
/// double-clicked (it has no console), the terminal otherwise. Before 28 Sep
/// 2026 these went to a console that wasn't there.
pub fn show_problem(text: &str) {
    #[cfg(windows)]
    {
        if started_without_a_terminal() {
            message_box(text, false);
            return;
        }
    }
    eprintln!("{text}");
}

/// Ask a yes-or-no question: a message box when double-clicked, the terminal
/// otherwise. No answer (no terminal to read) is a no.
pub fn ask_yes_no(text: &str) -> bool {
    #[cfg(windows)]
    {
        if started_without_a_terminal() {
            return message_box(text, true);
        }
    }
    print!("{text} Type yes to go ahead: ");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer).is_ok() && answer.trim().eq_ignore_ascii_case("yes")
}

#[cfg(windows)]
fn message_box(text: &str, question: bool) -> bool {
    use windows::core::HSTRING;
    use windows::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, IDYES, MB_ICONQUESTION, MB_ICONWARNING, MB_OK, MB_SETFOREGROUND, MB_YESNO,
    };
    let style = if question { MB_YESNO | MB_ICONQUESTION } else { MB_OK | MB_ICONWARNING };
    // SAFETY: both strings outlive the call; no owner window.
    let r = unsafe { MessageBoxW(None, &HSTRING::from(text), &HSTRING::from("Atlas"), style | MB_SETFOREGROUND) };
    r == IDYES
}

/// Where Windows keeps the "this came from the internet" mark on a file: a
/// second stream beside the file's contents, named `Zone.Identifier`. It's
/// what makes SmartScreen stop a program with "Windows protected your PC".
pub fn download_mark_of(file: &Path) -> PathBuf {
    let mut s = file.as_os_str().to_os_string();
    s.push(":Zone.Identifier");
    PathBuf::from(s)
}

/// Take the download mark off a file Atlas put in its own home, so Windows
/// asks about Atlas once — at the download — and not at every start. Returns
/// whether there was a mark to take off. No certificate involved: this is
/// the same thing as ticking "Unblock" in the file's Properties.
pub fn forget_download_mark(file: &Path) -> bool {
    if !cfg!(windows) {
        return false;
    }
    std::fs::remove_file(download_mark_of(file)).is_ok()
}

/// Windows 11's Smart App Control, as this machine has it set.
///
/// Smart App Control blocks any program that isn't signed with a paid
/// certificate and that Microsoft's cloud doesn't already know. There is no
/// per-program exception. Atlas and its voice tools are unsigned, so where it
/// is on it blocks them; where it's "evaluating" it may switch itself on
/// later and start blocking them then. Since the April 2026 Windows update
/// (KB5083769) it can be switched off and back on in Windows Security without
/// reinstalling Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppControl {
    Off,
    Evaluating,
    On,
    Unknown,
}

/// Read `reg query`'s answer for `VerifiedAndReputablePolicyState`: 0 off,
/// 1 on, 2 evaluating.
pub fn app_control_from(reg_output: &str) -> AppControl {
    let line = reg_output.lines().find(|l| l.contains("VerifiedAndReputablePolicyState"));
    let Some(value) = line.and_then(|l| l.split_whitespace().last()) else { return AppControl::Unknown };
    let n = value
        .strip_prefix("0x")
        .map(|h| u32::from_str_radix(h, 16).ok())
        .unwrap_or_else(|| value.parse().ok());
    match n {
        Some(0) => AppControl::Off,
        Some(1) => AppControl::On,
        Some(2) => AppControl::Evaluating,
        _ => AppControl::Unknown,
    }
}

/// This machine's Smart App Control setting. Unknown off Windows, or when
/// Windows doesn't say (older Windows has no Smart App Control at all).
pub fn app_control() -> AppControl {
    if !cfg!(windows) {
        return AppControl::Unknown;
    }
    let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let reg = root.join("System32").join("reg.exe");
    match run_quietly(
        &reg,
        &["query", r"HKLM\SYSTEM\CurrentControlSet\Control\CI\Policy", "/v", "VerifiedAndReputablePolicyState"],
    ) {
        Ok(out) => app_control_from(&String::from_utf8_lossy(&out.stdout)),
        Err(_) => AppControl::Unknown,
    }
}

/// What to tell you about Smart App Control, if anything.
pub fn app_control_words(state: AppControl) -> Option<String> {
    const WHERE: &str = "Windows Security → App & browser control → Smart App Control settings → Off";
    match state {
        AppControl::Off | AppControl::Unknown => None,
        AppControl::Evaluating => Some(format!(
            "Windows' Smart App Control is still deciding whether to switch itself on. If it does, it will \
             start blocking Atlas and its voice tools, because they aren't signed with a paid certificate, \
             and it has no way to make an exception for one program. To keep Atlas working, switch it off: \
             {WHERE}. You can switch it back on later if you stop using Atlas."
        )),
        AppControl::On => Some(format!(
            "Windows' Smart App Control is on. It blocks programs that aren't signed with a paid certificate, \
             so it may stop Atlas's voice tools from running, and it has no way to make an exception for one \
             program. To let them run, switch it off: {WHERE}. You can switch it back on later."
        )),
    }
}

/// Marked once the setup window has run to the end, so later opens go
/// straight to "Atlas is running" instead of walking the steps again.
pub fn is_set_up(root: &Path) -> bool {
    root.join("data").join("state").join("set_up").is_file()
}

pub fn mark_set_up(root: &Path) -> std::io::Result<()> {
    let dir = root.join("data").join("state");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("set_up"), crate::store::now().to_string())
}

/// What opening Atlas (a double-click, the Start menu, the tray's "Open
/// Atlas") does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opening {
    /// Start the background Atlas first, with no window of its own.
    pub start_background: bool,
    /// The page the window opens on.
    pub first: First,
}

/// Eric, 28 Sep 2026: "When on windows I don't even want to have the hub or
/// the application open for Atlas to run." Opening Atlas once it is set up
/// makes sure the background Atlas is running — so closing the window never
/// leaves you without it — and opens the hub, which is what "open Atlas"
/// means once there's nothing left to set up. Not set up yet: the setup
/// window, as before, which starts the background Atlas itself when it
/// finishes. A particular page asked for (`home settings`) is kept.
pub fn what_opening_does(set_up: bool, running: bool, first: First) -> Opening {
    if !set_up {
        return Opening { start_background: false, first };
    }
    let first = if first == First::Home { First::Hub("/hub".into()) } else { first };
    Opening { start_background: !running, first }
}

/// Is the background Atlas of the install at `root` running?
///
/// ## Why not "does something answer on the hub's port"
///
/// That was the whole test until 28 Sep 2026, and it was wrong both ways:
///
/// - **Another program on the port read as Atlas.** "Open Atlas" then
///   didn't start the background Atlas at all, and nothing was running.
/// - **A hub switched off (or not open yet) read as Atlas stopped.** Setup
///   said Atlas wasn't running, offered to start a second one, and its
///   Restart "stopped" an Atlas that was never asked.
///
/// ## What it is now
///
/// The instance lock (`onlyone`), which every running Atlas holds and beats
/// whether or not its hub is on; or, failing that, the hub answering
/// `/hub/ping` with the id this install's Atlas recorded when its hub opened
/// (`server::atlas_hub_port`) -- which covers a lock that reads abandoned
/// for the few seconds after the laptop wakes (`onlyone::WOKE_GRACE_SECS`),
/// while the hub is already answering.
pub fn atlas_running(root: &Path) -> bool {
    let lock = crate::onlyone::OnlyOne::at(&root.join("data"));
    matches!(lock.look(crate::store::now()), crate::onlyone::Found::Running { .. })
        || crate::server::atlas_hub_port(&root.join("data").join("state")).is_some()
}

/// The port the hub of the Atlas at `root` really answers on, when it
/// answers as that Atlas; else `configured` (what a bookmark and a fresh
/// start use).
pub fn hub_port_at(root: &Path, configured: u16) -> u16 {
    crate::server::hub_port(&root.join("data").join("state"), configured)
}

/// Ask the background Atlas to stop, and wait (up to `wait`) until it has.
/// Returns whether it stopped. It finishes what it's saving on the way out,
/// and lets go of its lock last -- which is what's waited for, so a restart
/// never starts the new one while the old one is still saving.
pub fn ask_atlas_to_stop(root: &Path, wait: std::time::Duration) -> bool {
    if !atlas_running(root) {
        return true;
    }
    let state = root.join("data").join("state");
    if std::fs::create_dir_all(&state).is_err()
        || std::fs::write(crate::goodbye::stop_file(&state), b"").is_err()
    {
        return false;
    }
    let until = std::time::Instant::now() + wait;
    while std::time::Instant::now() < until {
        if !atlas_running(root) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    // Not taken up: don't leave it for the next start to trip over.
    let _ = std::fs::remove_file(crate::goodbye::stop_file(&state));
    false
}

/// Open Atlas's own window on its Settings page — what "show me settings"
/// does. Only from the Atlas program itself: anything else running this code
/// (a test harness) would be starting a copy of itself.
pub fn open_atlas_window(first: &First) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let is_atlas = exe.file_stem().map(|s| s.to_string_lossy().eq_ignore_ascii_case("atlas")).unwrap_or(false);
    if !is_atlas {
        return Err("I'm not running as the Atlas program, so there's no window of mine to open".into());
    }
    let words = first.words();
    let args: Vec<&str> = words.iter().map(|w| w.as_str()).collect();
    let child = spawn_quietly(&exe, &args).map_err(|e| e.to_string())?;
    crate::unwaited::dont_wait(child);
    Ok(())
}

/// Start a copy of Atlas with no window of its own — the background Atlas.
/// Its output goes nowhere: it runs for days, and a pipe nobody reads would
/// fill and stall it.
pub fn spawn_quietly(exe: &Path, args: &[&str]) -> std::io::Result<std::process::Child> {
    hidden_command(exe, args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
}

/// Run a step like `adapt` with no window of its own, and keep what it said
/// so a problem can be shown in words.
pub fn run_quietly(exe: &Path, args: &[&str]) -> std::io::Result<std::process::Output> {
    hidden_command(exe, args).stdin(std::process::Stdio::null()).output()
}

fn hidden_command(exe: &Path, args: &[&str]) -> std::process::Command {
    let mut cmd = crate::tools::command(exe);
    cmd.args(args);
    if let Some(dir) = exe.parent() {
        cmd.current_dir(dir);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Was this started by a double-click (or by Windows at logon) rather than
/// typed in a terminal? A double-clicked console program is the only process
/// on its console; one typed in a terminal shares it with the shell.
pub fn started_without_a_terminal() -> bool {
    #[cfg(windows)]
    {
        // Atlas is a windowed program (`windows_subsystem`), so it has no
        // console of its own. Typed in a terminal, it joins that terminal's
        // console so what it prints shows there; opened any other way there
        // is no terminal to join. Decided once, on the first call, which
        // `main` makes before printing anything.
        static STARTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *STARTED.get_or_init(|| {
            use windows::Win32::System::Console::{AttachConsole, GetConsoleProcessList, ATTACH_PARENT_PROCESS};
            let mut ids = [0u32; 4];
            // SAFETY: the buffer outlives the call and its length is passed.
            let n = unsafe { GetConsoleProcessList(&mut ids) };
            if n > 0 {
                // Built as a console program (no desktop UI): the old rule.
                return n == 1;
            }
            // SAFETY: no pointers; failing just means there's no terminal.
            unsafe { AttachConsole(ATTACH_PARENT_PROCESS) }.is_err()
        })
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Close the console window a double-click opened, so what's left is Atlas's
/// own window (or nothing, for the background Atlas).
pub fn let_go_of_the_console() {
    #[cfg(windows)]
    {
        // SAFETY: no arguments; failing just leaves the console up.
        let _ = unsafe { windows::Win32::System::Console::FreeConsole() };
    }
}

/// Put Atlas in the Start menu and on the desktop. Returns what was made, in
/// words; a shortcut that couldn't be made is said, not hidden.
pub fn make_shortcuts(exe: &Path) -> Vec<String> {
    let mut said = Vec::new();
    for (place, path) in shortcut_places() {
        match make_shortcut(exe, &path) {
            Ok(()) => said.push(format!("Atlas is in your {place}.")),
            Err(e) => said.push(format!("I couldn't put Atlas in your {place}: {e}")),
        }
    }
    said
}

#[cfg(windows)]
fn shortcut_places() -> Vec<(&'static str, PathBuf)> {
    use windows::Win32::UI::Shell::{FOLDERID_Desktop, FOLDERID_Programs};
    let mut v = Vec::new();
    for (place, id) in [("Start menu", &FOLDERID_Programs), ("desktop", &FOLDERID_Desktop)] {
        if let Some(dir) = known_folder(id) {
            v.push((place, dir.join("Atlas.lnk")));
        }
    }
    v
}

/// Where downloads land and the desktop, as this person has them.
///
/// On Windows, asked of Windows itself (`SHGetKnownFolderPath`): Downloads
/// can be moved to another drive, and OneDrive moves the Desktop into
/// `OneDrive\Desktop`. Until 28 Sep 2026 these were `%USERPROFILE%\Downloads`
/// and `\Desktop` by assumption, so on a laptop with OneDrive's backup on,
/// "Send an update" never saw a build saved to the desktop. Elsewhere, and
/// when Windows doesn't say, the folders under home.
pub fn downloads_and_desktop() -> Vec<PathBuf> {
    let home = crate::doctor::lookup_env("USERPROFILE").or_else(|| crate::doctor::lookup_env("HOME")).map(PathBuf::from);
    let fallback = |name: &str| home.as_ref().map(|h| h.join(name));
    #[cfg(windows)]
    let (downloads, desktop) = {
        use windows::Win32::UI::Shell::{FOLDERID_Desktop, FOLDERID_Downloads};
        (known_folder(&FOLDERID_Downloads), known_folder(&FOLDERID_Desktop))
    };
    #[cfg(not(windows))]
    let (downloads, desktop): (Option<PathBuf>, Option<PathBuf>) = (None, None);
    let mut out: Vec<PathBuf> = Vec::new();
    for p in [downloads.or_else(|| fallback("Downloads")), desktop.or_else(|| fallback("Desktop"))].into_iter().flatten() {
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

#[cfg(windows)]
fn known_folder(id: &windows::core::GUID) -> Option<PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{SHGetKnownFolderPath, KF_FLAG_DEFAULT};
    // SAFETY: the returned buffer is read once and freed with CoTaskMemFree.
    unsafe {
        let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s.map(PathBuf::from)
    }
}

#[cfg(windows)]
fn make_shortcut(exe: &Path, lnk: &Path) -> Result<(), String> {
    use windows::core::{Interface, HSTRING};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, IPersistFile, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    // SAFETY: plain COM calls on this thread; every interface is dropped
    // (released) before return.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let link: IShellLinkW =
            CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(|e| e.to_string())?;
        link.SetPath(&HSTRING::from(exe.as_os_str())).map_err(|e| e.to_string())?;
        if let Some(dir) = exe.parent() {
            link.SetWorkingDirectory(&HSTRING::from(dir.as_os_str())).map_err(|e| e.to_string())?;
        }
        link.SetDescription(&HSTRING::from("Atlas")).map_err(|e| e.to_string())?;
        let file: IPersistFile = link.cast().map_err(|e| e.to_string())?;
        file.Save(&HSTRING::from(lnk.as_os_str()), true).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Elsewhere, a desktop entry in the applications menu — the same promise,
/// the way Linux keeps it.
#[cfg(not(windows))]
fn shortcut_places() -> Vec<(&'static str, PathBuf)> {
    std::env::var_os("HOME")
        .map(|h| vec![("applications menu", PathBuf::from(h).join(".local/share/applications/atlas.desktop"))])
        .unwrap_or_default()
}

#[cfg(not(windows))]
fn make_shortcut(exe: &Path, entry: &Path) -> Result<(), String> {
    if let Some(dir) = entry.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(entry, desktop_entry(exe)).map_err(|e| e.to_string())
}

/// The Linux applications-menu entry for `exe`.
pub fn desktop_entry(exe: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Atlas\nComment=Your workspace assistant\nExec=\"{}\" home\nTerminal=false\n",
        exe.display()
    )
}

/// Which page the window opens on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum First {
    Home,
    Settings,
    /// The hub, at this page (`/hub`, `/hub/outstanding`, …).
    Hub(String),
}

impl First {
    /// Read from the words after `atlas home`: nothing, `settings`, or `hub`
    /// with an optional page name (`hub outstanding` → `/hub/outstanding`).
    pub fn from_words(words: &[String]) -> First {
        match words.first().map(|w| w.to_lowercase()) {
            Some(w) if w == "settings" => First::Settings,
            Some(w) if w == "hub" => {
                let page = words.get(1).map(|p| p.trim_matches('/').to_lowercase()).unwrap_or_default();
                let page = page.trim_start_matches("hub/").to_string();
                let ok = !page.is_empty() && page.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
                First::Hub(if ok { format!("/hub/{page}") } else { "/hub".into() })
            }
            _ => First::Home,
        }
    }

    /// The words that reopen the window on this page — what `from_words`
    /// reads back.
    pub fn words(&self) -> Vec<String> {
        match self {
            First::Home => vec!["home".into()],
            First::Settings => vec!["home".into(), "settings".into()],
            First::Hub(page) => {
                let mut w = vec!["home".to_string(), "hub".to_string()];
                let rest = page.trim_start_matches("/hub").trim_matches('/');
                if !rest.is_empty() {
                    w.push(rest.to_string());
                }
                w
            }
        }
    }
}
