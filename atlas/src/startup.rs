//! Atlas starting itself when you log in.
//!
//! **Eric's ruling, 17 Sep 2026: yes, in the background.** So the answer to
//! "how do I start it" should be that you do not — it is already running, and
//! the launcher is for setup and repair rather than for starting things.
//!
//! ## Why this did not exist, and why that showed
//!
//! Nothing in this tree created a startup entry, a shortcut, or a scheduled
//! task. Two modules nevertheless *assumed* one: `onlyone.rs` opens with
//! "Start it from the shortcut, forget, start it again from the batch file,
//! and there are two", and `doctor.rs` explains that the install root "used to
//! hang off the current working directory, so a shortcut with the wrong
//! `Start in` broke it". Both were written about a shortcut nobody could have
//! had, because nothing made one.
//!
//! That is the quiet kind of gap: the code reads as though the feature exists,
//! nothing fails, and the person just keeps opening the folder by hand.
//!
//! ## The one that would have bitten
//!
//! **A scheduled task does not start in your install folder.** Windows starts
//! it in `system32`, and a logon task has no inherited working directory worth
//! anything. Before 17 Sep that would have been fatal in a way nobody would
//! have diagnosed: `Store::new("data/state")` resolved relative to the
//! process's working directory, so Atlas would have come up with an empty
//! memory, written a `data/` tree into `system32`, and reported nothing wrong.
//!
//! `roots::decide()` fixes it by resolving from `current_exe()` rather than
//! from where the process was started, and `tests/one_install_root.rs` keeps
//! it that way. This module is only safe *because* that landed first, which is
//! worth writing down: the ordering was luck, not planning.
//!
//! ## What it does NOT do
//!
//! It does not run Atlas elevated. `LeastPrivilege` in the task's XML (it was
//! `/RL LIMITED` before 28 Sep 2026) is deliberate — an assistant
//! that holds your microphone and reads your screen has no business running as
//! administrator, and a task created with `/RL HIGHEST` would also need an
//! elevated shell to create, which would make "set this up for me" a UAC
//! prompt. Nothing here needs admin.
//!
//! It does not install a service. A service runs without a desktop session,
//! and Atlas needs one: the panels, the microphone and the screen reading are
//! all session-bound. A logon task is the honest shape.

use std::path::Path;

/// Which door Atlas comes up through at logon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// `--daemon`: wake word, scheduled work, proactive offers. No window.
    Background,
    /// `--voice`: the wake word only.
    Listening,
}

impl Mode {
    pub fn flag(self) -> &'static str {
        match self {
            Mode::Background => "--daemon",
            // The wake word's own loop. `--voice` is the press-Enter-to-talk
            // loop, which at sign-in has no keyboard to wait on (29 Sep 2026).
            Mode::Listening => "--wake",
        }
    }

    /// What a person should be told this will do.
    pub fn plainly(self) -> &'static str {
        match self {
            Mode::Background => {
                "listen for its name, run anything you've scheduled, and offer things it notices"
            }
            Mode::Listening => "listen for its name, and nothing else on its own",
        }
    }

    pub fn parse(said: &str) -> Option<Mode> {
        let s = said.trim().to_lowercase();
        if s.contains("background") || s.contains("daemon") {
            Some(Mode::Background)
        } else if s.contains("listen") || s.contains("voice") || s.contains("wake") {
            Some(Mode::Listening)
        } else {
            None
        }
    }
}

/// The task's name, everywhere. One constant so removing finds what adding made.
pub const TASK_NAME: &str = "Atlas";

/// A command to run, and what it is for.
///
/// Separated from running it so the decision is testable on a machine that is
/// not the target. The Windows path below has **never been executed** — there
/// is no Windows host on the build side, the same caveat the cross-compiled
/// `.exe` carries. What is tested is the command that would be run, which is
/// where the quoting bugs live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub program: String,
    pub args: Vec<String>,
    /// One line, for showing a person before it runs.
    pub what: String,
}

