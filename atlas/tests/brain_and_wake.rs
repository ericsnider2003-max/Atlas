use atlas::b64;
use atlas::brain::{dig, parse_decision, Brain, Decision, Llm, MockLlm};
use atlas::config::Config;
use atlas::error::{AtlasError, Result};
use atlas::intent::{Intent, Parser};
use atlas::voice::{loose, ToolsConfig};
use std::path::Path;

fn parser() -> Parser {
    Parser::new(&Config::load(Path::new("config")).unwrap().commands)
}

struct BrokenLlm;
impl Llm for BrokenLlm {
    fn complete(&self, _: &str, _: &str) -> Result<String> {
        Err(AtlasError::Platform("connection refused".into()))
    }
}

struct SpyLlm(std::sync::atomic::AtomicU32);
impl Llm for SpyLlm {
    fn complete(&self, _: &str, _: &str) -> Result<String> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(r#"{"action":"say","arg":null,"say":"hi"}"#.into())
    }
}

// ---------- the model is an enhancement, not a dependency ----------

#[test]
fn known_phrases_never_reach_the_model() {
    let p = parser();
    let spy = SpyLlm(std::sync::atomic::AtomicU32::new(0));
    let b = Brain { llm: &spy, fallback: &p, voice: None };
    b.decide("boot workspace", "");
    b.decide("open chrome", "");
    assert_eq!(spy.0.load(std::sync::atomic::Ordering::SeqCst), 0, "fixed phrases must not cost a model call");
}

