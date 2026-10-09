//! Mutation testing: does any test notice when the code is broken on purpose?
//!
//! Research report, 30 Sep 2026, Stage 2 item 12. Atlas's chronic fault is
//! code that compiles, passes its tests and does nothing. A test that passes
//! whether or not the code under it works can't catch that, and nothing ran
//! the code broken to find out: `selfaudit::Kind::NeverFailed` ("the test has
//! never once failed") was defined, weighted and phrased, with no producer.
//!
//! cargo-mutants (MIT, sourcefrog/cargo-mutants) is that producer. It
//! rewrites one function at a time -- `Default::default()`, `true`, `Ok(())`,
//! a flipped operator -- and runs the tests against each. A mutant that
//! *survives* is code no test depends on. This module:
//!
//! * reads its results (`outcomes.json`, else `missed.txt`) -- pure, tested;
//! * turns survivors into `NeverFailed` signals (`signals::from_survivors`);
//! * writes the diff that scopes a run to the lines a self-fix changed
//!   (`--in-diff`), so a fix whose new lines no test checks doesn't land
//!   (`Daemon::prove_in_a_copy`).
//!
//! cargo-mutants is a developer tool, installed with `cargo install
//! cargo-mutants`; it's only used where Atlas works on its own source. When
//! it isn't installed, the check says it didn't run -- never that it passed.

use serde::{Deserialize, Serialize};

/// One mutant no test caught.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Survivor {
    pub file: String,
    pub function: String,
    pub line: u32,
    /// What the body or expression was replaced with.
    pub replacement: String,
}

impl Survivor {
    /// "src/x.rs::total (line 12): replaced with 0".
    pub fn said(&self) -> String {
        let f = if self.function.is_empty() { String::new() } else { format!("::{}", self.function) };
        format!("{}{f} (line {}): replaced with {}", self.file, self.line, self.replacement)
    }
}

/// Where cargo-mutants writes its results under its output folder.
fn results_dir(out: &std::path::Path) -> std::path::PathBuf {
    let inner = out.join("mutants.out");
    if inner.is_dir() { inner } else { out.to_path_buf() }
}

/// The survivors from a run's output folder: `outcomes.json` when it reads,
/// `missed.txt` otherwise.
pub fn read_survivors(out: &std::path::Path) -> Vec<Survivor> {
    let dir = results_dir(out);
    if let Ok(json) = std::fs::read_to_string(dir.join("outcomes.json")) {
        if let Some(s) = from_outcomes_json(&json) {
            return s;
        }
    }
    std::fs::read_to_string(dir.join("missed.txt")).map(|t| from_missed_txt(&t)).unwrap_or_default()
}

/// Survivors from `outcomes.json`: every outcome whose summary is
/// `MissedMutant`. Read loosely -- the field names have moved between
/// versions -- and `None` when it isn't that file at all.
pub fn from_outcomes_json(json: &str) -> Option<Vec<Survivor>> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let outcomes = v.get("outcomes")?.as_array()?;
    let mut out = Vec::new();
    for o in outcomes {
        if o.get("summary").and_then(|s| s.as_str()) != Some("MissedMutant") {
            continue;
        }
        let m = o.get("scenario").and_then(|s| s.get("Mutant")).unwrap_or(&serde_json::Value::Null);
        let file = m.get("file").and_then(|f| f.as_str()).unwrap_or("").to_string();
        let function = m
            .get("function")
            .and_then(|f| f.get("function_name").and_then(|n| n.as_str()).or_else(|| f.as_str()))
            .unwrap_or("")
            .to_string();
        let line = m
            .get("span")
            .and_then(|s| s.get("start"))
            .and_then(|s| s.get("line"))
            .and_then(|l| l.as_u64())
            .unwrap_or(0) as u32;
        let replacement = m.get("replacement").and_then(|r| r.as_str()).unwrap_or("").to_string();
        if !file.is_empty() {
            out.push(Survivor { file, function, line, replacement });
        }
    }
    Some(out)
}

/// Survivors from `missed.txt`: one per line, `src/x.rs:12:5: replace total
/// -> u32 with 0`.
pub fn from_missed_txt(text: &str) -> Vec<Survivor> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            let parts: Vec<&str> = l.splitn(4, ':').collect();
            if parts.len() < 3 {
                return None;
            }
            let file = parts[0].trim().to_string();
            let line: u32 = parts[1].trim().parse().ok()?;
            let rest = parts[parts.len() - 1].trim();
            let rest = rest.strip_prefix("replace ").unwrap_or(rest);
            let (what, with) = rest.split_once(" with ").unwrap_or((rest, ""));
            let function = what.split(" -> ").next().unwrap_or(what).trim().to_string();
            Some(Survivor { file, function, line, replacement: with.trim().to_string() })
        })
        .collect()
}

/// A unified diff of one file's change, for `cargo mutants --in-diff`: one
/// hunk from the first line that differs to the last.
pub fn diff_of(path: &str, before: &str, after: &str) -> String {
    let a: Vec<&str> = before.lines().collect();
    let b: Vec<&str> = after.lines().collect();
    if a == b {
        return String::new();
    }
    let head = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let tail = a[head..].iter().rev().zip(b[head..].iter().rev()).take_while(|(x, y)| x == y).count();
    let old = &a[head..a.len() - tail];
    let new = &b[head..b.len() - tail];
    let mut d = format!("--- a/{path}\n+++ b/{path}\n@@ -{},{} +{},{} @@\n", head + 1, old.len(), head + 1, new.len());
    for l in old {
        d.push_str(&format!("-{l}\n"));
    }
    for l in new {
        d.push_str(&format!("+{l}\n"));
    }
    d
}

