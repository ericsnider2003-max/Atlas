//! Handing code work to a coding agent installed on this computer (2 Oct
//! 2026): Claude Code (`claude`) or OpenAI's Codex (`codex`).
//!
//! The owner's complaint was "it can't seem to code still". Part of the
//! answer is a better loop for Atlas's own models (`build_it`); the other
//! part is that a coding agent already on the machine is far better at it
//! than a 4B model, and Atlas should use it when it's there rather than
//! pretend it isn't. Nothing here assumes one user: whether an agent is
//! installed is looked up on this machine's PATH, each time.
//!
//! What's used, from each one's own documentation (read 2 Oct 2026):
//!
//! - Claude Code: `claude -p` runs one task without its interactive screen
//!   and reads the task from stdin when it isn't given on the command line;
//!   `--permission-mode acceptEdits` lets it write files in the folder it's
//!   started in without asking, while other shell commands and network
//!   requests still need permission it won't get here; `--output-format
//!   text` prints just its final answer. It exits 0 on success.
//! - Codex: `codex exec` is its non-interactive mode; `--full-auto` lets it
//!   edit files without asking, inside its `workspace-write` sandbox (the
//!   folder it's started in); `--skip-git-repo-check` lets it work in a
//!   folder that isn't a git repository; `-` reads the task from stdin.
//!
//! The task goes in on stdin, never on the command line: a long description
//! with quotes in it is exactly what a Windows command line mangles.
//!
//! Atlas never trusts what comes back on the agent's word: its own checks
//! (`craft`'s ladder) run on the result before anything is said to work, the
//! same as for code its own models wrote. And before an agent touches one of
//! your projects you're asked, and a copy of the folder is kept first.

use std::path::{Path, PathBuf};

/// A coding agent Atlas knows how to drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agent {
    ClaudeCode,
    Codex,
}

impl Agent {
    /// In the order they're preferred when both are installed.
    pub const ALL: [Agent; 2] = [Agent::ClaudeCode, Agent::Codex];

    /// The program's name on the PATH.
    fn program(&self) -> &'static str {
        match self {
            Agent::ClaudeCode => "claude",
            Agent::Codex => "codex",
        }
    }

    /// How it's named to you.
    pub fn named(&self) -> &'static str {
        match self {
            Agent::ClaudeCode => "Claude Code",
            Agent::Codex => "Codex",
        }
    }

    /// The arguments for one task, read from stdin, in the folder it is
    /// started in. See the module's notes for where each comes from.
    pub fn args(&self) -> Vec<String> {
        let a: &[&str] = match self {
            Agent::ClaudeCode => &["-p", "--permission-mode", "acceptEdits", "--output-format", "text"],
            Agent::Codex => &["exec", "--full-auto", "--skip-git-repo-check", "-"],
        };
        a.iter().map(|s| s.to_string()).collect()
    }
}

/// The first coding agent installed here, and where its program is, with
/// how a program is looked up handed in: this machine's PATH
/// (`tools::which`) in Atlas, a stand-in in the tests.
pub fn installed_with(find: impl Fn(&str) -> Option<String>) -> Option<(Agent, String)> {
    Agent::ALL.iter().find_map(|a| find(a.program()).map(|p| (*a, p)))
}

/// The marker on a build or change you said yes to handing over.
pub const HAND_OVER: &str = "\u{2192}agent:";
/// The marker on one you said no to: Atlas's own models write it.
pub const OWN_MODELS: &str = "\u{2192}own:";

/// The words with a marker taken off, and which marker it was.
pub fn marked(what: &str) -> (Option<&'static str>, &str) {
    for m in [HAND_OVER, OWN_MODELS] {
        if let Some(rest) = what.strip_prefix(m) {
            return (Some(m), rest);
        }
    }
    (None, what)
}

/// The same request, for Atlas's own models: what a "no" to handing it over
/// means (it was the hand-over that was declined, not the work).
pub fn declined(intent: &crate::intent::Intent) -> Option<crate::intent::Intent> {
    use crate::intent::Intent;
    match intent {
        Intent::Build(w) => w.strip_prefix(HAND_OVER).map(|r| Intent::Build(format!("{OWN_MODELS}{r}"))),
        Intent::Improve(w) => w.strip_prefix(HAND_OVER).map(|r| Intent::Improve(format!("{OWN_MODELS}{r}"))),
        _ => None,
    }
}

