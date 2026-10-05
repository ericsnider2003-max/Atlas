//! InstallStep 2 of the update courier: getting a signed release installed, and
//! undone.
//!
//! What was already there: `update_courier` hears a signed release notice and
//! fetches the file, checked piece by piece against the signed fingerprint.
//! `release` holds the checks. `upgrade` swaps a new build in only after it
//! passes its health check, then keeps it on probation (O1). Nothing joined
//! them up: a downloaded release sat in the state folder and never went in.
//!
//! This module is the join:
//!
//! - **Stage** (`stage_update`). Right before installing, check everything
//!   again rather than trusting the record from download time: the notice's
//!   signature against the key trusted *now*, the release order, the data
//!   format, and the downloaded file's own size and fingerprint. Only then is
//!   the file copied to `updates/`, the one way in, so it still has to pass
//!   O1's health check and trial.
//! - **Finish** (`finish_after_start`). The release number moves only once the
//!   new build has actually got through its trial. A build that was rolled
//!   back leaves the number where it was, and is never offered again.
//! - **When** (`mode_for`, `next_step`, `quiet_enough`).
//!   - Automatic on Eric's own devices, ask on friends' copies (Eric, 26 Sep).
//!     Anyone can change it on their own device, and nothing arriving over the
//!     network can.
//!   - An automatic install waits for a quiet moment.
//! - **Undo** (`undo_update`). Going back one version needs a
//!   `release::LocalApproval`, which only the person at this device can give.
//!   The version you went back from isn't offered again; a newer one is.
//! - **Key rotation arriving** (`heard_rotation`). A signed rotation posted in
//!   the release channel moves this device's trust, or is refused and said
//!   once.
//!
//! The daemon tick, the `atlas update install/undo` commands, the hub's
//! Updates page and the voice phrases call these. They are wired after the
//! merge of the three Atlas versions, because every one of those files is in
//! that merge.

use crate::release::{self, Direction, Installed, LocalApproval, SignedRotation};
use crate::store::Store;
use crate::update_courier::Available;
use crate::upgrade;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const PENDING: &str = "update_pending";
const HISTORY: &str = "update_history";
const NOT_AGAIN: &str = "update_not_again";
const ASKED: &str = "update_asked";
const CHOSEN: &str = "update_mode";
const YES: &str = "update_yes";
const NEWS: &str = "update_news";
const RESTART: &str = "update_restart";

/// How long a "not now" holds before Atlas asks again.
pub const ASK_AGAIN_SECS: u64 = 24 * 3600;
/// How long you must have been away from the keyboard and mouse before an
/// automatic install restarts Atlas.
pub const QUIET_AFTER_SECS: u64 = 10 * 60;

// ---------------------------------------------------------------- when

/// The `update.auto` setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoUpdate {
    /// Heard, downloaded, installed at the next quiet moment; told afterwards.
    Automatic,
    /// Heard, downloaded, then asked. "Yes" installs; "not now" asks tomorrow.
    Ask,
    /// Nothing is fetched or installed. `atlas update` still says what's out.
    Off,
}

/// The mode this device uses: your own choice if you made one. Otherwise
/// automatic on the release owner's own devices (the channel's owner key, or
/// a device that key vouched for) and ask everywhere else (Eric, 26 Sep:
/// "automatic on my devices, ask on friends'").
///
/// Phones never install by themselves whatever this says: Android and iOS
/// both need a tap (spec §6), so the phone side reads `Automatic` as "fetch
/// and check, then offer one tap".
pub fn mode_for(chosen: Option<AutoUpdate>, my_key: &str, channel: Option<&crate::groups::GroupState>) -> AutoUpdate {
    if let Some(m) = chosen {
        return m;
    }
    match channel {
        Some(c) if !my_key.is_empty() && (my_key == c.owner || c.is_delegate(my_key)) => AutoUpdate::Automatic,
        _ => AutoUpdate::Ask,
    }
}

/// Is now a good moment to restart Atlas into a new version on its own?
///
/// - You've been away from the keyboard and mouse long enough. An unknown
///   idle time is never taken as quiet.
/// - Windows isn't saying you're presenting, gaming or in quiet time.
///   Locked (`Away`) counts as quiet.
/// - You're not on a call.
/// - No project work is half done.
pub fn quiet_enough(idle_secs: Option<u64>, os: Option<crate::platform::OsQuiet>, on_call: bool, work_in_hand: bool) -> bool {
    use crate::platform::OsQuiet;
    let away_long_enough = idle_secs.is_some_and(|s| s >= QUIET_AFTER_SECS);
    let os_ok = matches!(os, None | Some(OsQuiet::Accepts) | Some(OsQuiet::Away));
    away_long_enough && os_ok && !on_call && !work_in_hand
}

