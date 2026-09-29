//! Replacing the binary without losing what Atlas knows.
//!
//! The reason this exists, in one sentence someone actually said: *"There
//! isn't enough evidence to justify deleting and reinstalling Atlas multiple
//! times."* That is the correct instinct about the wrong situation —
//! updating Atlas should never have meant deleting it, and until this file
//! there was nothing that said so, nothing that checked it, and nothing that
//! could be pointed at as evidence.
//!
//! ## What an update actually is
//!
//! Atlas keeps three things in three places, and only one of them comes from
//! a download:
//!
//! | | what | comes from |
//! |---|---|---|
//! | **the program** | the `atlas` binary | rebuilt or re-downloaded |
//! | **the recipe** | `config/*.yaml` | ships with the program, generic, no username in it |
//! | **your machine and your memory** | `config/machine.yaml`, `data/` | written here, never shipped, never replaced |
//!
//! So an update is: stop Atlas, replace one file, start Atlas. The third row
//! is untouched, and the second is only touched when the shipped recipe
//! itself changed.
//!
//! ## Why it needs code rather than a sentence in a README
//!
//! Because "it should be fine" is what every destructive upgrade has said.
//! `check` looks at a real install and answers a specific question — if the
//! binary were replaced right now, what survives, what is regenerated, and
//! what is at risk — before anything is moved. It is the difference between
//! believing an update is safe and having looked.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// What happens to one thing when the binary is replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Fate {
    /// Untouched. Yours, and an update has no business with it.
    Kept,
    /// Comes from the download. Replaced, and that is the point.
    Replaced,
    /// Not there, and will be made from scratch on first run. Not a problem.
    Regenerated,
    /// There, yours, and *also* shipped — so a careless update would
    /// overwrite it. The only row that ever needs a decision.
    AtRisk,
}

impl Fate {
    pub fn plain(&self) -> &'static str {
        match self {
            Fate::Kept => "kept",
            Fate::Replaced => "replaced by the update",
            Fate::Regenerated => "will be recreated",
            Fate::AtRisk => "AT RISK — yours, but an update would overwrite it",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    /// Relative to the install directory, always with forward slashes so the
    /// answer reads the same on every platform.
    pub path: String,
    pub fate: Fate,
    /// Why, in a sentence, for the person reading the report.
    pub what: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub items: Vec<Item>,
}

impl Report {
    pub fn at_risk(&self) -> Vec<&Item> {
        self.items.iter().filter(|i| i.fate == Fate::AtRisk).collect()
    }

    pub fn kept(&self) -> Vec<&Item> {
        self.items.iter().filter(|i| i.fate == Fate::Kept).collect()
    }

    /// Is it safe to replace the binary right now?
    ///
    /// Safe means nothing that is yours would be lost. It deliberately does
    /// *not* mean everything is present and healthy — a fresh install with no
    /// data at all is perfectly safe to update, and `atlas doctor` is what
    /// answers the health question.
    pub fn safe(&self) -> bool {
        self.at_risk().is_empty()
    }