impl Plan {
    /// How it would be typed, for showing rather than for running.
    pub fn as_typed(&self) -> String {
        let mut out = self.program.clone();
        for a in &self.args {
            out.push(' ');
            if a.contains(' ') && !a.starts_with('"') {
                out.push('"');
                out.push_str(a);
                out.push('"');
            } else {
                out.push_str(a);
            }
        }
        out
    }
}

/// The `/TR` value: the program to run, quoted, plus its flag.
///
/// **This is the line that gets quoting wrong.** `schtasks /TR` takes one
/// string holding the whole command, and the program path contains spaces on
/// almost every real install (`C:\Program Files\...`, or a user folder with a
/// space in the name). Unquoted, `schtasks` accepts it and the task fails at
/// logon with a file-not-found nobody sees, because a logon task's failure is
/// a row in Task Scheduler and not a message.
///
/// Quoted with `\"`, and the outer quoting is handled by passing this as a
/// single argument rather than by building a shell string — `Command` does not
/// go through a shell, so there is no second level of parsing to escape for.
pub fn task_command(exe: &Path, mode: Mode) -> String {
    format!("\"{}\" {}", exe.display(), mode.flag())
}

/// Register Atlas to start at logon.
///
/// On Windows the task is created from an XML definition (`task_xml`,
/// written beside Atlas's state by `write_task_file` before this runs) rather
/// than from `/SC ONLOGON /TR ...` flags (28 Sep 2026). The flags cannot say
/// the three things that matter for a program that runs for days:
///
/// * **No time limit.** A task made with `schtasks /Create` flags gets the
///   default `ExecutionTimeLimit` of 72 hours, and Task Scheduler *ends* the
///   program when it is reached: Atlas would have been killed three days
///   after you signed in, every time you left the laptop signed in that long.
///   `PT0S` is "no limit".
/// * **Batteries.** The default is "don't start on battery, stop when the
///   power lead comes out". On a laptop that is Atlas not there on the train.
/// * **Priority.** A task's default priority, 7, starts the program below
///   normal priority. 5 is normal.
///
/// It also names the program and its working folder separately, so there is
/// no quoted command line to get wrong.
pub fn register(exe: &Path, mode: Mode) -> Plan {
    if cfg!(windows) {
        Plan {
            program: "schtasks".into(),
            args: vec![
                "/Create".into(),
                "/TN".into(),
                TASK_NAME.into(),
                // Not elevated: the definition says LeastPrivilege and an
                // interactive (signed-in) logon. See the module note.
                "/XML".into(),
                task_file_path(exe).display().to_string(),
                // Replace an existing one rather than failing, so running this
                // twice is not an error and changing mode works.
                "/F".into(),
            ],
            what: format!("start Atlas at logon so it can {}", mode.plainly()),
        }
    } else {
        // systemd user unit, written by `unit_file` and enabled here. A user
        // unit rather than a system one, for the same reason the Windows task
        // is not elevated and not a service: Atlas needs the logged-in
        // session.
        Plan {
            program: "systemctl".into(),
            args: vec!["--user".into(), "enable".into(), "--now".into(), "atlas.service".into()],
            what: format!("start Atlas at login so it can {}", mode.plainly()),
        }
    }
}

/// Where the task's definition is written before `schtasks /XML` reads it:
/// Atlas's own state folder, beside the program.
pub fn task_file_path(exe: &Path) -> std::path::PathBuf {
    exe.parent().unwrap_or_else(|| Path::new(".")).join("data").join("state").join("atlas-task.xml")
}