/// What the agent is asked to do. Plain about the limits: this folder only,
/// the language, finished code rather than a plan, and a short account of
/// what it did -- which is read back to you.
pub fn task(what: &str, lang: crate::craft::Lang, folder: &Path, fresh: bool) -> String {
    let place = if fresh {
        format!("This is a new, empty folder ({}). Write the program here, as a single file if it fits in one.", folder.display())
    } else {
        format!("This is the user's existing project in {}. Change only what the request needs, in keeping with the code that's there.", folder.display())
    };
    format!(
        "{place}\n\nWhat to do: {what}\n\nWrite it in {} unless the request names another language. \
         Write finished, working code: no placeholders, no TODOs. Work only inside this folder. \
         Don't delete files you didn't create. When you're done, say in two or three plain \
         sentences what you changed and how to run it.",
        lang.plain()
    )
}

/// What happened when the agent ran.
#[derive(Debug, Clone, PartialEq)]
pub struct Ran {
    pub finished: bool,
    /// Its final words, trimmed to something that can be said.
    pub said: String,
}

/// Run an agent on a task in `folder`, for no longer than `limit_secs`.

/// One program, with `input` on its stdin, in `folder`, stopped at the limit
/// or when Atlas is asked to stop.

pub(crate) fn run_controlled(agent: Agent, program: &str, folder: &Path, task_text: &str, limit_secs: u64, stop: &dyn Fn() -> bool) -> Ran {
    run_with_stdin_controlled(program, &agent.args(), folder, task_text, limit_secs, Some(stop))
}

fn run_with_stdin_controlled(program: &str, args: &[String], folder: &Path, input: &str, limit_secs: u64, stop: Option<&dyn Fn() -> bool>) -> Ran {
    let mut command = crate::tools::command(program);
    command.args(args).current_dir(folder);
    let (finished, said) = crate::tools::run_scoped(&mut command,
        std::time::Duration::from_secs(limit_secs), 8 * 1024 * 1024, Some(input.into()), stop).said(2000);
    Ran { finished, said }
}

/// Folders never copied into a backup: what a build makes again.
const NOT_COPIED: &[&str] = &[".git", "target", "node_modules", ".venv", "venv", "__pycache__", "dist", "build", ".mypy_cache", ".ruff_cache"];