/// What to do with a release that's here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallStep {
    /// Nothing to do.
    Nothing,
    /// Ask the person, with this sentence.
    Ask(String),
    /// Stage it and restart now.
    InstallNow,
    /// Automatic, but not a quiet moment yet.
    WaitForQuiet,
}

/// Decide, from what's true right now.
///
/// - `said_yes`: the person said yes to this version.
/// - `ask_due`: it's time to ask again (see `ask_due`).
pub fn next_step(mode: AutoUpdate, available: &Available, said_yes: bool, ask_due: bool, quiet: bool) -> InstallStep {
    if mode == AutoUpdate::Off || available.notice.is_none() || available.downloaded.is_empty() {
        return InstallStep::Nothing;
    }
    if said_yes {
        // You asked for it: now, not at some later quiet moment.
        return InstallStep::InstallNow;
    }
    match mode {
        AutoUpdate::Automatic if quiet => InstallStep::InstallNow,
        AutoUpdate::Automatic => InstallStep::WaitForQuiet,
        AutoUpdate::Ask if ask_due => InstallStep::Ask(format!(
            "Atlas {} is here and checked against your release key. Say \"install the update\" when it suits you \
             (or the Updates page): it restarts me, and the version I'm on now is kept so we can go back.",
            available.version
        )),
        _ => InstallStep::Nothing,
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Asked {
    version: String,
    next_ask_at: u64,
}

/// Is it time to ask about `version`? Yes the first time, and again a day
/// after a "not now".
pub fn ask_due(store: &Store, version: &str, now: u64) -> bool {
    let a: Asked = store.load(ASKED);
    a.version != version || now >= a.next_ask_at
}

/// Record that the person was asked about `version` (or said "not now"):
/// don't ask again for a day.
pub fn asked(store: &Store, version: &str, now: u64) {
    let _ = store.save(ASKED, &Asked { version: version.to_string(), next_ask_at: now + ASK_AGAIN_SECS });
}

// ---------------------------------------------------------------- builds not offered again

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct NotAgain {
    /// (build, version, why). The build is its SHA-256, so a *fixed* build is
    /// offered even if it keeps the same version label; `version:<v>` when the
    /// build isn't known (a copy put in place by hand).
    builds: Vec<(String, String, String)>,
}

/// Should this exact build never be offered or installed here again? One that
/// failed here because of the build itself, or one you went back from. A
/// failure caused by this machine (a full disk, a folder it couldn't write)
/// doesn't count: that's fixed here and the build tried again.
pub fn not_offered_again(store: &Store, sha256: &str, version: &str) -> Option<String> {
    let n: NotAgain = store.load(NOT_AGAIN);
    let by_version = format!("version:{version}");
    n.builds.into_iter().find(|(b, _, _)| (!sha256.is_empty() && b == sha256) || *b == by_version).map(|(_, _, why)| why)
}

fn never_again(store: &Store, sha256: &str, version: &str, why: &str) {
    let key = if sha256.is_empty() { format!("version:{version}") } else { sha256.to_string() };
    let mut n: NotAgain = store.load(NOT_AGAIN);
    if !n.builds.iter().any(|(b, _, _)| *b == key) {
        n.builds.push((key, version.to_string(), why.to_string()));
        let _ = store.save(NOT_AGAIN, &n);
    }
}

// ---------------------------------------------------------------- when an update fails: find out why, get it fixed
//
// Eric, 26 Sep: "If an update fails it needs to be reworked, bugs identified
// and fixed, not just dropped." And then: "I don't want my friends' Atlas to
// tell me. I want a way for my friends to submit feedback when the friend
// makes that determination." So:
//
// 1. What went wrong is written down here, on this device, as a
//    `FailureReport`: which stage (the health check, the probation, the
//    restart), the build's own reasons, and the crash note if it crashed.
//    Paths are scrubbed of your user name. Nothing leaves the device by itself.
// 2. It's sorted. Something wrong on *this machine* (a full disk, a folder it
//    couldn't write, your files where the program goes) isn't the build's
//    fault: you're told what to put right, and the same build is tried again.
//    Anything else is the build's, and that exact build waits for a fix.
// 3. On the releaser's own devices, the report goes straight into the
//    releaser's list, and that build stops being handed out (`is_halted`).
// 4. On a friend's device it stays there until the friend decides to tell the
//    releaser. `feedback` is how: the friend writes what's wrong and may attach
//    this report, sees exactly what's in it, and sends it or doesn't.
// 5. The fix is a new signed release. It's offered as normal, because the
//    block is on the failing build, not on its version label.

const FAILURES: &str = "update_failures";
const LAST_FAILURE: &str = "update_last_failure";
const INBOX: &str = "update_failure_reports";
const HALTED: &str = "release_halted";
const RETRY_AFTER: &str = "update_retry_after";

/// After a failure caused by this machine, how long before the same build is
/// tried again: long enough to put it right, short enough not to forget.
pub const MACHINE_RETRY_SECS: u64 = 6 * 3600;

/// Whose fault a failure was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum FailedBecause {
    /// Something in the build. It needs a fix and a new release.
    #[default]
    TheBuild,
    /// Something on this machine. Put right here, the same build is tried again.
    ThisMachine,
}

/// What an update failure looked like, for whoever fixes it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailureReport {
    pub version: String,
    /// The build that failed, by fingerprint (empty for a hand-placed copy).
    #[serde(default)]
    pub sha256: String,
    /// The version it was replacing, which is what's running again now.
    pub replaces: String,
    pub platform: String,
    /// "health check", "probation" or "restart".
    pub stage: String,
    /// The build's own words about what failed, scrubbed of your user name.
    pub reasons: Vec<String>,
    /// If it crashed: where and what, from the crash note.
    #[serde(default)]
    pub crash: String,
    pub at: u64,
    /// Whose Atlas it failed on (set by the receiving side from the pairing).
    #[serde(default)]
    pub from: String,
    #[serde(default = "default_cause")]
    pub because: FailedBecause,
}

