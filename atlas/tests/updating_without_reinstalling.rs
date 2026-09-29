//! Proving that replacing the binary does not lose anything.
//!
//! This exists because of a specific, correct objection: there is not enough
//! evidence to justify deleting and reinstalling Atlas several times to try
//! things out. The objection is about *evidence*, so the answer has to be
//! evidence rather than reassurance.
//!
//! An update is one file being replaced. What these tests establish is that
//! the file being replaced and the things worth keeping are disjoint sets,
//! that `atlas update` says so before anything moves, and that a simulated
//! update over a populated install really does leave every byte of it alone.

use atlas::upgrade::{check, check_with, keep_old_at, version, Fate, SHIPPED, YOURS};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-upgrade-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// An install someone has actually been using.
fn lived_in(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut written = BTreeMap::new();
    let files = [
        ("config/machine.yaml", "detected_at: 7\napps: {}\n"),
        ("config/apps.yaml", "apps: {}\n"),
        ("data/state/notes.json", r#"{"items":[{"id":1,"what":"the thing"}]}"#),
        ("data/state/vault.json", r#"{"secrets":[{"name":"broker","sealed":[1,2,3]}]}"#),
        ("data/state/kin_peers.yaml", "peers: [{name: Priya, token: abc}]\n"),
        ("data/notes/monday.md", "# Monday\nwhat happened\n"),
        ("data/logs/atlas.log", "started\n"),
        ("data/backups/state-10000/notes.json", "{}"),
    ];
    for (rel, body) in files {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, body).unwrap();
        written.insert(rel.to_string(), body.as_bytes().to_vec());
    }
    written
}

/// What an update does: overwrite the shipped config and the binary. Nothing
/// else. Written out longhand here so the test is checking a real sequence
/// of file operations rather than a claim about one.
fn apply_an_update(root: &Path) {
    for (rel, _) in SHIPPED {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "# a newer shipped config\n").unwrap();
    }
    std::fs::write(root.join("atlas"), b"a newer binary").unwrap();
}

#[test]
fn nothing_is_both_yours_and_shipped() {
    // The property the whole design rests on. If these two lists ever
    // overlap, "replace the shipped files" starts meaning "delete something
    // of yours", and every reassuring sentence elsewhere becomes false.
    for (mine, _) in YOURS {
        assert!(
            !SHIPPED.iter().any(|(s, _)| s == mine),
            "{mine} is listed as both yours and shipped — an update would overwrite it"
        );
    }
}

#[test]
fn an_update_over_a_used_install_changes_nothing_of_yours() {
    let root = tmp("lived-in");
    let before = lived_in(&root);

    let report = check(&root);
    assert!(report.safe(), "reported unsafe before anything happened: {}", report.spoken());

    apply_an_update(&root);

    for (rel, original) in &before {
        if SHIPPED.iter().any(|(s, _)| s == rel) {
            continue; // meant to be replaced
        }
        let now = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("{rel} is gone: {e}"));
        assert_eq!(&now, original, "{rel} changed during an update");
    }
}

#[test]
fn the_shipped_config_really_was_replaced() {
    // The other half. A test that only checks nothing was lost would pass on
    // an update that did nothing at all.
    let root = tmp("replaced");
    lived_in(&root);
    apply_an_update(&root);
    let text = std::fs::read_to_string(root.join("config/apps.yaml")).unwrap();
    assert!(text.contains("newer shipped config"), "the update did not land: {text}");
}

#[test]
fn a_first_install_is_safe_to_update_and_says_what_it_will_build() {
    // Nothing there yet is not a problem, and must not read like one.
    let root = tmp("fresh");
    let report = check(&root);
    assert!(report.safe());
    assert!(report.kept().is_empty());
    assert!(
        report.items.iter().filter(|i| i.fate == Fate::Regenerated).count() >= YOURS.len(),
        "a fresh install should report everything of yours as still to be created"
    );
}