    /// The whole thing, in the shape a person reads at a terminal.
    pub fn spoken(&self) -> String {
        let kept = self.kept().len();
        let risky = self.at_risk();
        let mut s = if risky.is_empty() {
            format!(
                "Safe to update. {kept} thing{} of yours stay exactly where they are.",
                if kept == 1 { "" } else { "s" }
            )
        } else {
            format!("Not safe yet — {} thing{} would be overwritten.", risky.len(), if risky.len() == 1 { "" } else { "s" })
        };
        for i in &self.items {
            s.push_str(&format!("\n  {:<28} {}  — {}", i.path, i.fate.plain(), i.what));
        }
        s
    }
}

/// Everything that is yours and must survive, with what it holds.
///
/// A list rather than a rule, on purpose. A rule ("anything under `data/`")
/// silently covers whatever a future session drops in there, which is how a
/// directory nobody decided about ends up holding something that matters. If
/// something new needs to survive an update, it gets a line here and a
/// sentence saying what it is.
pub const YOURS: &[(&str, &str)] = &[
    ("config/machine.yaml", "the app paths, displays and device names `atlas adapt` found here"),
    ("config/settings.yaml", "every switch you changed in Settings -- tools.yaml is \
     replaced by an update, which is exactly why your choices do not live in it"),
    ("config/local", "your own edits to the shipped config files, moved out of them so an \
     update cannot undo them, with the shipped text they were made against"),
    ("data/state", "notes, the tray, the vault, peer tokens, profiles, the journal, \
     and the model-call log that says how the local model has actually performed"),
    ("data/notes", "what you have written"),
    ("data/plugins", "the add-ons you or friends added -- your approval of each is in \
     data/state, so an update keeps both the add-on and what you allowed it"),
    ("data/index.md", "the index of what you have written -- rebuildable, but \
     losing it costs a rebuild and hides any drift that had accumulated"),
    ("data/backups", "snapshots `atlas backups` can restore from"),
    ("data/logs", "what Atlas did, and when"),
    ("data/trash", "what Atlas removed and is holding for 30 days -- an update \
     that wiped this would destroy the only copy of anything you had not yet \
     restored, which is the opposite of what the trash is for"),
];

/// What comes from the download and is meant to be replaced.
pub const SHIPPED: &[(&str, &str)] = &[
    ("config/apps.yaml", "the generic app list, with %VARS% rather than your paths"),
    ("config/tools.yaml", "which features are on, and how external tools are called"),
    ("config/layouts.yaml", "monitor roles and window layouts"),
    ("config/commands.yaml", "the spoken command vocabulary"),
    ("config/policy.yaml", "what Atlas may do without asking"),
    ("config/indexing.yaml", "which folders Atlas may look in"),
];

/// Look at a real install and say what an update would do to it.
pub fn check(root: &Path) -> Report {
    check_with(root, YOURS, SHIPPED)
}

/// The same, against lists given to it.
///
/// Split out because `check`'s `AtRisk` branch cannot fire against the real
/// lists — they are disjoint, and a test asserts they stay that way. A
/// branch that protects an invariant and can never run while the invariant
/// holds is exactly the kind of code this project keeps finding built and
/// unexercised, so it is made reachable rather than trusted: a test passes
/// deliberately overlapping lists and checks it fires.
pub fn check_with(
    root: &Path,
    yours: &[(&str, &str)],
    shipped: &[(&str, &str)],
) -> Report {
    let mut items = Vec::new();

    for (rel, what) in yours {
        let p = root.join(rel);
        let fate = if !exists(&p) {
            // Nothing there yet. A first install, or a part of Atlas you have
            // not used. Either way there is nothing to lose.
            Fate::Regenerated
        } else if shipped.iter().any(|(s, _)| s == rel) {
            // Both yours and shipped. Nothing is in both lists today, and
            // this branch is here so that the day something is, it is
            // reported rather than quietly overwritten.
            Fate::AtRisk
        } else {
            Fate::Kept
        };
        items.push(Item { path: (*rel).to_string(), fate, what: (*what).to_string() });
    }

    for (rel, what) in shipped {
        items.push(Item {
            path: (*rel).to_string(),
            fate: Fate::Replaced,
            what: (*what).to_string(),
        });
    }

    Report { items }
}

fn exists(p: &Path) -> bool {
    p.exists()
}

/// The shape of everything this build stores — the files under `data/` and
/// the YOURS config layer — as one number.
///
/// The version label says which build this is; it says nothing about whether
/// a different build can read what this one wrote. This does. A signed release
/// states the oldest data format it can open (`min_data_format`) and the one it
/// writes (`data_format`), and a device refuses a release that cannot open its
/// data rather than installing it and finding out.
///
/// **Bump this in the same change as anything that alters the shape of stored
/// data** — a renamed field in a state file, a moved directory, a new required
/// key — and ship the migration with it. Adding a field that has a default
/// does not need a bump; changing or removing one does.
pub const DATA_FORMAT: u32 = 1;

/// Settings this build renamed: `(file, old.dotted.path, new.dotted.path)`.
///
/// Built into the program rather than carried in the signed release notice,
/// so it is one fact in one place and it applies however the program arrived
/// -- through the courier, a download, or a copied folder. At startup
/// `yourchanges::keep_hand_edits` moves your Settings choices and your kept
/// hand edits from each old name to the new one, so a rename never silently
/// turns off something you chose. **Add a line here in the same change that
/// renames a key in a shipped file.** Never remove one: a device that skipped
/// releases still needs it.
pub const RENAMED_SETTINGS: &[(&str, &str, &str)] = &[];

/// The version this binary was built as: `0.1.<n>` for CI build number `n`,
/// `0.1.0-dev` for a build made anywhere else (`build.rs`).
///
/// This is the name a person reads. It is **not** what tells two builds
/// apart: until 28 Sep 2026 every build said 0.1.0, so a courier update was
/// taken for the build already running and deleted, then recorded as
/// installed. What a build *is* is its SHA-256 (`build_tag`).
pub fn version() -> &'static str {
    env!("ATLAS_VERSION")
}