fn default_cause() -> FailedBecause {
    FailedBecause::TheBuild
}


/// The largest report accepted over a pairing.
pub const MAX_REPORT_BYTES: usize = 16 * 1024;

/// Sort a failure by its reasons. Only the machine-side reasons the health
/// check itself names are counted as the machine's; anything unrecognised is
/// the build's, so nothing wrong with a build is ever explained away.
pub fn failed_because(reasons: &[String], crash: &str) -> FailedBecause {
    let machine = [
        "can't keep state",
        "can't read your settings folder",
        "something of yours is where the program goes",
        "no space left",
        "disk full",
        "gave back something other than what was written",
    ];
    let all = reasons.join(" ").to_lowercase();
    if crash.is_empty() && !all.is_empty() && reasons.iter().all(|r| {
        let r = r.to_lowercase();
        r.starts_with("(passed)") || machine.iter().any(|m| r.contains(m))
    }) {
        FailedBecause::ThisMachine
    } else {
        FailedBecause::TheBuild
    }
}

/// Take your user name and home folder out of a line before it leaves.
pub fn scrub_personal(line: &str) -> String {
    let mut out = line.to_string();
    for var in ["USERPROFILE", "HOME"] {
        if let Ok(home) = std::env::var(var) {
            if home.len() > 3 {
                out = out.replace(&home, "~").replace(&home.replace('\\', "/"), "~");
            }
        }
    }
    for var in ["USERNAME", "USER"] {
        if let Ok(user) = std::env::var(var) {
            if user.len() > 2 {
                out = out.replace(&user, "<you>");
            }
        }
    }
    out
}

/// Write a failure down here, sort it, and queue it to be sent. Returns what
/// to tell you.
pub fn record_failure(store: &Store, install_root: &Path, mut report: FailureReport) -> String {
    report.reasons = report.reasons.iter().map(|r| scrub_personal(r)).collect();
    report.crash = scrub_personal(&report.crash);
    report.because = failed_because(&report.reasons, &report.crash);
    let mut all: Vec<FailureReport> = store.load(FAILURES);
    all.push(report.clone());
    let _ = store.save(FAILURES, &all);
    let what = report.reasons.iter().filter(|r| !r.starts_with("(passed)")).cloned().collect::<Vec<_>>().join("; ");
    match report.because {
        FailedBecause::ThisMachine => {
            // Not the build's fault: fix it here and try the same build again,
            // after a pause, so a disk that's still full isn't hit every tick.
            upgrade::forgive(install_root, &upgrade::build_tag(&report.version, &report.sha256));
            let _ = store.save(RETRY_AFTER, &(report.at.max(crate::store::now()) + MACHINE_RETRY_SECS));
            format!(
                "Atlas {} couldn't start here because of something on this computer: {what}. Once that's put \
                 right I'll try {} again -- nothing is wrong with the update itself.",
                report.version, report.version
            )
        }
        FailedBecause::TheBuild => {
            never_again(store, &report.sha256, &report.version, "it failed here and is waiting for a fix");
            let _ = store.save(LAST_FAILURE, &Some(report.clone()));
            format!(
                "Atlas {} failed at its {} here ({}), so I went back to {}. I've written down exactly what went \
                 wrong. To tell whoever sends you updates, say \"report a bug\", or use the Feedback page, to describe it and attach \
                 this; nothing is sent unless you do. A fixed release will be offered as usual.",
                report.version,
                report.stage,
                if what.is_empty() { report.crash.clone() } else { what },
                report.replaces
            )
        }
    }
}

