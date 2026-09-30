//! Every knob Atlas has, in YAML rather than in code.
//!
//! The reason for this being a hard rule rather than a preference: a setting
//! compiled into the program is one nobody can change without rebuilding, and
//! this has to be usable by people who will never run a compiler. If it's a
//! choice, it lives in a file with a comment saying why the default is what it
//! is.
//!
//! Loading is deliberately strict — a mistyped key is an error rather than a
//! silently ignored line, because a setting you think you changed and didn't
//! is worse than one that refuses to load.

use crate::error::{AtlasError, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

// ---------- apps.yaml ----------

#[derive(Debug, Clone, Deserialize)]
pub struct AppSpec {
    /// Executable, or a Store app id when `store` is set.
    pub launch: String,
    /// This is a Microsoft Store app. `launch` is then its AppUserModelID,
    /// and Atlas starts it through the shell rather than by path — Windows
    /// blocks running a packaged app from its install folder directly.
    #[serde(default)]
    pub store: bool,
    #[serde(default)]
    pub args: Vec<String>,
    /// Process image names used to find/kill the app.
    pub process_names: Vec<String>,
    /// Substrings that help disambiguate the main window from popups.
    #[serde(default)]
    pub title_hints: Vec<String>,
    /// Logical monitor role (NOT a Windows monitor number).
    pub role: String,
    /// Named layout rect from layouts.yaml.
    pub layout: String,
    /// Layout to use when there is only the laptop screen. Undocked, four
    /// windows tiled into quarters is unusable — most should be maximized and
    /// stacked instead. Falls back to `layout` if unset.
    #[serde(default)]
    pub standalone_layout: Option<String>,
    /// Skip this app entirely when running on the laptop alone.
    #[serde(default)]
    pub docked_only: bool,
    /// Atlas may launch, place and focus this app, but must never send it a
    /// click or a keystroke. This is the Discord guard: a stray synthetic
    /// Enter into a chat client can join a voice call or send a message, and
    /// no amount of careful targeting makes that risk worth taking.
    #[serde(default)]
    pub no_input: bool,
    #[serde(default = "d_poll_ms")]
    pub poll_ms: u64,
    #[serde(default = "d_retries")]
    pub retries: u32,
}
impl AppSpec {
    /// A spec that only finds a window: the app whose process is `process`
    /// (`"notepad.exe"`), with nothing to launch and no layout of its own.
    ///
    /// Five places used to write this as YAML text and parse it back, which
    /// broke on a name containing `"` (and panicked, in `atlas window read`).
    /// Built directly, any name is taken literally.
    pub fn for_process(process: &str) -> AppSpec {
        AppSpec {
            launch: "x".into(),
            store: false,
            args: Vec::new(),
            process_names: vec![process.to_string()],
            title_hints: Vec::new(),
            role: "main".into(),
            layout: "full".into(),
            standalone_layout: None,
            docked_only: false,
            no_input: false,
            poll_ms: d_poll_ms(),
            retries: d_retries(),
        }
    }
}

fn d_poll_ms() -> u64 {
    500
}
fn d_retries() -> u32 {
    20
}

#[derive(Debug, Clone, Deserialize)]
pub struct AppsConfig {
    pub apps: BTreeMap<String, AppSpec>,
    pub startup_order: Vec<String>,
    pub shutdown_order: Vec<String>,
}

impl AppsConfig {
    pub fn get(&self, name: &str) -> Result<&AppSpec> {
        self.apps
            .get(name)
            .ok_or_else(|| AtlasError::UnknownApp(name.to_string()))
    }

    /// Fail at load time, not at 7am when you say "boot workspace".
    pub fn validate(&self, layouts: &LayoutsConfig) -> Result<()> {
        for (name, spec) in &self.apps {
            if let Some(sl) = &spec.standalone_layout {
                if !layouts.layouts.contains_key(sl) {
                    return Err(AtlasError::Config(format!(
                        "app '{}' wants standalone layout '{}' which is not defined",
                        name, sl
                    )));
                }
            }
            if !layouts.layouts.contains_key(&spec.layout) {
                return Err(AtlasError::Config(format!(
                    "app '{}' wants layout '{}' which is not defined",
                    name, spec.layout
                )));
            }
            if !layouts.roles.iter().any(|r| r.name == spec.role) {
                return Err(AtlasError::Config(format!(
                    "app '{}' wants monitor role '{}' which is not defined",
                    name, spec.role
                )));
            }
        }
        for name in self.startup_order.iter().chain(self.shutdown_order.iter()) {
            if !self.apps.contains_key(name) {
                return Err(AtlasError::Config(format!(
                    "'{}' is in a startup/shutdown order but has no app definition",
                    name
                )));
            }
        }
        Ok(())
    }
}

// ---------- layouts.yaml ----------

/// Fractional rect within a monitor's work area. 0.0..=1.0.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
pub struct FracRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// How a logical role claims a physical monitor at runtime.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RoleMatch {
    /// The OS-designated primary display.
    Primary,
    /// Furthest left by x coordinate, among monitors not yet claimed.
    Leftmost,
    /// Furthest right by x coordinate, among monitors not yet claimed.
    Rightmost,
    /// Any remaining monitor.
    Any,
    /// The laptop's own screen (29 Sep 2026). Where that can't be told (a
    /// desktop, or a platform that doesn't say), the primary display, as the
    /// "laptop" role always matched before; with the lid shut, nothing, and
    /// the role falls back.
    Builtin,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RoleSpec {
    pub name: String,
    #[serde(rename = "match")]
    pub match_by: RoleMatch,
    /// If this role can't claim a monitor, fall back to this role instead of failing.
    #[serde(default)]
    pub fallback_to: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LayoutsConfig {
    /// Ordered. Earlier roles claim monitors first.
    pub roles: Vec<RoleSpec>,
    pub layouts: BTreeMap<String, FracRect>,
}

impl LayoutsConfig {
    pub fn layout(&self, name: &str) -> Result<FracRect> {
        self.layouts
            .get(name)
            .copied()
            .ok_or_else(|| AtlasError::UnknownLayout(name.to_string()))
    }
}

// ---------- commands.yaml ----------

#[derive(Debug, Clone, Deserialize)]
pub struct CommandSpec {
    pub intent: String,
    pub phrases: Vec<String>,
    /// Text after the phrase becomes the intent's argument.
    #[serde(default)]
    pub takes_argument: bool,
    /// The argument is welcome but not required.
    ///
    /// Without this, "open" with no target correctly fails to match — and so
    /// did "what did you do", which is a complete sentence. Anything you can
    /// say on its own *or* with a subject needs this.
    #[serde(default)]
    pub argument_optional: bool,
    /// The argument is a token, not words -- case and punctuation are the
    /// content, not noise to normalize away. Without this, `normalize()`
    /// lowercases the argument and strips everything but letters, digits,
    /// underscore and spaces before the intent ever sees it, which silently
    /// destroys anything shaped like `ATLAS-KIN-1:name|host|port|token`.
    /// Found via `accept_pairing_takes_the_pasted_code_as_its_argument`
    /// failing, not by inspection -- every other command here is fine with
    /// normalized text, which is exactly why this stayed invisible until an
    /// argument that isn't prose needed to survive intact.
    #[serde(default)]
    pub raw_argument: bool,
    /// One line saying what the command does, for the model: every command
    /// is offered to it as a tool described by this (27 Sep 2026).
    #[serde(default)]
    pub describe: Option<String>,
    /// How the command is offered to the model: `core` (every turn, in a
    /// fixed order), `retrieved` (the default: when the sentence reads like
    /// it) or `never` (only ever by its phrases -- typing into windows, the
    /// vault, installing updates).
    #[serde(default)]
    pub expose: Option<String>,
    /// The whole sentence must be the phrase (after "please", "now", "for
    /// me", "Atlas" come off). "wait" is a pause; "wait, what do you mean"
    /// is a question.
    #[serde(default)]
    pub anchored: bool,
    /// The argument must be a name Atlas knows: an app, a mode, another
    /// Atlas. "go to chrome" switches windows; "go to sleep" does not.
    #[serde(default)]
    pub names_only: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommandsConfig {
    pub commands: Vec<CommandSpec>,
}

// ---------- bundle ----------

#[derive(Debug, Clone)]
pub struct Config {
    pub apps: AppsConfig,
    pub layouts: LayoutsConfig,
    pub commands: CommandsConfig,
    /// Optional: absent means voice and screen capture are simply unavailable,
    /// which must not stop workspace control from working.
    pub tools: Option<crate::voice::ToolsConfig>,
    pub policy: crate::policy::PolicyConfig,
    /// Optional: absent means Atlas has no file awareness, which must not
    /// stop workspace control from working.
    pub indexing: Option<crate::index::IndexConfig>,
    /// Settings you changed that reached nothing.
    ///
    /// A preference naming a key `tools.yaml` no longer has is the same
    /// failure the settings layer was built to end -- you change something
    /// and nothing happens -- so it is carried out of loading rather than
    /// dropped, and `atlas doctor` says which ones.
    pub settings_that_went_nowhere: Vec<String>,
    /// Your hand edits (`config/local`, see `yourchanges`) that could not be
    /// laid over the shipped files: a section a release removed, an overlay
    /// that does not parse, or edits that made a file unreadable. Reported by
    /// `atlas doctor` for the same reason as the line above.
    pub edits_that_went_nowhere: Vec<String>,
}

fn load_yaml<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let text = std::fs::read_to_string(path)?;
    serde_yaml::from_str(&text).map_err(|source| {
        // A Windows path pasted into a double-quoted YAML string fails with
        // "did not find expected hexadecimal number", because \U starts a
        // unicode escape. That message tells you nothing, so translate it.
        let msg = source.to_string();
        if msg.contains("hexadecimal") || msg.contains("unknown escape") {
            return AtlasError::Config(format!(
                "{}: a Windows path in quotes needs forward slashes. Write \"C:/Users/you/Documents\", not \"C:\\Users\\you\\Documents\". ({msg})",
                path.display()
            ));
        }
        AtlasError::Yaml { path: path.display().to_string(), source }
    })
}

/// One shipped file, with your kept hand edits over it and, for `tools.yaml`,
/// your Settings choices over those.
///
/// Never lets your edits stop Atlas starting: if they leave the file
/// unreadable, it loads the shipped file (with Settings choices) instead and
/// says so, rather than refusing to start over a setting.
fn load_layered<T: for<'de> Deserialize<'de>>(
    dir: &Path,
    file: &str,
    prefs: Option<&crate::preferences::Preferences>,
    settings_unplaced: &mut Vec<String>,
    edits_unplaced: &mut Vec<String>,
) -> Result<T> {
    let path = dir.join(file);
    let shipped: serde_yaml::Value = load_yaml(&path)?;
    let edits = match crate::yourchanges::load_overlay(dir, file) {
        Ok(c) => c,
        Err(e) => {
            edits_unplaced.push(format!("{e} -- none of those edits are in effect"));
            Vec::new()
        }
    };
    let with_prefs = |mut v: serde_yaml::Value, report: &mut Vec<String>| {
        if let Some(p) = prefs {
            report.extend(p.apply_to(&mut v));
        }
        v
    };
    let to_t = |v: serde_yaml::Value| -> Result<T> {
        serde_yaml::from_value(v).map_err(|e| AtlasError::Config(format!("{}: {e}", path.display())))
    };

    if edits.is_empty() {
        return to_t(with_prefs(shipped, settings_unplaced));
    }
    let mut mine = shipped.clone();
    for c in crate::yourchanges::apply(&mut mine, &edits) {
        edits_unplaced.push(format!(
            "{}: {} (the shipped file no longer has that section)",
            crate::yourchanges::overlay_path(dir, file).display(),
            c.dotted()
        ));
    }
    let mut report = Vec::new();
    match to_t(with_prefs(mine, &mut report)) {
        Ok(t) => {
            settings_unplaced.extend(report);
            Ok(t)
        }
        Err(e) => {
            edits_unplaced.push(format!(
                "{}: with your edits the file cannot be read ({e}), so I am running on \
                 the shipped {file} until they are fixed",
                crate::yourchanges::overlay_path(dir, file).display()
            ));
            to_t(with_prefs(shipped, settings_unplaced))
        }
    }
}

impl Config {
    pub fn load(dir: &Path) -> Result<Self> {
        let mut unplaced: Vec<String> = Vec::new();
        let mut edits_unplaced: Vec<String> = Vec::new();
        let apps: AppsConfig = load_layered(dir, "apps.yaml", None, &mut unplaced, &mut edits_unplaced)?;
        let layouts: LayoutsConfig = load_layered(dir, "layouts.yaml", None, &mut unplaced, &mut edits_unplaced)?;
        let commands: CommandsConfig = load_layered(dir, "commands.yaml", None, &mut unplaced, &mut edits_unplaced)?;
        apps.validate(&layouts)?;

        // The settings layer, over the shipped one.
        //
        // Applied to the parsed YAML *before* it becomes a `ToolsConfig`,
        // which is what lets one piece of code carry all 55 settings. The
        // alternative -- a setter per key -- would be a second place the key
        // list lives, and this file already documents what happens when one
        // fact is declared twice.
        //
        // Reported, not swallowed: a preference naming a key that no longer
        // exists is exactly the "I changed it and nothing happened" failure
        // this whole layer was built to end, so it is collected and surfaced
        // by `atlas doctor` rather than dropped here.
        //
        // Order: the shipped file, then your hand edits (`config/local`), then
        // what you chose in Settings -- the most deliberate choice last.
        let tools = if dir.join("tools.yaml").is_file() {
            let prefs = crate::preferences::Preferences::load(dir);
            Some(load_layered(dir, "tools.yaml", Some(&prefs), &mut unplaced, &mut edits_unplaced)?)
        } else {
            None
        };

        let policy = if dir.join("policy.yaml").is_file() {
            load_layered(dir, "policy.yaml", None, &mut unplaced, &mut edits_unplaced)?
        } else {
            crate::policy::PolicyConfig::default()
        };

        let indexing = if dir.join("indexing.yaml").is_file() {
            Some(load_layered(dir, "indexing.yaml", None, &mut unplaced, &mut edits_unplaced)?)
        } else {
            None
        };

        let mut cfg = Config {
            apps,
            layouts,
            commands,
            tools,
            policy,
            indexing,
            settings_that_went_nowhere: unplaced,
            edits_that_went_nowhere: edits_unplaced,
        };

        // The machine layer, over the generic one.
        //
        // Everything above ships to anyone: `%LOCALAPPDATA%` placeholders, no
        // literal username, a browser path that is a guess about where Chrome
        // lives. `machine.yaml` is what `atlas adapt` found on *this*
        // computer, and it is deliberately applied here rather than at each
        // call site — a config that is only correct if you remember to
        // correct it is the shape of bug this split exists to remove.
        //
        // Absent is the normal case (a fresh clone, and every test in this
        // tree), and absent means the generic layer stands unchanged.
        if let Some(m) = crate::adapt::Machine::load(dir) {
            crate::adapt::apply(&mut cfg, &m);
        }

        Ok(cfg)
    }
}

// ---------------------------------------------------------------------------
// Settings that reach nothing.
//
// This file's own opening says loading is "deliberately strict — a mistyped
// key is an error rather than a silently ignored line". That is not true of
// `tools.yaml`: `ToolsConfig` carries `#[serde(default)]` and no
// `deny_unknown_fields`, so an unmatched key is dropped without a word.
//
// The result, measured rather than guessed: of 137 top-level sections in the
// shipped `config/tools.yaml`, **34 reach nothing**. Seventeen have no field
// to land in at all and are discarded by serde. Seventeen more parse
// perfectly into a `pub` field that no line anywhere in `src` ever reads —
// the same shape as the `lifecycle:` block, which carried a memory budget and
// a keep-warm table into a Supervisor that was built from `default()`.
//
// Some of them carry real numbers someone argued for: `retention.total_budget_mb`,
// `triage.draft_replies`. Editing any of them changes nothing and nothing says
// so.
//
// Named, not counted, and for the reason this project already established
// with `ORPHANS`: a ceiling cannot tell "cleared three, added three" from
// "did nothing". Each entry says what it was for, so clearing one is a
// decision about that thing rather than a number going down.
//
// `deny_unknown_fields` is deliberately NOT the fix. It would turn every one
// of the first seventeen into a hard load failure on a config file people
// already have, which trades a silent nothing for a refusal to start. Saying
// so in `atlas doctor` is the fix; deleting or wiring each entry is the work.
// ---------------------------------------------------------------------------

/// Sections serde drops on the floor — no field exists for them.
pub const NO_FIELD_TO_LAND_IN: &[(&str, &str)] = &[
    // A nested key, which this list can now carry. `vault.kdf_rounds` shipped
    // with the comment "a second to unlock, years to attack" beside a number
    // the vault has ignored since it moved to Argon2id, whose cost is memory
    // rather than rounds. The field is deleted; anyone whose file still sets
    // it is told rather than left with the comment.
    ("vault.kdf_rounds", "the vault uses Argon2id, whose cost is memory and passes \
rather than a round count. The number in your file has not done anything since that \
change, and the comment beside it in the shipped file was wrong about what it bought"),
    ("files.look_inside_archives", "nothing here opens an archive. The index \
classifies a .zip and stops, and the unpacking guard beside this setting has no caller \
-- so a switch shipped on read as a behaviour you had. It comes back when something \
unpacks"),
    ("household.household", "the household's id lives in the store, made once at \
first run. A copy of it in a text file is a thing people move between machines, which is \
how two devices claim the same household without either having been invited -- so there \
is one place it can come from, and this is not it"),
    ("household.discoverable", "nothing here announces itself on a local network. \
`mesh` is unwired and says so, and `server.reachable_from` is the opposite approach, \
where you give the address yourself. A switch for a behaviour that does not exist reads \
as one you have turned off"),
    ("cdp", "duplicated by the live `browser:` block, which is the one that works"),
    ("chain", "carries `confirm_before_sending: true`, which reads as a guarantee and is not one"),
    ("freshness", "no FreshnessConfig type exists"),
    ("improve", "no ImproveConfig type exists"),
    ("knowhow", "no KnowhowConfig type exists"),
    ("learned", "no LearnedConfig type exists"),
    ("ledger", "no LedgerConfig type exists"),
    ("otherside", "no OthersideConfig type exists"),
    ("publishing", "no PublishingConfig type exists"),
    ("stance", "no StanceConfig type exists"),
    ("timebox", "no TimeboxConfig type exists"),
    ("triage", "the block you would reach for to turn inbox triage on, now that imap.rs exists"),
    ("undo", "no UndoConfig type exists"),
];

/// Sections that parse into a field nothing ever reads.
pub const PARSED_AND_NEVER_READ: &[(&str, &str)] = &[
    ("android", "AndroidConfig is loaded and never consulted"),
    // `booking` came off this list on 21 Sep 2026: the Booking intent's handler
    // reads `self.tools_cfg().booking` and passes it to `booking::assess` and
    // `booking::could_offer`, so the config genuinely steers the answer now.
    // `consult` and `strategy` came off 26 Sep 2026 with the Atlas Project
    // chat's 25j: the fix loop is handed both (`fixloop::run` in main.rs).
    // `editcraft` and `handoff` came off on 25 Sep 2026: creator advice (I)
    // checks `editcraft.enabled`, and a build handed to a bigger model (D)
    // writes its brief to `handoff`'s limits.
    ("ios", "IosConfig is loaded and never consulted"),
    ("layout_prefs", "LayoutPrefsConfig is loaded and never consulted"),
    // `push_to_talk` came off this list on 25 Sep 2026 (Eric, H1): a
    // low-level keyboard hook on Windows (`hotkeys`) feeds the hold-to-talk
    // state machine, and the key and hold time are read at start-up.
    // `mesh` came off this list on 19 Sep 2026 and the split is worth a line,
    // because it is not "mesh works now". Reaching another device directly is
    // still not built -- `choose` picks between four routes and only the
    // cloud folder exists, and `mesh` stays on CAPABILITY_UNWIRED. What is
    // read is the *advice*: `atlas mesh` says which of the four kinds you
    // named in `mesh.kind`, what it costs, what Atlas would do and what you
    // would have to do yourself, and opens by saying the transport is
    // missing. `mesh.prefer_direct` is the half that genuinely needs the
    // transport, so it is `#[serde(skip)]` now rather than settable.
];

/// Which of those a particular `tools.yaml` actually sets.
///
/// Reads the top level by indentation rather than by parsing, so it says the
/// same thing whether or not the file as a whole is valid — a user whose
/// config half-loads is exactly who needs to be told which half did nothing.
pub fn settings_that_do_nothing(tools_yaml: &str) -> Vec<(&'static str, &'static str)> {
    let mut present: Vec<(&'static str, &'static str)> = Vec::new();
    let top: Vec<&str> = tools_yaml
        .lines()
        .filter(|l| !l.starts_with(char::is_whitespace) && !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split(':').next())
        .map(|k| k.trim())
        .filter(|k| !k.is_empty())
        .collect();
    // Nested keys, for a dead field inside a section that is otherwise alive.
    // `vault:` is read and `vault.kdf_rounds` is not, and saying "vault does
    // nothing" would be a worse answer than saying nothing at all.
    let nested: Vec<String> = tools_yaml
        .lines()
        .scan(String::new(), |section, line| {
            if !line.starts_with(char::is_whitespace) && !line.trim_start().starts_with('#') {
                if let Some(k) = line.split(':').next() {
                    let k = k.trim();
                    if !k.is_empty() {
                        *section = k.to_string();
                    }
                }
                return Some(None);
            }
            let t = line.trim_start();
            if t.starts_with('#') || section.is_empty() {
                return Some(None);
            }
            t.split(':').next().map(|k| {
                let k = k.trim();
                (!k.is_empty()).then(|| format!("{section}.{k}"))
            })
        })
        .flatten()
        .collect();

    for (key, why) in NO_FIELD_TO_LAND_IN.iter().chain(PARSED_AND_NEVER_READ.iter()) {
        let there = if key.contains('.') {
            nested.iter().any(|n| n == key)
        } else {
            top.contains(key)
        };
        if there {
            present.push((*key, *why));
        }
    }
    present
}
