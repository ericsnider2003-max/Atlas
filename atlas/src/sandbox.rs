//! Where Atlas does its own work.
//!
//! For Atlas to write and test code, it needs somewhere to be wrong. A
//! sandbox is a scratch directory it owns completely: it can create, edit and
//! run things there, and **nothing reaches your machine until you say so.**
//!
//! Three properties carry the safety, and all three are enforced here rather
//! than left to whoever writes the next feature:
//!
//! 1. **Nothing is written outside the sandbox root.** A path that climbs out
//!    with `..` is rejected, not normalised and allowed.
//! 2. **Promotion requires an explicit yes**, per batch, showing what changes.
//! 3. **Anything replaced goes to trash first**, so accepting a bad change is
//!    undoable.

use crate::error::{AtlasError, Result};
use crate::safety::Trash;
use crate::store::now;
use crate::tools::{ExternalTool, Vars};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attempt {
    pub command: String,
    pub passed: bool,
    /// Trimmed: a compiler can produce megabytes and none of it is speakable.
    pub output: String,
    pub at: u64,
}

impl Attempt {
    /// The first line that looks like an error, for saying out loud.
    pub fn first_problem(&self) -> Option<String> {
        self.output
            .lines()
            .find(|l| {
                let t = l.trim_start().to_lowercase();
                t.starts_with("error") || t.starts_with("failed") || t.contains("panicked at")
            })
            .map(|l| l.trim().to_string())
    }
}

/// A file Atlas wants to put on your machine.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    /// Where it would land.
    pub target: PathBuf,
    /// Where it is now, inside the sandbox.
    pub source: PathBuf,
    pub new_file: bool,
    pub bytes: u64,
    /// What the target looked like when the plan was made.
    ///
    /// `Edit` promises "the whole new file, so what lands is exactly what was
    /// tested". That is the right property for the sandbox and the wrong one
    /// for landing: a whole-file copy silently wins against anything you
    /// changed in the meantime, where a patch would have refused.
    ///
    /// Overnight work makes the gap hours wide. Approval in the morning would
    /// have overwritten an evening's editing, and the old version going to
    /// trash makes it recoverable but not visible.
    pub target_was: Option<Fingerprint>,
}

/// Enough of a file to tell whether it is still the same one.
///
/// Length and modified time rather than a hash of the contents: this runs on
/// every planned change and a hash of a large file is not free. Both changing
/// together by coincidence is possible and vanishingly unlikely; either
/// changing at all is the signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fingerprint {
    pub len: u64,
    pub modified_secs: u64,
}

impl Change {
    /// Has the real file changed since this change was planned?
    ///
    /// A file that did not exist and now does counts too: something else
    /// created it, and writing over it would be the same loss.
    fn moved_under_us(&self) -> bool {
        Fingerprint::of(&self.target) != self.target_was
    }
}

impl Fingerprint {
    pub fn of(path: &std::path::Path) -> Option<Fingerprint> {
        let md = std::fs::metadata(path).ok()?;
        if !md.is_file() {
            return None;
        }
        let modified_secs = md
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Some(Fingerprint { len: md.len(), modified_secs })
    }
}

pub struct Sandbox {
    pub root: PathBuf,
    pub name: String,
    pub attempts: Vec<Attempt>,
}

impl Sandbox {
    pub fn create(base: &Path, name: &str) -> Result<Sandbox> {
        let safe: String = name
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' })
            .collect();
        let root = base.join(format!("{safe}-{}", now()));
        std::fs::create_dir_all(&root)?;
        Ok(Sandbox { root, name: safe, attempts: Vec::new() })
    }

    /// Resolve a path inside the sandbox, refusing anything that escapes.
    ///
    /// The check is on the components, before touching the filesystem —
    /// canonicalising first would follow a symlink straight out.
    pub fn resolve(&self, rel: &str) -> Result<PathBuf> {
        let p = Path::new(rel);
        if p.is_absolute() {
            return Err(AtlasError::Platform(format!("{rel} is outside the sandbox")));
        }
        for c in p.components() {
            match c {
                Component::Normal(_) | Component::CurDir => {}
                _ => {
                    return Err(AtlasError::Platform(format!(
                        "{rel} tries to climb out of the sandbox"
                    )))
                }
            }
        }
        Ok(self.root.join(p))
    }