/// The SHA-256 of the file at `path`, or `None` if it can't be read.
pub fn sha256_of(path: &Path) -> Option<String> {
    crate::digest::sha256_file_hex(path).ok()
}

/// The SHA-256 of the program running now, worked out once.
fn this_build_sha() -> &'static str {
    static SHA: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    SHA.get_or_init(|| std::env::current_exe().ok().and_then(|p| sha256_of(&p)).unwrap_or_default())
}

/// How many hex digits of the SHA-256 a tag carries: enough that two builds
/// never share one, short enough to read in a file name.
const TAG_SHA: usize = 12;

/// One build, as the updater names it: the version a person reads, then the
/// start of the file's SHA-256 (`0.1.57-3fa9c01d22be`). Every place a
/// build's *identity* matters -- the kept previous and set-aside failed
/// files, the trial, the known-bad list -- uses this, so two builds that
/// happen to carry the same version are never mistaken for each other.
/// With no fingerprint (unknown), it is the version alone.
pub fn build_tag(version: &str, sha256: &str) -> String {
    if sha256.len() >= TAG_SHA {
        format!("{version}-{}", sha256[..TAG_SHA].to_ascii_lowercase())
    } else {
        version.to_string()
    }
}

/// The version inside a tag: `0.1.57-3fa9c01d22be` -> `0.1.57`. A tag with
/// no fingerprint (an old file name) is its own version.
pub fn tag_version(tag: &str) -> &str {
    match tag.rsplit_once('-') {
        Some((v, sha)) if sha.len() == TAG_SHA && sha.chars().all(|c| c.is_ascii_hexdigit()) => v,
        _ => tag,
    }
}

/// The tag of the build at `path`, which says it is `version`.
pub fn tag_of(path: &Path, version: &str) -> String {
    build_tag(version, &sha256_of(path).unwrap_or_default())
}

/// The tag of the program running now.
pub fn this_tag() -> String {
    build_tag(version(), this_build_sha())
}

/// Is `a` an older version than `b`? Compared number by number (`0.1.9` is
/// older than `0.1.10`); a `-dev` build counts as older than the same
/// numbers without it. Unreadable versions are never called older.
pub fn older_version(a: &str, b: &str) -> bool {
    fn parts(v: &str) -> Option<(Vec<u64>, bool)> {
        let (nums, dev) = match v.split_once('-') {
            Some((n, _)) => (n, true),
            None => (v, false),
        };
        let n: Option<Vec<u64>> = nums.split('.').map(|x| x.parse().ok()).collect();
        Some((n?, dev))
    }
    match (parts(a), parts(b)) {
        (Some((na, da)), Some((nb, db))) => na < nb || (na == nb && da && !db),
        _ => false,
    }
}

/// Where an update should put the old binary before writing the new one.
///
/// Keeping the previous binary is what makes an update reversible without a
/// download: if the new one misbehaves, the old one is right there. Named
/// with the build's tag (`build_tag`) so two updates in a row -- even two
/// builds with the same version -- do not lose the one that worked.
pub fn keep_old_at(root: &Path, tag: &str) -> PathBuf {
    root.join(format!("atlas-{tag}.previous"))
}