/// The last update failure written down here, for attaching to feedback.
pub fn last_failure(store: &Store) -> Option<FailureReport> {
    store.load::<Option<FailureReport>>(LAST_FAILURE)
}

/// On the releaser's own devices: the last failure, once, to file straight
/// into the releaser's list (no feedback needed from yourself to yourself).
pub fn unfiled_own_failure(store: &Store) -> Option<FailureReport> {
    let r = last_failure(store)?;
    let filed: u64 = store.load("update_last_failure_filed");
    if r.at <= filed && filed != 0 {
        return None;
    }
    let _ = store.save("update_last_failure_filed", &r.at.max(1));
    Some(r)
}

/// File a report in the releaser's list, and stop handing out a build that
/// failed on its own account. Used for the releaser's own devices; a friend's
/// report comes with their feedback, and the releaser decides what to do.
fn file_report(store: &Store, r: FailureReport, halt: bool) {
    if halt && r.because == FailedBecause::TheBuild && !r.sha256.is_empty() {
        let mut halted: Vec<String> = store.load(HALTED);
        if !halted.contains(&r.sha256) {
            halted.push(r.sha256.clone());
            let _ = store.save(HALTED, &halted);
        }
    }
    let mut inbox: Vec<FailureReport> = store.load(INBOX);
    inbox.push(r);
    let _ = store.save(INBOX, &inbox);
}

/// On the releaser's own devices a failure doesn't need to travel: file it here.
pub fn file_own_report(store: &Store, r: &FailureReport) {
    let mut r = r.clone();
    if r.from.is_empty() {
        r.from = "this computer".into();
    }
    file_report(store, r, true);
}

/// A failure a friend attached to their feedback: filed for the fix brief.
/// Their say-so alone doesn't stop the build being handed out -- that's
/// yours to decide (`atlas release hold <version>`).
pub fn file_friend_report(store: &Store, r: FailureReport) {
    file_report(store, r, false);
}

/// Stop handing out a build yourself, after reading the feedback about it.
pub fn hold_release(store: &Store, sha256: &str) {
    let mut halted: Vec<String> = store.load(HALTED);
    if !halted.iter().any(|s| s == sha256) {
        halted.push(sha256.to_string());
        let _ = store.save(HALTED, &halted);
    }
}

/// Has a report stopped this build being handed out? Read by the file door
/// (`update_courier::chunk`), from the state folder it serves.
pub fn is_halted(state_root: &Path, sha256: &str) -> bool {
    Store::new(state_root).load::<Vec<String>>(HALTED).iter().any(|s| s == sha256)
}

/// Every failure report the releaser has, newest last.
pub fn failure_reports(store: &Store) -> Vec<FailureReport> {
    store.load(INBOX)
}

/// A written brief for fixing one failing build: what failed, where, how
/// often, and what to check. What `atlas update failures brief <version>` writes for a
/// coding session or for Atlas working on itself.
pub fn fix_brief(reports: &[FailureReport], version: &str) -> Option<String> {
    let these: Vec<&FailureReport> = reports.iter().filter(|r| r.version == version).collect();
    if these.is_empty() {
        return None;
    }
    let mut b = format!("# Fix brief: Atlas {version} failed after updating\n\n");
    let builds: std::collections::BTreeSet<&str> = these.iter().map(|r| r.sha256.as_str()).collect();
    b.push_str(&format!(
        "{} report(s), from {} machine(s), {} build(s).\n\n",
        these.len(),
        these.iter().map(|r| r.from.as_str()).collect::<std::collections::BTreeSet<_>>().len(),
        builds.len()
    ));
    for r in &these {
        b.push_str(&format!(
            "## {} ({}), replacing {}\n- stage: {}\n- sorted as: {}\n",
            if r.from.is_empty() { "unknown" } else { &r.from },
            r.platform,
            r.replaces,
            r.stage,
            match r.because {
                FailedBecause::TheBuild => "the build",
                FailedBecause::ThisMachine => "that machine",
            }
        ));
        for reason in &r.reasons {
            b.push_str(&format!("- {reason}\n"));
        }
        if !r.crash.is_empty() {
            b.push_str(&format!("- crash:\n```\n{}\n```\n", r.crash));
        }
        b.push('\n');
    }
    b.push_str(
        "## To close this\n\
         1. Reproduce: run the failing build with `--health-check` on the same platform, or start it and read the crash note.\n\
         2. Write a test that fails the way the report says, then fix the cause.\n\
         3. Sign and announce a new release. The failing build stays blocked on every device and stops being handed out; the fixed one is offered as usual.\n",
    );
    Some(b)
}

// ---------------------------------------------------------------- stage and finish

