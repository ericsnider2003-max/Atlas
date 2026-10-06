//! One install root, and nothing allowed to invent its own.
//!
//! The bug this exists to kill: every command said `Store::new("data/state")`
//! and `Config::load(Path::new("config"))`, so everything Atlas remembered
//! hung off the *current working directory*. `ATLAS.bat` hid it with
//! `cd /d "%~dp0"`. Anywhere the `.bat` was not the launcher — a shortcut
//! with the wrong "Start in", Task Scheduler (working directory `system32`),
//! a terminal standing somewhere else, `atlas update` run from anywhere —
//! Atlas started with an empty `data/state`, silently, and wrote a second
//! `data/` tree wherever it had been launched from.
//!
//! It had already been found and fixed four times at the leaves
//! (`BackupConfig.dir`, `data/index.md`, the trace log, `research.notes_dir`)
//! and was still live at the trunk. A fifth leaf fix would have been a fifth
//! correction to the same mistake, so this is the trunk rule instead:
//!
//!   **`src/roots.rs` and `src/store.rs` are the only two files in `src` that
//!   may write a `data/…` or `config/…` path literal. Everything else asks
//!   them.**
//!
//! That is one grep, it runs in the suite, and it would have caught all four
//! earlier instances at once.

use std::fs;
use std::path::Path;

/// The two files that are allowed to know the layout.
///
/// `roots.rs` decides where the install is; `store.rs` knows what sits
/// beside `data/state`. Two files, both of which say so in their own doc
/// comments, both covered by their own unit tests.
const MAY_SPELL_PATHS: &[&str] = &["src/roots.rs", "src/store.rs"];

/// Files that legitimately carry install-relative *declarations* rather than
/// paths they resolve themselves.
///
/// `upgrade::YOURS` and `SHIPPED` are lists of relative segments joined
/// against a caller-supplied root (`upgrade::check_with` does `root.join(rel)`).
/// `install::pieces` is the same shape, anchored by `roots::under_install` at
/// its one consumer. These are declarations of layout, not resolutions of it,
/// and turning them into function calls would make them harder to read
/// without making them any safer. They are named here so the exemption is a
/// decision on the record rather than a hole.
const DECLARATION_FILES: &[&str] = &["src/upgrade.rs", "src/install.rs"];

/// `selfwork.rs`'s `may_touch` list is not an install path at all — it is a
/// set of *source-tree prefixes* naming which parts of its own checkout
/// Atlas is allowed to edit when it works on itself. `"config/"` there means
/// "files under config/ in the repository", and anchoring it to the install
/// root would be wrong rather than safer. Named rather than silently
/// matched, so the distinction is on the record.
const SOURCE_TREE_PREFIX_FILES: &[&str] = &["src/selfwork.rs"];

/// A literal inside a console-printing macro is a sentence for a human, not
/// a path being opened. "config/tools.yaml is missing" is prose.
///
/// Checked per line rather than per file, and only for the printing macros —
/// `format!` is deliberately *not* here, because `format!` is one of the
/// ways a path gets built, and a guard that waves those through is the sixth
/// green-for-the-wrong-reason ratchet this tree has had.
fn is_console_prose(line: &str) -> bool {
    ["println!(", "eprintln!(", "print!(", "eprint!(", "writeln!("]
        .iter()
        .any(|m| line.contains(m))
}

fn rust_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().map(|x| x == "rs").unwrap_or(false) {
            out.push(p);
        }
    }
}

/// Is this line inside a `#[cfg(test)]` region, a comment, or a string of
/// prose rather than a path?
///
/// Deliberately crude but *conservative*: when in doubt it does NOT skip, so
/// the failure mode is a false alarm a human reads, never a silent pass. The
/// standing caution in this tree is that guards which parse Rust with
/// substrings keep being green for reasons that have nothing to do with the
/// code — so this one is built to fail loudly instead.
fn is_ignorable(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("//") || t.starts_with("*") || t.starts_with("#[")
}

