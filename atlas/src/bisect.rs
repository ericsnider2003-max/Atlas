//! What broke it: the change that made a passing test fail (5 Oct 2026).
//!
//! Most of what goes wrong in Atlas is a regression -- it worked, then a
//! change stopped it working -- and the history already knows which change.
//! This finds it the way `git bisect` does, with no model and nothing from
//! outside the machine: the test run at older and older commits until one
//! passes, then halved between the last pass and the first fail.
//!
//! Done in a git worktree of its own (`tmp/selffix/bisect`, the self-work's
//! folder, which the hourly sweep leaves alone), so the checkout you work in
//! is never moved. Cargo builds share the warm cache (`roots::build_cache`),
//! so each step is an incremental build, not a cold one.
//!
//! Along the first-parent line only: a merge counts as one change, which is
//! how Atlas's history is made (each chat's work lands as one merge), and it
//! keeps every commit tried a whole, buildable state of main.
//!
//! A commit that doesn't build can't say whether the test passes. It is
//! skipped, a neighbour tried, and if a whole range can't be built the answer
//! names the range instead of pretending to a single commit.

use std::path::Path;

/// What one run of the test said about one commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Passes,
    Fails,
    /// It couldn't be run there (it doesn't build, the test isn't there yet).
    CantTell(String),
}

/// The change, and how it was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub commit: String,
    /// The last commit before it where the test passed.
    pub last_good: String,
    /// How many commits were tried.
    pub tried: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Found(Change),
    /// It passes at the newest commit: nothing is broken here.
    PassesNow,
    /// It fails as far back as was looked.
    NeverPassed { looked_back: usize },
    /// It broke somewhere in a run of commits none of which would build, or
    /// the newest couldn't be tested at all.
    Unclear(String),
}

/// Find the first failing commit in `commits` (oldest first), asking
/// `verdict` about one commit at a time. The search, without git: what the
/// tests drive directly.
pub fn find_first_bad(commits: &[String], mut verdict: impl FnMut(&str) -> Verdict) -> Outcome {
    let Some(newest) = commits.len().checked_sub(1) else {
        return Outcome::Unclear("there are no commits to look at".into());
    };
    let mut tried = 0usize;
    let mut ask = |i: usize, tried: &mut usize| {
        *tried += 1;
        verdict(&commits[i])
    };
    match ask(newest, &mut tried) {
        Verdict::Passes => return Outcome::PassesNow,
        Verdict::CantTell(why) => return Outcome::Unclear(format!("the newest commit can't be tested: {why}")),
        Verdict::Fails => {}
    }
    // Back in doubling steps until it passes: the break is usually recent,
    // and this finds a good commit in a handful of runs either way.
    let mut bad = newest;
    let mut step = 1usize;
    let good = loop {
        if bad == 0 {
            return Outcome::NeverPassed { looked_back: commits.len() };
        }
        let i = bad.saturating_sub(step);
        match ask(i, &mut tried) {
            Verdict::Passes => break i,
            Verdict::Fails => bad = i,
            // Can't build there: keep going back past it.
            Verdict::CantTell(_) => {}
        }
        if i == 0 {
            return Outcome::NeverPassed { looked_back: commits.len() };
        }
        step = step.saturating_mul(2);
    };
    // Halve between them: `good` passes, `bad` fails, the first failure is
    // in (good, bad].
    let (mut good, mut bad) = (good, bad);
    while bad - good > 1 {
        let mid = good + (bad - good) / 2;
        // The middle, or the nearest commit to it that builds.
        let mut order: Vec<usize> = ((good + 1)..bad).collect();
        order.sort_by_key(|i| (i.abs_diff(mid), *i));
        let mut moved = false;
        for i in order {
            match ask(i, &mut tried) {
                Verdict::Passes => {
                    good = i;
                    moved = true;
                    break;
                }
                Verdict::Fails => {
                    bad = i;
                    moved = true;
                    break;
                }
                Verdict::CantTell(_) => continue,
            }
        }
        if !moved {
            return Outcome::Unclear(format!(
                "it broke between {} and {}, and none of the {} commits between them builds, so I can't narrow it further",
                short(&commits[good]),
                short(&commits[bad]),
                bad - good - 1
            ));
        }
    }
    Outcome::Found(Change { commit: commits[bad].clone(), last_good: commits[good].clone(), tried })
}

fn short(c: &str) -> &str {
    &c[..c.len().min(8)]
}