/// How many kept previous builds, and set-aside failed ones, stay beside
/// Atlas. Each is a whole program; without a limit every update left one more.
pub const KEEP_BUILDS: usize = 2;

/// Remove all but the newest `keep` files named `atlas-*<suffix>` in `root`
/// (`.previous`, `.failed`, `.undone`). Never touches `except`. Returns how
/// many went.
pub fn prune_kept(root: &Path, suffix: &str, keep: usize, except: Option<&Path>) -> usize {
    let Ok(entries) = std::fs::read_dir(root) else { return 0 };
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            n.starts_with("atlas-") && n.ends_with(suffix) && e.path().is_file()
        })
        .filter(|e| except.is_none_or(|x| e.path() != x))
        .filter_map(|e| Some((e.metadata().and_then(|m| m.modified()).ok()?, e.path())))
        .collect();
    let keep = if except.is_some() { keep.saturating_sub(1) } else { keep };
    found.sort();
    let extra = found.len().saturating_sub(keep);
    let mut gone = 0;
    for (_, p) in found.into_iter().take(extra) {
        if std::fs::remove_file(&p).is_ok() {
            gone += 1;
        }
    }
    gone
}


// ---------------------------------------------------------------- the updates folder

/// The folder a new Atlas is dropped into (`docs/OPEN_ITEMS.md` §15: "Drop
/// a new exe in, Atlas notices, tells you it is there, and swaps on next
/// start"). Nothing is fetched: this only notices a file you put there.
fn updates_dir(root: &Path) -> PathBuf {
    root.join("updates")
}

/// Where a checked build is put for the next start to swap in: the same
/// place a build dropped by hand goes, so there is one way in, and it always
/// passes the health check and the trial (`swap_checked`).
pub fn staging_path(root: &Path) -> PathBuf {
    updates_dir(root).join(exe_name())
}

fn exe_name() -> &'static str {
    if cfg!(windows) {
        "atlas.exe"
    } else {
        "atlas"
    }
}

/// A new binary waiting in `updates/`, with the version it reports about
/// itself. It is asked (`--version`) rather than trusted: a file that does
/// not run here, or reports nothing, is not an update and is said to be so.
pub fn waiting(root: &Path) -> Option<Result<(PathBuf, String), String>> {
    let path = updates_dir(root).join(exe_name());
    if !path.is_file() {
        return None;
    }
    Some(version_of(&path).map(|v| (path, v)))
}

/// Ask the Atlas program at `path` which version it is (`--version`, with
/// `ATLAS_UPDATE_PROBE` so it swaps nothing in while answering).
pub fn version_of(path: &Path) -> Result<String, String> {
    let out = crate::tools::command(path).arg("--version").env("ATLAS_UPDATE_PROBE", "1").output();
    match out {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            match text.lines().next().and_then(|l| l.strip_prefix("atlas ")) {
                Some(v) => Ok(v.trim().to_string()),
                None => Err(format!("{} ran but didn't say it was Atlas", path.display())),
            }
        }
        Ok(o) => Err(format!("{} didn't run here (exit {:?})", path.display(), o.status.code())),
        Err(e) => Err(format!("{} didn't run here: {e}", path.display())),
    }
}

/// A build put in place by `swap_in`.
struct SwappedIn {
    path: PathBuf,
    version: String,
    /// The new build's tag, and the tag of the one it replaced.
    tag: String,
    replaced: String,
}