/// Who is signed in, as Task Scheduler names them (`DOMAIN\user`).
fn signed_in_user() -> Option<String> {
    let user = std::env::var("USERNAME").ok().filter(|u| !u.trim().is_empty())?;
    match std::env::var("USERDOMAIN").ok().filter(|d| !d.trim().is_empty()) {
        Some(domain) => Some(format!("{domain}\\{user}")),
        None => Some(user),
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

/// The scheduled task, as Task Scheduler's own XML. Starts `exe` with the
/// mode's flag when `user` signs in (every user, when `user` is `None`), in
/// the program's own folder, with no time limit, on battery too, one copy at
/// a time, at normal priority, and only while you are signed in
/// (`InteractiveToken`: it can use your desktop, your microphone and your
/// screen, and it needs no stored password).
pub fn task_xml(exe: &Path, mode: Mode, user: Option<&str>) -> String {
    // The folder, cut at the last separator of either kind, so the XML for a
    // Windows path is the same whichever machine writes it (and is tested).
    let whole = exe.display().to_string();
    let dir = whole.rfind(['\\', '/']).map(|i| whole[..i].to_string()).unwrap_or_default();
    let user_line = |indent: &str| match user {
        Some(u) => format!("{indent}<UserId>{}</UserId>\n", xml_escape(u)),
        None => String::new(),
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-16\"?>\n\
<Task version=\"1.2\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\n\
  <RegistrationInfo>\n\
    <Description>Starts Atlas in the background when you sign in, so it can {what}.</Description>\n\
  </RegistrationInfo>\n\
  <Triggers>\n\
    <LogonTrigger>\n\
      <Enabled>true</Enabled>\n\
{trigger_user}\
    </LogonTrigger>\n\
    <SessionStateChangeTrigger>\n\
      <Enabled>true</Enabled>\n\
{trigger_user}\
      <StateChange>SessionUnlock</StateChange>\n\
    </SessionStateChangeTrigger>\n\
    <EventTrigger>\n\
      <Enabled>true</Enabled>\n\
      <Subscription>{woke}</Subscription>\n\
    </EventTrigger>\n\
  </Triggers>\n\
  <Principals>\n\
    <Principal id=\"Author\">\n\
{principal_user}\
      <LogonType>InteractiveToken</LogonType>\n\
      <RunLevel>LeastPrivilege</RunLevel>\n\
    </Principal>\n\
  </Principals>\n\
  <Settings>\n\
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>\n\
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>\n\
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>\n\
    <AllowHardTerminate>true</AllowHardTerminate>\n\
    <StartWhenAvailable>false</StartWhenAvailable>\n\
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>\n\
    <IdleSettings>\n\
      <StopOnIdleEnd>false</StopOnIdleEnd>\n\
      <RestartOnIdle>false</RestartOnIdle>\n\
    </IdleSettings>\n\
    <AllowStartOnDemand>true</AllowStartOnDemand>\n\
    <Enabled>true</Enabled>\n\
    <Hidden>false</Hidden>\n\
    <RunOnlyIfIdle>false</RunOnlyIfIdle>\n\
    <WakeToRun>false</WakeToRun>\n\
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>\n\
    <Priority>5</Priority>\n\
  </Settings>\n\
  <Actions Context=\"Author\">\n\
    <Exec>\n\
      <Command>{exe}</Command>\n\
      <Arguments>{flag}</Arguments>\n\
      <WorkingDirectory>{dir}</WorkingDirectory>\n\
    </Exec>\n\
  </Actions>\n\
</Task>\n",
        what = xml_escape(mode.plainly()),
        trigger_user = user_line("      "),
        principal_user = user_line("      "),
        exe = xml_escape(&exe.display().to_string()),
        flag = mode.flag(),
        dir = xml_escape(&dir),
        woke = xml_escape(WOKE_QUERY),
    )
}

/// The events that mean the computer has just woken (item 33): sleep ending
/// (Power-Troubleshooter 1) and Modern Standby ending (Kernel-Power 507,
/// which is how a lid-closed laptop like Eric's sleeps). With the unlock
/// trigger beside it, Atlas is started whenever you come back to the
/// computer -- with the lid shut behind two monitors, Windows locks and
/// unlocks rather than signing in again, so the sign-in trigger alone left
/// Atlas absent for 14 hours of 30 Sep. Starting it when it's already
/// running does nothing (`onlyone`; the task's own `IgnoreNew`).
pub const WOKE_QUERY: &str = "<QueryList><Query Id=\"0\" Path=\"System\"><Select Path=\"System\">\
*[System[(Provider[@Name='Microsoft-Windows-Power-Troubleshooter'] and EventID=1) or \
(Provider[@Name='Microsoft-Windows-Kernel-Power'] and EventID=507)]]</Select></Query></QueryList>";

/// The XML as the bytes `schtasks /XML` reads without complaint: UTF-16
/// little-endian with its byte-order mark, which is what the declaration says
/// and what Task Scheduler itself writes when it exports a task.
pub fn task_file_bytes(xml: &str) -> Vec<u8> {
    let mut out = vec![0xFF, 0xFE];
    for unit in xml.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out
}

/// Write the task's definition where `register`'s plan reads it.
pub fn write_task_file(exe: &Path, mode: Mode) -> Result<std::path::PathBuf, String> {
    let path = task_file_path(exe);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("couldn't make {}: {e}", dir.display()))?;
    }
    let xml = task_xml(exe, mode, signed_in_user().as_deref());
    std::fs::write(&path, task_file_bytes(&xml)).map_err(|e| format!("couldn't write {}: {e}", path.display()))?;
    Ok(path)
}