/// Is cargo-mutants installed where `cargo` runs?
pub fn available(root: &std::path::Path) -> bool { available_controlled(root, None) }

fn available_controlled(root: &std::path::Path, budget: Option<&crate::tools::WorkBudget<'_>>) -> bool {
    let mut c = crate::tools::command("cargo");
    c.args(["mutants", "--version"]).current_dir(root);
    let stop = || budget.is_some_and(|b| b.stopping());
    let cap = std::time::Duration::from_secs(10);
    crate::tools::run_scoped(&mut c, budget.map_or(cap, |b| b.remaining(cap)), 64 * 1024, None, Some(&stop)).said(1000).0
}

/// How long a diff-scoped run may take.
pub const RUN_LIMIT_SECS: u64 = 30 * 60;

/// What a diff-scoped run found.
#[derive(Debug, Clone, PartialEq)]
pub enum Checked {
    /// Every mutant in the changed lines was caught.
    AllCaught,
    /// These weren't.
    Survivors(Vec<Survivor>),
    /// It didn't run, and why. Never read as a pass.
    NotRun(String),
}

/// Mutate only the lines in `diff`, in the sandbox copy at `root`.

pub(crate) fn check_diff_controlled(root: &std::path::Path, diff: &str, budget: Option<&crate::tools::WorkBudget<'_>>) -> Checked {
    if let Some(b) = budget { if let Err(why) = b.check() { return Checked::NotRun(why); } }
    if diff.trim().is_empty() {
        return Checked::AllCaught;
    }
    if !available_controlled(root, budget) {
        return Checked::NotRun("cargo-mutants isn't installed (`cargo install cargo-mutants`)".into());
    }
    let diff_file = root.join("atlas-change.diff");
    if let Err(e) = std::fs::write(&diff_file, diff) {
        return Checked::NotRun(format!("couldn't write the diff: {e}"));
    }
    let out = root.join("atlas-mutants");
    let tool = crate::tools::ExternalTool {
        command: "cargo".into(),
        args: vec![
            "mutants".into(),
            "--in-diff".into(),
            diff_file.display().to_string(),
            "--no-shuffle".into(),
            "--jobs".into(),
            "2".into(),
            "--timeout".into(),
            "120".into(),
            "--output".into(),
            out.display().to_string(),
        ],
        timeout_secs: budget.map_or(RUN_LIMIT_SECS, |b| b.remaining(std::time::Duration::from_secs(RUN_LIMIT_SECS)).as_secs().max(1)),
        ..Default::default()
    };
    let mut sb = crate::sandbox::Sandbox { root: root.to_path_buf(), name: "mutants".into(), attempts: Vec::new() };
    let stop = || budget.is_some_and(|b| b.stopping());
    let a = sb.run_controlled(&tool, &Default::default(), 4000, Some(&stop));
    if let Some(b) = budget { if let Err(why) = b.check() { return Checked::NotRun(why); } }
    if a.output.contains("stopped after") || a.output.contains("could not start") {
        return Checked::NotRun(a.output.lines().next().unwrap_or("it didn't finish").to_string());
    }
    let survivors = read_survivors(&out);
    // cargo-mutants exits non-zero when anything survived; zero with none.
    if survivors.is_empty() {
        if a.passed { Checked::AllCaught } else { Checked::NotRun(a.first_problem().unwrap_or_else(|| "it failed without saying why".into())) }
    } else {
        Checked::Survivors(survivors)
    }
}

/// Where the last run's survivors are kept, for `signals::from_survivors`.
pub const KEPT: &str = "mutation_survivors";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missed_mutants_are_read_from_outcomes_json() {
        let json = r#"{"outcomes":[
            {"scenario":"Baseline","summary":"Success"},
            {"scenario":{"Mutant":{"file":"src/a.rs","function":{"function_name":"total"},"span":{"start":{"line":12,"column":5}},"replacement":"0","genre":"FnValue"}},"summary":"MissedMutant"},
            {"scenario":{"Mutant":{"file":"src/a.rs","function":{"function_name":"ok"},"span":{"start":{"line":30,"column":5}},"replacement":"true"}},"summary":"CaughtMutant"}
        ]}"#;
        let s = from_outcomes_json(json).unwrap();
        assert_eq!(s, vec![Survivor { file: "src/a.rs".into(), function: "total".into(), line: 12, replacement: "0".into() }]);
        assert_eq!(s[0].said(), "src/a.rs::total (line 12): replaced with 0");
    }

    #[test]
    fn missed_txt_is_read_when_the_json_isnt_there() {
        let s = from_missed_txt("src/b.rs:7:9: replace is_ready -> bool with true\n\n");
        assert_eq!(s, vec![Survivor { file: "src/b.rs".into(), function: "is_ready".into(), line: 7, replacement: "true".into() }]);
    }

    #[test]
    fn the_diff_covers_only_the_lines_that_changed() {
        let d = diff_of("src/x.rs", "a\nb\nc\nd\n", "a\nB\nc\nd\n");
        assert_eq!(d, "--- a/src/x.rs\n+++ b/src/x.rs\n@@ -2,1 +2,1 @@\n-b\n+B\n");
        assert!(diff_of("src/x.rs", "same\n", "same\n").is_empty());
    }
}