/// On start: if a checked update is waiting, keep the running binary as
/// `atlas-<tag>.previous` (so going back needs no download), put the new
/// one in its place, and return what went in. Private since O1: the only
/// way in is `swap_checked`, which has the new one check itself before
/// anything starts it.
///
/// Builds are told apart by their SHA-256, not their version (28 Sep 2026:
/// every build said 0.1.0, so a real update was deleted as "the version
/// already running"). The same bytes as the running program are removed;
/// anything else, whatever version it says, is a different build.
/// Renaming a running binary is allowed on Windows and Linux alike; writing
/// over it is not, which is why this renames first.
fn swap_in(root: &Path, running: &Path) -> Option<Result<SwappedIn, String>> {
    let (new, v) = match waiting(root)? {
        Ok(x) => x,
        Err(why) => return Some(Err(why)),
    };
    let new_sha = sha256_of(&new).unwrap_or_default();
    let running_sha = sha256_of(running).unwrap_or_default();
    let tag = build_tag(&v, &new_sha);
    let replaced = build_tag(version(), &running_sha);
    if is_known_bad(root, &tag) {
        return Some(Err(format!("the file in updates/ is {tag}, which already failed here and was rolled back; left it alone")));
    }
    if !new_sha.is_empty() && new_sha == running_sha {
        let _ = std::fs::remove_file(&new);
        return Some(Err(format!("the file in updates/ is {tag}, the very build already running; removed it")));
    }
    let keep = keep_old_at(root, &replaced);
    Some((|| {
        let _ = std::fs::remove_file(&keep);
        std::fs::rename(running, &keep).map_err(|e| format!("couldn't keep the old one as {}: {e}", keep.display()))?;
        if let Err(e) = std::fs::rename(&new, running).or_else(|_| std::fs::copy(&new, running).map(|_| ()).and_then(|_| std::fs::remove_file(&new))) {
            // Put the old one back rather than leave nothing to start.
            let _ = std::fs::rename(&keep, running);
            return Err(format!("couldn't put {tag} in place ({e}); still on {replaced}"));
        }
        // The one just kept, and one before it; older ones go.
        prune_kept(root, ".previous", KEEP_BUILDS, Some(&keep));
        Ok(SwappedIn { path: running.to_path_buf(), version: v.clone(), tag: tag.clone(), replaced: replaced.clone() })
    })())
}


// ---------------------------------------------------------------- the trial after an update
//
// O1 (26 Sep): a new version has to prove itself on this machine, or the one
// that worked comes back on its own. Two layers, because they catch different
// failures:
//
// 1. Before the new build is started at all, the old one runs it once with
//    `--health-check` (and `ATLAS_UPDATE_PROBE`, so the new one does not try
//    to update anything itself). A build that won't start here, can't read its
//    own settings, can't write its state folder, or hangs is put back in
//    `updates/`'s place as `atlas-<v>.failed`, the kept previous binary goes
//    back to its name, and the old process simply carries on -- it never
//    stopped running.
// 2. A build that passes the check can still fall over once it is really
//    running. So the swap writes a trial marker; every start of the new build
//    counts against it, and a start that gets through (returns normally, or
//    stays up `HEALTHY_AFTER_SECS`) clears it. After `TRIAL_STARTS` starts
//    that never got through, the next start puts the previous binary back and
//    starts that instead.
//
// Either way the version that failed is written down, and a file carrying it
// dropped into `updates/` again is refused rather than tried a second time.
// Every step lands in `updates.log` beside the binary, so "what happened to
// the update" always has an answer.

/// How long the new build gets to answer `--health-check` before it counts as hung.
pub const HEALTH_TIMEOUT_SECS: u64 = 60;
/// Starts of a new build that may fail to get through before the previous one comes back.
pub const TRIAL_STARTS: u32 = 3;
/// A start that stays up this long has got through, even if it never returns.
pub const HEALTHY_AFTER_SECS: u64 = 90;

fn trial_file(root: &Path) -> PathBuf {
    root.join("update-trial.txt")
}

fn known_bad_file(root: &Path) -> PathBuf {
    root.join("update-known-bad.txt")
}

/// Where a build that failed here is set aside (not deleted: it is
/// evidence). Named by its tag (`build_tag`).
pub fn failed_at(root: &Path, tag: &str) -> PathBuf {
    root.join(format!("atlas-{tag}.failed"))
}

fn stamp() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// One line appended to `updates.log`. Best effort: a log that can't be
/// written never stops an update or a rollback.
pub(crate) fn log_update(root: &Path, line: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(root.join("updates.log")) {
        let _ = writeln!(f, "{} {line}", stamp());
    }
}

/// Everything `updates.log` holds, oldest first.
pub fn update_history(root: &Path) -> Vec<String> {
    std::fs::read_to_string(root.join("updates.log")).map(|s| s.lines().map(str::to_string).collect()).unwrap_or_default()
}

