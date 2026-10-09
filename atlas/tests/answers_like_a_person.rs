//! Found by Eric's friend on 26 Sep 2026: "Atlas would only respond in
//! reports… literally making a document to reply."
//!
//! Traced to five things, each fixed and held here:
//!
//! 1. Atlas's own model connection was built on `research.fetch`, which ships
//!    as a headless Chrome: every question went to the model as a bare GET
//!    and the answer came back as a web page.
//! 2. A reply that wasn't the action JSON was quoted back raw ("I couldn't
//!    work that out (no JSON in model reply: <html>…)").
//! 3. "How do I …" matched Atlas's own internal procedures on any shared
//!    4-letter substring, so pancakes got the steps for reading a document.
//! 4. Anything Atlas didn't recognise was treated as an action needing
//!    approval ("I didn't catch that. Go ahead?"), and the next thing said
//!    was taken as the yes or no; ordinary questions without a question mark
//!    got "think about it with you, or just listen?", with the same hijack.
//! 5. The shipped config pointed at Ollama, which overrode the model Atlas
//!    downloads for itself.

use atlas::brain::{spoken_text, Brain, Llm, Reached};
use atlas::error::Result;
use atlas::intent::{Intent, Parser};
use std::path::Path;
use std::sync::Mutex;

/// Answers from a script, in order, and remembers what it was asked.
struct Scripted {
    replies: Mutex<Vec<String>>,
    asked: Mutex<Vec<(String, String)>>,
}

impl Scripted {
    fn new(r: &[&str]) -> Scripted {
        Scripted { replies: Mutex::new(r.iter().rev().map(|s| s.to_string()).collect()), asked: Mutex::new(vec![]) }
    }
    fn calls(&self) -> usize {
        self.asked.lock().unwrap().len()
    }
}

impl Llm for Scripted {
    fn complete(&self, system: &str, user: &str) -> Result<String> {
        self.asked.lock().unwrap().push((system.to_string(), user.to_string()));
        Ok(self.replies.lock().unwrap().pop().unwrap_or_default())
    }
}

fn parser() -> Parser {
    let c = atlas::config::Config::load(Path::new("config")).unwrap();
    Parser::new(&c.commands)
}

// ------------------------------------------------------------------ 1

#[test]
fn the_model_server_is_posted_to_not_opened_in_a_browser() {
    let post = atlas::models::server_post();
    assert_eq!(post.command, "curl");
    assert!(post.stdin_text, "the request body goes in on stdin");
    assert!(post.args.windows(2).any(|w| w == ["-X", "POST"]));
    assert!(post.args.contains(&"{url}".to_string()));
    assert!(!post.args.iter().any(|a| a.contains("dump-dom") || a.contains("headless")));
    let get = atlas::models::server_get();
    assert!(!get.stdin_text && get.command == "curl");

    // And the program builds its connection with it, not with research's fetch.
    // `models::connection`, which the desktop program's doors and the phone
    // core both use (since 27 Sep 2026).
    let main = crate::common::source_of("models");
    let body = &main[main.find("pub fn connection(").unwrap()..];
    let body = &body[..body.find("\n}\n").unwrap()];
    assert!(body.contains("server_post()"), "{body}");
    assert!(!body.contains("research.fetch.as_ref()"), "a web browser is not a model client");
}

// ------------------------------------------------------------------ 2

#[test]
fn a_web_page_or_raw_json_is_never_read_out() {
    assert_eq!(spoken_text("<html><head></head><body><pre>File Not Found</pre></body></html>"), None);
    assert_eq!(spoken_text("{\"action\": \"ask\"}"), None);
    assert_eq!(spoken_text("```json\n{}\n```"), None);
    assert_eq!(spoken_text("## Paris\n- **Paris** is the capital."), Some("Paris Paris is the capital.".into()));
    assert_eq!(spoken_text("You: I'm fine."), Some("I'm fine.".into()));
    let looped = "Ideas: plan a party with a theme, ".repeat(200);
    let cut = spoken_text(&looped).unwrap();
    assert!(cut.len() < 100, "a loop is said once: {cut}");
}