#[test]
fn a_file_that_is_both_yours_and_shipped_is_reported_at_risk() {
    // The branch that protects the property in the first test. Proven by
    // constructing the situation rather than trusting the branch is right,
    // since by design it never fires against the real lists.
    let root = tmp("risk");
    lived_in(&root);
    let overlap: Vec<&str> = YOURS
        .iter()
        .map(|(p, _)| *p)
        .filter(|p| SHIPPED.iter().any(|(s, _)| s == p))
        .collect();
    assert!(overlap.is_empty(), "the real lists overlap, which the first test should have caught");

    // Now make them overlap on purpose and check the branch really fires.
    // Without this the protective branch is unexercised code guarding an
    // invariant that keeps it unexercised -- which is precisely the shape of
    // thing that turns out not to work the day it is needed.
    let both: &[(&str, &str)] = &[("config/apps.yaml", "pretend this is also yours")];
    let report = check_with(&root, both, SHIPPED);
    assert!(!report.safe(), "a file in both lists was not reported as at risk");
    assert_eq!(report.at_risk().len(), 1);
    assert_eq!(report.at_risk()[0].path, "config/apps.yaml");
    assert!(report.spoken().contains("Not safe yet"));

    // And a file of yours that is not shipped stays kept even in that run.
    let mixed: &[(&str, &str)] =
        &[("config/apps.yaml", "also yours"), ("data/notes", "only yours")];
    let report = check_with(&root, mixed, SHIPPED);
    assert_eq!(report.at_risk().len(), 1);
    assert_eq!(report.kept().len(), 1, "an unshipped file of yours stopped being kept");
}

#[test]
fn the_report_names_every_single_thing_rather_than_summarising() {
    // A summary is what you write when you have not checked. Every entry in
    // both lists has to appear by name, with a sentence saying what it holds,
    // because this report is the evidence and a vague one is worthless.
    let root = tmp("named");
    lived_in(&root);
    let report = check(&root);
    for (rel, _) in YOURS.iter().chain(SHIPPED.iter()) {
        assert!(
            report.items.iter().any(|i| i.path == *rel),
            "{rel} is not in the report at all"
        );
    }
    let spoken = report.spoken();
    for (rel, _) in YOURS {
        assert!(spoken.contains(rel), "{rel} is missing from what the user is shown");
    }
    for i in &report.items {
        assert!(i.what.len() > 15, "{} is listed with no explanation of what it is", i.path);
    }
}

#[test]
fn going_back_does_not_need_a_download() {
    // An update you cannot undo is one you put off, which is the situation
    // this whole file is trying to end.
    let old = keep_old_at(Path::new("/opt/atlas"), "0.1.0");
    assert!(old.to_string_lossy().contains("0.1.0"), "the kept binary is not named by version");
    assert!(old.to_string_lossy().contains("previous"));
    // Two updates in a row must not lose the one that worked.
    assert_ne!(keep_old_at(Path::new("/opt/atlas"), "0.1.0"), keep_old_at(Path::new("/opt/atlas"), "0.2.0"));
}

#[test]
fn the_things_worth_keeping_are_actually_where_atlas_puts_them() {
    // `YOURS` is a hand-written list, so it can drift from where the program
    // really writes. `data/state` is the one that would hurt most: it holds
    // the vault and the peer tokens.
    // This used to assert that `src/main.rs` contained the literal
    // `Store::new("data/state")`. That string *was* the bug: a relative path,
    // so `Store::install_root()` climbed to the empty path and every command
    // read and wrote whichever folder you were standing in — and this test
    // was green the whole time, because the spelling was right. A guard that
    // pins a string cannot tell you whether the path resolves.
    //
    // So: check the real thing. `roots` decides where the install is, and
    // what it calls the state directory has to be the same relative path
    // `upgrade::YOURS` promises to preserve.
    let state = atlas::roots::state_dir();
    let root = atlas::roots::install_root();
    let rel = state.strip_prefix(&root).expect("the state dir is not inside the install root");
    assert_eq!(
        rel,
        Path::new("data").join("state"),
        "the state directory moved and upgrade::YOURS was not updated with it"
    );
    assert!(YOURS.iter().any(|(p, _)| *p == "data/state"));

    // And the round trip that `atlas update` actually depends on: a store
    // opened at that directory must agree about which install it is in, or
    // the preserve list is checked against the wrong folder.
    assert_eq!(atlas::roots::store().install_root(), root);
    assert!(!version().is_empty());
}