/// A new build on trial: which one, which one it replaced (both as tags,
/// `build_tag`), and how many starts it has had without getting through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trial {
    pub new: String,
    pub previous: String,
    pub starts: u32,
}

impl Trial {
    fn load(root: &Path) -> Option<Trial> {
        let text = std::fs::read_to_string(trial_file(root)).ok()?;
        let get = |k: &str| text.lines().find_map(|l| l.strip_prefix(k)).map(|v| v.trim().to_string());
        Some(Trial { new: get("new=")?, previous: get("previous=")?, starts: get("starts=")?.parse().ok()? })
    }

    fn save(&self, root: &Path) -> std::io::Result<()> {
        std::fs::write(trial_file(root), format!("new={}\nprevious={}\nstarts={}\n", self.new, self.previous, self.starts))
    }
}

/// The trial marker, if a new build is on trial.
pub fn current_trial(root: &Path) -> Option<Trial> {
    Trial::load(root)
}

/// Start the trial of `new`, which has just replaced `previous`.
pub fn begin_trial(root: &Path, new: &str, previous: &str) {
    let _ = Trial { new: new.into(), previous: previous.into(), starts: 0 }.save(root);
    log_update(root, &format!("{new} put in place of {previous}; on trial"));
}

/// Whether the build `tag` (`build_tag`) failed here before.
///
/// Keyed by tag since 28 Sep 2026. Before, it was keyed by version, and
/// every build said 0.1.0: one failure blocked every build after it. A line
/// written then (a bare version) no longer matches any tag, so those blocks
/// are lifted rather than carried forward.
pub fn is_known_bad(root: &Path, tag: &str) -> bool {
    std::fs::read_to_string(known_bad_file(root)).map(|s| s.lines().any(|l| l.split_whitespace().next() == Some(tag))).unwrap_or(false)
}

/// Why the build `tag` failed here, as written down when it was set aside.
pub fn known_bad_reason(root: &Path, tag: &str) -> Option<String> {
    let text = std::fs::read_to_string(known_bad_file(root)).ok()?;
    text.lines().find_map(|l| {
        let (v, why) = l.split_once(' ').unwrap_or((l, ""));
        (v == tag).then(|| why.trim().to_string())
    })
}

/// Take the build `tag` off the failed list: it failed because of something on
/// this machine (a full disk, a folder it couldn't write), not because of the
/// build, so once that's put right it may be tried again. The reason is kept
/// in `updates.log`.
pub fn forgive(root: &Path, tag: &str) {
    let Ok(text) = std::fs::read_to_string(known_bad_file(root)) else { return };
    let kept: Vec<&str> = text.lines().filter(|l| l.split_whitespace().next() != Some(tag)).collect();
    let _ = std::fs::write(known_bad_file(root), if kept.is_empty() { String::new() } else { kept.join("\n") + "\n" });
    log_update(root, &format!("{tag} may be tried again: what stopped it was on this machine, not in the build"));
}