/// What a test run's output says, given whether it exited cleanly.
pub fn verdict_of_run(passed: bool, output: &str) -> Verdict {
    if passed {
        return Verdict::Passes;
    }
    let doesnt_build = output.contains("error: could not compile") || output.contains("error[E") || output.contains("error: no test target");
    if doesnt_build {
        return Verdict::CantTell("it doesn't build there".into());
    }
    // A filter that matched nothing: the test wasn't written yet.
    if output.contains("running 0 tests") && !output.contains("FAILED") {
        return Verdict::CantTell("the test isn't there yet".into());
    }
    Verdict::Fails
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = crate::tools::command("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| format!("couldn't run git: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Find the change in `repo` that made `test` (a program and its arguments,
/// run in the checkout) start failing, looking back at most `look_back`
/// commits along the first-parent line, each run stopped at `limit_secs`.
pub fn what_broke(repo: &Path, test: &[String], look_back: usize, limit_secs: u64) -> Result<Outcome, String> {
    let Some((program, args)) = test.split_first() else {
        return Err("no test to run".into());
    };
    let listed = git(repo, &["rev-list", "--first-parent", &format!("--max-count={}", look_back.max(2)), "HEAD"])?;
    let mut commits: Vec<String> = listed.lines().map(str::to_string).collect();
    commits.reverse();
    let base = crate::roots::tmp_dir().join("selffix");
    let _ = std::fs::create_dir_all(&base);
    let dir = base.join(format!("bisect-{}", std::process::id()));
    let _ = git(repo, &["worktree", "remove", "--force", &dir.to_string_lossy()]);
    let _ = std::fs::remove_dir_all(&dir);
    git(repo, &["worktree", "add", "--detach", &dir.to_string_lossy(), "HEAD"])?;
    // The crate may sit in a folder of the repo (Atlas's is `atlas/`): the
    // test runs where it does in the checkout you gave.
    let top = git(repo, &["rev-parse", "--show-toplevel"]).map(std::path::PathBuf::from).unwrap_or_else(|_| repo.to_path_buf());
    let inner = repo.canonicalize().ok().and_then(|r| top.canonicalize().ok().and_then(|t| r.strip_prefix(t).ok().map(|p| p.to_path_buf()))).unwrap_or_default();
    let run_in = dir.join(&inner);
    let args: Vec<String> = args.to_vec();
    let outcome = find_first_bad(&commits, |c| {
        if crate::goodbye::asked_to_stop() {
            return Verdict::CantTell("you asked me to stop".into());
        }
        if let Err(e) = git(&dir, &["checkout", "-q", "--detach", c]) {
            return Verdict::CantTell(format!("couldn't check it out: {e}"));
        }
        let (passed, out) = crate::sandbox::run_within(program, &args, &[], &run_in, limit_secs, 20_000);
        verdict_of_run(passed, &out)
    });
    let _ = git(repo, &["worktree", "remove", "--force", &dir.to_string_lossy()]);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(outcome)
}

/// The answer in words, with what the change was.
pub fn told(repo: &Path, test: &str, o: &Outcome) -> String {
    match o {
        Outcome::PassesNow => format!("{test} passes now -- nothing's broken there."),
        Outcome::NeverPassed { looked_back } => {
            format!("{test} fails as far back as I looked ({looked_back} changes), so it isn't a recent change that broke it.")
        }
        Outcome::Unclear(why) => format!("I couldn't pin it down: {why}."),
        Outcome::Found(c) => {
            let what = git(repo, &["log", "-1", "--format=%s (%ad)", "--date=short", &c.commit]).unwrap_or_default();
            let files = git(repo, &["diff-tree", "--no-commit-id", "--name-only", "-r", "-m", "--first-parent", &c.commit]).unwrap_or_default();
            let files: Vec<&str> = files.lines().take(8).collect();
            format!(
                "{test} broke at {} -- \"{what}\". It passed at {} just before. Files it changed: {}. (Tried {} commits.) \
                 Undoing that change is one fix; seeing what in it broke the test is the better one.",
                short(&c.commit),
                short(&c.last_good),
                if files.is_empty() { "none listed".to_string() } else { files.join(", ") },
                c.tried
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("c{i:02}")).collect()
    }

    fn broke_at(at: usize) -> impl FnMut(&str) -> Verdict {
        move |c: &str| if c[1..].parse::<usize>().unwrap() >= at { Verdict::Fails } else { Verdict::Passes }
    }

    #[test]
    fn it_finds_the_first_failing_commit_in_few_runs() {
        let c = line(64);
        for at in [1, 5, 31, 62, 63] {
            match find_first_bad(&c, broke_at(at)) {
                Outcome::Found(ch) => {
                    assert_eq!(ch.commit, format!("c{at:02}"));
                    assert_eq!(ch.last_good, format!("c{:02}", at - 1));
                    assert!(ch.tried <= 14, "{} runs for 64 commits", ch.tried);
                }
                other => panic!("broke at {at}: {other:?}"),
            }
        }
    }

    #[test]
    fn passing_now_and_never_passing_are_said_as_such() {
        let c = line(10);
        assert_eq!(find_first_bad(&c, |_| Verdict::Passes), Outcome::PassesNow);
        assert_eq!(find_first_bad(&c, |_| Verdict::Fails), Outcome::NeverPassed { looked_back: 10 });
    }

    #[test]
    fn a_commit_that_doesnt_build_is_stepped_round() {
        let c = line(32);
        // Broke at 20; 18, 21 and 22 don't build. 19 does, so the break is
        // still pinned to 20.
        let mut v = |x: &str| {
            let i: usize = x[1..].parse().unwrap();
            if [18, 21, 22].contains(&i) {
                Verdict::CantTell("doesn't build".into())
            } else if i >= 20 {
                Verdict::Fails
            } else {
                Verdict::Passes
            }
        };
        assert!(matches!(find_first_bad(&c, &mut v), Outcome::Found(ch) if ch.commit == "c20"));
        // None in the gap builds: the range is named, not a guess.
        let mut w = |x: &str| {
            let i: usize = x[1..].parse().unwrap();
            if (17..=22).contains(&i) {
                Verdict::CantTell("doesn't build".into())
            } else if i >= 20 {
                Verdict::Fails
            } else {
                Verdict::Passes
            }
        };
        assert!(matches!(find_first_bad(&c, &mut w), Outcome::Unclear(why) if why.contains("between c16 and c23")));
    }

    #[test]
    fn a_run_that_doesnt_compile_tells_nothing() {
        assert_eq!(verdict_of_run(true, ""), Verdict::Passes);
        assert_eq!(verdict_of_run(false, "test result: FAILED. 1 passed; 1 failed"), Verdict::Fails);
        assert!(matches!(verdict_of_run(false, "error[E0425]: cannot find value"), Verdict::CantTell(_)));
    }
}
