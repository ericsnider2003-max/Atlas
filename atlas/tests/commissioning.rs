//! How much of "never run on real hardware" actually needs you.
//!
//! The count that matters at setup isn't the thousands of dev tests — it's the
//! built-but-never-run capabilities, and specifically the few where only you
//! can confirm it worked (a window you watch move, an object the camera names).
//! Most compute an answer Atlas can check itself, or act and read the machine
//! back to witness the effect. This proves the split is honest and small.

use atlas::capability::{self, Verify};
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn dir(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-commission-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}
fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}
fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

#[test]
fn the_split_covers_every_never_run_capability() {
    let (computes, read_back, eyes_n, eyes) = capability::commissioning();
    assert_eq!(eyes_n, eyes.len(), "the your-eyes count matches its named list");
    let untested = capability::all().iter().filter(|c| !c.state.usable()).filter(|c| {
        matches!(c.state, atlas::capability::State::Untested)
    }).count();
    assert_eq!(computes + read_back + eyes_n, untested, "every never-run capability is classified once");
}

#[test]
fn the_part_that_needs_you_is_small_and_the_rest_is_atlas() {
    let (computes, read_back, eyes_n, _eyes) = capability::commissioning();
    let auto = computes + read_back;
    // The whole point: the overwhelming majority is Atlas's to verify, not
    // yours to sit through.
    assert!(auto > eyes_n * 3, "most of it verifies without you: auto={auto} vs eyes={eyes_n}");
    // And computing-from-data is the largest bucket.
    assert!(computes >= read_back, "the compute-and-check bucket is the biggest: {computes} vs {read_back}");
}

#[test]
fn the_classifier_reads_the_verification_from_what_a_thing_needs() {
    // A camera capability needs your eyes; a window one is read back; a
    // data/logic one just computes.
    let cap = |id: &str| capability::all().into_iter().find(|c| c.id == id).expect("cap exists");
    assert_eq!(capability::how_verified(&cap("vision")), Verify::YourEyes, "the camera needs your eyes");
    assert_eq!(capability::how_verified(&cap("layout")), Verify::ReadBack, "moving windows is read back");
    assert_eq!(capability::how_verified(&cap("levels")), Verify::Computes, "trade logic computes and checks");
}

#[test]
fn the_report_says_your_part_is_a_short_list() {
    let report = capability::commissioning_report();
    assert!(report.to_lowercase().contains("verify"), "it's about verification: {report}");
    assert!(
        report.to_lowercase().contains("your eyes") || report.to_lowercase().contains("none need you"),
        "it names your part explicitly: {report}"
    );
}

#[test]
fn asking_whats_left_carries_the_commissioning_split() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(dir("left")), Proactive::new(ProactiveConfig::default()));
    let reply = d.turn("what's left to finish", 100);
    assert!(
        reply.to_lowercase().contains("verify") && reply.to_lowercase().contains("myself"),
        "the 'needs a run' bucket is broken down by who verifies it: {reply}"
    );
}

#[test]
fn the_self_check_tells_you_your_part_in_setup() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(dir("check")), Proactive::new(ProactiveConfig::default()));
    let reply = d.turn("check yourself", 100);
    assert!(reply.to_lowercase().contains("your part in setup"), "the self-check names your part: {reply}");
    // And the number it reports for "your eyes" matches the classification, so
    // the reassurance is real, not a canned phrase.
    let (_c, _r, eyes_n, _names) = capability::commissioning();
    // Six since the 26 Sep merge: the two camera ones, the three that need your
    // voice or your key (round 5), and one real call for call notes (the third
    // chat's line). Seven once 25j's Windows key hook came in (hold a key to
    // talk, press one to type) -- only you at the keyboard can confirm it.
    // Eight on 28 Sep 2026: `audio` (choosing the microphone and speakers,
    // and not opening a Bluetooth headset's microphone for nothing) was built
    // and unlisted until the catalogue accounted for the whole tree, and only
    // your ears can say the headset still sounds right.
    // Ten on 1 Oct 2026: `camwatch` (watching through the camera for a
    // while) and `callmute` (Atlas on a call) -- only you can see the camera
    // keep its word and hear the call.
    assert!(eyes_n <= 10, "your part is genuinely small: {eyes_n}");
    assert!(reply.contains(&eyes_n.to_string()), "the reported your-eyes count is the real one: {reply}");
}