fn mark_known_bad(root: &Path, tag: &str, why: &str) {
    use std::io::Write;
    if is_known_bad(root, tag) {
        return;
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(known_bad_file(root)) {
        let _ = writeln!(f, "{tag} {}", why.replace('\n', " "));
    }
}

/// What this build checks about itself when asked `--health-check`: the
/// things that, broken, make everything else fail. `Ok` carries what passed,
/// `Err` what didn't (and what passed, so the log shows how far it got).
pub fn health_check(root: &Path, config_dir: &Path, state_dir: &Path) -> Result<Vec<String>, Vec<String>> {
    let mut ok = vec![format!("this is atlas {}, data format {DATA_FORMAT}", version())];
    let mut bad = Vec::new();

    // Its own settings, as it ships them, have to load. Written to a scratch
    // folder, never over yours: a build on trial changes nothing that the
    // previous one would find different if it came back.
    let scratch = state_dir.join(format!(".health-check-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    match crate::firstlaunch::write_default_config(&scratch) {
        Ok(_) => match crate::config::Config::load(&scratch) {
            Ok(_) => ok.push("its shipped settings load".into()),
            Err(e) => bad.push(format!("its own shipped settings don't load: {e}")),
        },
        Err(e) => bad.push(format!("couldn't write its shipped settings to a scratch folder: {e}")),
    }
    let _ = std::fs::remove_dir_all(&scratch);

    // Your settings folder is readable, if there is one yet.
    if config_dir.exists() {
        match std::fs::read_dir(config_dir) {
            Ok(_) => ok.push("your settings folder reads".into()),
            Err(e) => bad.push(format!("can't read your settings folder {}: {e}", config_dir.display())),
        }
    }

    // It can keep state: a file written, read back, removed.
    let probe = state_dir.join(format!(".health-check-{}.tmp", std::process::id()));
    let wrote = std::fs::create_dir_all(state_dir).and_then(|_| std::fs::write(&probe, b"atlas")).and_then(|_| std::fs::read(&probe));
    let _ = std::fs::remove_file(&probe);
    match wrote {
        Ok(b) if b == b"atlas" => ok.push("its state folder writes and reads back".into()),
        Ok(_) => bad.push(format!("{} gave back something other than what was written", state_dir.display())),
        Err(e) => bad.push(format!("can't keep state in {}: {e}", state_dir.display())),
    }

    // Nothing of yours sits where an update would replace it.
    let report = check(root);
    if report.safe() {
        ok.push("nothing of yours is where the program goes".into());
    } else {
        bad.push("something of yours is where the program goes (see `atlas update`)".into());
    }

    if bad.is_empty() {
        Ok(ok)
    } else {
        bad.extend(ok.into_iter().map(|l| format!("(passed) {l}")));
        Err(bad)
    }
}

/// Run `new --health-check` and wait up to `timeout` for it. `Ok` is the
/// version it reported; `Err` says what went wrong, in words for the log.
pub fn check_new_build(new: &Path, timeout: std::time::Duration) -> Result<String, String> {
    use std::io::Read;
    let mut child = crate::tools::command(new)
        .arg("--health-check")
        .env("ATLAS_UPDATE_PROBE", "1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("it didn't start: {e}"))?;
    // Read as it writes, so a chatty build can't fill the pipe and look hung.
    let mut out = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(o) = out.as_mut() {
            let _ = o.read_to_string(&mut s);
        }
        s
    });
    let started = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("it didn't finish its check within {} seconds", timeout.as_secs()));
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
            Err(e) => return Err(format!("couldn't wait for it: {e}")),
        }
    };
    let text = reader.join().unwrap_or_default();
    let v = text.lines().find_map(|l| l.strip_prefix("healthy: atlas ")).map(|v| v.trim().to_string());
    match (status.success(), v) {
        (true, Some(v)) => Ok(v),
        (true, None) => Err("it exited cleanly but never said it was healthy".into()),
        (false, _) => {
            let why: Vec<&str> = text.lines().filter(|l| l.starts_with("unhealthy: ")).map(|l| &l[11..]).collect();
            if why.is_empty() {
                Err(format!("its check failed (exit {:?})", status.code()))
            } else {
                Err(why.join("; "))
            }
        }
    }
}

/// Put the previous build back: the one at `running` is set aside as
/// `atlas-<failed>.failed`, the kept `atlas-<previous>.previous` goes back to
/// `running`'s name, `failed` is written down as known bad, and the trial
/// ends. If the previous one can't be put back, the failed one is left where
/// it was rather than leaving nothing to start.
pub fn roll_back(root: &Path, running: &Path, failed: &str, previous: &str, why: &str) -> Result<(), String> {
    let keep = keep_old_at(root, previous);
    // A previous file named before tags (bare version) still counts.
    let keep = if keep.is_file() { keep } else { keep_old_at(root, tag_version(previous)) };
    if !keep.is_file() {
        log_update(root, &format!("{failed} failed ({why}) but {} is missing; left it in place", keep.display()));
        return Err(format!("{failed} failed ({why}), and the previous build isn't at {} to go back to", keep.display()));
    }
    let aside = failed_at(root, failed);
    let _ = std::fs::remove_file(&aside);
    std::fs::rename(running, &aside).map_err(|e| format!("couldn't set {failed} aside: {e}"))?;
    if let Err(e) = std::fs::rename(&keep, running) {
        let _ = std::fs::rename(&aside, running);
        log_update(root, &format!("{failed} failed ({why}); couldn't put {previous} back: {e}"));
        return Err(format!("couldn't put {previous} back ({e}); still on {failed}"));
    }
    mark_known_bad(root, failed, why);
    let _ = std::fs::remove_file(trial_file(root));
    prune_kept(root, ".failed", KEEP_BUILDS, Some(&aside));
    log_update(root, &format!("{failed} failed here ({why}); back on {previous}, {failed} kept as {}", aside.display()));
    Ok(())
}