/// The per-user Run entry: the fallback when Task Scheduler refuses the task
/// (some managed machines don't let a standard account make logon tasks).
/// It needs no administrator, has no time limit, and starts the same
/// program the same way.
pub fn run_entry_add(exe: &Path, mode: Mode) -> Plan {
    Plan {
        program: "reg".into(),
        args: vec![
            "add".into(),
            RUN_KEY.into(),
            "/v".into(),
            TASK_NAME.into(),
            "/t".into(),
            "REG_SZ".into(),
            "/d".into(),
            task_command(exe, mode),
            "/f".into(),
        ],
        what: format!("start Atlas when you sign in so it can {}", mode.plainly()),
    }
}

/// Take the Run entry away again.
pub fn run_entry_remove() -> Plan {
    Plan {
        program: "reg".into(),
        args: vec!["delete".into(), RUN_KEY.into(), "/v".into(), TASK_NAME.into(), "/f".into()],
        what: "stop Atlas starting when you sign in".into(),
    }
}

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

/// Start Atlas with Windows, the whole way: write the task's definition,
/// register it, and — only if Task Scheduler says no — use the per-user Run
/// entry instead. What happened, in words, or why neither worked.
pub fn turn_on(exe: &Path, mode: Mode) -> Result<String, String> {
    if !cfg!(windows) {
        return match run(&register(exe, mode)) {
            Ok(true) => Ok("Atlas will start when you sign in.".into()),
            Ok(false) => Err("the system's service manager said no".into()),
            Err(e) => Err(e),
        };
    }
    let task = write_task_file(exe, mode).and_then(|_| run(&register(exe, mode)));
    match task {
        Ok(true) => {
            // One way in, not two: an entry left from an earlier fallback
            // would start a second copy at sign-in (which `onlyone` would
            // then turn away, but still).
            crate::heard!(run(&run_entry_remove()));
            Ok("Atlas will start when you sign in.".into())
        }
        other => {
            let why = match other {
                Ok(_) => "Task Scheduler said no".to_string(),
                Err(e) => e,
            };
            match run(&run_entry_add(exe, mode)) {
                Ok(true) => Ok("Atlas will start when you sign in (from your sign-in list, because Task Scheduler said no).".into()),
                Ok(false) => Err(format!("{why}, and so did the sign-in list")),
                Err(e) => Err(format!("{why}; {e}")),
            }
        }
    }
}

/// A task registered by an earlier Atlas, from before it was also started
/// on unlock and on waking (item 33): its definition as written then, and
/// the mode it starts in. `None` when there's nothing to bring up to date --
/// no task file, or one that already has the new triggers.
pub fn needs_new_triggers(old_xml: &str) -> Option<Mode> {
    if old_xml.is_empty() || old_xml.contains("SessionStateChangeTrigger") {
        return None;
    }
    let args = old_xml.split("<Arguments>").nth(1).and_then(|r| r.split("</Arguments>").next()).unwrap_or("");
    Some(if args.contains(Mode::Listening.flag()) { Mode::Listening } else { Mode::Background })
}

