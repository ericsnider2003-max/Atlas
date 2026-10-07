//! The bookkeeping for a new ability, written by Atlas rather than by hand
//! (6 Oct 2026).
//!
//! Adding a module to Atlas means five edits in four files before a line of
//! its own code: the module file, its `pub mod` line, its entry in the
//! catalogue (`capability::all`), the module count the catalogue pins, its
//! reason for being unwired yet (`tests/capability_wiring.rs`), and the
//! regenerated `docs/CAPABILITIES.md`. Miss one and a guard fails -- which
//! is right, and is also how a whole session's time went on bookkeeping.
//! None of it needs judgement, so none of it should need a model.
//!
//! What this writes is honest by construction: the catalogue entry says
//! `Planned` ("not built yet"), the module holds the request in your words
//! and nothing that pretends to work, and the guards pass because the tree
//! now says exactly what is true. The work that's left -- the code that
//! does it, a test that proves it, wiring it in, and moving it off
//! `Planned` -- is said, not hidden.
//!
//! `plan` decides every change from the files' current text and touches
//! nothing; `apply` writes them. So what it would do is testable without a
//! tree, and can be shown before it's done.

use std::path::Path;

/// What's being added.
#[derive(Debug, Clone, PartialEq)]
pub struct Ask {
    /// The module and capability name: `snake_case`.
    pub id: String,
    /// What it should do, in the words it was asked for in.
    pub what: String,
    /// The day it was asked for, as written in comments ("6 Oct 2026").
    pub day: String,
}

/// One file as it is and as it would be.
#[derive(Debug, Clone, PartialEq)]
pub struct FileChange {
    /// Relative to the crate (`src/lib.rs`).
    pub path: String,
    /// `None`: a new file.
    pub before: Option<String>,
    pub after: String,
}

/// The files the plan reads and changes, relative to the crate.
pub const LIB: &str = "src/lib.rs";
pub const CATALOGUE: &str = "src/capability.rs";
pub const UNWIRED: &str = "tests/capability_wiring.rs";
pub const WIRING: &str = "tests/wiring.rs";

const KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn", "for", "if", "impl", "in",
    "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self", "static", "struct", "super", "trait", "true", "type",
    "unsafe", "use", "where", "while", "yield", "main", "lib", "tests",
];

/// A name a module and a capability can both have: lower case, starting
/// with a letter, letters digits and underscores, 3 to 32 long, not a Rust
/// word.
pub fn fit_name(id: &str) -> Result<(), String> {
    let ok = (3..=32).contains(&id.len())
        && id.starts_with(|c: char| c.is_ascii_lowercase())
        && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && !id.ends_with('_')
        && !id.contains("__");
    if !ok {
        return Err(format!("\"{id}\" can't be a module name: lower case letters, digits and single underscores, 3 to 32 long"));
    }
    if KEYWORDS.contains(&id) {
        return Err(format!("\"{id}\" is a word Rust keeps for itself"));
    }
    Ok(())
}

/// A name for what was asked: its first few meaningful words, joined.
/// "read my texts out loud" -> `read_texts_out_loud`.
pub fn name_for(what: &str) -> String {
    const SKIP: &[&str] = &["a", "an", "the", "my", "me", "i", "you", "your", "to", "of", "for", "and", "it", "that", "this", "when", "with", "on", "in"];
    let words: Vec<String> = what
        .to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty() && !SKIP.contains(w))
        .take(4)
        .map(str::to_string)
        .collect();
    let mut id = words.join("_");
    if id.starts_with(|c: char| c.is_ascii_digit()) {
        id.insert_str(0, "do_");
    }
    id.truncate(32);
    id.trim_end_matches('_').to_string()
}

