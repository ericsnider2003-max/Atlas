//! InstallStep 2: a signed release that has arrived gets installed, and can be undone.
//!
//! Every test runs the real checks with a real ed25519 key (a test seed, not
//! Eric's), real files in a temporary install, and O1's real trial records.
//! Covered:
//! - install time checks everything again;
//! - the release number moves only after the new build gets through;
//! - a rolled-back or undone version is never offered again;
//! - automatic on your own devices and ask on friends' copies, at a quiet
//!   moment;
//! - a key change arriving over the channel.

use atlas::release::{
    self, anchor_of, manifest_for, seal_manifest, seal_rotation, signing_key_from_seed, Installed, LocalApproval,
    Rotation, TrustState,
};
use atlas::store::Store;
use atlas::update_apply::{
    self as apply, ask_due, asked, finish_after_start, heard_rotation_with, mode_for, next_step, not_offered_again,
    pending, previous_build, quiet_enough, rotation_notice, stage_update, undo_update, UpdateSettled, AutoUpdate, InstallStep,
};
use atlas::update_courier::{self, announcement, Available};
use atlas::upgrade;
use std::path::PathBuf;

const SEED: [u8; 32] = [21; 32];

fn fresh(tag: &str) -> (Store, PathBuf) {
    let root = std::env::temp_dir().join(format!("atlas-step2-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("data/state")).unwrap();
    let store = Store::new(root.join("data/state"));
    let mut i = Installed::load(&store);
    i.trust = TrustState { current: anchor_of(&signing_key_from_seed(&SEED)), rotations: 0 };
    i.save(&store).unwrap();
    (store, root)
}

fn platform() -> &'static str {
    release::this_platform().expect("the tests run on a platform Atlas ships for")
}

/// Hear a signed release of `bytes` as `version`, number `sequence`, and put
/// the file where a finished download goes.
fn arrived(store: &Store, version: &str, sequence: u64, bytes: &[u8]) {
    let m = manifest_for(
        sequence,
        version,
        1,
        1_000,
        1_000 + 30 * 86_400,
        &[(platform().to_string(), "atlas".to_string(), bytes.to_vec())],
    )
    .unwrap();
    let notice = announcement(&seal_manifest(&signing_key_from_seed(&SEED), &m));
    let said = update_courier::heard(store, &notice, "owner-key", 2_000);
    assert!(said.is_some_and(|s| s.contains(version)), "the notice was heard");
    // What `fetch_step` leaves once the whole file has matched.
    let dl = store.root().join("release_download");
    std::fs::create_dir_all(&dl).unwrap();
    let file = dl.join("atlas");
    std::fs::write(&file, bytes).unwrap();
    let mut a = Available::load(store);
    a.downloaded = file.to_string_lossy().to_string();
    store.save("update_available", &a).unwrap();
}

#[test]
fn a_release_that_arrived_is_checked_again_and_staged_for_the_next_start() {
    let (s, root) = fresh("stage");
    let build = b"#!/bin/sh\necho 'atlas 9.1.0'\n".to_vec();
    arrived(&s, "9.1.0", 5, &build);
    let v = stage_update(&s, &root, platform()).expect("staged");
    assert_eq!(v, "9.1.0");
    assert_eq!(std::fs::read(upgrade::staging_path(&root)).unwrap(), build, "the exact signed bytes, in updates/");
    let p = pending(&s).expect("recorded as pending");
    // What it replaces is named by tag (version + fingerprint), 28 Sep 2026.
    assert_eq!((p.version.as_str(), p.replaces.as_str()), ("9.1.0", upgrade::this_tag().as_str()));
    // Nothing is recorded as installed yet: it hasn't run.
    assert_eq!(Installed::load(&s).sequence, 0);
    assert!(upgrade::update_history(&root).iter().any(|l| l.contains("9.1.0 checked again and staged")));
}

#[test]
fn a_download_changed_on_disk_after_it_arrived_is_refused_and_thrown_away() {
    let (s, root) = fresh("tampered");
    arrived(&s, "9.1.0", 5, b"the real build");
    let a = Available::load(&s);
    std::fs::write(&a.downloaded, b"the real build, then something else").unwrap();
    let e = stage_update(&s, &root, platform()).unwrap_err();
    println!("LIVE [step2] {e}");
    assert!(e.contains("changed on disk") && e.contains("fetching it again"), "{e}");
    assert!(!upgrade::staging_path(&root).exists(), "nothing staged");
    let a = Available::load(&s);
    assert!(a.notice.is_some() && a.downloaded.is_empty(), "the damaged file is gone, the release isn't: it's fetched again");
}

#[test]
fn a_notice_no_longer_trusted_at_install_time_is_refused() {
    let (s, root) = fresh("rotated-away");
    arrived(&s, "9.1.0", 5, b"build");
    // The key changed between download and install: the notice is re-checked
    // against the key trusted now, not the one trusted when it arrived.
    let mut i = Installed::load(&s);
    i.trust = TrustState { current: anchor_of(&signing_key_from_seed(&[99; 32])), rotations: 1 };
    i.save(&s).unwrap();
    let e = stage_update(&s, &root, platform()).unwrap_err();
    assert!(e.contains("won't install") && e.contains("not signed"), "{e}");
    assert!(!upgrade::staging_path(&root).exists());
}

#[test]
fn nothing_waiting_or_not_finished_arriving_says_so() {
    let (s, root) = fresh("nothing");
    assert!(stage_update(&s, &root, platform()).unwrap_err().contains("no update waiting"));
    arrived(&s, "9.1.0", 5, b"build");
    let mut a = Available::load(&s);
    a.downloaded.clear();
    s.save("update_available", &a).unwrap();
    assert!(stage_update(&s, &root, platform()).unwrap_err().contains("hasn't finished arriving"));
}

#[test]
fn the_release_number_moves_only_after_the_new_build_gets_through_its_trial() {
    let (s, root) = fresh("finish");
    arrived(&s, upgrade::version(), 5, b"build");
    stage_update(&s, &root, platform()).unwrap();
    // The next start swapped it in and it's on probation. The running build
    // is named by its tag: the version *and* the fingerprint of b"build".
    let running = upgrade::build_tag(upgrade::version(), &atlas::digest::sha256_hex(b"build"));
    upgrade::begin_trial(&root, &running, "0.0.9");
    assert_eq!(finish_after_start(&s, &root, &running), UpdateSettled::Waiting);
    assert_eq!(Installed::load(&s).sequence, 0, "not while on probation");
    assert!(upgrade::trial_passed_by(&root, &running));
    assert_eq!(finish_after_start(&s, &root, &running), UpdateSettled::Installed(upgrade::version().into()));
    let i = Installed::load(&s);
    assert_eq!((i.sequence, i.version.as_str()), (5, upgrade::version()));
    assert!(pending(&s).is_none());
    assert!(Available::load(&s).notice.is_none(), "offered no more");
    assert_eq!(finish_after_start(&s, &root, &running), UpdateSettled::Nothing, "once");
}

/// 28 Sep 2026: every build said 0.1.0, so a staged release carrying the
/// running build's version was taken to be running -- and recorded as
/// installed ("Updated to Atlas 0.1.0") while the old build was still the one
/// in place. Now only the fingerprint says a staged build is running.
#[test]
fn a_staged_build_with_the_same_version_but_other_bytes_is_never_recorded_as_installed() {
    let (s, root) = fresh("sameversion");
    arrived(&s, upgrade::version(), 5, b"a different build with the same version");
    stage_update(&s, &root, platform()).unwrap();
    // Still the old build: same version, other bytes.
    let old = upgrade::build_tag(upgrade::version(), &atlas::digest::sha256_hex(b"the build that was already here"));
    assert_eq!(finish_after_start(&s, &root, &old), UpdateSettled::Waiting);
    assert_eq!(Installed::load(&s).sequence, 0, "nothing recorded as installed");
    assert!(pending(&s).is_some(), "still staged: the restart hasn't put it in yet");
    assert!(apply::take_news(&s).is_none(), "no \"Updated to\" for an update that didn't happen");
}

#[test]
fn a_staged_build_that_was_rolled_back_is_never_offered_again() {
    let (s, root) = fresh("rolledback");
    arrived(&s, "9.1.0", 5, b"build");
    stage_update(&s, &root, platform()).unwrap();
    // O1 put the previous build back and wrote the version down as failed.
    std::fs::write(upgrade::keep_old_at(&root, "0.0.9"), "old").unwrap();
    std::fs::write(root.join("atlas"), "new").unwrap();
    // Set aside by tag: the failing build's version and its fingerprint.
    let failed = upgrade::build_tag("9.1.0", &atlas::digest::sha256_hex(b"build"));
    upgrade::roll_back(&root, &root.join("atlas"), &failed, "0.0.9", "its check failed").unwrap();
    assert!(upgrade::is_known_bad(&root, &failed) && !upgrade::is_known_bad(&root, "9.1.0"), "blocked by build, not by version");
    assert_eq!(finish_after_start(&s, &root, "0.0.9"), UpdateSettled::RolledBack("9.1.0".into()));
    assert_eq!(Installed::load(&s).sequence, 0, "the number stayed where it was");
    let failing = atlas::digest::sha256_hex(b"build");
    assert!(not_offered_again(&s, &failing, "9.1.0").is_some(), "the failing build is held back");
    // Rolling back wrote up what went wrong -- kept here, sent nowhere.
    let r = apply::last_failure(&s).expect("written down");
    assert_eq!((r.version.as_str(), r.stage.as_str(), r.sha256.as_str()), ("9.1.0", "health check", failing.as_str()));
    assert!(r.reasons.iter().any(|x| x.contains("its check failed")), "{r:?}");
    assert!(atlas::feedback::feedback_outbox(&s).is_empty(), "nothing leaves by itself");
    assert!(apply::take_news(&s).is_some_and(|n| n.contains("report a bug") && n.contains("Feedback page") && n.contains("nothing is sent unless you do")));
    // The same release announced again is said once and not offered.
    let m = manifest_for(5, "9.1.0", 1, 1_000, 1_000 + 30 * 86_400, &[(platform().into(), "atlas".into(), b"build".to_vec())]).unwrap();
    let said = update_courier::heard(&s, &announcement(&seal_manifest(&signing_key_from_seed(&SEED), &m)), "owner", 3_000).unwrap();
    println!("LIVE [step2] {said}");
    assert!(said.contains("isn't being offered here") && said.contains("waiting for a fix"), "{said}");
    assert!(Available::load(&s).notice.is_none());
    // The fix is offered as normal -- a newer release, or the same version
    // label with a fixed build: the block is on the failing build itself.
    arrived(&s, "9.2.0", 6, b"fixed build");
    assert_eq!(stage_update(&s, &root, platform()).unwrap(), "9.2.0");
    assert!(not_offered_again(&s, &atlas::digest::sha256_hex(b"fixed build"), "9.1.0").is_none());
}

#[test]
fn automatic_on_your_own_devices_and_ask_on_friends_copies() {
    use atlas::groups::GroupState;
    let owner = atlas::peerkey::Identity::from_seed_for_test([1; 32]);
    let phone = atlas::peerkey::Identity::from_seed_for_test([2; 32]);
    let friend = atlas::peerkey::Identity::from_seed_for_test([3; 32]);
    let channel = GroupState {
        format: 1,
        group_id: "og-test".into(),
        owner: owner.public(),
        name: "Atlas releases".into(),
        version: 1,
        seats: vec![],
        release_channel: true,
        delegates: vec![owner.delegate(&phone.public())],
    };
    assert_eq!(mode_for(None, &owner.public(), Some(&channel)), AutoUpdate::Automatic, "the owner's own Atlas");
    assert_eq!(mode_for(None, &phone.public(), Some(&channel)), AutoUpdate::Automatic, "a device the owner vouched for");
    assert_eq!(mode_for(None, &friend.public(), Some(&channel)), AutoUpdate::Ask, "a friend's copy");
    assert_eq!(mode_for(None, &friend.public(), None), AutoUpdate::Ask);
    assert_eq!(mode_for(None, "", Some(&channel)), AutoUpdate::Ask, "no key of its own: never automatic");
    // Your own choice wins, either way.
    assert_eq!(mode_for(Some(AutoUpdate::Off), &owner.public(), Some(&channel)), AutoUpdate::Off);
    assert_eq!(mode_for(Some(AutoUpdate::Automatic), &friend.public(), Some(&channel)), AutoUpdate::Automatic);
}

#[test]
fn an_automatic_install_waits_for_a_quiet_moment() {
    use atlas::platform::OsQuiet;
    let q = apply::QUIET_AFTER_SECS;
    assert!(quiet_enough(Some(q), Some(OsQuiet::Accepts), false, false));
    assert!(quiet_enough(Some(q), Some(OsQuiet::Away), false, false), "locked counts as quiet");
    assert!(quiet_enough(Some(q + 5), None, false, false));
    assert!(!quiet_enough(Some(30), Some(OsQuiet::Accepts), false, false), "typing a moment ago");
    assert!(!quiet_enough(None, Some(OsQuiet::Accepts), false, false), "unknown idle is never quiet");
    for busy in [OsQuiet::Presenting, OsQuiet::Game, OsQuiet::FullScreen, OsQuiet::QuietTime] {
        assert!(!quiet_enough(Some(q * 3), Some(busy), false, false), "{busy:?}");
    }
    assert!(!quiet_enough(Some(q * 3), None, true, false), "on a call");
    assert!(!quiet_enough(Some(q * 3), None, false, true), "project work mid-phase");
}

#[test]
fn what_happens_next_follows_the_mode_the_moment_and_your_answer() {
    let mut a = Available { version: "9.1.0".into(), ..Default::default() };
    assert_eq!(next_step(AutoUpdate::Automatic, &a, false, true, true), InstallStep::Nothing, "nothing heard");
    a.notice = Some(release::SignedManifest { manifest: "{}".into(), signature: String::new() });
    assert_eq!(next_step(AutoUpdate::Automatic, &a, false, true, true), InstallStep::Nothing, "not arrived");
    a.downloaded = "/x".into();
    assert_eq!(next_step(AutoUpdate::Automatic, &a, false, false, true), InstallStep::InstallNow);
    assert_eq!(next_step(AutoUpdate::Automatic, &a, false, false, false), InstallStep::WaitForQuiet);
    assert!(matches!(next_step(AutoUpdate::Ask, &a, false, true, true), InstallStep::Ask(ref s) if s.contains("9.1.0") && s.contains("kept")));
    assert_eq!(next_step(AutoUpdate::Ask, &a, false, false, true), InstallStep::Nothing, "asked recently");
    assert_eq!(next_step(AutoUpdate::Ask, &a, true, false, false), InstallStep::InstallNow, "yes means now");
    assert_eq!(next_step(AutoUpdate::Off, &a, true, true, true), InstallStep::Nothing);
}

#[test]
fn not_now_asks_again_a_day_later() {
    let (s, _) = fresh("asked");
    assert!(ask_due(&s, "9.1.0", 10_000));
    asked(&s, "9.1.0", 10_000);
    assert!(!ask_due(&s, "9.1.0", 10_000 + 3_600));
    assert!(ask_due(&s, "9.1.0", 10_000 + apply::ASK_AGAIN_SECS));
    assert!(ask_due(&s, "9.2.0", 10_001), "a newer version is asked about at once");
}

#[test]
fn going_back_needs_your_yes_puts_the_previous_build_back_and_is_remembered() {
    let (s, root) = fresh("undo");
    // Installed through the courier, so there's a record to put back.
    arrived(&s, upgrade::version(), 7, b"build");
    stage_update(&s, &root, platform()).unwrap();
    let installed = upgrade::build_tag(upgrade::version(), &atlas::digest::sha256_hex(b"build"));
    assert!(matches!(finish_after_start(&s, &root, &installed), UpdateSettled::Installed(_)));
    assert_eq!(Installed::load(&s).sequence, 7);
    let running = root.join("atlas");
    std::fs::write(&running, "the current build").unwrap();
    std::fs::write(upgrade::keep_old_at(&root, "0.0.9"), "the build before").unwrap();
    assert_eq!(previous_build(&root).map(|(v, _)| v), Some("0.0.9".into()));

    let back = undo_update(&s, &root, &running, LocalApproval::given_by_the_person_at_this_device()).unwrap();
    assert_eq!(back, "0.0.9");
    assert_eq!(std::fs::read_to_string(&running).unwrap(), "the build before");
    let left = upgrade::build_tag(upgrade::version(), &atlas::digest::sha256_hex(b"the current build"));
    assert_eq!(std::fs::read_to_string(root.join(format!("atlas-{left}.undone"))).unwrap(), "the current build");
    assert_eq!(Installed::load(&s).sequence, 0, "the release record is back to before");
    assert_eq!(Installed::load(&s).trust.current, anchor_of(&signing_key_from_seed(&SEED)), "trust kept");
    assert!(not_offered_again(&s, &atlas::digest::sha256_hex(b"build"), upgrade::version()).is_some_and(|w| w.contains("went back")));
    assert!(upgrade::update_history(&root).iter().any(|l| l.contains("you went back from")));
    // Nothing left to go back to.
    let e = undo_update(&s, &root, &running, LocalApproval::given_by_the_person_at_this_device()).unwrap_err();
    assert!(e.contains("no previous version"), "{e}");
}

#[test]
fn a_key_change_in_the_channel_moves_trust_and_a_forged_one_is_refused() {
    let (s, _) = fresh("rotation");
    let recovery = signing_key_from_seed(&[50; 32]);
    let next = signing_key_from_seed(&[51; 32]);
    let rot = |n: u32, to: [u8; 32]| Rotation { number: n, new_anchor: hex(&to), reason: "planned".into() };
    // Planned: signed by the current release key.
    let planned = rotation_notice(&seal_rotation(&signing_key_from_seed(&SEED), &rot(1, anchor_of(&next))));
    let said = heard_rotation_with(&s, &planned, &anchor_of(&recovery)).unwrap();
    println!("LIVE [step2] {said}");
    assert!(said.contains("release key changed"), "{said}");
    assert_eq!(Installed::load(&s).trust, TrustState { current: anchor_of(&next), rotations: 1 });
    // The same notice again is silent.
    assert_eq!(heard_rotation_with(&s, &planned, &anchor_of(&recovery)), None);
    // Signed by nobody this device trusts: refused and said.
    let stranger = signing_key_from_seed(&[77; 32]);
    let forged = rotation_notice(&seal_rotation(&stranger, &rot(2, anchor_of(&stranger))));
    let said = heard_rotation_with(&s, &forged, &anchor_of(&recovery)).unwrap();
    assert!(said.contains("refused"), "{said}");
    assert_eq!(Installed::load(&s).trust.rotations, 1);
    // Ordinary messages aren't read as key changes.
    assert_eq!(heard_rotation_with(&s, "morning", &anchor_of(&recovery)), None);
}

fn hex(b: &[u8; 32]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[test]
fn a_daemon_tick_says_asks_installs_and_never_restarts_twice_for_one_build() {
    use apply::{update_tick, say_yes, Ticked, UpdateMoment};
    let (s, root) = fresh("tick");
    let quiet = UpdateMoment { idle_secs: Some(apply::QUIET_AFTER_SECS), ..Default::default() };
    let busy = UpdateMoment { idle_secs: Some(5), ..Default::default() };
    assert_eq!(update_tick(&s, &root, platform(), "friend", None, &quiet, 10_000), Ticked::Nothing, "nothing here");
    arrived(&s, "9.1.0", 5, b"build");
    // A friend's copy: asked once, then not again for a day.
    let t = update_tick(&s, &root, platform(), "friend", None, &quiet, 10_000);
    assert!(matches!(t, Ticked::Say(ref q) if q.contains("say \"install the update\"") || q.contains("Say \"install the update\"")), "{t:?}");
    assert_eq!(update_tick(&s, &root, platform(), "friend", None, &quiet, 10_060), Ticked::Nothing);
    assert!(!upgrade::staging_path(&root).exists(), "asking installs nothing");
    // The yes: staged and restarted into, even mid-typing, because you asked.
    say_yes(&s, "9.1.0");
    let t = update_tick(&s, &root, platform(), "friend", None, &busy, 10_120);
    assert!(matches!(t, Ticked::Restart(ref m) if m.contains("9.1.0")), "{t:?}");
    assert!(upgrade::staging_path(&root).exists());
    // Still the old build a tick later: it didn't go in. Said, not retried.
    let t = update_tick(&s, &root, platform(), "friend", None, &quiet, 10_180);
    println!("LIVE [step2] {t:?}");
    assert!(matches!(t, Ticked::Say(ref m) if m.contains("failed at its restart")), "{t:?}");
    let r = apply::last_failure(&s).expect("written up to be fixed");
    assert_eq!(r.stage, "restart");
    assert_eq!(update_tick(&s, &root, platform(), "friend", None, &quiet, 10_240), Ticked::Nothing, "no restart loop");
}

#[test]
fn automatic_waits_for_quiet_then_installs_and_news_is_said_once() {
    use apply::{choose_mode, take_news, update_tick, Ticked, UpdateMoment};
    let (s, root) = fresh("auto");
    choose_mode(&s, Some(AutoUpdate::Automatic)).unwrap();
    arrived(&s, "9.1.0", 5, b"build");
    let typing = UpdateMoment { idle_secs: Some(20), ..Default::default() };
    assert_eq!(update_tick(&s, &root, platform(), "", None, &typing, 1), Ticked::Nothing, "you're at the keyboard");
    let away = UpdateMoment { idle_secs: Some(apply::QUIET_AFTER_SECS + 1), ..Default::default() };
    assert!(matches!(update_tick(&s, &root, platform(), "", None, &away, 2), Ticked::Restart(_)));
    // The new build ran, got through, and was recorded: said on the next tick, once.
    std::fs::remove_file(upgrade::staging_path(&root)).unwrap();
    let mut p = pending(&s).unwrap();
    p.replaces = "0.0.1".into(); // as seen from the new build
    s.save("update_pending", &p).unwrap();
    let _ = finish_after_start(&s, &root, &upgrade::build_tag("9.1.0", &atlas::digest::sha256_hex(b"build")));
    let t = update_tick(&s, &root, platform(), "", None, &away, 3);
    assert!(matches!(t, Ticked::Say(ref m) if m.contains("Updated to Atlas 9.1.0")), "{t:?}");
    assert_eq!(take_news(&s), None);
}

/// The real program: `atlas update install`, then `atlas update undo` with a
/// typed yes, against a temporary install.
#[test]
#[cfg(unix)]
fn the_real_commands_install_and_go_back() {
    let (s, root) = fresh("cli");
    let exe = root.join("atlas");
    std::fs::copy(env!("CARGO_BIN_EXE_atlas"), &exe).unwrap();
    arrived(&s, "9.1.0", 5, b"#!/bin/sh\necho 'atlas 9.1.0'\n");
    let run = |args: &[&str], input: &str| {
        use std::io::Write;
        let mut c = std::process::Command::new(&exe)
            .args(args)
            .env("ATLAS_HOME", &root)
            .env("ATLAS_UPDATE_PROBE", "1") // no swap on this start; the command is what's tested
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        c.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
        let o = c.wait_with_output().unwrap();
        String::from_utf8_lossy(&o.stdout).to_string()
    };
    let out = run(&["update", "install"], "");
    println!("LIVE [atlas update install] {out}");
    assert!(out.contains("9.1.0") && out.contains("ready"), "{out}");
    assert!(upgrade::staging_path(&root).exists());
    let out = run(&["update", "auto", "ask"], "");
    assert!(out.contains("ask before"), "{out}");
    let out = run(&["update"], "");
    assert!(out.contains("ask first (your choice)") && out.contains("waiting to go in"), "{out}");
    // Going back: refused without a typed yes, done with one.
    std::fs::write(upgrade::keep_old_at(&root, "0.0.9"), "the build before").unwrap();
    let out = run(&["update", "undo"], "no\n");
    assert!(out.contains("Left as it is"), "{out}");
    assert!(exe.exists() && std::fs::read(&exe).unwrap().len() > 1000, "untouched");
    let out = run(&["update", "undo"], "yes\n");
    println!("LIVE [atlas update undo] {out}");
    assert!(out.contains("Back on Atlas 0.0.9"), "{out}");
    assert_eq!(std::fs::read_to_string(&exe).unwrap(), "the build before");
    assert!(not_offered_again(&s, "", upgrade::version()).is_some());
}


// ---------------------------------------------------------------- when an update fails, it gets fixed

#[test]
fn a_failure_caused_by_this_machine_is_not_blamed_on_the_build() {
    use apply::{failed_because, record_failure, FailedBecause, FailureReport};
    let (s, root) = fresh("machine");
    let r = FailureReport {
        version: "9.1.0".into(),
        sha256: "aa".repeat(32),
        replaces: "0.1.0".into(),
        platform: platform().into(),
        stage: "health check".into(),
        reasons: vec!["can't keep state in D:\\data: No space left on device".into(), "(passed) its shipped settings load".into()],
        ..Default::default()
    };
    // O1 set it aside (by tag, 28 Sep 2026); this is what gets written.
    let tag = upgrade::build_tag("9.1.0", &"aa".repeat(32));
    std::fs::write(root.join("update-known-bad.txt"), format!("{tag} can't keep state\n")).unwrap();
    let said = record_failure(&s, &root, r);
    println!("LIVE [failure] {said}");
    assert!(said.contains("something on this computer") && said.contains("try 9.1.0 again"), "{said}");
    assert!(!upgrade::is_known_bad(&root, &tag), "the same build may be tried again once that's put right");
    assert!(not_offered_again(&s, &"aa".repeat(32), "9.1.0").is_none());
    assert!(apply::last_failure(&s).is_none(), "nothing for anyone to fix");
    // Not retried every tick while the disk is still full: after a pause.
    use apply::{update_tick, Ticked, UpdateMoment};
    apply::choose_mode(&s, Some(AutoUpdate::Automatic)).unwrap();
    arrived(&s, "9.1.0", 5, b"build");
    let away = UpdateMoment { idle_secs: Some(apply::QUIET_AFTER_SECS * 2), ..Default::default() };
    let now = atlas::store::now();
    assert_eq!(update_tick(&s, &root, platform(), "", None, &away, now + 60), Ticked::Nothing, "paused");
    assert!(matches!(update_tick(&s, &root, platform(), "", None, &away, now + apply::MACHINE_RETRY_SECS + 1), Ticked::Restart(_)), "then tried again");
    // Anything the check didn't name as this machine's is the build's.
    assert_eq!(failed_because(&["its own shipped settings don't load: bad key".into()], ""), FailedBecause::TheBuild);
    assert_eq!(failed_because(&["can't keep state".into()], "panicked at src/x.rs:1"), FailedBecause::TheBuild, "a crash is never the machine's");
    assert_eq!(failed_because(&[], ""), FailedBecause::TheBuild, "no reason at all is not an excuse");
}

#[test]
fn a_report_carries_no_user_name_or_home_folder() {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/home/tester".into());
    let line = format!("can't read your settings folder {home}/Atlas/config: denied");
    let clean = apply::scrub_personal(&line);
    assert!(!clean.contains(&home), "{clean}");
    assert!(clean.contains("~/Atlas/config"), "{clean}");
}

/// The real program on the releaser's side: `atlas update failures` lists a
/// report, and `--brief` writes the fix brief.
#[test]
#[cfg(unix)]
fn the_real_failures_command_lists_reports_and_writes_a_brief() {
    let (s, root) = fresh("failures-cli");
    let r = apply::FailureReport {
        version: "9.1.0".into(),
        sha256: "bb".repeat(32),
        replaces: "0.1.0".into(),
        platform: "windows-x86_64".into(),
        stage: "health check".into(),
        reasons: vec!["its own shipped settings don't load: unknown field `beep`".into()],
        ..Default::default()
    };
    let f = atlas::feedback::Feedback { id: "f1".into(), words: "it won't start after the update".into(), version: "0.1.0".into(), attached: Some(r), ..Default::default() };
    atlas::feedback::heard_feedback(&s, "Priya", &serde_json::to_string(&f).unwrap()).unwrap();
    let run = |args: &[&str]| {
        let o = std::process::Command::new(env!("CARGO_BIN_EXE_atlas"))
            .args(args)
            .current_dir(&root)
            .env("ATLAS_HOME", &root)
            .env("ATLAS_UPDATE_PROBE", "1")
            .output()
            .unwrap();
        String::from_utf8_lossy(&o.stdout).to_string()
    };
    let out = run(&["update", "failures"]);
    println!("LIVE [atlas update failures] {out}");
    assert!(out.contains("Atlas 9.1.0: 1 report") && out.contains("Priya") && out.contains("unknown field"), "{out}");
    let out = run(&["update", "failures", "brief", "9.1.0"]);
    assert!(out.contains("atlas-9.1.0-fix-brief.md"), "{out}");
    let brief = std::fs::read_to_string(root.join("atlas-9.1.0-fix-brief.md")).unwrap();
    assert!(brief.contains("# Fix brief: Atlas 9.1.0") && brief.contains("unknown field"));
    // One section per report, then how to close it; the command read the
    // releaser's filed reports, not a canned page.
    assert_eq!(brief.lines().filter(|l| l.starts_with("## ")).count(), 2, "{brief}");
    assert_eq!(apply::failure_reports(&s).len(), 1, "listing and briefing change nothing");
    assert!(!root.join("atlas-8.0.0-fix-brief.md").exists());
    let none = run(&["update", "failures", "brief", "8.0.0"]);
    assert_eq!(none.lines().last().map(str::trim), Some("No reports about Atlas 8.0.0."));
}