#[test]
fn unknown_speech_does_reach_the_model() {
    let p = parser();
    let spy = SpyLlm(std::sync::atomic::AtomicU32::new(0));
    Brain { llm: &spy, fallback: &p, voice: None }.decide("could you tidy things up a bit", "");
    assert_eq!(spy.0.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn workspace_still_boots_when_the_model_is_down() {
    // The whole point: an unreachable LLM must not break basic control.
    let p = parser();
    let b = Brain { llm: &BrokenLlm, fallback: &p, voice: None };
    assert_eq!(b.decide("boot workspace", "").intent, Intent::WorkspaceOn);
}

#[test]
fn model_being_down_degrades_gracefully_for_unknown_speech() {
    let p = parser();
    let d = Brain { llm: &BrokenLlm, fallback: &p, voice: None }.decide("what's the weather", "");
    assert!(matches!(d.intent, Intent::Say(_)));
    assert!(d.say.contains("unreachable"), "should say why: {}", d.say);
}

#[test]
fn model_garbage_does_not_panic_or_execute_anything() {
    let p = parser();
    let junk = MockLlm("I'm afraid I can't do that".into());
    let d = Brain { llm: &junk, fallback: &p, voice: None }.decide("do the thing", "");
    assert!(matches!(d.intent, Intent::Say(_)), "must not invent an action");
}

// ---------- decoding what the model says ----------

#[test]
fn plain_json_decodes() {
    let d = parse_decision(r#"{"action":"open_app","arg":"chrome","say":"Opening Chrome."}"#).unwrap();
    assert_eq!(d.intent, Intent::OpenApp("chrome".into()));
    assert_eq!(d.say, "Opening Chrome.");
}

#[test]
fn json_wrapped_in_prose_or_code_fences_still_decodes() {
    // Small local models do this constantly.
    let fenced = "Sure!\n```json\n{\"action\":\"workspace_on\",\"say\":\"Working.\"}\n```\nHope that helps.";
    assert_eq!(parse_decision(fenced).unwrap().intent, Intent::WorkspaceOn);
}

#[test]
fn an_invented_action_is_rejected_not_guessed_at() {
    let e = parse_decision(r#"{"action":"format_c_drive","say":"ok"}"#).unwrap_err();
    assert!(e.to_string().contains("invented"), "{e}");
}

#[test]
fn missing_fields_do_not_panic() {
    assert!(parse_decision("{}").is_err());
    assert!(parse_decision("not json at all").is_err());
}

#[test]
fn ask_carries_the_question_through_to_speech() {
    let d = parse_decision(r#"{"action":"ask","say":"Which browser?"}"#).unwrap();
    assert_eq!(d.intent, Intent::Ask("Which browser?".into()));
    assert_eq!(d.say, "Which browser?");
}

// ---------- response_path navigation ----------

#[test]
fn dig_walks_objects_and_array_indexes() {
    let v: serde_json::Value =
        serde_json::from_str(r#"{"content":[{"text":"hello"}],"response":"hi"}"#).unwrap();
    assert_eq!(dig(&v, "response").unwrap(), "hi");            // Ollama shape
    assert_eq!(dig(&v, "content.0.text").unwrap(), "hello");   // Anthropic shape
    assert!(dig(&v, "content.9.text").is_none());
    assert!(dig(&v, "nope").is_none());
}

// ---------- base64 for vision ----------

#[test]
fn base64_matches_the_spec() {
    assert_eq!(b64::encode(b""), "");
    assert_eq!(b64::encode(b"f"), "Zg==");
    assert_eq!(b64::encode(b"fo"), "Zm8=");
    assert_eq!(b64::encode(b"foo"), "Zm9v");
    assert_eq!(b64::encode(b"foobar"), "Zm9vYmFy");
    assert_eq!(b64::encode(&[0xFF, 0x00, 0xFF]), "/wD/");
}

// ---------- wake word matching ----------

#[test]
fn loose_normalization_collapses_punctuation_and_case() {
    assert_eq!(loose("Hey, Atlas!"), "heyatlas");
    assert_eq!(loose("HEY ATLAS."), "heyatlas");
}

// ---------- shipped config wires the brain up ----------

#[test]
fn shipped_tools_yaml_declares_a_model_and_a_webcam() {
    let y = std::fs::read_to_string("config/tools.yaml").unwrap();
    let t: ToolsConfig = serde_yaml::from_str(&y).unwrap();
    // No hand-written `llm:` since 26 Sep 2026: it overrode the model Atlas
    // downloads for itself. The connection is built from `models:`.
    assert!(t.llm.is_none(), "a shipped llm: overrides the model in models/");
    assert!(!t.models.dir.is_empty() && t.models.server.is_some(), "the model Atlas runs itself");
    assert!(t.capture_webcam.is_some());
    assert!(t.wake.is_some());
}

#[test]
fn default_decision_helper_gives_short_spoken_lines() {
    // Spoken confirmations that run long are unusable.
    for i in [Intent::WorkspaceOn, Intent::OpenApp("chrome".into()), Intent::CaptureWebcam] {
        let s = atlas::brain::default_say(&i);
        assert!(s.len() < 40, "too long to speak: {s}");
        assert!(!s.contains('\n'));
    }
}

#[test]
fn decision_is_comparable_for_tests() {
    let a = Decision { intent: Intent::WorkspaceOn, say: "Working.".into(), model: atlas::brain::Reached::NotNeeded };
    assert_eq!(a.clone(), a);
}

/// Every command the model may never choose is refused when the model names
/// it (audit Q3, 5 Oct 2026). Thirteen of them used to be accepted: a second
/// copy of the command table in `parse_decision` matched them before the
/// check ran -- among them that the other side of a call agreed to be
/// recorded, "this is me", and a lesson applied to Atlas itself.
#[test]
fn the_model_cannot_choose_what_only_you_may_say() {
    for name in atlas::intent::NEVER_FOR_THE_MODEL {
        let reply = format!(r#"{{"action":"{name}","arg":"x","say":"ok"}}"#);
        assert!(parse_decision(&reply).is_err(), "the model chose \"{name}\" and it was accepted");
    }
    // And an ordinary command still reaches the same intent the phrases do.
    let d = parse_decision(r#"{"action":"close_app","arg":"notepad","say":"Closing it."}"#).unwrap();
    assert_eq!(d.intent, Intent::CloseApp("notepad".into()));
}
