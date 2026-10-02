//! What ranking three talking models on Atlas's own sentences turned up (1
//! Oct 2026), fixed so no model has to get it right: everyday ways of
//! asking that reached nothing, a reminder that reached "sign in", "I've
//! noted that" with nothing noted, and a plain question refused as not code.

use atlas::daemon::Daemon;
use atlas::intent::Parser;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn cfg() -> atlas::config::Config {
    atlas::config::Config::load(Path::new("config")).unwrap()
}
fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-ranking-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}
const NOW: u64 = 1_790_776_800;

#[test]
fn everyday_ways_of_asking_reach_the_command() {
    let c = cfg();
    let p = Parser::new(&c.commands);
    for (said, want) in [
        ("jot down that the car insurance renews in march", "capture"),
        ("has anybody written to me today", "mail"),
        ("where did I put that lease agreement", "find_file"),
        ("can you see what I've got open right now", "view_display"),
        ("dig into the best budget mechanical keyboards and write it up for me", "research"),
    ] {
        let (_, name) = p.parse_named(said);
        assert_eq!(name.as_deref(), Some(want), "{said}");
    }
}

#[test]
fn a_nudge_is_a_reminder() {
    let c = cfg();
    let plat = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let mut d = Daemon::new(&c, &plat, None, Store::new(tmp("nudge")), Proactive::new(ProactiveConfig::default()));
    let said = d.turn("give me a nudge in 20 minutes to call the dentist", NOW);
    assert!(said.contains("remind you to call the dentist"), "{said}");
    // And it's really booked: due twenty minutes on, not before.
    assert!(d.scheduler.due(NOW + 19 * 60).is_empty());
    assert_eq!(d.scheduler.due(NOW + 21 * 60).len(), 1);
}

#[test]
fn saying_it_was_noted_without_noting_it_is_caught() {
    for s in ["I've noted that your car insurance renews in March.", "I've pulled your Friday schedule.", "I've just added it to your calendar."] {
        assert!(atlas::backed::claims_work_started(s), "{s}");
    }
    assert!(!atlas::backed::claims_work_started("The capital of Australia is Canberra."));
}

/// Eric, 1 Oct 2026: everything a capability needs, Atlas downloads itself.
#[test]
fn atlas_fetches_the_rest_of_what_it_needs_itself() {
    let rest = atlas::getpieces::everything_else();
    let keys: Vec<&str> = rest.iter().map(|p| p.key_path()).collect();
    for want in ["models/kws/", "models/parakeet/", "models/campplus_en_voxceleb.onnx"] {
        assert!(keys.iter().any(|k| k.starts_with(want)), "{want} isn't fetched: {keys:?}");
    }
    // Nothing setup already fetches, nothing twice.
    let setup: Vec<&str> = atlas::getpieces::setup_pieces().iter().map(|p| p.key_path()).collect();
    assert!(keys.iter().all(|k| !setup.contains(k)));
    let mut sorted = keys.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), keys.len());
    // Not the model whose tool calls failed in the pinned server.
    assert!(!rest.iter().any(|p| p.sha256 == atlas::getpieces::better_talk_model().sha256));
    // Every code checker archive is pinned (Windows only; empty elsewhere).
    for p in atlas::codetools::tool_pieces() {
        assert_eq!(p.sha256.len(), 64);
        assert!(p.url.starts_with("https://"));
    }
}

/// "pull up chrome for me" and "show me what jobs you've found" (round two:
/// "I can't put spotify for me on screen", "...what jobs youve found...").
#[test]
fn pulling_up_an_app_or_the_jobs_is_not_a_missing_panel() {
    let c = cfg();
    let plat = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let mut d = Daemon::new(&c, &plat, None, Store::new(tmp("pullup")), Proactive::new(ProactiveConfig::default()));
    let jobs = d.turn("show me what jobs you've found", NOW);
    assert!(!jobs.contains("on screen"), "{jobs}");
    let app = c.apps.apps.keys().next().expect("a configured app").clone();
    let said = d.turn(&format!("pull up {app} for me"), NOW + 60);
    assert!(!said.contains("for me on screen"), "{said}");
    let p = Parser::new(&c.commands);
    assert_eq!(p.parse_named("keep in mind that my passport expires in june").1.as_deref(), Some("capture"));
}

/// From the updated research report (section 10): "learn from book.pdf"
/// learned the path as a fact, and a PDF Atlas had read was never findable.
#[test]
fn a_file_that_isnt_there_is_said_not_learned_as_a_fact() {
    let c = cfg();
    let plat = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let mut d = Daemon::new(&c, &plat, None, Store::new(tmp("learnpdf")), Proactive::new(ProactiveConfig::default()));
    let before = d.facts.facts.len();
    let said = d.turn("learn from C:/Books/nowhere-at-all.pdf", NOW);
    assert!(said.contains("can't find a file"), "{said}");
    assert_eq!(d.facts.facts.len(), before, "the path was learned as a fact");
}