/// A release copied into `updates/`, waiting for the next start.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagedUpdate {
    pub version: String,
    /// The build it replaces: the one running when it was staged, as its tag
    /// (`upgrade::build_tag`; a bare version in a record written before 28
    /// Sep 2026).
    pub replaces: String,
    /// The build's fingerprint, so a failure blocks this build and not a fixed one.
    #[serde(default)]
    pub sha256: String,
    /// The notice, exactly as signed, so finishing checks it again.
    pub notice: Option<release::SignedManifest>,
    /// Atlas has restarted itself once to take it in. A second restart for
    /// the same staged build never happens: if it didn't go in, that's said.
    #[serde(default)]
    pub restarted: bool,
}

/// The release that was installed before the current one, so undoing it can
/// put the record back as it was.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct History {
    previous: Option<Installed>,
}

/// Check the downloaded release again, then put it where the next start picks
/// it up (`upgrade::staging_path`). Returns the version staged.
///
/// Refused, and said:
/// - no longer signed by the key this device trusts;
/// - not newer;
/// - needs a newer data format;
/// - no build for this device;
/// - the file on disk isn't the size and fingerprint the notice gave;
/// - a version that failed here or that you went back from.
pub fn stage_update(store: &Store, install_root: &Path, platform: &str) -> Result<String, String> {
    let available = Available::load(store);
    let Some(notice) = available.notice.clone() else { return Err("There's no update waiting.".into()) };
    if available.downloaded.is_empty() {
        return Err(format!("Atlas {} hasn't finished arriving yet.", available.version));
    }
    let installed = Installed::load(store);
    let accepted = release::accept(&installed, &notice, upgrade::DATA_FORMAT, platform, Direction::Forward)
        .map_err(|r| format!("I won't install Atlas {}: {}", available.version, r.plain()))?;
    let version = accepted.manifest.version.clone();
    if let Some(why) = not_offered_again(store, &accepted.artifact.sha256, &version) {
        return Err(format!("I won't install Atlas {version} again: {why}."));
    }
    if upgrade::is_known_bad(install_root, &upgrade::build_tag(&version, &accepted.artifact.sha256)) {
        return Err(format!("Atlas {version} failed here before and is waiting for a fixed release."));
    }
    let bytes = std::fs::read(&available.downloaded)
        .map_err(|e| format!("I couldn't read the downloaded Atlas {version}: {e}"))?;
    release::check_artifact(&accepted.artifact, &bytes).map_err(|r| {
        // Damaged on disk after it arrived: fetched again, not given up on.
        Available::fetch_again(store);
        format!(
            "The downloaded Atlas {version} changed on disk after it arrived, so I threw it away and I'm fetching it \
             again. {}",
            r.plain()
        )
    })?;
    let to = upgrade::staging_path(install_root);
    std::fs::create_dir_all(to.parent().unwrap_or(install_root)).map_err(|e| format!("I couldn't make the updates folder: {e}"))?;
    // Written beside, then renamed into place: the next start never sees half a file.
    let part = to.with_extension("part");
    std::fs::write(&part, &bytes).map_err(|e| format!("I couldn't put Atlas {version} in place: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        crate::heard!(std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o755)));
    }
    std::fs::rename(&part, &to).map_err(|e| format!("I couldn't put Atlas {version} in place: {e}"))?;
    let _ = store.save(
        PENDING,
        &StagedUpdate {
            version: version.clone(),
            replaces: upgrade::this_tag(),
            sha256: accepted.artifact.sha256.clone(),
            notice: Some(notice),
            restarted: false,
        },
    );
    upgrade::log_update(install_root, &format!("{version} checked again and staged for the next start"));
    Ok(version)
}

/// The staged release, if any.
pub fn pending(store: &Store) -> Option<StagedUpdate> {
    store.exists(PENDING).then(|| store.load::<StagedUpdate>(PENDING)).filter(|p| !p.version.is_empty())
}

/// What finishing found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateSettled {
    /// Nothing was staged.
    Nothing,
    /// Staged, and not settled yet: the new build is still on probation, or
    /// hasn't started.
    Waiting,
    /// The new build got through its trial: recorded as installed.
    Installed(String),
    /// The new build failed and the previous one is back: never offered again.
    RolledBack(String),
}