/// Bring the start-with-Windows task up to date, when you chose to have one
/// and it predates the unlock and wake triggers (item 33). Run by Atlas at
/// start, off the loop. `None` when nothing needed doing.
pub fn bring_up_to_date(exe: &Path, state_dir: &Path) -> Option<Result<String, String>> {
    if !cfg!(windows) || decided(state_dir) != Some(true) {
        return None;
    }
    let old = std::fs::read(task_file_path(exe)).ok()?;
    // UTF-16 with its mark, as `task_file_bytes` writes it.
    let units: Vec<u16> = old.get(2..).unwrap_or(&[]).as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    let mode = needs_new_triggers(&String::from_utf16_lossy(&units))?;
    Some(turn_on(exe, mode))
}

/// Stop Atlas starting with Windows, whichever way it was set up.
pub fn turn_off() -> Result<String, String> {
    let task = run(&remove());
    if cfg!(windows) {
        crate::heard!(run(&run_entry_remove()));
    }
    match task {
        // "Not there" is the outcome asked for -- checked, not assumed (29 Sep
        // 2026: a delete Windows refused was reported as done).
        Ok(_) => turned_off_says(run(&whether_registered())),
        Err(e) => Err(e),
    }
}

/// What `turn_off` says, from asking afterwards whether the task is still
/// registered (`Ok(true)`: it is).
pub fn turned_off_says(still: Result<bool, String>) -> Result<String, String> {
    match still {
        Ok(true) => Err("Windows didn't let me take Atlas out of what starts when you sign in -- \
                         it's in Task Scheduler as \"Atlas\"; you can delete it there"
            .into()),
        _ => Ok("Atlas won't start on its own when you sign in.".into()),
    }
}

// ---------- starting with Windows, once, after setup ----------

/// The file that remembers what was decided about starting with Windows:
/// `on` or `off`. Its existence is what makes the automatic switch-on a
/// one-time thing, so unticking "Start Atlas when I sign in" sticks.
pub fn decision_file(state_dir: &Path) -> std::path::PathBuf {
    state_dir.join("start_with_windows")
}

/// What was last decided, if anything ever was.
pub fn decided(state_dir: &Path) -> Option<bool> {
    let s = std::fs::read_to_string(decision_file(state_dir)).ok()?;
    match s.trim() {
        "on" => Some(true),
        "off" => Some(false),
        _ => None,
    }
}

/// Keep what was decided: by setup switching it on, or by you.
pub fn remember(state_dir: &Path, on: bool) -> std::io::Result<()> {
    crate::store::write_whole_in_state(state_dir, &decision_file(state_dir), if on { b"on\n" } else { b"off\n" })
}

/// What finishing setup does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AfterSetup {
    /// Switch on starting with Windows.
    pub register: bool,
    /// Start the background Atlas now.
    pub start: bool,
}

/// Eric's ruling (17 Sep 2026): Atlas starts with Windows, in the
/// background. And (28 Sep 2026): "I don't want a command terminal to be
/// open. When on windows I don't even want to have the hub or the
/// application open for Atlas to run."
///
/// So once setup has finished with its settings readable (`settings_ok`), on
/// Windows: switch on starting with Windows — **only if nothing was ever
/// decided**, so a later untick is never undone — and, when this walk is the
/// one that finished setting Atlas up (`just_set_up`), start the background
/// Atlas if it isn't running. Later opens of the window start it before the
/// window opens (`firstlaunch::what_opening_does`), so starting it here too
/// would race that one. Nothing, elsewhere or when the settings can't be
/// read (the background Atlas would stop at once without them).
pub fn after_setup(on_windows: bool, settings_ok: bool, just_set_up: bool, decided: Option<bool>, running: bool) -> AfterSetup {
    if !on_windows || !settings_ok {
        return AfterSetup { register: false, start: false };
    }
    AfterSetup { register: decided.is_none(), start: just_set_up && !running }
}