fn rust_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Every change adding `ask` takes, from the current text of the files
/// (`read`, by crate-relative path). Nothing is written.
pub fn plan(ask: &Ask, read: impl Fn(&str) -> Option<String>) -> Result<Vec<FileChange>, String> {
    fit_name(&ask.id)?;
    let what = one_line(&ask.what);
    if what.is_empty() {
        return Err("say what it should do".into());
    }
    let module = format!("src/{}.rs", ask.id);
    if read(&module).is_some() {
        return Err(format!("{module} is already there"));
    }
    let lib = read(LIB).ok_or("src/lib.rs isn't there -- is this Atlas's source?")?;
    let cat = read(CATALOGUE).ok_or("src/capability.rs isn't there -- is this Atlas's source?")?;
    let unwired = read(UNWIRED).ok_or("tests/capability_wiring.rs isn't there -- is this Atlas's source?")?;
    let wiring = read(WIRING).ok_or("tests/wiring.rs isn't there -- is this Atlas's source?")?;
    if cat.contains(&format!("id: \"{}\"", ask.id)) {
        return Err(format!("there's already a capability called {}", ask.id));
    }
    if lib.lines().any(|l| l.trim() == format!("pub mod {};", ask.id) || l.trim() == format!("mod {};", ask.id)) {
        return Err(format!("src/lib.rs already has a module called {}", ask.id));
    }

    // The module: the request, and nothing that pretends to do it.
    let file = format!(
        "//! {what}\n//!\n//! Asked for {day}. Planned: nothing in the running program reaches this\n//! yet, and its catalogue entry says so (`capability::all`, \"not built\n//! yet\"). Written by `scaffold`, which did the bookkeeping and nothing else.\n//!\n//! Left to do, in order: the code that does it, here; a test that fails\n//! without it; wiring it in where it's asked for; then its catalogue entry\n//! off `Planned` and its line in `tests/capability_wiring.rs`'s\n//! `CAPABILITY_UNWIRED` taken out.\n",
        day = ask.day
    );

    // lib.rs: after the last `pub mod` line.
    // (A line, not "\npub mod": the first line of the file is one too.)
    let mut end = None;
    let mut offset = 0;
    for line in lib.split_inclusive('\n') {
        offset += line.len();
        if line.starts_with("pub mod ") {
            end = Some(offset);
        }
    }
    let end = end.ok_or("src/lib.rs has no `pub mod` lines")?;
    let mut new_lib = lib.clone();
    new_lib.insert_str(end, &format!("pub mod {};\n", ask.id));

    // The catalogue: an entry at the end of `all()`, and the pinned count.
    let start = cat.find("pub fn all() -> Vec<Capability> {").ok_or("capability::all() isn't where it was")?;
    let close = cat[start..].find("\n    ]\n").map(|i| start + i + 1).ok_or("the end of capability::all() isn't where it was")?;
    let added = cat[start..close]
        .split("added: ")
        .skip(1)
        .filter_map(|s| s.split(|c: char| !c.is_ascii_digit()).next()?.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    let entry = format!(
        "        Capability {{ id: \"{id}\", what: \"{w}\", area: Itself, state: Planned, needs: Some(\"building -- asked for {day}\"), offline: true, added: {n}, runs: &[Needs::JustThinking], modules: &[\"{id}\"] }},\n",
        id = ask.id,
        w = rust_str(&what),
        day = ask.day,
        n = added + 1
    );
    let mut new_cat = cat.clone();
    new_cat.insert_str(close, &entry);
    let pin = "pub const MODULES_IN_TREE: usize = ";
    let p = new_cat.find(pin).ok_or("MODULES_IN_TREE isn't where it was")?;
    let num_end = new_cat[p + pin.len()..].find(';').map(|i| p + pin.len() + i).ok_or("MODULES_IN_TREE has no end")?;
    let n: usize = new_cat[p + pin.len()..num_end].trim().parse().map_err(|_| "MODULES_IN_TREE isn't a number")?;
    new_cat.replace_range(p..num_end + 1, &format!("// {}: + `{}` (scaffolded, planned) = {}.\n{pin}{};", ask.day, ask.id, n + 1, n + 1));

    // Why it's unwired: one line, so the honesty guard hears the reason.
    let head = "const CAPABILITY_UNWIRED: &[&str] = &[\n";
    let u = unwired.find(head).ok_or("CAPABILITY_UNWIRED isn't where it was")? + head.len();
    let mut new_unwired = unwired.clone();
    new_unwired.insert_str(
        u,
        &format!("    // {}: `{}` -- asked for, scaffolded, not built yet: \"{}\".\n    \"{}\",\n", ask.day, ask.id, rust_str(&what).replace('\n', " "), ask.id),
    );

    // And for the reachability guard: a module nothing calls yet, on purpose.
    let head = "const UNWIRED_BASELINE: &[&str] = &[\n";
    let w = wiring.find(head).ok_or("UNWIRED_BASELINE isn't where it was")? + head.len();
    let mut new_wiring = wiring.clone();
    new_wiring.insert_str(w, &format!("    // {}: scaffolded, not built yet -- see its catalogue entry.\n    \"{}\",\n", ask.day, ask.id));

    Ok(vec![
        FileChange { path: module, before: None, after: file },
        FileChange { path: WIRING.into(), before: Some(wiring), after: new_wiring },
        FileChange { path: LIB.into(), before: Some(lib), after: new_lib },
        FileChange { path: CATALOGUE.into(), before: Some(cat), after: new_cat },
        FileChange { path: UNWIRED.into(), before: Some(unwired), after: new_unwired },
    ])
}

/// Write a plan into the crate at `root`. A file that changed since the plan
/// read it is refused, and nothing is written.
pub fn apply(root: &Path, changes: &[FileChange]) -> Result<(), String> {
    for c in changes {
        let now = std::fs::read_to_string(root.join(&c.path)).ok();
        if now != c.before {
            return Err(format!("{} changed since I read it -- nothing written", c.path));
        }
    }
    for c in changes {
        let p = root.join(&c.path);
        std::fs::write(&p, &c.after).map_err(|e| format!("couldn't write {}: {e}", c.path))?;
    }
    Ok(())
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = crate::tools::command("git")
        // The worktree holds the files exactly as committed (6 Oct 2026): with
        // Git for Windows' core.autocrlf=true they came out with CRLF endings,
        // the edits below look for `\n` and found nothing, and "set that
        // ability up" failed on Windows every time.
        .args(["-c", "user.name=Atlas", "-c", "user.email=atlas@localhost", "-c", "commit.gpgsign=false", "-c", "core.autocrlf=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| format!("couldn't run git: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The bookkeeping, committed on a branch of its own (`atlas-ability-<id>`)
/// in the source at `repo`, through a worktree of its own so the checkout
/// you work in never moves. Never main, never pushed. The branch's name.
pub fn on_a_branch(repo: &Path, ask: &Ask) -> Result<String, String> {
    let branch = format!("atlas-ability-{}", ask.id.replace('_', "-"));
    let top = std::path::PathBuf::from(git(repo, &["rev-parse", "--show-toplevel"])?);
    // Where the crate sits inside the repository (Atlas's is `atlas/`).
    let inner = repo
        .canonicalize()
        .ok()
        .and_then(|r| top.canonicalize().ok().and_then(|t| r.strip_prefix(t).ok().map(|p| p.to_path_buf())))
        .unwrap_or_default();
    let base = crate::roots::tmp_dir().join("selffix");
    std::fs::create_dir_all(&base).map_err(|e| format!("couldn't make room to work in: {e}"))?;
    let dir = base.join(format!("scaffold-{}", ask.id));
    if dir.exists() {
        let _ = git(repo, &["worktree", "remove", "--force", &dir.to_string_lossy()]); // unheard-ok: a leftover from an earlier try; the add below says if it's still in the way
        let _ = std::fs::remove_dir_all(&dir); // unheard-ok: as above
    }
    git(repo, &["worktree", "add", "-b", &branch, &dir.to_string_lossy(), "HEAD"])?;
    let crate_dir = dir.join(&inner);
    let done = (|| {
        let changes = plan(ask, |p| std::fs::read_to_string(crate_dir.join(p)).ok())?;
        apply(&crate_dir, &changes)?;
        git(&dir, &["add", "-A"])?;
        git(&dir, &["commit", "-q", "-m", &format!("Scaffold `{}`: {}\n\nThe bookkeeping only (scaffold): not built yet.", ask.id, one_line(&ask.what))])?;
        Ok::<(), String>(())
    })();
    let _ = git(repo, &["worktree", "remove", "--force", &dir.to_string_lossy()]); // unheard-ok: the branch holds the work; the folder was only a place to write it
    if let Err(e) = done {
        let _ = git(repo, &["branch", "-D", &branch]); // unheard-ok: an empty branch from a failed try, removed so the next try can use the name
        return Err(e);
    }
    Ok(branch)
}

/// What's left after the bookkeeping, said plainly.
pub fn left_to_do(ask: &Ask) -> String {
    format!(
        "Set up `{id}`: src/{id}.rs, its line in src/lib.rs, a catalogue entry marked not built yet, its reasons in \
         tests/capability_wiring.rs and tests/wiring.rs, and the module count. Left: the code that does it (\"{what}\"), a test that fails without it, wiring it \
         in, then moving it off Planned and out of both lists.",
        id = ask.id,
        what = one_line(&ask.what)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn tree() -> HashMap<String, String> {
        let mut t = HashMap::new();
        t.insert(LIB.into(), "//! Atlas\npub mod alpha;\npub mod beta;\n\n#[cfg(test)]\nmod x;\n".into());
        t.insert(
            CATALOGUE.into(),
            "pub fn all() -> Vec<Capability> {\n    vec![\n        Capability { id: \"alpha\", added: 3, modules: &[\"alpha\"] },\n        Capability { id: \"beta\", added: 7, modules: &[\"beta\"] },\n    ]\n}\n\npub const MODULES_IN_TREE: usize = 2;\n".into(),
        );
        t.insert(UNWIRED.into(), "const CAPABILITY_UNWIRED: &[&str] = &[\n    \"mesh\",\n];\n".into());
        t.insert(WIRING.into(), "const UNWIRED_BASELINE: &[&str] = &[\n    \"old\",\n];\n".into());
        t
    }

    fn ask(id: &str) -> Ask {
        Ask { id: id.into(), what: "read my \"texts\" out loud".into(), day: "6 Oct 2026".into() }
    }

    #[test]
    fn every_piece_of_bookkeeping_is_planned_and_nothing_written() {
        let t = tree();
        let p = plan(&ask("read_texts_out_loud"), |path| t.get(path).cloned()).unwrap();
        let get = |path: &str| p.iter().find(|c| c.path == path).unwrap().after.clone();
        assert!(get("src/read_texts_out_loud.rs").contains("read my \"texts\" out loud"));
        assert!(get(LIB).contains("pub mod beta;\npub mod read_texts_out_loud;\n"), "after the last pub mod: {}", get(LIB));
        let cat = get(CATALOGUE);
        assert!(cat.contains("id: \"read_texts_out_loud\", what: \"read my \\\"texts\\\" out loud\", area: Itself, state: Planned"), "{cat}");
        assert!(cat.contains("added: 8,"), "one past the newest: {cat}");
        assert!(cat.contains("pub const MODULES_IN_TREE: usize = 3;"), "{cat}");
        let entry_at = cat.find("read_texts_out_loud").unwrap();
        assert!(entry_at < cat.find("\n    ]\n").unwrap(), "inside all(): {cat}");
        assert!(get(UNWIRED).starts_with("const CAPABILITY_UNWIRED: &[&str] = &[\n    // 6 Oct 2026: `read_texts_out_loud`"));
        assert!(get(WIRING).contains("\"read_texts_out_loud\",\n    \"old\""));
        // Nothing pretends to work.
        assert!(!get("src/read_texts_out_loud.rs").contains("fn "), "a scaffold has no function to look alive");
    }

    #[test]
    fn a_name_that_cant_be_a_module_or_is_taken_is_refused() {
        let t = tree();
        assert!(plan(&ask("Bad-Name"), |p| t.get(p).cloned()).is_err());
        assert!(plan(&ask("type"), |p| t.get(p).cloned()).is_err());
        assert!(plan(&ask("alpha"), |p| t.get(p).cloned()).unwrap_err().contains("already"));
        let mut with_file = tree();
        with_file.insert("src/gamma.rs".into(), String::new());
        assert!(plan(&ask("gamma"), |p| with_file.get(p).cloned()).unwrap_err().contains("already there"));
    }

    #[test]
    fn a_name_comes_from_the_words_asked_for() {
        assert_eq!(name_for("read my texts out loud"), "read_texts_out_loud");
        assert_eq!(name_for("Track when I go to the gym!"), "track_go_gym");
        assert!(fit_name(&name_for("3D print a part")).is_ok());
    }

    #[test]
    fn a_file_changed_since_the_plan_is_not_written_over() {
        let dir = std::env::temp_dir().join(format!("atlas-scaffold-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (p, text) in tree() {
            let f = dir.join(&p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, text).unwrap();
        }
        let p = plan(&ask("read_texts"), |path| std::fs::read_to_string(dir.join(path)).ok()).unwrap();
        std::fs::write(dir.join(LIB), "changed meanwhile").unwrap();
        assert!(apply(&dir, &p).unwrap_err().contains("changed since"));
        assert!(!dir.join("src/read_texts.rs").exists(), "nothing written");
        std::fs::write(dir.join(LIB), &tree()[LIB]).unwrap();
        let p = plan(&ask("read_texts"), |path| std::fs::read_to_string(dir.join(path)).ok()).unwrap();
        apply(&dir, &p).unwrap();
        assert!(dir.join("src/read_texts.rs").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