/// Called once a start has settled (`upgrade::trial_passed`), and on every
/// start of an old build. Records the outcome of a staged release.
///
/// `running` is the running build's tag (`upgrade::this_tag`). The staged
/// release counts as the one running only when its fingerprint matches --
/// never by version alone. Until 28 Sep 2026 it was by version, every build
/// said 0.1.0, and an update that had been thrown away was recorded as
/// installed: "Updated to Atlas 0.1.0" on the build that was already there.
pub fn finish_after_start(store: &Store, install_root: &Path, running: &str) -> UpdateSettled {
    let Some(p) = pending(store) else { return UpdateSettled::Nothing };
    let settle = |store: &Store| {
        let _ = store.save(PENDING, &StagedUpdate::default());
    };
    let staged = upgrade::build_tag(&p.version, &p.sha256);
    let is_running = if p.sha256.is_empty() { p.version == upgrade::tag_version(running) } else { staged == running };
    let running_version = upgrade::tag_version(running);
    if is_running {
        if upgrade::current_trial(install_root).is_some_and(|t| t.new == running) {
            return UpdateSettled::Waiting;
        }
        let mut installed = Installed::load(store);
        let before = installed.clone();
        let Some(notice) = p.notice.as_ref() else {
            settle(store);
            return UpdateSettled::Nothing;
        };
        let Some(platform) = release::this_platform() else {
            settle(store);
            return UpdateSettled::Nothing;
        };
        match release::accept(&installed, notice, upgrade::DATA_FORMAT, platform, Direction::Forward) {
            Ok(acc) => {
                installed.installed(&acc);
                if installed.save(store).is_err() {
                    return UpdateSettled::Waiting;
                }
                let _ = store.save(HISTORY, &History { previous: Some(before) });
            }
            // Already recorded (a second call) is fine; anything else leaves
            // the record alone and says nothing new.
            Err(_) => {}
        }
        settle(store);
        Available::forget_offer(store);
        upgrade::log_update(install_root, &format!("{} recorded as installed (release {})", p.version, Installed::load(store).sequence));
        tell(store, &format!(
            "Updated to Atlas {}. It passed its check here. {} is kept, and \"go back to the last version\" (or the Updates page) goes back to it.",
            p.version, upgrade::tag_version(&p.replaces)
        ));
        return UpdateSettled::Installed(p.version);
    }
    // Running something other than what was staged. If the staged one failed
    // here, it was rolled back: it isn't offered again.
    if let Some(why) = upgrade::known_bad_reason(install_root, &staged) {
        let probation = why.contains("never got through");
        let crash = if probation { crate::crash::last(store).map(|n| n.detail()).unwrap_or_default() } else { String::new() };
        let report = FailureReport {
            version: p.version.clone(),
            sha256: p.sha256.clone(),
            replaces: running_version.to_string(),
            platform: release::this_platform().unwrap_or("unknown").to_string(),
            stage: if probation { "probation".into() } else { "health check".into() },
            reasons: why.split("; ").map(str::to_string).collect(),
            crash,
            at: crate::store::now(),
            from: String::new(),
            because: FailedBecause::TheBuild,
        };
        let said = record_failure(store, install_root, report);
        tell(store, &said);
        settle(store);
        if failed_because(&why.split("; ").map(str::to_string).collect::<Vec<_>>(), "") == FailedBecause::TheBuild || probation {
            Available::forget_offer(store);
        }
        return UpdateSettled::RolledBack(p.version);
    }
    UpdateSettled::Waiting
}

/// Start this program again with the same arguments, detached, so the caller
/// can exit and the new copy's start swaps the staged build in.
pub fn relaunch_self(running: &Path, args: &[String]) -> Result<(), String> {
    // The self-test's copy never puts anything on the screen or starts
    // another Atlas (1 Oct 2026: a briefing panel opened on Eric's screen from
    // inside the test).
    if crate::selftest::in_a_test() {
        return Err(crate::selftest::NOT_IN_A_TEST.into());
    }
    crate::tools::command(running)
        .args(args)
        .stdin(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("I couldn't start myself again: {e}"))
}

// ---------------------------------------------------------------- news, choices, the tick

/// Something to tell you about an update, kept until said once.
fn tell(store: &Store, what: &str) {
    let _ = store.save(NEWS, &what.to_string());
}

/// What there is to say about an update since last asked, once.
pub fn take_news(store: &Store) -> Option<String> {
    let said: String = store.load(NEWS);
    if said.is_empty() {
        return None;
    }
    let _ = store.save(NEWS, &String::new());
    Some(said)
}

/// Your own `update.auto` choice, if you made one (`atlas update auto ...`).
pub fn chosen_mode(store: &Store) -> Option<AutoUpdate> {
    store.exists(CHOSEN).then(|| store.load::<Option<AutoUpdate>>(CHOSEN)).flatten()
}

/// Make (or, with `None`, undo) your own choice. Only ever from you, here.
pub fn choose_mode(store: &Store, mode: Option<AutoUpdate>) -> crate::error::Result<()> {
    store.save(CHOSEN, &mode)
}

/// You said yes to installing `version` (the hub, a voice answer, or `atlas
/// update install`).
pub fn say_yes(store: &Store, version: &str) {
    let _ = store.save(YES, &version.to_string());
}

fn said_yes(store: &Store, version: &str) -> bool {
    !version.is_empty() && store.load::<String>(YES) == version
}

