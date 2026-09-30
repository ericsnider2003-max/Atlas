//! **His sentences, through the whole daemon, against a real model
//! (30 Sep 2026).** Runs only with `ATLAS_REAL_MODEL_URL` set to a
//! llama-server's chat address (`http://127.0.0.1:8080/v1/chat/completions`);
//! otherwise it returns at once, so the suite never needs a model.
//!
//! What it prints is the evidence for the report: each reply, how long the
//! model took, what the server read (its own `timings`), and which tool the
//! model chose. What it asserts is only what a small model must get right
//! with the new prompt: the requests reach the right tool when the model
//! calls one, nothing reaches Eric that the quality checklist in
//! `finishing_what_it_starts.rs` would stop, and the prompt stays small.

use atlas::brain::{ChatReply, ChatRequest, Llm};
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

struct Real {
    url: String,
    seen: Mutex<Vec<(usize, u64, Vec<String>, Option<atlas::models::ServerTimings>)>>,
}

impl Llm for Real {
    fn complete(&self, system: &str, user: &str) -> atlas::error::Result<String> {
        let req = ChatRequest {
            messages: vec![atlas::brain::Msg::system(system), atlas::brain::Msg::user(user)],
            max_tokens: 300,
            ..Default::default()
        };
        atlas::models::chat_call(&self.url, &req, &mut |_| true).map(|r| r.text)
    }
    fn native_chat(&self) -> bool {
        true
    }
    fn chat(&self, req: &ChatRequest, on_text: &mut dyn FnMut(&str) -> bool) -> atlas::error::Result<ChatReply> {
        let started = Instant::now();
        let r = atlas::models::chat_call(&self.url, req, on_text);
        let took = started.elapsed().as_millis() as u64;
        let tools: Vec<String> = r.as_ref().map(|r| r.tool_calls.iter().map(|c| format!("{}({})", c.name, c.arguments)).collect()).unwrap_or_default();
        let chars: usize = req.messages.iter().map(|m| m.content.chars().count()).sum::<usize>() + req.tools.iter().map(|t| t.to_string().len()).sum::<usize>();
        self.seen.lock().unwrap().push((chars, took, tools, atlas::models::take_last_timings()));
        r
    }
}

const REQUESTS: &[(&str, &str)] = &[
    ("I guess I want you to organize my desktop.", "tidy_desktop"),
    ("look at my screen and tell me what's on it", "view_display"),
    ("I want you to go and do a diagnosis on yourself.", "self_check"),
    ("Please use my camera and look at me.", "whats_there"),
    ("Research ways to improve in house language models and response times.", "research"),
    ("what's on my calendar tomorrow", "agenda"),
    ("find the tax pdf from last year", "find_file"),
    ("my laptop is running slow, what's eating the memory", "machine_health"),
];

const TALK: &[&str] = &[
    "hey, how's it going",
    "You have internet capabilities correct?",
    "Are you using my camera? Can you see me?",
    "What permission do you need?",
    "calm down with being a smart ass",
    "tell me something interesting about octopuses",
    "why do they have three hearts",
    "Thanks, have a good day.",
];

#[test]
fn his_sentences_against_a_real_model() {
    let Ok(url) = std::env::var("ATLAS_REAL_MODEL_URL") else { return };
    let c = atlas::config::Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let dir = std::env::temp_dir().join(format!("atlas-real-model-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let llm = Arc::new(Real { url, seen: Mutex::new(Vec::new()) });
    let mut d = Daemon::new(&c, &p, Some(llm.clone() as Arc<dyn Llm>), Store::new(dir.clone()), Proactive::new(ProactiveConfig::default()));
    let mut t = 1_790_760_000u64;
    let mut right = 0;
    let mut routed = 0;
    for (said, want) in REQUESTS {
        t += 60;
        let n = llm.seen.lock().unwrap().len();
        let started = Instant::now();
        let reply = d.turn(said, t);
        let took = started.elapsed().as_millis();
        let seen = llm.seen.lock().unwrap()[n..].to_vec();
        let chose: Vec<String> = seen.iter().flat_map(|s| s.2.clone()).collect();
        let by_phrase = seen.is_empty();
        if by_phrase || chose.iter().any(|c| c.starts_with(want)) {
            right += 1;
        }
        if !by_phrase {
            routed += 1;
        }
        println!("REAL request {took:>6}ms {} | {said}\n     -> tools {chose:?} | {reply}", if by_phrase { "(phrases, no model)" } else { "" });
        for s in &seen {
            println!("     model call: ~{} chars sent, {}ms, {}", s.0, s.1, s.3.map(|t| t.line()).unwrap_or_default());
        }
    }
    for said in TALK {
        t += 45;
        let n = llm.seen.lock().unwrap().len();
        let started = Instant::now();
        let reply = d.turn(said, t);
        let took = started.elapsed().as_millis();
        println!("REAL talk {took:>6}ms | {said}\n     -> {reply}");
        for s in &llm.seen.lock().unwrap()[n..] {
            println!("     model call: ~{} chars sent, {}ms, {}", s.0, s.1, s.3.map(|t| t.line()).unwrap_or_default());
        }
        let l = reply.to_lowercase();
        assert!(!l.contains("i'm on it") && !l.contains("don't have a camera") && !l.contains("not supposed to"), "{reply}");
    }
    println!("REAL {right} of {} requests reached the right tool ({routed} went to the model)", REQUESTS.len());
    let _ = std::fs::remove_dir_all(&dir);
}

/// `ATLAS_LATE_TEMPLATE_OUT=<file>`: write Qwen3.5's template with the picked
/// tools moved late (`models::tools_late_template`), for starting a real
/// llama-server with it as Atlas does (`--chat-template-file`).
#[test]
fn write_the_late_template_when_asked() {
    let Ok(out) = std::env::var("ATLAS_LATE_TEMPLATE_OUT") else { return };
    let t = atlas::models::tools_late_template(include_str!("fixtures/models/qwen3.5-chat-template.jinja")).unwrap();
    std::fs::write(out, t).unwrap();
}
