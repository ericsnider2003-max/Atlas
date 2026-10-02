//! Eric's own sentences from 1 Oct 2026, where Atlas misunderstood him.
//! Each one is held to what it should have done.

use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::Path;

fn daemon_at<'a>(c: &'a atlas::config::Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    let dir = std::env::temp_dir().join(format!("atlas-eric-1oct-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    Daemon::new(c, p, None, Store::new(dir), Proactive::new(ProactiveConfig::default()))
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

/// "that" and "it" inside a research request are ordinary English, not
/// "research *this*" pointing at the clipboard.
#[test]
fn a_research_request_with_that_or_it_in_it_is_researched() {
    let c = atlas::config::Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = daemon_at(&c, &p, "research-that");
    let t = 1_790_899_251;
    for said in [
        "research things that would allow you to advance your own capabilities",
        "research how an AI system can gain the ability to add its own features when it gets approval from the user",
    ] {
        let reply = d.turn(said, t);
        assert!(!reply.contains("can't tell what you mean"), "{said} -> {reply}");
    }
    // "research this" with nothing to point at still asks, in a fresh Atlas.
    let mut fresh = daemon_at(&c, &p, "research-this");
    let reply = fresh.turn("research this", t + 10);
    assert!(reply.contains("can't tell what you mean") || reply.contains("haven't been given a topic"), "{reply}");
}

/// A long sentence keeps its own "it": the last topic isn't swapped in.
#[test]
fn a_long_sentence_keeps_its_own_it() {
    let c = atlas::config::Config::load(Path::new("config")).unwrap();
    let p = plat();
    let mut d = daemon_at(&c, &p, "keeps-it");
    d.referents.last_topic = Some("Practical ways to extend this Atlas assistant's capabilities".into());
    let said = "I want you to research how an AI system can gain the ability to add its own features when it gets approval from the user.";
    let _ = d.turn(said, 1_790_899_302);
    let kept = d.thread.said_earlier(said).map(|e| e.said.clone()).unwrap_or_default();
    assert_eq!(kept, said, "the words reached Atlas changed");
    assert_eq!(kept.matches("Practical ways").count(), 0);
}

/// Atlas knows it can grow with your yes, and never says it can't.
#[test]
fn atlas_knows_it_can_add_abilities_with_your_yes() {
    let persona = atlas::persona::Persona::default();
    let who = persona.character();
    assert!(who.contains("work on yourself"), "{who}");
    assert!(who.contains("can't add abilities to yourself"), "the false limit isn't named as one: {who}");
}

/// "Speak." was answered with a two-sentence research brief read out as
/// "We were on ...". The topic is said as its first clause.
#[test]
fn the_topic_we_were_on_is_said_briefly() {
    let brief = "Practical ways to extend this Atlas assistant's capabilities, especially continuous camera viewing with explicit consent and a clear stop control, persistent local memory. Prioritize currently available tools.";
    assert_eq!(atlas::thread::spoken_topic(brief), "Practical ways to extend this Atlas assistant's capabilities");
    assert_eq!(atlas::thread::spoken_topic("tide times at ventura"), "tide times at ventura");
}