/// What's true about the moment, from the platform.
#[derive(Debug, Clone, Copy, Default)]
pub struct UpdateMoment {
    pub idle_secs: Option<u64>,
    pub os: Option<crate::platform::OsQuiet>,
    pub on_call: bool,
    pub work_in_hand: bool,
}

/// What a tick decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ticked {
    Nothing,
    /// Say this to the person.
    Say(String),
    /// A release is staged: start again now (`relaunch_self`) so it goes in.
    Restart(String),
}

/// One daemon tick of step 2: finish, say, ask, or install.
pub fn update_tick(
    store: &Store,
    install_root: &Path,
    platform: &str,
    my_key: &str,
    channel: Option<&crate::groups::GroupState>,
    moment: &UpdateMoment,
    now: u64,
) -> Ticked {
    if let Some(news) = take_news(store) {
        return Ticked::Say(news);
    }
    // 30 Sep 2026: the flag cleared (or `restarted` marked) is what stops
    // the next start restarting again; a save that failed restarted Atlas
    // every time it came up. No save, no restart -- it's said instead.
    if store.load::<bool>(RESTART) {
        if let Err(e) = store.save(RESTART, &false) {
            return Ticked::Say(format!("I need to restart to finish going back a version, but couldn't note that I had ({e}), so I'd restart over and over. Restart me yourself when it suits."));
        }
        return Ticked::Restart("Restarting to finish going back a version.".into());
    }
    if let Some(mut p) = pending(store) {
        if p.replaces != upgrade::this_tag() && p.replaces != upgrade::version() {
            return Ticked::Nothing; // the new build is running; finishing records it
        }
        if !p.restarted {
            p.restarted = true;
            if let Err(e) = store.save(PENDING, &p) {
                return Ticked::Say(format!("Atlas {} is ready, but I couldn't note that I'm restarting for it ({e}), so I'd restart over and over. Restart me yourself when it suits.", p.version));
            }
            return Ticked::Restart(format!("Restarting to install Atlas {}.", p.version));
        }
        // Restarted once and still the old build: it didn't go in. Why is in
        // updates.log ("not swapped in: ..."); it's written up like any other
        // failure, so it gets fixed rather than quietly retried or dropped.
        let _ = store.save(PENDING, &StagedUpdate::default());
        upgrade::log_update(install_root, &format!("{} was staged but didn't go in on restart", p.version));
        let why: Vec<String> = upgrade::update_history(install_root)
            .iter()
            .rev()
            .filter_map(|l| l.split_once("not swapped in: ").map(|(_, w)| w.to_string()))
            .take(1)
            .collect();
        let report = FailureReport {
            version: p.version.clone(),
            sha256: p.sha256.clone(),
            replaces: upgrade::version().to_string(),
            platform: platform.to_string(),
            stage: "restart".into(),
            reasons: if why.is_empty() { vec!["the restart didn't pick it up, and nothing said why".into()] } else { why },
            crash: String::new(),
            at: now,
            from: String::new(),
            because: FailedBecause::TheBuild,
        };
        return Ticked::Say(record_failure(store, install_root, report));
    }
    let available = Available::load(store);
    if now < store.load::<u64>(RETRY_AFTER) && !said_yes(store, &available.version) {
        return Ticked::Nothing; // waiting for this machine to be put right
    }
    let mode = mode_for(chosen_mode(store), my_key, channel);
    let quiet = quiet_enough(moment.idle_secs, moment.os, moment.on_call, moment.work_in_hand);
    let yes = said_yes(store, &available.version);
    match next_step(mode, &available, yes, ask_due(store, &available.version, now), quiet) {
        InstallStep::Nothing | InstallStep::WaitForQuiet => Ticked::Nothing,
        InstallStep::Ask(question) => {
            asked(store, &available.version, now);
            Ticked::Say(question)
        }
        InstallStep::InstallNow => match stage_update(store, install_root, platform) {
            Ok(v) => {
                let _ = store.save(YES, &String::new());
                match pending(store) {
                    Some(mut p) => {
                        p.restarted = true;
                        let _ = store.save(PENDING, &p);
                        Ticked::Restart(format!("Restarting to install Atlas {v}."))
                    }
                    None => Ticked::Nothing,
                }
            }
            Err(why) => {
                let _ = store.save(YES, &String::new());
                Ticked::Say(why)
            }
        },
    }
}

// ---------------------------------------------------------------- undo

/// The previous build this install could go back to: (version, path). The
/// file is named by the build's tag (`upgrade::build_tag`); what's returned
/// is the version a person reads.
pub fn previous_build(install_root: &Path) -> Option<(String, PathBuf)> {
    let mut found: Vec<(std::time::SystemTime, String, PathBuf)> = std::fs::read_dir(install_root)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let v = name.strip_prefix("atlas-")?.strip_suffix(".previous")?.to_string();
            let at = e.metadata().and_then(|m| m.modified()).ok()?;
            Some((at, v, e.path()))
        })
        .collect();
    found.sort();
    found.pop().map(|(_, v, p)| (upgrade::tag_version(&v).to_string(), p))
}

