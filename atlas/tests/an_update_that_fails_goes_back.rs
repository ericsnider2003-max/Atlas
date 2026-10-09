//! O1: a new version has to prove itself here, or the one that worked comes
//! back by itself.
//!
//! Two layers, both exercised against real files and real processes: the new
//! build is run with `--health-check` before it is started (a build that
//! fails or hangs is set aside and the running one put straight back), and a
//! build that passes is on trial for its first starts (too many that never
//! get through and the previous one returns).

use atlas::upgrade::{
    begin_trial, current_trial, failed_at, health_check, is_known_bad, keep_old_at, roll_back,
    tag_of, trial_on_start, trial_passed_by, update_history, version, TrialStep, TRIAL_STARTS,
};
#[cfg(unix)]
use atlas::upgrade::{check_new_build, swap_checked, Swapped};
use std::path::PathBuf;
#[cfg(unix)]
use std::path::Path;
#[cfg(unix)]
use std::time::Duration;

fn fresh(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-o1-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[cfg(unix)]
fn script(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A dropped build answering `--version` as 9.9.9 and `--health-check` with `check`.
#[cfg(unix)]
fn drop_build(root: &Path, check: &str) {
    script(
        &root.join("updates/atlas"),
        &format!("case \"$1\" in --version) echo 'atlas 9.9.9';; --health-check) {check};; esac"),
    );
}

#[test]
fn this_build_passes_its_own_health_check_on_a_fresh_install() {
    let root = fresh("self");
    let r = health_check(&root, &root.join("config"), &root.join("data/state"));
    println!("LIVE [health] {r:?}");
    let lines = r.expect("a fresh install is healthy");
    assert!(lines[0].contains(version()));
    assert!(lines.iter().any(|l| l.contains("shipped settings load")));
    assert!(lines.iter().any(|l| l.contains("state folder writes")));
    // It leaves nothing behind: no scratch settings, no probe file.
    let left: Vec<_> = std::fs::read_dir(root.join("data/state")).unwrap().flatten().map(|e| e.file_name()).collect();
    assert!(left.is_empty(), "the check cleans up after itself: {left:?}");
}

#[test]
fn a_health_check_that_cannot_keep_state_says_so() {
    let root = fresh("nostate");
    // The state folder's place is taken by a file: nothing can be written there.
    std::fs::write(root.join("state"), "not a folder").unwrap();
    let r = health_check(&root, &root.join("config"), &root.join("state"));
    let lines = r.expect_err("no state folder is not healthy");
    assert!(lines.iter().any(|l| l.contains("can't keep state")), "{lines:?}");
    assert!(lines.iter().any(|l| l.starts_with("(passed)")), "and says how far it got");
}

#[test]
#[cfg(unix)]
fn a_build_that_passes_its_check_goes_in_on_trial() {
    let root = fresh("pass");
    let running = root.join("atlas");
    script(&running, "echo old");
    drop_build(&root, "echo 'healthy: atlas 9.9.9'");
    // 28 Sep 2026: builds are named by tag -- version plus the start of the
    // file's SHA-256 -- not by version alone (every build said 0.1.0).
    let old_tag = tag_of(&running, version());
    let new_tag = tag_of(&root.join("updates/atlas"), "9.9.9");
    let r = swap_checked(&root, &running, Duration::from_secs(20)).unwrap().unwrap();
    println!("LIVE [o1] {r:?}");
    assert_eq!(r, Swapped::Started { path: running.clone(), version: "9.9.9".into(), tag: new_tag.clone(), previous: old_tag.clone() });
    assert!(new_tag.starts_with("9.9.9-") && new_tag.len() == "9.9.9-".len() + 12, "{new_tag}");
    assert!(std::fs::read_to_string(&running).unwrap().contains("9.9.9"));
    assert!(std::fs::read_to_string(keep_old_at(&root, &old_tag)).unwrap().contains("echo old"));
    let t = current_trial(&root).expect("on trial");
    assert_eq!((t.new.as_str(), t.previous.as_str(), t.starts), (new_tag.as_str(), old_tag.as_str(), 0));
    assert!(update_history(&root).iter().any(|l| l.contains(&format!("{new_tag} put in place")) && l.contains("on trial")));
}

#[test]
#[cfg(unix)]
fn a_build_that_fails_its_check_is_set_aside_and_the_running_one_put_back() {
    let root = fresh("fail");
    let running = root.join("atlas");
    script(&running, "echo old");
    drop_build(&root, "echo 'unhealthy: its shipped settings don'\"'\"'t load'; exit 1");
    let tag = tag_of(&root.join("updates/atlas"), "9.9.9");
    let old_tag = tag_of(&running, version());
    let r = swap_checked(&root, &running, Duration::from_secs(20)).unwrap().unwrap();
    println!("LIVE [o1] {r:?}");
    match &r {
        Swapped::RolledBack { version, tag: t, why } => {
            assert_eq!((version.as_str(), t), ("9.9.9", &tag));
            assert!(why.contains("shipped settings"), "the build's own reason is kept: {why}");
        }
        other => panic!("expected a rollback, got {other:?}"),
    }
    assert!(std::fs::read_to_string(&running).unwrap().contains("echo old"), "the running one is back");
    assert!(std::fs::read_to_string(failed_at(&root, &tag)).unwrap().contains("9.9.9"), "the failed one kept as evidence");
    assert!(!keep_old_at(&root, &old_tag).exists());
    assert!(is_known_bad(&root, &tag));
    assert!(!is_known_bad(&root, "9.9.9"), "known bad by build, not by version");
    assert!(current_trial(&root).is_none(), "no trial for a build that never started");
    // The same build dropped in again is refused, not tried a second time.
    // The same bytes: since 28 Sep 2026 a build is known by its fingerprint,
    // so only the very build that failed is refused (a fixed one with the
    // same version is tried, `updating_without_reinstalling`).
    drop_build(&root, "echo 'unhealthy: its shipped settings don'\"'\"'t load'; exit 1");
    let again = swap_checked(&root, &running, Duration::from_secs(20)).unwrap();
    println!("LIVE [o1] dropped again: {again:?}");
    assert!(again.unwrap_err().contains("already failed here"));
    assert!(std::fs::read_to_string(&running).unwrap().contains("echo old"));
}

#[test]
#[cfg(unix)]
fn a_build_that_hangs_on_its_check_counts_as_failed() {
    let root = fresh("hang");
    let running = root.join("atlas");
    script(&running, "echo old");
    drop_build(&root, "sleep 30");
    let started = std::time::Instant::now();
    let r = swap_checked(&root, &running, Duration::from_secs(1)).unwrap().unwrap();
    assert!(started.elapsed() < Duration::from_secs(15), "it was given up on, not waited out");
    match r {
        Swapped::RolledBack { why, .. } => assert!(why.contains("didn't finish"), "{why}"),
        other => panic!("expected a rollback, got {other:?}"),
    }
    assert!(std::fs::read_to_string(&running).unwrap().contains("echo old"));
}

#[test]
#[cfg(unix)]
fn a_build_that_exits_cleanly_without_saying_healthy_is_not_trusted() {
    let root = fresh("silent");
    let exe = root.join("x");
    script(&exe, "exit 0");
    let r = check_new_build(&exe, Duration::from_secs(10));
    assert!(r.unwrap_err().contains("never said it was healthy"));
    assert!(check_new_build(&root.join("missing"), Duration::from_secs(1)).unwrap_err().contains("didn't start"));
}

#[test]
fn starts_that_never_get_through_bring_the_previous_build_back() {
    let root = fresh("trial");
    let running = root.join("atlas");
    std::fs::write(&running, "the new build").unwrap();
    std::fs::write(keep_old_at(&root, "0.0.1"), "the build that worked").unwrap();
    let new = tag_of(&running, version());
    begin_trial(&root, &new, "0.0.1");
    for n in 1..=TRIAL_STARTS {
        assert_eq!(trial_on_start(&root, &running), TrialStep::Trying(n));
    }
    let step = trial_on_start(&root, &running);
    println!("LIVE [o1] start {}: {step:?}", TRIAL_STARTS + 1);
    assert_eq!(step, TrialStep::RolledBack { failed: new.clone(), previous: "0.0.1".into() });
    assert_eq!(std::fs::read_to_string(&running).unwrap(), "the build that worked");
    assert_eq!(std::fs::read_to_string(failed_at(&root, &new)).unwrap(), "the new build");
    assert!(is_known_bad(&root, &new));
    assert!(current_trial(&root).is_none());
    for l in update_history(&root) {
        println!("LIVE [updates.log] {l}");
    }
    assert!(update_history(&root).iter().any(|l| l.contains("never got through") && l.contains("back on 0.0.1")));
}

#[test]
fn a_start_that_gets_through_ends_the_trial_and_keeps_the_build() {
    let root = fresh("passed");
    let running = root.join("atlas");
    std::fs::write(&running, "the new build").unwrap();
    std::fs::write(keep_old_at(&root, "0.0.1"), "the build that worked").unwrap();
    let new = tag_of(&running, version());
    begin_trial(&root, &new, "0.0.1");
    assert_eq!(trial_on_start(&root, &running), TrialStep::Trying(1));
    assert!(!trial_passed_by(&root, "0.0.2-aaaaaaaaaaaa"), "another build's start doesn't end this trial");
    assert!(trial_passed_by(&root, &new));
    assert!(!trial_passed_by(&root, &new), "only once");
    assert_eq!(trial_on_start(&root, &running), TrialStep::Settled);
    assert_eq!(std::fs::read_to_string(&running).unwrap(), "the new build");
    assert!(keep_old_at(&root, "0.0.1").exists(), "the previous one stays for going back by hand");
    assert!(!is_known_bad(&root, &new));
}

#[test]
fn a_trial_of_some_other_version_is_dropped_not_acted_on() {
    let root = fresh("other");
    let running = root.join("atlas");
    std::fs::write(&running, "whatever was put here by hand").unwrap();
    begin_trial(&root, "7.7.7", "0.0.1");
    assert_eq!(trial_on_start(&root, &running), TrialStep::Settled);
    assert!(current_trial(&root).is_none());
    assert_eq!(std::fs::read_to_string(&running).unwrap(), "whatever was put here by hand");
}

#[test]
fn going_back_without_a_kept_build_leaves_the_running_one_alone() {
    let root = fresh("nokeep");
    let running = root.join("atlas");
    std::fs::write(&running, "only build").unwrap();
    let e = roll_back(&root, &running, "9.9.9", "0.0.1", "test").unwrap_err();
    assert!(e.contains("isn't at"), "{e}");
    assert_eq!(std::fs::read_to_string(&running).unwrap(), "only build", "never left with nothing to start");
    assert!(!is_known_bad(&root, "9.9.9"));
}

/// The real program, not a stand-in: during a trial, commands that end
/// deliberately with an error code ("there's no add-on called that") are the
/// program working, not a broken build. Before `leave`, `process::exit`
/// skipped the trial guard, so three typos in a row rolled back a good build.
#[test]
fn deliberate_error_exits_during_a_trial_are_not_failed_starts() {
    let root = fresh("typos");
    let running = root.join(if cfg!(windows) { "atlas.exe" } else { "atlas" });
    std::fs::copy(env!("CARGO_BIN_EXE_atlas"), &running).unwrap();
    std::fs::write(keep_old_at(&root, "0.0.1"), "the build that worked").unwrap();
    // On trial, and already at the last start it's allowed to fail.
    // The trial names the build by tag (version + fingerprint), 28 Sep 2026.
    let new = tag_of(&running, version());
    std::fs::write(root.join("update-trial.txt"), format!("new={new}\nprevious=0.0.1\nstarts={}\n", TRIAL_STARTS - 1)).unwrap();
    for _ in 0..TRIAL_STARTS + 1 {
        let out = std::process::Command::new(&running)
            .args(["plugins", "approve", "no-such-add-on"])
            .env("ATLAS_HOME", &root)
            .env_remove("ATLAS_UPDATE_PROBE")
            .output()
            .expect("atlas runs");
        assert_eq!(out.status.code(), Some(1), "the command refuses, as it should: {}", String::from_utf8_lossy(&out.stderr));
    }
    for l in update_history(&root) {
        println!("LIVE [updates.log] {l}");
    }
    assert!(current_trial(&root).is_none(), "the first deliberate exit ended the trial");
    assert!(!failed_at(&root, &new).exists(), "nothing was rolled back");
    assert!(!is_known_bad(&root, &new));
    assert_eq!(std::fs::read(&running).unwrap(), std::fs::read(env!("CARGO_BIN_EXE_atlas")).unwrap(), "the new build is still the one in place");
    assert!(update_history(&root).iter().any(|l| l.contains("got through start")));
}
