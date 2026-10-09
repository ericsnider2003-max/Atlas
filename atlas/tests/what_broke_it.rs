//! Finding the change that broke a test, in a real git history (5 Oct 2026).

use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    assert!(ok, "git {args:?}");
}

#[test]
fn the_commit_that_broke_it_is_found_without_moving_your_checkout() {
    if Command::new("git").arg("--version").output().is_err() {
        eprintln!("no git here: nothing to run");
        return;
    }
    let repo = std::env::temp_dir().join(format!("atlas-what-broke-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&repo);
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    // Nine changes; the seventh breaks it.
    for i in 1..=9 {
        let state = if i >= 7 { "BROKEN" } else { "ok" };
        std::fs::write(repo.join("state.txt"), format!("{state}\n")).unwrap();
        std::fs::write(repo.join("n.txt"), format!("{i}\n")).unwrap();
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", &format!("change {i}")]);
    }
    let test: Vec<String> = if cfg!(windows) {
        ["findstr", "/X", "ok", "state.txt"].iter().map(|s| s.to_string()).collect()
    } else {
        ["grep", "-qx", "ok", "state.txt"].iter().map(|s| s.to_string()).collect()
    };
    let stop = || false;
    let o = atlas::bisect::what_broke_until(&repo, &test, 64, 30, std::time::Duration::from_secs(30), &stop).expect("it ran");
    let said = atlas::bisect::told(&repo, "the state check", &o);
    assert!(said.contains("\"change 7"), "{said}");
    assert!(said.contains("state.txt"), "names the files it changed: {said}");
    // Your checkout stays where it was, and no worktree is left behind.
    assert_eq!(std::fs::read_to_string(repo.join("n.txt")).unwrap().trim(), "9");
    let wt = Command::new("git").args(["worktree", "list"]).current_dir(&repo).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&wt.stdout).lines().count(), 1, "{}", String::from_utf8_lossy(&wt.stdout));
    let _ = std::fs::remove_dir_all(&repo);
}