/// Stop Atlas starting at logon.
pub fn remove() -> Plan {
    if cfg!(windows) {
        Plan {
            program: "schtasks".into(),
            args: vec!["/Delete".into(), "/TN".into(), TASK_NAME.into(), "/F".into()],
            what: "stop Atlas starting when you log in".into(),
        }
    } else {
        Plan {
            program: "systemctl".into(),
            args: vec!["--user".into(), "disable".into(), "--now".into(), "atlas.service".into()],
            what: "stop Atlas starting when you log in".into(),
        }
    }
}

/// Ask whether it is registered.
///
/// Named `whether_registered` and not `query`, which is what it was called for
/// about ten minutes. `asking::query` exists, `calls()` matches bare names, and
/// one `query(` call here made `asking::query` look reachable --
/// `tests/dead_methods.rs` reported it as "now has a caller" within the same
/// run. That is the collision hazard this tree has a whole guard for
/// (`name_collisions.rs`, 139 entries), walked into while adding a module.
///
/// Renaming was the fix; deleting `asking::query` from the dead list would
/// have been a false clearing, and the list would have quietly stopped meaning
/// anything.
pub fn whether_registered() -> Plan {
    if cfg!(windows) {
        Plan {
            program: "schtasks".into(),
            args: vec!["/Query".into(), "/TN".into(), TASK_NAME.into()],
            what: "ask whether Atlas starts when you log in".into(),
        }
    } else {
        Plan {
            program: "systemctl".into(),
            args: vec!["--user".into(), "is-enabled".into(), "atlas.service".into()],
            what: "ask whether Atlas starts when you log in".into(),
        }
    }
}

/// The systemd user unit, for the non-Windows path.
///
/// `WorkingDirectory` is set even though `roots` no longer needs it. Belt as
/// well as braces: it costs one line and it means a future reader who reaches
/// for a relative path does not get a surprise that only shows up at logon.
pub fn unit_file(exe: &Path, mode: Mode) -> String {
    format!(
        "[Unit]\n\
         Description=Atlas\n\
         After=graphical-session.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={exe} {flag}\n\
         WorkingDirectory={dir}\n\
         Restart=on-failure\n\
         RestartSec=5\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        exe = exe.display(),
        flag = mode.flag(),
        dir = exe.parent().map(|p| p.display().to_string()).unwrap_or_else(|| ".".into()),
    )
}

/// Where the unit file goes.
pub fn unit_path() -> std::path::PathBuf {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default();
    home.join(".config/systemd/user/atlas.service")
}