/// Go back one version: the kept previous build returns to `running`'s name,
/// the current one is set aside as `atlas-<v>.undone`, the release record goes
/// back to what it was, and the version you left is never offered again.
///
/// Only with `LocalApproval`, which only the person at this device can give.
/// Returns the version gone back to. The caller restarts (`relaunch_self`).
pub fn undo_update(store: &Store, install_root: &Path, running: &Path, _yes: LocalApproval) -> Result<String, String> {
    let (previous, kept) = previous_build(install_root)
        .ok_or_else(|| "There's no previous version kept here to go back to.".to_string())?;
    // The build running now, by tag: two builds with one version are told
    // apart by their fingerprint (28 Sep 2026).
    let current_tag = upgrade::tag_of(running, upgrade::version());
    let current = upgrade::version().to_string();
    // The build being left, by fingerprint, before the record goes back.
    let current_build = Installed::load(store).sha256;
    if upgrade::sha256_of(&kept).is_some_and(|k| Some(k) == upgrade::sha256_of(running)) {
        return Err(format!("The build kept is {previous}, the one already running."));
    }
    let aside = install_root.join(format!("atlas-{current_tag}.undone"));
    crate::heard!(std::fs::remove_file(&aside));
    std::fs::rename(running, &aside).map_err(|e| format!("I couldn't set {current} aside: {e}"))?;
    if let Err(e) = std::fs::rename(&kept, running) {
        crate::kept!(std::fs::rename(&aside, running));
        return Err(format!("I couldn't put {previous} back ({e}); still on {current}."));
    }
    upgrade::prune_kept(install_root, ".undone", upgrade::KEEP_BUILDS, Some(&aside));
    let history: History = store.load(HISTORY);
    if let Some(before) = history.previous {
        // Keep the trust as it is now: a key rotation taken since stays taken.
        let mut back = before;
        back.trust = Installed::load(store).trust;
        let _ = back.save(store);
        let _ = store.save(HISTORY, &History::default());
    }
    never_again(store, &current_build, &current, "you went back from it");
    let _ = store.save(PENDING, &StagedUpdate::default());
    // The running Atlas is still the version you left, in memory: it restarts
    // itself on its next tick (`update_tick`).
    let _ = store.save(RESTART, &true);
    upgrade::log_update(install_root, &format!("you went back from {current} to {previous}; {current} kept as {}", aside.display()));
    Ok(previous)
}

// ---------------------------------------------------------------- key rotation

/// How a key rotation starts, inside a release-channel message.
pub const ROTATION_PREFIX: &str = "atlas-rotation:";

/// The text to post into the release channel for a signed rotation.
pub fn rotation_notice(signed: &SignedRotation) -> String {
    format!("{ROTATION_PREFIX}{}", serde_json::to_string(signed).unwrap_or_default())
}

/// A message from the release channel's owner that may be a key rotation.
/// Moves this device's trust if it verifies; says a refusal once.
pub fn heard_rotation(store: &Store, body: &str) -> Option<String> {
    heard_rotation_with(store, body, &release::RECOVERY_PUBLIC_KEY)
}

/// `heard_rotation`, with the recovery key given (tests use their own).
pub fn heard_rotation_with(store: &Store, body: &str, recovery: &[u8; 32]) -> Option<String> {
    let json = body.trim().strip_prefix(ROTATION_PREFIX)?;
    let Ok(signed) = serde_json::from_str::<SignedRotation>(json) else {
        return Some("A key-change notice arrived that I couldn't read, so I ignored it.".into());
    };
    let mut installed = Installed::load(store);
    // The same notice again, after it was taken: nothing to say.
    let already = serde_json::from_str::<release::Rotation>(&signed.rotation)
        .ok()
        .is_some_and(|r| r.number <= installed.trust.rotations && r.new_anchor == hex32(&installed.trust.current));
    if already {
        return None;
    }
    match release::apply_rotation_with(&installed.trust, recovery, &signed) {
        Ok(trust) => {
            installed.trust = trust;
            installed.save(store).ok()?;
            Some(
                "Your release key changed. Updates are now checked against the new key; anything signed with \
                 the old one is refused."
                    .into(),
            )
        }
        Err(release::Refusal::Malformed(m)) if m.contains("out of order") => None,
        Err(release::Refusal::NotSigned(_)) => Some(
            "A key-change notice arrived that isn't signed by your release key or your recovery key, so I \
             refused it. Updates are still checked against the key you had."
                .into(),
        ),
        Err(r) => Some(format!("A key-change notice arrived and I refused it: {}", r.plain())),
    }
}

fn hex32(b: &[u8; 32]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