#[test]
fn a_reply_that_is_not_the_schema_is_asked_again_plainly() {
    let p = parser();
    // A web page, then a proper answer.
    let llm = Scripted::new(&["<html><body>File Not Found</body></html>", "The capital of France is Paris."]);
    let d = Brain { llm: &llm, fallback: &p, voice: None }.decide("what's the capital of france", "");
    assert_eq!(d.say, "The capital of France is Paris.");
    assert_eq!(d.intent, Intent::Say("The capital of France is Paris.".into()));
    assert_eq!(d.model, Reached::Yes);
    assert_eq!(llm.calls(), 2);
    let second = &llm.asked.lock().unwrap()[1].0;
    assert!(second.contains("no JSON, no markdown"), "asked to just talk: {second}");

    // Plain prose first time is simply used.
    let llm = Scripted::new(&["Paris."]);
    let d = Brain { llm: &llm, fallback: &p, voice: None }.decide("what's the capital of france", "");
    assert_eq!((d.say.as_str(), llm.calls()), ("Paris.", 1));

    // An empty "say" is not an answer.
    let llm = Scripted::new(&["{\"action\":\"ask\",\"arg\":\"x\"}", "Paris."]);
    let d = Brain { llm: &llm, fallback: &p, voice: None }.decide("what's the capital of france", "");
    assert_eq!(d.say, "Paris.");

    // Nothing usable either time: said as such, never the raw reply.
    let llm = Scripted::new(&["<html></html>", "<html></html>"]);
    let d = Brain { llm: &llm, fallback: &p, voice: None }.decide("what's the capital of france", "");
    assert!(d.say.starts_with("I didn't get a usable answer") && !d.say.contains('<'), "{}", d.say);
}

#[test]
fn an_action_nobody_asked_for_is_dropped_and_the_answer_kept() {
    let p = parser();
    let llm = Scripted::new(&["{\"action\":\"focus_app\",\"arg\":\"chrome\",\"say\":\"The capital of France is Paris.\"}"]);
    let d = Brain { llm: &llm, fallback: &p, voice: None }.decide("what's the capital of france", "");
    assert_eq!(d.intent, Intent::Say("The capital of France is Paris.".into()));
    // Named, it's a real request.
    let llm = Scripted::new(&["{\"action\":\"focus_app\",\"arg\":\"chrome\",\"say\":\"Switching.\"}"]);
    let d = Brain { llm: &llm, fallback: &p, voice: None }.decide("bring chrome up front please", "");
    assert_eq!(d.intent, Intent::FocusApp("chrome".into()));
}

#[test]
fn saying_the_last_thing_again_is_asked_again_without_the_conversation() {
    let p = parser();
    let ctx = "Known apps: chrome\nConversation so far:\nyou: how are you\natlas: I'm doing well, thanks.\n";
    let llm = Scripted::new(&["I'm doing well, thanks.", "Paris."]);
    let d = Brain { llm: &llm, fallback: &p, voice: None }.decide("what's the capital of france", ctx);
    assert_eq!(d.say, "Paris.");
    let second = &llm.asked.lock().unwrap()[1].1;
    assert!(!second.contains("Conversation so far") && second.contains("Known apps"), "{second}");
}

// ------------------------------------------------------------------ 3

#[test]
fn a_how_to_only_gets_a_procedure_that_is_about_it() {
    let k = atlas::knowhow::Knowhow::shipped();
    assert!(k.for_request("make pancakes", true).is_none(), "shared 'make' is not a match");
    assert!(k.for_request("what is photosynthesis", true).is_none(), "'what is' alone is not a match");
    assert!(k.for_request("tie a tie", true).is_none());
    assert_eq!(k.for_request("freeing up memory", true).unwrap().id, "free-up-memory");
    assert_eq!(k.for_request("it's running slowly", true).unwrap().id, "free-up-memory");
    assert_eq!(atlas::knowhow::content_words("how do I make pancakes"), vec!["pancakes"]);
}

// ------------------------------------------------------------------ 4 and 5

fn daemon(tag: &str, llm: Option<std::sync::Arc<dyn Llm>>) -> atlas::daemon::Daemon<'static> {
    let c: &'static atlas::config::Config =
        Box::leak(Box::new(atlas::config::Config::load(Path::new("config")).unwrap()));
    let p = Box::leak(Box::new(atlas::platform::mock::MockPlatform::new(vec![atlas::platform::Monitor {
        id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true,
    }])));
    let dir = std::env::temp_dir().join(format!("atlas-person-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    atlas::daemon::Daemon::new(c, p, llm, atlas::store::Store::new(dir),
        atlas::proactive::Proactive::new(atlas::proactive::ProactiveConfig::default()))
}