/// Run a plan.
///
/// `Ok(true)` when the command reported success, `Ok(false)` when it ran and
/// said no — which is the normal answer from `query` for "not registered" and
/// must not be reported as a failure. `Err` is only for not being able to run
/// it at all.
pub fn run(plan: &Plan) -> Result<bool, String> {
    use std::process::Stdio;
    let mut cmd = crate::tools::command(&plan.program);
    cmd.args(&plan.args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    // No console flashing up from the windowless Atlas (29 Sep 2026:
    // schtasks and reg each opened one when the switch was flipped).
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd.status()
        .map(|s| s.success())
        .map_err(|e| format!("couldn't run {}: {e}", plan.program))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn the_program_path_is_quoted_because_installs_have_spaces_in_them() {
        // The bug this guards: `schtasks /TR` takes the whole command as one
        // string, and an unquoted path with a space in it is accepted at
        // create time and fails at logon, silently.
        let exe = PathBuf::from(r"C:\Program Files\Atlas\atlas.exe");
        let cmd = task_command(&exe, Mode::Background);
        assert!(cmd.starts_with('"'), "the program path is not quoted: {cmd}");
        assert!(
            cmd.contains(r#"Atlas\atlas.exe" --daemon"#),
            "the quote closes in the wrong place: {cmd}"
        );
    }

    #[test]
    fn the_flag_matches_the_mode_asked_for() {
        let exe = PathBuf::from("/opt/atlas/atlas");
        assert!(task_command(&exe, Mode::Background).ends_with("--daemon"));
        // `--wake`, the wake word's own loop (29 Sep 2026).
        assert!(task_command(&exe, Mode::Listening).ends_with("--wake"));
    }

    #[test]
    fn an_absolute_path_is_required_for_this_to_mean_anything() {
        // A relative path in a logon task resolves against whatever Windows
        // starts the task in, which is `system32`. The caller passes
        // `current_exe()`; this records why.
        let exe = PathBuf::from("atlas.exe");
        let cmd = task_command(&exe, Mode::Background);
        assert_eq!(
            cmd, "\"atlas.exe\" --daemon",
            "the shape is right, and the caller is responsible for it being absolute"
        );
    }

    #[test]
    fn registering_is_not_elevated() {
        // An assistant holding your microphone has no business as admin, and
        // `/RL HIGHEST` would need an elevated shell to create.
        // 28 Sep 2026: the task is made from XML now (`task_xml`), so "not
        // elevated" is the definition's RunLevel rather than `/RL LIMITED`.
        let p = register(&PathBuf::from("/opt/atlas/atlas"), Mode::Background);
        assert!(!p.as_typed().contains("HIGHEST"), "registered as elevated: {}", p.as_typed());
        let xml = task_xml(&PathBuf::from("/opt/atlas/atlas"), Mode::Background, Some("PC\\eric"));
        assert!(xml.contains("<RunLevel>LeastPrivilege</RunLevel>"), "{xml}");
        assert!(!xml.contains("HighestAvailable"), "{xml}");
    }

    #[test]
    fn registering_twice_replaces_rather_than_failing() {
        let p = register(&PathBuf::from("/opt/atlas/atlas"), Mode::Background);
        if p.program == "schtasks" {
            assert!(p.args.iter().any(|a| a == "/F"), "a second run would fail: {:?}", p.args);
        }
    }

    #[test]
    fn adding_and_removing_name_the_same_task() {
        // The failure this prevents: removing looks like it worked and the
        // task is still there under a different name.
        let add = register(&PathBuf::from("/opt/atlas/atlas"), Mode::Background);
        let del = remove();
        if add.program == "schtasks" {
            assert!(add.args.contains(&TASK_NAME.to_string()));
            assert!(del.args.contains(&TASK_NAME.to_string()));
        }
    }

    #[test]
    fn the_unit_file_starts_the_right_thing_from_the_right_place() {
        let exe = PathBuf::from("/opt/atlas/atlas");
        let unit = unit_file(&exe, Mode::Background);
        assert!(unit.contains("ExecStart=/opt/atlas/atlas --daemon"), "{unit}");
        assert!(unit.contains("WorkingDirectory=/opt/atlas"), "{unit}");
        assert!(unit.contains("WantedBy=default.target"), "{unit}");
    }

    #[test]
    fn a_mode_can_be_asked_for_in_words() {
        assert_eq!(Mode::parse("in the background"), Some(Mode::Background));
        assert_eq!(Mode::parse("--daemon"), Some(Mode::Background));
        assert_eq!(Mode::parse("just listen"), Some(Mode::Listening));
        assert_eq!(Mode::parse("wake word"), Some(Mode::Listening));
        assert_eq!(Mode::parse("sideways"), None);
    }

    #[test]
    fn what_it_says_it_will_do_is_specific_enough_to_disagree_with() {
        // "set things up" tells a person nothing. Both modes have to name
        // what will actually happen without them asking.
        for m in [Mode::Background, Mode::Listening] {
            let said = m.plainly();
            assert!(said.len() > 30, "too vague to consent to: {said}");
            assert!(said.contains("listen"), "does not mention the microphone: {said}");
        }
    }
}
