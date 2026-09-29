//! Typing to Atlas used to reach the phrase list and nothing else.
//!
//! Everything needed for conversation was built and working — but only by
//! voice. `Daemon::turn` assembles the context (focused window, recent files,
//! standing corrections, the conversation so far), hands an unrecognised
//! sentence to `Brain::decide`, and records the exchange in `thread` so the
//! next line can refer to the last one.
//!
//! The typed path did none of that. `prompt_line` parsed the line against the
//! deterministic phrase list and ran whatever came back — `execute_timed`'s
//! own note says it outright: the one-shot path "parses and executes without
//! going through a turn at all". So a sentence the list did not know came back
//! "I didn't catch that", with no model consulted and nothing written down.
//!
//! That is the whole of why typing felt scripted: not a missing feature, a
//! door that was never connected to the room behind it.
//!
//! These tests work against the daemon directly, which is what `prompt_line`
//! now calls. They pin the two properties that make it a conversation: the
//! model is reached, and the thread remembers.

use atlas::brain::Llm;
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::error::Result;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-typing-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}

/// Answers in conversation, and counts how often it was consulted. Also keeps
/// the last context it was given, so a test can check what Atlas knew.
struct Talker {
    calls: AtomicU32,
    last_ctx: std::sync::Mutex<String>,
    /// Every context it was handed. Atlas asks a second time, without the
    /// conversation, when a reply is word for word its last one (this mock
    /// always says the same thing), so the last context alone isn't the
    /// one the turn was built with.
    all_ctx: std::sync::Mutex<Vec<String>>,
}
impl Talker {
    fn new() -> Self {
        Talker { calls: AtomicU32::new(0), last_ctx: std::sync::Mutex::new(String::new()), all_ctx: Default::default() }
    }
}
impl Llm for Talker {
    fn complete(&self, _: &str, user: &str) -> Result<String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.last_ctx.lock().unwrap() = user.to_string();
        self.all_ctx.lock().unwrap().push(user.to_string());
        Ok(r#"{"action":"say","arg":null,"say":"The rack would take a 10-inch shelf, yes."}"#
            .into())
    }
}

#[test]
fn a_sentence_the_phrase_list_does_not_know_reaches_the_model() {
    let (c, p) = (cfg(), plat());
    let talker = Arc::new(Talker::new());
    let mut d = Daemon::new(
        &c,
        &p,
        Some(talker.clone()),
        Store::new(tmp("reaches")),
        Proactive::new(ProactiveConfig::default()),
    );

    let reply = d.turn("do you think the mini rack is worth the shelf space", 100);

    assert_eq!(
        talker.calls.load(Ordering::SeqCst),
        1,
        "the model was never consulted, so this was the phrase list answering"
    );
    assert!(
        !reply.to_lowercase().contains("didn't catch"),
        "an ordinary sentence came back as a failure to hear: {reply}"
    );
    assert!(reply.contains("10-inch shelf"), "the model's answer did not reach you: {reply}");
}

#[test]
fn the_second_thing_you_say_knows_about_the_first() {
    // The property that makes it a conversation rather than a series of
    // unrelated questions. `thread` was already doing this for voice; typing
    // never wrote to it because it never took a turn.
    let (c, p) = (cfg(), plat());
    let talker = Arc::new(Talker::new());
    let mut d = Daemon::new(
        &c,
        &p,
        Some(talker.clone()),
        Store::new(tmp("thread")),
        Proactive::new(ProactiveConfig::default()),
    );

    d.turn("what do you make of the mini rack idea", 100);
    d.turn("and the noise", 160);

    let ctx = talker
        .all_ctx
        .lock()
        .unwrap()
        .iter()
        .find(|c| c.contains("and the noise"))
        .cloned()
        .unwrap_or_default();
    assert!(
        ctx.contains("Conversation so far"),
        "the second turn carried no conversation at all:\n{ctx}"
    );
    assert!(
        ctx.contains("mini rack"),
        "the second turn did not know what the first was about:\n{ctx}"
    );
}

#[test]
fn a_known_phrase_still_answers_without_the_model() {
    // The reason the deterministic list exists: instant, and it cannot
    // misread you. Opening conversation up must not route everything through
    // a model that is slower and less certain.
    let (c, p) = (cfg(), plat());
    let talker = Arc::new(Talker::new());
    let mut d = Daemon::new(
        &c,
        &p,
        Some(talker.clone()),
        Store::new(tmp("known")),
        Proactive::new(ProactiveConfig::default()),
    );

    d.turn("rebuild the index", 100);

    assert_eq!(
        talker.calls.load(Ordering::SeqCst),
        0,
        "a phrase Atlas knows word-for-word was sent to the model anyway"
    );
}

// ---------- the prompt is allowed to hold a conversation ----------

#[test]
fn the_prompt_no_longer_caps_every_reply_at_one_short_sentence() {
    // The cap was real and it was the whole ceiling on conversation: "say is
    // spoken aloud. One short sentence." Being heard is a genuine constraint;
    // one sentence is not the only way to honour it.
    // Whitespace-normalised: the prompt is hard-wrapped, so a rule can sit
    // across a line break and a naive `contains` would miss it. The first
    // draft of this test did exactly that and failed on a rule that was
    // present.
    let flat = atlas::brain::ACTION_SCHEMA
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        !flat.contains("One short sentence"),
        "the one-sentence cap is still in the prompt"
    );
    assert!(
        flat.contains("spoken aloud"),
        "the reason for the constraint should survive the constraint changing"
    );
    assert!(
        flat.contains("no markdown"),
        "spoken output still must not be markdown"
    );
}

#[test]
fn talking_is_described_as_the_ordinary_case() {
    // `say` was listed as "no action needed, just answer" — the leftover
    // branch after twelve real actions. Most of what anyone says to an
    // assistant is conversation, so it is the common case, and the prompt
    // should say so.
    let p = atlas::brain::ACTION_SCHEMA;
    assert!(
        p.contains("the ordinary case"),
        "conversation is still framed as the leftover branch"
    );
}

// ---------- the typed door is actually connected ----------

#[test]
fn the_typed_path_takes_a_turn_rather_than_executing_an_intent() {
    // The tests above drive `Daemon::turn` directly, which proves the room is
    // there. This one proves the door leads to it. Without it, someone could
    // put `prompt_line` back to `execute_timed` and every test above would
    // still pass while typing went back to being a phrase list.
    //
    // A source check because `prompt_line` is in the binary, not the library.
    // Coarse, and worth it: the whole bug was one call site.
    let src = crate::common::source_of("main");
    let start = src.find("fn prompt_line(").expect("prompt_line still exists");
    let body = &src[start..start + 4000.min(src.len() - start)];
    // To the closing brace at the function's own depth (29 Sep 2026): cutting at the next plain `fn` at that depth ran on past `pub(super) fn`s once main.rs and daemon.rs were split.
    let end = body.find("\n}\n").unwrap_or(body.len());
    let body = &body[..end];

    assert!(
        body.contains("d.turn("),
        "prompt_line no longer takes a turn, so typing has gone back to \
         reaching the phrase list and nothing else"
    );
    assert!(
        !body.contains("d.execute_timed("),
        "prompt_line is executing a parsed intent again, which skips the \
         model, the context and the conversation"
    );
}