/// Going back to an older release takes you, at this device -- nothing that
/// arrives over the mesh can ask for it. The approval can only be made by
/// calling `given_by_the_person_at_this_device`, and a forward check refuses
/// the same older release.
#[test]
fn only_you_can_take_a_device_back_to_an_older_release() {
    use atlas::release::{
        accept_against, anchor_of, seal_manifest, signing_key_from_seed, Artifact, Direction, LocalApproval, Manifest,
        Refusal, MANIFEST_FORMAT,
    };
    let key = signing_key_from_seed(&[3; 32]);
    let old = Manifest {
        format: MANIFEST_FORMAT,
        sequence: 2,
        version: "1.2.0".into(),
        min_data_format: 1,
        data_format: 1,
        released_at: 100,
        next_word_by: 200,
        artifacts: vec![Artifact { platform: "windows-x86_64".into(), file: "atlas.exe".into(), size: 5, sha256: "ab".repeat(32) }],
    };
    let signed = seal_manifest(&key, &old);
    let forward = accept_against(&anchor_of(&key), &signed, 5, 1, "windows-x86_64", Direction::Forward);
    assert!(matches!(forward, Err(Refusal::Downgrade { .. })), "an older release was accepted without you");
    let back = accept_against(
        &anchor_of(&key),
        &signed,
        5,
        1,
        "windows-x86_64",
        Direction::Rollback(LocalApproval::given_by_the_person_at_this_device()),
    );
    assert!(back.is_ok(), "your own rollback was refused");
}

// ---------------------------------------------------------------- one build, one identity (28 Sep 2026)
//
// Every build said 0.1.0 (the Cargo.toml version, never bumped). So the
// courier's update was thrown away as "the version already running", then
// recorded as installed; one failure put 0.1.0 on the known-bad list and
// blocked every build after it; and `atlas-0.1.0.previous` was overwritten
// by each update. Now each CI build has its own version (`build.rs`, from
// ATLAS_BUILD_NUMBER) and every identity check is by SHA-256.

#[test]
fn every_build_says_its_own_version_and_a_local_one_says_dev() {
    let v = version();
    let expected = match option_env!("ATLAS_BUILD_NUMBER").and_then(|n| n.trim().parse::<u32>().ok()) {
        Some(n) => format!("0.1.{n}"),
        None => "0.1.0-dev".to_string(),
    };
    assert_eq!(v, expected, "build.rs stamps the version from ATLAS_BUILD_NUMBER");
    assert_ne!(v, "0.1.0", "the version that every build used to share");
}

#[test]
fn two_builds_with_one_version_are_two_builds() {
    use atlas::upgrade::{build_tag, older_version, tag_version};
    let a = build_tag("0.1.0-dev", &atlas::digest::sha256_hex(b"one build"));
    let b = build_tag("0.1.0-dev", &atlas::digest::sha256_hex(b"another build"));
    assert_ne!(a, b);
    assert_ne!(keep_old_at(Path::new("/opt/atlas"), &a), keep_old_at(Path::new("/opt/atlas"), &b), "two updates in a row keep both");
    assert_eq!(tag_version(&a), "0.1.0-dev");
    assert_eq!(tag_version("0.0.9"), "0.0.9", "an old file name is its own version");
    assert_eq!(build_tag("0.1.5", ""), "0.1.5", "no fingerprint: the version alone");
    assert!(older_version("0.1.9", "0.1.10"));
    assert!(older_version("0.1.0-dev", "0.1.0"));
    assert!(!older_version("0.1.10", "0.1.9"));
    assert!(!older_version("0.1.7", "0.1.7"));
    assert!(!older_version("nonsense", "0.1.7"), "unreadable is never called older");
}