/// What an update found on start.
#[derive(Debug, PartialEq, Eq)]
pub enum Swapped {
    /// The new build passed its check and is in place, on trial; start it.
    /// `tag` is the new build's, `previous` the one it replaced.
    Started { path: PathBuf, version: String, tag: String, previous: String },
    /// The new build failed its check and the running one is back in place.
    /// Carry on as you are.
    RolledBack { version: String, tag: String, why: String },
}

/// The update on start, checked: swap the waiting build in, have it check
/// itself, and either start its trial or put the running one straight back.
pub fn swap_checked(root: &Path, running: &Path, timeout: std::time::Duration) -> Option<Result<Swapped, String>> {
    let SwappedIn { path, version: v, tag, replaced } = match swap_in(root, running)? {
        Ok(x) => x,
        Err(why) => {
            log_update(root, &format!("not swapped in: {why}"));
            return Some(Err(why));
        }
    };
    Some(match check_new_build(&path, timeout) {
        Ok(_) => {
            begin_trial(root, &tag, &replaced);
            Ok(Swapped::Started { path, version: v, tag, previous: replaced })
        }
        Err(why) => match roll_back(root, running, &tag, &replaced, &why) {
            Ok(()) => Ok(Swapped::RolledBack { version: v, tag, why }),
            Err(e) => Err(e),
        },
    })
}

/// What the trial says to a starting build.
#[derive(Debug, PartialEq, Eq)]
pub enum TrialStep {
    /// Not on trial.
    Settled,
    /// On trial; this is start number `n`.
    Trying(u32),
    /// Too many starts that never got through: the previous build is back at
    /// `running`'s name. Start it instead.
    RolledBack { failed: String, previous: String },
}

/// Called on every normal start. Counts this start against a trial of this
/// build, and after `TRIAL_STARTS` that never got through, puts the
/// previous build back.
pub fn trial_on_start(root: &Path, running: &Path) -> TrialStep {
    let Some(mut t) = Trial::load(root) else { return TrialStep::Settled };
    let here = tag_of(running, version());
    if t.new != here {
        // Something else was put in place by hand since; this trial is not about us.
        let _ = std::fs::remove_file(trial_file(root));
        log_update(root, &format!("trial of {} dropped: {here} is what's running", t.new));
        return TrialStep::Settled;
    }
    if t.starts >= TRIAL_STARTS {
        let why = format!("{} starts in a row never got through", t.starts);
        return match roll_back(root, running, &t.new, &t.previous, &why) {
            Ok(()) => TrialStep::RolledBack { failed: t.new, previous: t.previous },
            Err(_) => TrialStep::Trying(t.starts + 1),
        };
    }
    t.starts += 1;
    let _ = t.save(root);
    TrialStep::Trying(t.starts)
}

/// This start of the build on trial got through: the trial is over and the
/// build is kept. `true` if that ended a trial. The running build is this
/// program (`this_tag`).
pub fn trial_passed(root: &Path) -> bool {
    // Only fingerprint this program when there's a trial to end: every exit
    // of every command comes through here.
    if Trial::load(root).is_none() {
        return false;
    }
    trial_passed_by(root, &this_tag())
}

/// `trial_passed`, for the build tagged `running`.
pub fn trial_passed_by(root: &Path, running: &str) -> bool {
    match Trial::load(root) {
        Some(t) if t.new == running => {
            let _ = std::fs::remove_file(trial_file(root));
            log_update(root, &format!("{} got through start {} of its trial; kept", t.new, t.starts));
            true
        }
        _ => false,
    }
}