#[test]
fn no_module_but_roots_and_store_spells_out_a_data_or_config_path() {
    let mut files = Vec::new();
    rust_files(Path::new("src"), &mut files);
    assert!(files.len() > 100, "found only {} source files — the walk is wrong", files.len());

    let mut offenders: Vec<String> = Vec::new();

    for f in &files {
        let rel = f.to_string_lossy().replace('\\', "/");
        if MAY_SPELL_PATHS.contains(&rel.as_str())
            || DECLARATION_FILES.contains(&rel.as_str())
            || SOURCE_TREE_PREFIX_FILES.contains(&rel.as_str())
        {
            continue;
        }
        let Ok(src) = fs::read_to_string(f) else { continue };

        let mut in_test_mod = false;
        let mut test_brace_depth: i32 = 0;
        let mut depth: i32 = 0;

        for (i, line) in src.lines().enumerate() {
            // Track `#[cfg(test)] mod … { … }` so a test's own temp paths
            // are not mistaken for production ones.
            if line.contains("#[cfg(test)]") {
                in_test_mod = true;
                test_brace_depth = depth;
            }
            let opens = line.matches('{').count() as i32;
            let closes = line.matches('}').count() as i32;

            if !is_ignorable(line) && !in_test_mod && !is_console_prose(line) {
                for needle in ["\"data/", "\"config/"] {
                    if line.contains(needle) {
                        offenders.push(format!("{rel}:{} {}", i + 1, line.trim()));
                    }
                }
            }

            depth += opens - closes;
            if in_test_mod && depth <= test_brace_depth && closes > 0 {
                in_test_mod = false;
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "these spell out a path that looks per-install and is not — the bug \
         that made Atlas start with an empty memory from a shortcut. Ask \
         `roots::` or the `Store`'s own accessors instead:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn nothing_in_production_builds_a_store_from_a_relative_path() {
    // The single most load-bearing assertion here. `Store::new("data/state")`
    // is what made `Store::install_root()` return the *empty path*, so even
    // the modules that dutifully asked for `install_root()` were still
    // cwd-relative. `roots::store()` is the only constructor production code
    // may use.
    let mut files = Vec::new();
    rust_files(Path::new("src"), &mut files);

    let mut offenders: Vec<String> = Vec::new();
    for f in &files {
        let rel = f.to_string_lossy().replace('\\', "/");
        if MAY_SPELL_PATHS.contains(&rel.as_str()) {
            continue;
        }
        let Ok(src) = fs::read_to_string(f) else { continue };
        let mut in_test_mod = false;
        let mut test_brace_depth: i32 = 0;
        let mut depth: i32 = 0;
        for (i, line) in src.lines().enumerate() {
            if line.contains("#[cfg(test)]") {
                in_test_mod = true;
                test_brace_depth = depth;
            }
            let opens = line.matches('{').count() as i32;
            let closes = line.matches('}').count() as i32;
            if !is_ignorable(line) && !in_test_mod {
                // A `Store::new` whose argument starts with a quote is a
                // literal path. One taking a variable or a `roots::` call is
                // fine — that is the whole point.
                if let Some(at) = line.find("Store::new(\"") {
                    let _ = at;
                    offenders.push(format!("{rel}:{} {}", i + 1, line.trim()));
                }
            }
            depth += opens - closes;
            if in_test_mod && depth <= test_brace_depth && closes > 0 {
                in_test_mod = false;
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "a Store built from a string literal is a Store rooted at the current \
         working directory. Use `roots::store()`:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn the_resolved_root_is_absolute() {
    // If this is ever relative again, every guarantee above is decoration.
    let r = atlas::roots::install_root();
    assert!(r.is_absolute(), "install root {r:?} is relative — the cwd bug is back");
    assert!(atlas::roots::data_dir().is_absolute());
    assert!(atlas::roots::config_dir().is_absolute() || std::env::var_os("ATLAS_CONFIG").is_some());
}

#[test]
fn an_unset_backup_or_trash_or_notes_dir_resolves_under_the_install_not_the_cwd() {
    use std::path::PathBuf;
    let install = PathBuf::from(if cfg!(windows) { "C:\\atlas" } else { "/opt/atlas" });

    let b = atlas::safety::BackupConfig::default().resolved(&install);
    assert_eq!(PathBuf::from(&b.dir), install.join("data").join("backups"));

    let t = atlas::safety::TrashConfig::default().resolved(&install);
    assert_eq!(PathBuf::from(&t.dir), install.join("data").join("trash"));

    let r = atlas::research::ResearchConfig::default().resolved(&install);
    assert_eq!(PathBuf::from(&r.notes_dir), install.join("data").join("notes"));
}

#[test]
fn two_installs_never_share_a_trash_can() {
    // `TrashConfig` was the one of the three with no `resolved()` at all,
    // and three CLI commands used its bare default. Two installs — or two
    // tests — sharing a working directory shared one real trash folder.
    let a = atlas::safety::TrashConfig::default().resolved(Path::new("/tmp/install-a"));
    let b = atlas::safety::TrashConfig::default().resolved(Path::new("/tmp/install-b"));
    assert_ne!(a.dir, b.dir);
}

#[test]
fn what_atlas_promises_to_keep_includes_the_trash() {
    // `atlas update` preserves exactly what `upgrade::YOURS` names. The
    // trash holds the only copy of anything Atlas removed in the last 30
    // days and was not on the list, so an update destroyed it.
    let names: Vec<&str> = atlas::upgrade::YOURS.iter().map(|(p, _)| *p).collect();
    for must in ["data/state", "data/notes", "data/backups", "data/logs", "data/trash"] {
        assert!(names.contains(&must), "`atlas update` does not preserve {must}");
    }
}

#[test]
fn every_derived_folder_hangs_off_the_one_root() {
    // Each one asserted directly rather than through a sibling, because
    // `tests/dead_capabilities.rs` counts a helper nothing tests as deadness
    // — and it is right to: `data_sub` and `models_dir` reached by way of
    // `logs_dir` are two functions with one test between them, which is how a
    // wrong base goes unnoticed in whichever of the two nobody exercised.
    // `data_home`: the install root, or this test process's own folder
    // shaped like one (Q18) -- the shape is what's checked.
    let r = atlas::roots::data_home();
    assert_eq!(atlas::roots::data_dir(), r.join("data"));
    assert_eq!(atlas::roots::state_dir(), r.join("data").join("state"));
    assert_eq!(atlas::roots::logs_dir(), r.join("data").join("logs"));
    assert_eq!(atlas::roots::notes_dir(), r.join("data").join("notes"));
    assert_eq!(atlas::roots::backups_dir(), r.join("data").join("backups"));
    assert_eq!(atlas::roots::trash_dir(), r.join("data").join("trash"));
    assert_eq!(atlas::roots::tmp_dir(), r.join("data").join("tmp"));
    // Models are read, never written by a test: always the real install's.
    assert_eq!(atlas::roots::models_dir(), atlas::roots::install_root().join("models"));
    assert_eq!(atlas::roots::data_sub("finance"), r.join("data").join("finance"));
    // `models/` is a sibling of `data/`, not a child of it. The installer
    // downloads into `<install>/models`; a `data/models` would be 4GB of
    // model files somewhere nothing looks.
    assert_ne!(atlas::roots::models_dir(), atlas::roots::data_sub("models"));
}

#[test]
fn an_empty_folder_is_never_mistaken_for_an_install() {
    // The silent failure, asserted directly: Atlas must not decide it
    // already lives somewhere that holds nothing.
    let d = std::env::temp_dir().join(format!("atlas-guard-empty-{}", std::process::id()));
    let _ = fs::create_dir_all(&d);
    assert!(!atlas::roots::looks_like_an_install(&d));
    let _ = fs::remove_dir_all(&d);
}