#[cfg(unix)]
fn script(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
#[cfg(unix)]
fn a_new_build_carrying_the_running_version_is_swapped_in_not_thrown_away() {
    use atlas::upgrade::{current_trial, swap_checked, tag_of, Swapped};
    let root = tmp("same-version");
    let running = root.join("atlas");
    let v = version();
    script(&running, &format!("# the old build\ncase \"$1\" in --version) echo 'atlas {v}';; esac"));
    // Same version, different bytes: a real update.
    script(
        &root.join("updates/atlas"),
        &format!("# the new build\ncase \"$1\" in --version) echo 'atlas {v}';; --health-check) echo 'healthy: atlas {v}';; esac"),
    );
    let old = tag_of(&running, v);
    let r = swap_checked(&root, &running, std::time::Duration::from_secs(20)).unwrap().unwrap();
    println!("LIVE [same version] {r:?}");
    assert!(matches!(r, Swapped::Started { .. }), "{r:?}");
    assert!(std::fs::read_to_string(&running).unwrap().contains("the new build"), "the new one is in place");
    assert!(std::fs::read_to_string(keep_old_at(&root, &old)).unwrap().contains("the old build"));
    assert_ne!(current_trial(&root).unwrap().new, old, "the trial is of the new build, by fingerprint");
}

#[test]
#[cfg(unix)]
fn the_very_same_bytes_dropped_in_again_are_removed() {
    use atlas::upgrade::swap_checked;
    let root = tmp("same-bytes");
    let running = root.join("atlas");
    let body = "case \"$1\" in --version) echo 'atlas 5.5.5';; esac";
    script(&running, body);
    script(&root.join("updates/atlas"), body);
    let e = swap_checked(&root, &running, std::time::Duration::from_secs(20)).unwrap().unwrap_err();
    assert!(e.contains("the very build already running"), "{e}");
    assert!(!root.join("updates/atlas").exists());
}

#[test]
#[cfg(unix)]
fn a_failed_build_blocks_only_itself() {
    use atlas::upgrade::{is_known_bad, swap_checked, tag_of, Swapped};
    let root = tmp("known-bad-by-sha");
    let running = root.join("atlas");
    script(&running, "# old\ncase \"$1\" in --version) echo 'atlas 1.0.0';; esac");
    let bad = root.join("updates/atlas");
    script(&bad, "# bad\ncase \"$1\" in --version) echo 'atlas 2.0.0';; --health-check) echo 'unhealthy: no'; exit 1;; esac");
    let bad_tag = tag_of(&bad, "2.0.0");
    assert!(matches!(swap_checked(&root, &running, std::time::Duration::from_secs(20)), Some(Ok(Swapped::RolledBack { .. }))));
    assert!(is_known_bad(&root, &bad_tag));
    // A fixed build with the same version is tried, not refused.
    script(&bad, "# fixed\ncase \"$1\" in --version) echo 'atlas 2.0.0';; --health-check) echo 'healthy: atlas 2.0.0';; esac");
    let r = swap_checked(&root, &running, std::time::Duration::from_secs(20)).unwrap();
    assert!(matches!(r, Ok(Swapped::Started { .. })), "{r:?}");
}

#[test]
fn only_the_newest_kept_builds_stay() {
    use atlas::upgrade::{prune_kept, KEEP_BUILDS};
    let root = tmp("prune");
    for (i, n) in ["a", "b", "c", "d"].iter().enumerate() {
        let p = root.join(format!("atlas-0.1.{i}-{n}.previous"));
        std::fs::write(&p, n).unwrap();
        let t = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000 + i as u64 * 100);
        std::fs::File::options().write(true).open(&p).unwrap().set_modified(t).unwrap();
    }
    std::fs::write(root.join("atlas-9.failed"), "other kind").unwrap();
    assert_eq!(prune_kept(&root, ".previous", KEEP_BUILDS, None), 2);
    let mut left: Vec<String> = std::fs::read_dir(&root).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
    left.sort();
    assert_eq!(left, vec!["atlas-0.1.2-c.previous", "atlas-0.1.3-d.previous", "atlas-9.failed"]);
}
