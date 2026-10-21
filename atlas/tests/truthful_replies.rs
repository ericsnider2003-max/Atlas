//! **Replies that say only what happened (20 Oct 2026).**
//!
//! The installed build's free online model (no tools, no local model)
//! answered "Create a text file ... then verify the file" with "On it. Making
//! atlas-test.txt ... I'll take a look at it to confirm once it's there." --
//! and made nothing; "quit Atlas" got "Sure, taking a break" and nothing
//! stopped. Run with `--test-threads=1`: the quit test rings the process-wide
//! stop flag and resets it.

use atlas::brain::{Brain, Llm, Turn};
use atlas::daemon::Daemon;
use atlas::intent::Intent;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn cfg() -> atlas::config::Config {
    let mut c = atlas::config::Config::load(Path::new("config")).unwrap();
    c.indexing = None;
    c
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor {
        id: 1,
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
        primary: true,
    }])
}

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-truthful-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// What the free online model said, word for word, whatever it was asked.
const FREE_ONLINE: &str = "It's been a while. We were on Practical ways to extend this Atlas assistant's capabilities. On it. Making atlas-test.txt on your Desktop with today's date, 20 October 2026. I'll take a look at it to confirm once it's there.";

/// A model with no tools (`native_chat` false, like the free online one):
/// every call gets the same words.
struct NoTools(&'static str);

impl Llm for NoTools {
    fn complete(&self, _: &str, _: &str) -> atlas::error::Result<String> {
        Ok(serde_json::json!({ "action": "say", "arg": null, "say": self.0 }).to_string())
    }
}

fn daemon(tag: &str, said: &'static str) -> Daemon<'static> {
    let c: &'static atlas::config::Config = Box::leak(Box::new(cfg()));
    let p: &'static MockPlatform = Box::leak(Box::new(plat()));
    Daemon::new(
        c,
        p,
        Some(Arc::new(NoTools(said)) as Arc<dyn Llm>),
        Store::new(tmp(tag)),
        Proactive::new(ProactiveConfig::default()),
    )
}

const ASK: &str = "Create a text file named atlas-test.txt in my Desktop folder containing the current date, then verify the file.";

/// A request the floor does not do itself (20 Oct 2026: the file sentence above is now made by
/// plain code before any model is asked, `tests/the_floor_without_a_model.rs`), so it still reaches
/// the model and the model's claim of work is what is being checked.
const ASK_MODEL: &str =
    "Tidy up my expenses spreadsheet by adding a totals row, then verify the sums.";

#[test]
fn a_toolless_model_cannot_claim_a_file_was_made() {
    let mut d = daemon("claim", FREE_ONLINE);
    let reply = d.turn(ASK_MODEL, 1_790_740_000);
    let low = reply.to_lowercase();
    assert!(!low.contains("on it"), "{reply}");
    assert!(!low.contains("making atlas-test"), "{reply}");
    assert!(!low.contains("take a look"), "{reply}");
    assert!(low.contains("no language model that can act"), "{reply}");
    assert!(low.contains("nothing was started"), "{reply}");
    assert!(reply.contains("Connections"), "{reply}");
}

#[test]
fn the_words_passed_on_to_be_spoken_are_checked_too() {
    let c = cfg();
    let parser = atlas::intent::Parser::new(&c.commands);
    let llm = NoTools(FREE_ONLINE);
    let brain = Brain {
        llm: &llm,
        fallback: &parser,
        voice: None,
    };
    let turn = Turn {
        said: ASK.into(),
        one_prompt: String::new(),
        skip_phrases: true,
        ..Default::default()
    };
    let mut spoken = String::new();
    let d = brain.converse(&turn, &mut |w| {
        spoken.push_str(w);
        true
    });
    assert!(
        !spoken.to_lowercase().contains("on it"),
        "said aloud: {spoken}"
    );
    assert!(
        spoken.contains("nothing was started"),
        "said aloud: {spoken}"
    );
    let Intent::Say(s) = d.intent else {
        panic!("{:?}", d.intent)
    };
    assert!(!s.contains("Making atlas-test"), "{s}");
}

#[test]
fn did_it_finish_with_no_job_says_there_is_no_job() {
    let mut d = daemon(
        "status",
        "The backup of C:\\Users\\erics\\atlas-e2e-backup-test is still in progress; once it completes you'll find the copied folder there.",
    );
    let reply = d.turn("did the backup of that folder finish?", 1_790_740_000);
    assert!(!reply.contains("still in progress"), "{reply}");
    assert!(reply.contains("Outstanding"), "{reply}");
}

#[test]
fn quit_atlas_stops_atlas_and_says_so() {
    let c = cfg();
    let parser = atlas::intent::Parser::new(&c.commands);
    for said in [
        "quit Atlas",
        "quit",
        "exit Atlas",
        "close Atlas",
        "stop Atlas",
        "shut down Atlas",
        "turn off Atlas",
        "Please quit Atlas.",
    ] {
        assert_eq!(parser.parse(said), Intent::QuitAtlas, "{said}");
    }
    // Not these: other commands, or plain talk.
    for said in [
        "stop",
        "stop listening",
        "quit smoking",
        "close the deal",
        "pause",
    ] {
        assert_ne!(parser.parse(said), Intent::QuitAtlas, "{said}");
    }
    atlas::goodbye::reset_for_test();
    let mut d = daemon("quit", "Sure, taking a break");
    let reply = d.turn("quit Atlas", 1_790_740_000);
    let stopped = atlas::goodbye::asked_to_stop();
    atlas::goodbye::reset_for_test();
    assert!(stopped, "nothing asked the daemon to stop");
    assert_eq!(reply, atlas::backed::STOPPING);
}

#[test]
fn recording_a_clip_is_answered_with_the_reason_and_the_closest_thing() {
    let mut d = daemon("mic", "I can't record audio from your microphone.");
    let reply = d.turn(
        "record 5 seconds from my microphone and tell me what you heard",
        1_790_740_000,
    );
    assert_eq!(reply, atlas::backed::NO_CLIP_RECORDING);
}