#[test]
fn what_atlas_has_read_is_found_again_with_where_it_says_so() {
    let dir = tmp("readings");
    std::fs::write(dir.join("lease.txt"), "Intro line.\n\nThe tenant must give sixty days notice before moving out.\n\nRent is due on the first.\n").unwrap();
    let mut lib = atlas::recall::Library::default();
    atlas::recall::add_readings(&mut lib, &dir);
    assert!(!lib.pieces.is_empty());
    let hit = lib.pieces.iter().find(|p| p.text.contains("sixty days")).expect("the notice clause is a piece");
    assert!(hit.source.ends_with(&format!("lease.txt:{}", hit.source.rsplit(':').next().unwrap())), "{}", hit.source);
    assert!(hit.source.contains("lease.txt:"), "cites file and lines: {}", hit.source);
}

/// The "why stale" report (1 Oct 2026): after Atlas had opened Chrome,
/// "Why is it…" reached the model as "Why is chrome…", every sentence
/// after. Free conversation keeps your exact words; "close it" still means
/// the app.
struct Heard(std::sync::Mutex<Vec<String>>);
impl atlas::brain::Llm for Heard {
    fn complete(&self, system: &str, user: &str) -> atlas::error::Result<String> {
        self.0.lock().unwrap().push(format!("{system}\n{user}"));
        Ok("Probably not yet.".into())
    }
}

#[test]
fn your_words_reach_the_model_as_you_said_them() {
    let c = cfg();
    let plat = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let heard = std::sync::Arc::new(Heard(Default::default()));
    let mut d = Daemon::new(&c, &plat, Some(heard.clone()), Store::new(tmp("itchrome")), Proactive::new(ProactiveConfig::default()));
    let app = c.apps.apps.keys().next().expect("a configured app").clone();
    d.turn(&format!("open {app}"), NOW);
    heard.0.lock().unwrap().clear();
    d.turn("would it be smart to upgrade my phone this year", NOW + 30);
    let seen = heard.0.lock().unwrap().join("\n---\n").to_lowercase();
    assert!(seen.contains("would it be smart to upgrade my phone"), "the model never saw the sentence: {seen}");
    assert!(!seen.contains(&format!("would {} be smart", app.to_lowercase())), "rewritten: {seen}");
    // And it's kept as said: the thread is what later turns read.
    assert_eq!(d.thread.recent.last().map(|e| e.said.as_str()), Some("would it be smart to upgrade my phone this year"));
}

/// The later list takes your words, never Atlas's own reply unless you said
/// "that" (the "why stale" report, 1 Oct 2026).
#[test]
fn the_later_list_holds_what_you_said() {
    let c = cfg();
    let plat = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let mut d = Daemon::new(&c, &plat, None, Store::new(tmp("later")), Proactive::new(ProactiveConfig::default()));
    let said = d.turn("add call the bank about the wire to my later list", NOW);
    assert!(said.contains("call the bank about the wire"), "{said}");
    let list = d.turn("what's on my later list", NOW + 10);
    assert!(list.contains("call the bank about the wire"), "{list}");
    assert_eq!(atlas::kws::misheard_name_opening("List, did you hear me?").as_deref(), Some("did you hear me?"));
}

/// Every model ranked reached for the capability list on "what should I make
/// for dinner" or "where did I put that lease". Not about Atlas: not that tool.
#[test]
fn the_capability_list_is_for_questions_about_atlas() {
    use atlas::brain::{decision_from_chat, ChatReply, ToolCall};
    use atlas::intent::Intent;
    let call = |said: &str| {
        let r = ChatReply { text: "Sure.".into(), tool_calls: vec![ToolCall { name: "capabilities".into(), arguments: serde_json::json!({}) }] };
        decision_from_chat(&r, said).intent
    };
    assert!(!matches!(call("what should I make for dinner with chicken and rice"), Intent::Capabilities(_)));
    assert!(!matches!(call("where did I put that lease agreement"), Intent::Capabilities(_)));
    assert!(matches!(call("what else can you do for me"), Intent::Capabilities(_)));
}

/// Phase 1 of the "why stale" report: one guided conversation fills Atlas
/// with your life, kept as things you said.
#[test]
fn get_to_know_me_fills_the_stores_from_your_answers() {
    let c = cfg();
    let plat = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let mut d = Daemon::new(&c, &plat, None, Store::new(tmp("getknow")), Proactive::new(ProactiveConfig::default()));
    let before = d.facts.facts.len();
    let first = d.turn("get to know me", NOW);
    assert!(first.contains("what should I call you"), "{first}");
    d.turn("Call me Eric", NOW + 10);
    d.turn("my trading system, my YouTube channel and Atlas", NOW + 20);
    d.turn("skip", NOW + 30);
    d.turn("posting twice a week", NOW + 40);
    d.turn("Desktop and Dropbox", NOW + 50);
    let end = d.turn("no", NOW + 60);
    assert!(end.contains("Here's what I kept") && end.contains("YouTube channel"), "{end}");
    assert_eq!(d.facts.facts.len(), before + 6, "name, three projects, push, folders");
    assert!(d.interview.is_none());
    // A question part way through ends it rather than being kept as an answer.
    d.turn("get to know me", NOW + 100);
    let _ = d.turn("what time is it?", NOW + 110);
    assert!(d.interview.is_none());
}