/// Keep a copy of a project folder before an agent changes it: every source
/// file within `most_bytes` in all, build output and `.git` left out
/// (git has its own history). Returns where the copy is, or why there isn't
/// one -- in which case the agent isn't run.
pub(crate) fn keep_a_copy_controlled(folder: &Path, into: &Path, most_bytes: u64, budget: Option<&crate::tools::WorkBudget<'_>>) -> Result<PathBuf, String> {
    fn walk(from: &Path, to: &Path, left: &mut u64, budget: Option<&crate::tools::WorkBudget<'_>>) -> Result<(), String> {
        if let Some(b) = budget { b.check()?; }
        std::fs::create_dir_all(to).map_err(|e| format!("couldn't make {}: {e}", to.display()))?;
        let entries = std::fs::read_dir(from).map_err(|e| format!("couldn't read {}: {e}", from.display()))?;
        for entry in entries {
            if let Some(b) = budget { b.check()?; }
            let entry = entry.map_err(|error| format!("project copy could not read an entry: {error}"))?;
            let name = entry.file_name();
            let path = entry.path();
            let meta = std::fs::symlink_metadata(&path).map_err(|error| format!("project copy could not inspect {}: {error}", path.display()))?;
            if meta.file_type().is_symlink() { return Err(format!("{} is a link; no complete recovery copy was made", path.display())); }
            if meta.is_dir() {
                if NOT_COPIED.iter().any(|n| name == *n) {
                    continue;
                }
                walk(&path, &to.join(&name), left, budget)?;
            } else if meta.is_file() {
                if meta.len() > *left {
                    return Err(format!("{} is bigger than I'll copy before letting an agent loose on it", from.display()));
                }
                use std::io::{Read, Write};
                let mut source = std::fs::File::open(&path).map_err(|error| error.to_string())?;
                let mut copy = std::fs::File::create(to.join(&name)).map_err(|error| error.to_string())?;
                let mut bytes = [0u8; 64 * 1024];
                loop {
                    if let Some(b) = budget { b.check()?; }
                    let read = source.read(&mut bytes).map_err(|error| error.to_string())?;
                    if read == 0 { break; }
                    if read as u64 > *left { return Err("The project grew beyond its recovery copy budget; the agent was not run.".into()); }
                    *left -= read as u64;
                    copy.write_all(&bytes[..read]).map_err(|error| error.to_string())?;
                }
                copy.sync_all().map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    }
    let mut left = most_bytes;
    walk(folder, into, &mut left, budget)?;
    Ok(into.to_path_buf())
}

/// The source files under a folder in one language, newest first, build
/// output left out -- what the agent wrote, for the checks to read.
pub fn files_in(folder: &Path, lang: crate::craft::Lang) -> Vec<PathBuf> {
    let mut found: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();
    let mut stack = vec![folder.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                if !NOT_COPIED.iter().any(|n| e.file_name() == *n) {
                    stack.push(p);
                }
            } else if crate::craft::Lang::of_path(&p.to_string_lossy()) == Some(lang) {
                let when = e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
                found.push((p, when));
            }
        }
    }
    found.sort_by_key(|b| std::cmp::Reverse(b.1));
    found.into_iter().map(|(p, _)| p).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_code_is_preferred_and_codex_is_found_alone() {
        let both = installed_with(|p| Some(format!("/bin/{p}")));
        assert_eq!(both.map(|(a, _)| a), Some(Agent::ClaudeCode));
        let codex = installed_with(|p| (p == "codex").then(|| "/bin/codex".to_string()));
        assert_eq!(codex.map(|(a, _)| a), Some(Agent::Codex));
        assert!(installed_with(|_| None).is_none());
    }

    #[test]
    fn each_agent_runs_without_its_screen_and_reads_the_task_from_stdin() {
        let c = Agent::ClaudeCode.args();
        assert!(c.contains(&"-p".to_string()) && c.contains(&"acceptEdits".to_string()));
        let x = Agent::Codex.args();
        assert_eq!(x.first().map(String::as_str), Some("exec"));
        assert_eq!(x.last().map(String::as_str), Some("-"), "codex reads the task from stdin");
    }

    #[test]
    fn a_no_to_the_hand_over_is_a_yes_to_atlas_doing_it() {
        use crate::intent::Intent;
        let asked = Intent::Build(format!("{HAND_OVER}a script that renames photos"));
        assert_eq!(declined(&asked), Some(Intent::Build(format!("{OWN_MODELS}a script that renames photos"))));
        assert_eq!(declined(&Intent::Build("plain".into())), None);
        assert_eq!(marked(&format!("{OWN_MODELS}x")), (Some(OWN_MODELS), "x"));
    }

    #[test]
    fn recovery_copy_includes_large_files_and_stops_before_start() {
        let root = std::env::temp_dir().join(format!("atlas-agent-controlled-copy-{}", std::process::id()));
        let src = root.join("source"); std::fs::create_dir_all(&src).unwrap();
        let bytes = vec![13u8; 2 * 1024 * 1024 + 17];
        std::fs::write(src.join("large.data"), &bytes).unwrap();
        let kept = keep_a_copy_controlled(&src, &root.join("complete"), bytes.len() as u64, None).unwrap();
        assert_eq!(std::fs::read(kept.join("large.data")).unwrap(), bytes);
        let stopped = || true;
        let budget = crate::tools::WorkBudget::new(std::time::Duration::from_secs(10), &stopped);
        assert!(keep_a_copy_controlled(&src, &root.join("canceled"), 10_000_000, Some(&budget)).is_err());
        assert!(!root.join("canceled").exists());
        assert_eq!(std::fs::read(src.join("large.data")).unwrap(), bytes);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_copy_leaves_out_build_output_and_refuses_a_folder_too_big() {
        let root = std::env::temp_dir().join(format!("atlas-agent-copy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let src = root.join("proj");
        std::fs::create_dir_all(src.join("target")).unwrap();
        std::fs::create_dir_all(src.join("src")).unwrap();
        std::fs::write(src.join("src/main.py"), "print(1)\n").unwrap();
        std::fs::write(src.join("target/big.bin"), vec![0u8; 100]).unwrap();
        let kept = keep_a_copy_controlled(&src, &root.join("copy"), 1_000_000, None).unwrap();
        assert!(kept.join("src/main.py").is_file());
        assert!(!kept.join("target").exists());
        assert!(keep_a_copy_controlled(&src, &root.join("copy2"), 3, None).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
