//! The bookkeeping for a new ability, committed on a branch of its own
//! (6 Oct 2026). The whole-tree proof -- a scaffolded ability passes every
//! guard in the real source -- was run by hand on 6 Oct (`atlas scaffold
//! "read my texts out loud"` in a worktree: `all` 6999 passed, and the five
//! guard suites); this checks the branch and the checkout.

use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

#[test]
fn an_approved_ability_is_set_up_on_its_own_branch_and_your_checkout_doesnt_move() {
    if Command::new("git").arg("--version").output().is_err() {
        eprintln!("no git here: nothing to run");
        return;
    }
    let repo = std::env::temp_dir().join(format!("atlas-scaffold-branch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&repo);
    let krate = repo.join("atlas");
    for (p, text) in [
        ("src/lib.rs", "pub mod alpha;\n"),
        ("src/capability.rs", "pub fn all() -> Vec<Capability> {\n    vec![\n        Capability { id: \"alpha\", added: 3 },\n    ]\n}\npub const MODULES_IN_TREE: usize = 1;\n"),
        ("tests/capability_wiring.rs", "const CAPABILITY_UNWIRED: &[&str] = &[\n];\n"),
        ("tests/wiring.rs", "const UNWIRED_BASELINE: &[&str] = &[\n];\n"),
    ] {
        let f = krate.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, text).unwrap();
    }
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "start"]);
    let ask = atlas::scaffold::Ask { id: "read_texts_out_loud".into(), what: "read my texts out loud".into(), day: "6 Oct 2026".into() };
    let branch = atlas::scaffold::on_a_branch(&krate, &ask).expect("set up");
    assert_eq!(branch, "atlas-ability-read-texts-out-loud");
    // On the branch: the module and the bookkeeping.
    let files = git(&repo, &["show", "--name-only", "--format=", &branch]);
    for f in ["atlas/src/read_texts_out_loud.rs", "atlas/src/lib.rs", "atlas/src/capability.rs", "atlas/tests/capability_wiring.rs", "atlas/tests/wiring.rs"] {
        assert!(files.contains(f), "{f} not in the commit: {files}");
    }
    // Your checkout: still on main, unchanged, no worktree left behind.
    assert_eq!(git(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]), "main");
    assert!(!krate.join("src/read_texts_out_loud.rs").exists());
    assert_eq!(git(&repo, &["worktree", "list"]).lines().count(), 1);
    // Asked again: the branch is there already, said, not overwritten.
    assert!(atlas::scaffold::on_a_branch(&krate, &ask).is_err());
    let _ = std::fs::remove_dir_all(&repo);
}