    pub fn write(&self, rel: &str, content: &str) -> Result<PathBuf> {
        let p = self.resolve(rel)?;
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&p, content)?;
        Ok(p)
    }

    pub fn read(&self, rel: &str) -> Result<String> {
        Ok(std::fs::read_to_string(self.resolve(rel)?)?)
    }

    pub fn exists(&self, rel: &str) -> bool {
        self.resolve(rel).map(|p| p.exists()).unwrap_or(false)
    }

    pub fn files(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        collect(&self.root, &mut out);
        out.sort();
        out
    }

    /// Run something with the sandbox as its working directory.
    pub fn run(&mut self, tool: &ExternalTool, vars: &Vars, max_output: usize) -> Attempt {
        let (cmd, args) = tool.resolved(vars);
        let out = crate::tools::command(&cmd).args(&args).current_dir(&self.root).output();

        let attempt = match out {
            Ok(o) => {
                let mut text = String::from_utf8_lossy(&o.stdout).to_string();
                text.push_str(&String::from_utf8_lossy(&o.stderr));
                Attempt {
                    command: format!("{cmd} {}", args.join(" ")),
                    passed: o.status.success(),
                    output: trim_output(&text, max_output),
                    at: now(),
                }
            }
            Err(e) => Attempt {
                command: cmd.clone(),
                passed: false,
                output: format!("could not start {cmd}: {e}"),
                at: now(),
            },
        };
        self.attempts.push(attempt.clone());
        attempt
    }

    pub fn last(&self) -> Option<&Attempt> {
        self.attempts.last()
    }

    /// Did it end up working?
    pub fn settled(&self) -> bool {
        self.attempts.last().map(|a| a.passed).unwrap_or(false)
    }

    /// What would land on your machine, given a mapping from sandbox paths to
    /// real ones. Nothing is moved — this is the preview.
    pub fn plan(&self, mapping: &[(String, PathBuf)]) -> Result<Vec<Change>> {
        let mut out = Vec::new();
        for (rel, target) in mapping {
            let source = self.resolve(rel)?;
            if !source.is_file() {
                return Err(AtlasError::Platform(format!("{rel} was never written")));
            }
            out.push(Change {
                bytes: std::fs::metadata(&source).map(|m| m.len()).unwrap_or(0),
                new_file: !target.exists(),
                target_was: Fingerprint::of(target),
                target: target.clone(),
                source,
            });
        }
        Ok(out)
    }

    /// One line per change, for reading out before you agree.
    pub fn describe(changes: &[Change]) -> String {
        if changes.is_empty() {
            return "Nothing to apply.".into();
        }
        let new = changes.iter().filter(|c| c.new_file).count();
        let replaced = changes.len() - new;
        let names: Vec<String> = changes
            .iter()
            .take(3)
            .map(|c| {
                c.target.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
            })
            .collect();
        format!(
            "{} new, {} replaced: {}{}. Apply?",
            new,
            replaced,
            names.join(", "),
            if changes.len() > 3 { format!(" and {} more", changes.len() - 3) } else { String::new() }
        )
    }

    /// Copy the changes onto the machine.
    ///
    /// Refuses without an explicit yes, and puts anything it replaces in the
    /// trash first — so accepting a bad change is still undoable.
    pub fn promote(changes: &[Change], trash: &Trash, approved: bool) -> Result<usize> {
        if !approved {
            return Err(AtlasError::ApprovalRequired(
                "changes need your go-ahead before they touch anything".into(),
            ));
        }
        // Refuse the whole batch rather than landing the safe half. A
        // partially applied change set is harder to reason about than one
        // that did not happen, and you would have to work out which half.
        let moved: Vec<&Change> = changes.iter().filter(|c| c.moved_under_us()).collect();
        if let Some(first) = moved.first() {
            return Err(AtlasError::Platform(format!(
                "{} changed since I planned this{}. I haven't touched anything — \
                 re-run it against the file as it is now.",
                first.target.display(),
                if moved.len() > 1 {
                    format!(" (and {} other{})", moved.len() - 1, if moved.len() > 2 { "s" } else { "" })
                } else {
                    String::new()
                }
            )));
        }

        let mut n = 0;
        for c in changes {
            if c.target.exists() {
                trash.take(&c.target, "replaced by a change Atlas made")?;
            }
            if let Some(parent) = c.target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&c.source, &c.target)?;
            n += 1;
        }
        Ok(n)
    }

    /// Throw the whole thing away. Nothing outside the sandbox is touched.
    pub fn discard(self) -> Result<()> {
        std::fs::remove_dir_all(&self.root)?;
        Ok(())
    }
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// Compilers produce megabytes. Keep the head and the tail — the head has the
/// first error and the tail has the summary.
pub fn trim_output(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let head: String = text.chars().take(max * 2 / 3).collect();
    let tail: String = text.chars().rev().take(max / 3).collect::<String>().chars().rev().collect();
    format!("{head}\n...[{} characters omitted]...\n{tail}", text.len() - max)
}