#[test]
fn a_question_atlas_cannot_answer_is_not_an_approval_and_does_not_eat_the_next_turn() {
    // Still never *run*: only answered.
    assert_eq!(atlas::policy::classify(&Intent::Unknown("x".into())), atlas::policy::Decision::RequireApproval);
    assert!(atlas::wanted::is_a_question("how are you"));
    assert!(atlas::wanted::is_a_question("tell me a joke"));
    let mut d = daemon("noq", None);
    let t = 1_790_500_000;
    for (i, q) in ["how are you", "what's the capital of france", "tell me a joke", "why is the sky blue"].iter().enumerate() {
        let a = d.turn(q, t + i as u64 * 30);
        assert!(!a.contains("Go ahead?") && !a.contains("just listen") && !a.contains("Left it alone"), "{q} -> {a}");
        assert!(a.contains("language model"), "says why it can't answer: {q} -> {a}");
    }
    // A feeling isn't a request for "which one?".
    let a = d.turn("i'm so tired of this", t + 300);
    assert_ne!(a, "Which one?");
    // And a new request after "…or just listen?" is taken as itself.
    let _ = d.turn("i'm bored", t + 330);
    let a = d.turn("what can you do", t + 360);
    assert!(!a.contains("Not sure which you meant"), "{a}");
}

#[test]
fn with_a_model_an_ordinary_question_is_answered_in_words() {
    let llm: std::sync::Arc<dyn Llm> =
        std::sync::Arc::new(Scripted::new(&["<html><body>nope</body></html>", "Paris, on the Seine."]));
    let mut d = daemon("model", Some(llm));
    let a = d.turn("what's the capital of france", 1_790_500_000);
    assert!(a.starts_with("Paris, on the Seine."), "{a}");
    assert!(!a.contains('<') && !a.contains('{'), "{a}");
}

#[test]
fn atlas_uses_the_model_it_downloads_for_itself() {
    let y = std::fs::read_to_string("config/tools.yaml").unwrap();
    let t: atlas::voice::ToolsConfig = serde_yaml::from_str(&y).unwrap();
    assert!(t.llm.is_none(), "a shipped `llm:` overrides the model in models/ on every install");
}

// ------------------------------------------------------------------ typing at `atlas`

#[test]
fn typing_at_atlas_reaches_the_model_and_is_never_blocked_for_not_being_a_command() {
    let main = crate::common::source_of("main");
    let pl = &main[main.find("fn prompt_line(").unwrap()..];
    let pl = &pl[..pl.find("\n}\n").unwrap()];
    assert!(pl.contains("if !matches!(intent, Intent::Unknown(_))"), "an unknown line is gated before turn: it prints 'blocked'");
    let h = &main[main.find("fn handle(").unwrap()..];
    let h = &h[..h.find("\n}\n").unwrap()];
    assert!(h.contains("model_connection"), "the one-shot path uses the same model connection");
    assert!(
        main.contains("Daemon::try_new(&cfg, plat.as_ref(), model_connection(tc), store"),
        "the typing prompt's Atlas is built with the model"
    );
    assert!(main.contains("Atlas could not safely open its configured output folders"), "failed configured-output initialization must remain visible");
}

#[test]
fn a_how_to_atlas_has_no_procedure_for_is_answered_by_the_model() {
    let llm: std::sync::Arc<dyn Llm> = std::sync::Arc::new(Scripted::new(&["Whisk flour, eggs and milk, then fry."]));
    let mut d = daemon("howto", Some(llm));
    let a = d.execute(&Intent::WalkThrough("make pancakes".into()));
    assert_eq!(a, "Whisk flour, eggs and milk, then fry.");
    let mut d = daemon("howto-none", None);
    let a = d.execute(&Intent::WalkThrough("make pancakes".into()));
    assert!(a.contains("language model") && !a.contains("here's how"), "{a}");
}

#[test]
fn a_line_from_earlier_in_the_conversation_is_not_taken_as_the_answer() {
    let p = parser();
    let ctx = "Conversation so far:\nyou: tell me a joke\natlas: I can't answer that one here — general questions need my language model, and there isn't one set up on this machine. `atlas doctor` says what's missing.\nyou: hi\natlas: Hello.\n";
    let llm = Scripted::new(&[
        "I can't recommend a movie here — general questions need my language model. `atlas doctor` says what's missing.",
        "Try Arrival — slow, clever, and it sticks with you.",
    ]);
    let d = Brain { llm: &llm, fallback: &p, voice: None }.decide("recommend a good movie", ctx);
    assert_eq!(d.say, "Try Arrival — slow, clever, and it sticks with you.");
}
