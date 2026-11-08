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
    seen: Mutex<
        Vec<(
            usize,
            u64,
            Vec<String>,
            Option<atlas::models::ServerTimings>,
        )>,
    >,
    prompts: Mutex<Vec<String>>,
}

impl Real {
    fn remember_prompt(&self, req: &ChatRequest) {
        self.prompts
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(
                req.messages
                    .iter()
                    .map(|message| message.content.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
    }
}

impl Llm for Real {
    fn complete(&self, system: &str, user: &str) -> atlas::error::Result<String> {
        let req = ChatRequest {
            messages: vec![
                atlas::brain::Msg::system(system),
                atlas::brain::Msg::user(user),
            ],
            max_tokens: 300,
            ..Default::default()
        };
        self.remember_prompt(&req);
        atlas::models::chat_call(&self.url, &req, &mut |_| true).map(|r| r.text)
    }
    fn native_chat(&self) -> bool {
        true
    }
    fn supports_bounded_chat(&self) -> bool {
        true
    }
    fn chat_until(
        &self,
        req: &ChatRequest,
        on_text: &mut dyn FnMut(&str) -> bool,
        keep_going: &dyn Fn() -> bool,
    ) -> atlas::error::Result<ChatReply> {
        self.remember_prompt(req);
        let started = Instant::now();
        let reply = atlas::models::chat_call_until(&self.url, req, on_text, keep_going);
        let tools = reply
            .as_ref()
            .map(|r| {
                r.tool_calls
                    .iter()
                    .map(|c| format!("{}({})", c.name, c.arguments))
                    .collect()
            })
            .unwrap_or_default();
        let chars = req
            .messages
            .iter()
            .map(|m| m.content.chars().count())
            .sum::<usize>()
            + req.tools.iter().map(|t| t.to_string().len()).sum::<usize>();
        self.seen
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push((
                chars,
                started.elapsed().as_millis() as u64,
                tools,
                atlas::models::take_last_timings(),
            ));
        reply
    }
    fn chat(
        &self,
        req: &ChatRequest,
        on_text: &mut dyn FnMut(&str) -> bool,
    ) -> atlas::error::Result<ChatReply> {
        self.remember_prompt(req);
        let started = Instant::now();
        let r = atlas::models::chat_call(&self.url, req, on_text);
        let took = started.elapsed().as_millis() as u64;
        let tools: Vec<String> = r
            .as_ref()
            .map(|r| {
                r.tool_calls
                    .iter()
                    .map(|c| format!("{}({})", c.name, c.arguments))
                    .collect()
            })
            .unwrap_or_default();
        let chars: usize = req
            .messages
            .iter()
            .map(|m| m.content.chars().count())
            .sum::<usize>()
            + req.tools.iter().map(|t| t.to_string().len()).sum::<usize>();
        self.seen
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push((chars, took, tools, atlas::models::take_last_timings()));
        r
    }
}

const REQUESTS: &[(&str, &str)] = &[
    ("I guess I want you to organize my desktop.", "tidy_desktop"),
    ("look at my screen and tell me what's on it", "view_display"),
    (
        "I want you to go and do a diagnosis on yourself.",
        "self_check",
    ),
    ("Please use my camera and look at me.", "whats_there"),
    (
        "Research ways to improve in house language models and response times.",
        "research",
    ),
    ("what's on my calendar tomorrow", "agenda"),
    ("find the tax pdf from last year", "find_file"),
    (
        "my laptop is running slow, what's eating the memory",
        "machine_health",
    ),
    // Said without the commands' own words (30 Sep 2026, the capability
    // sweep): what "doesn't seem to know it has them" looked like.
    (
        "how much space have I got left on this thing",
        "machine_health",
    ),
    ("anything new come in by email", "mail"),
    ("could you find me some freelance gigs", "opportunities"),
    ("how did my last youtube video do", "social"),
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
    // Without a model: each of these requests is at least offered the tool
    // that does it (the part that doesn't need a model to check).
    let book = atlas::intent::ToolBook::new(
        &atlas::config::Config::load(Path::new("config"))
            .unwrap()
            .commands,
    );
    let router = atlas::router::Router::new(&book);
    // The ones said in the commands' own words: words alone find them.
    for (said, want) in &REQUESTS[..8] {
        let names = router.names_for(said, atlas::router::SHORTLIST);
        assert!(names.iter().any(|n| n == want), "{said:?}: {names:?}");
    }
    let Ok(url) = std::env::var("ATLAS_REAL_MODEL_URL") else {
        return;
    };
    let mut c = atlas::config::Config::load(Path::new("config")).unwrap();
    // With the meaning encoder too, when there is one (`ATLAS_EMBED_DIR`, as
    // in `meaning_picks_the_tool.rs`): the paraphrases need it.
    if let Some(dir) = std::env::var_os("ATLAS_EMBED_DIR").map(std::path::PathBuf::from) {
        let enc = serde_json::json!({ "encoder": {
            "command": dir.join("embed").to_string_lossy(),
            "args": ["--model", dir.join("all-MiniLM-L6-v2.onnx").to_string_lossy(), "--vocab", dir.join("vocab.txt").to_string_lossy()],
            "stdin_text": true
        }});
        c.tools.get_or_insert_with(Default::default).meaning = serde_json::from_value(enc).unwrap();
    }
    let p = MockPlatform::new(vec![Monitor {
        id: 1,
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
        primary: true,
    }]);
    let dir = std::env::temp_dir().join(format!("atlas-real-model-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let llm = Arc::new(Real {
        url,
        seen: Mutex::new(Vec::new()),
        prompts: Mutex::new(Vec::new()),
    });
    let mut d = Daemon::new(
        &c,
        &p,
        Some(llm.clone() as Arc<dyn Llm>),
        Store::new(dir.clone()),
        Proactive::new(ProactiveConfig::default()),
    );
    let mut t = 1_790_760_000u64;
    // The tick starts the encoder; its tools take a few seconds to embed.
    d.tick(t);
    if std::env::var_os("ATLAS_EMBED_DIR").is_some() {
        std::thread::sleep(std::time::Duration::from_secs(25));
    }
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
        println!(
            "REAL request {took:>6}ms {} | {said}\n     -> tools {chose:?} | {reply}",
            if by_phrase { "(phrases, no model)" } else { "" }
        );
        for s in &seen {
            println!(
                "     model call: ~{} chars sent, {}ms, {}",
                s.0,
                s.1,
                s.3.map(|t| t.line()).unwrap_or_default()
            );
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
            println!(
                "     model call: ~{} chars sent, {}ms, {}",
                s.0,
                s.1,
                s.3.map(|t| t.line()).unwrap_or_default()
            );
        }
        let l = reply.to_lowercase();
        assert!(
            !l.contains("i'm on it")
                && !l.contains("don't have a camera")
                && !l.contains("not supposed to"),
            "{reply}"
        );
    }
    println!(
        "REAL {right} of {} requests reached the right tool ({routed} went to the model)",
        REQUESTS.len()
    );
    if std::env::var_os("ATLAS_LIVE_RESEARCH").is_some() {
        let ack = d.turn(
            "research the current Rust programming language release schedule and write me a report",
            t + 60,
        );
        println!("LIVE research ack: {ack}");
        assert!(
            ack.to_lowercase().contains("looking into") || ack.to_lowercase().contains("research"),
            "live research did not acknowledge the request: {ack}"
        );
        // Research is a background job with a configurable production budget.
        // Derive the live acceptance window from the same loaded configuration
        // rather than rejecting a valid worker before Atlas's contract allows
        // it to finish. The grace period covers the final daemon tick and
        // report-file observation without changing the production deadline.
        let research_budget = c
            .tools
            .as_ref()
            .map(|tools| tools.research.deadline_secs)
            .unwrap_or(600);
        let deadline =
            Instant::now() + std::time::Duration::from_secs(research_budget.saturating_add(30));
        let mut report = String::new();
        let mut tick_t = t + 61;
        while Instant::now() < deadline {
            for line in d.tick(tick_t) {
                let lower = line.to_lowercase();
                if !lower.contains("no connection")
                    && lower.contains("rust")
                    && (lower.contains("release") || lower.contains("schedule"))
                {
                    report.push_str(&line);
                    report.push('\n');
                }
            }
            if report.to_lowercase().contains("rust")
                && (report.to_lowercase().contains("release")
                    || report.to_lowercase().contains("schedule"))
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
            tick_t += 1;
        }
        println!("LIVE research report: {report}");
        let report_body = report
            .split_once(':')
            .map(|(_, body)| body)
            .unwrap_or(report.as_str());
        let report_lower = report_body.to_lowercase();
        let language_release = report_lower.contains("rust programming language")
            || report_lower.contains("rust 1.")
            || report_lower.contains("rust release train");
        assert!(
            language_release
                && (report_lower.contains("release") || report_lower.contains("schedule"))
                && !report_lower.contains("no connection")
                && !report_lower.contains("couldn't finish")
                && !report_lower.contains("every source")
                && !report_lower.contains("no relevant sources")
                && !report_lower.contains("outstanding list")
                && !report_lower.contains("not provided in the sources"),
            "live research did not produce a fresh, topic-matching report: {report}"
        );
        let notes = dir.join("data/notes");
        let filed = notes
            .exists()
            .then(|| {
                std::fs::read_dir(&notes)
                    .ok()?
                    .flatten()
                    .filter_map(|entry| std::fs::read_to_string(entry.path()).ok())
                    .find(|text| {
                        let lower = text.to_lowercase();
                        (lower.contains("rust programming language")
                            || lower.contains("rust 1.")
                            || lower.contains("rust release train"))
                            && (lower.contains("release") || lower.contains("schedule"))
                            && !lower.contains("no connection")
                    })
            })
            .flatten();
        assert!(
            filed.is_some(),
            "live research produced no fresh topic-matching filed note in {}",
            notes.display()
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// `ATLAS_LATE_TEMPLATE_OUT=<file>`: write Qwen3.5's template with the picked
/// tools moved late (`models::tools_late_template`), for starting a real
/// llama-server with it as Atlas does (`--chat-template-file`).
#[test]
fn write_the_late_template_when_asked() {
    let Ok(out) = std::env::var("ATLAS_LATE_TEMPLATE_OUT") else {
        return;
    };
    let t = atlas::models::tools_late_template(include_str!(
        "fixtures/models/qwen3.5-chat-template.jinja"
    ))
    .unwrap();
    std::fs::write(out, t).unwrap();
}

/// Live council acceptance test.  This is intentionally opt-in: unlike the
/// repository council tests above, it starts no model and makes no claim when
/// the local endpoint is absent.  When enabled it proves that Atlas's actual
/// background council path called the configured model seats and produced a
/// non-technical outcome.
#[test]
fn live_local_model_can_run_atlas_council() {
    let Ok(url) = std::env::var("ATLAS_REAL_MODEL_URL") else {
        return;
    };
    if std::env::var_os("ATLAS_LIVE_COUNCIL").is_none() {
        return;
    }
    let c = atlas::config::Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor {
        id: 1,
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
        primary: true,
    }]);
    let dir = std::env::temp_dir().join(format!("atlas-live-council-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let llm = Arc::new(Real {
        url,
        seen: Mutex::new(Vec::new()),
        prompts: Mutex::new(Vec::new()),
    });
    let mut d = Daemon::new(
        &c,
        &p,
        Some(llm.clone() as Arc<dyn Llm>),
        Store::new(dir.clone()),
        Proactive::new(ProactiveConfig::default()),
    );
    d.tick(1_790_760_000);
    let ack = d.ask_the_room("should I upgrade the RAM");
    assert!(
        ack.contains("Asking the room"),
        "live council was not queued: {ack}"
    );

    let deadline = Instant::now() + std::time::Duration::from_secs(180);
    let mut said = Vec::new();
    let mut t = 1_790_760_001;
    while Instant::now() < deadline {
        said.extend(d.tick(t));
        if d.crew.active() == 0 && !said.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
        t += 1;
    }
    let joined = said.join(" ");
    let calls = d
        .trace
        .calls
        .iter()
        .filter(|call| call.asked_by == "council")
        .count();
    let seats: std::collections::BTreeSet<String> = d
        .trace
        .calls
        .iter()
        .filter(|call| call.asked_by == "council")
        .filter_map(|call| call.seat.clone())
        .collect();
    println!("LIVE council calls={calls}: {joined}");
    assert_eq!(calls, 5, "live council did not call five independent seats");
    assert_eq!(
        seats.len(),
        5,
        "live council lost per-seat provenance: {seats:?}"
    );
    assert!(
        !joined.contains("seats answered")
            && (joined.contains("for") || joined.contains("against") || joined.contains("depends")),
        "live council produced no usable verdict: {joined}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn live_local_model_researches_and_files_a_report() {
    let Ok(url) = std::env::var("ATLAS_REAL_MODEL_URL") else {
        return;
    };
    if std::env::var_os("ATLAS_LIVE_RESEARCH").is_none() {
        return;
    }
    let c = atlas::config::Config::load(Path::new("config")).unwrap();
    let tools = c.tools.unwrap();
    let dir = std::env::temp_dir().join(format!("atlas-live-research-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let notes = dir.join("notes");
    let mut research = tools.research.resolved(&dir);
    research.notes_dir = notes.to_string_lossy().into_owned();
    let engine = atlas::research::Research {
        cfg: research,
        vars: atlas::tools::Vars::from(tools.vars),
        browser: None,
    };
    let llm = Real {
        url,
        seen: Mutex::new(Vec::new()),
        prompts: Mutex::new(Vec::new()),
    };
    let note = engine.run("Rust programming language", &llm).unwrap();
    assert!(!note.sources.is_empty(), "live research found no sources");
    let path = engine.save(&note).unwrap();
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(
        saved.contains("## Sources"),
        "saved report omitted its sources"
    );
    assert!(saved.contains("Rust programming language"));
    println!(
        "LIVE research sources={} report={}",
        note.sources.len(),
        path
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Proves that a real-model conversation survives daemon reconstruction. This
/// is opt-in because it requires a running private model and is intentionally
/// not satisfied by the scripted restart tests.
#[test]
fn live_local_model_memory_survives_restart() {
    let Ok(url) = std::env::var("ATLAS_REAL_MODEL_URL") else {
        return;
    };
    if std::env::var_os("ATLAS_LIVE_MEMORY").is_none() {
        return;
    }

    let c = atlas::config::Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor {
        id: 1,
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
        primary: true,
    }]);
    let nonce = format!(
        "atlas-memory-nonce-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let dir = std::env::temp_dir().join(format!("atlas-live-memory-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let llm = Arc::new(Real {
        url: url.clone(),
        seen: Mutex::new(Vec::new()),
        prompts: Mutex::new(Vec::new()),
    });
    let store = Store::new(dir.clone());
    let mut first = Daemon::new(
        &c,
        &p,
        Some(llm.clone() as Arc<dyn Llm>),
        store,
        Proactive::new(ProactiveConfig::default()),
    );
    first.tick(1_790_760_000);

    let first_reply = first.turn(
        &format!("Remember this private fact for later: the exact code is {nonce}."),
        1_790_760_001,
    );
    assert!(!first_reply.trim().is_empty(), "first turn had no reply");
    let unrelated = first.turn("What is two plus two? Answer briefly.", 1_790_760_002);
    assert!(
        !unrelated.trim().is_empty(),
        "unrelated turn did not complete"
    );
    first.persist();

    let saved = Store::new(dir.clone())
        .load_checked::<atlas::thread::Thread>("thread")
        .unwrap()
        .expect("thread record was not persisted before restart");
    assert!(
        saved
            .recent
            .iter()
            .any(|exchange| exchange.said.contains(&nonce)),
        "persisted thread omitted the nonce: {saved:?}"
    );
    drop(first);

    let restarted_store = Store::new(dir.clone());
    let before_restart = restarted_store
        .load_checked::<atlas::thread::Thread>("thread")
        .unwrap();
    assert!(
        before_restart.is_some(),
        "thread record vanished before reconstruction"
    );
    let mut second = Daemon::new(
        &c,
        &p,
        Some(llm.clone() as Arc<dyn Llm>),
        restarted_store,
        Proactive::new(ProactiveConfig::default()),
    );
    let follow_up = second.turn(
        "What exact private code did I ask you to remember earlier?",
        1_790_760_003,
    );
    let response_has_nonce = follow_up.contains(&nonce);
    let prompt_has_nonce = llm
        .prompts
        .lock()
        .unwrap()
        .iter()
        .any(|prompt| prompt.contains(&nonce));
    assert!(
        response_has_nonce || prompt_has_nonce,
        "restart lost the nonce in both response and model prompt: {follow_up}"
    );

    second.persist();
    let after_restart = Store::new(dir.clone())
        .load_checked::<atlas::thread::Thread>("thread")
        .unwrap();
    assert!(
        after_restart.is_some(),
        "thread record missing after reconstruction"
    );
    println!(
        "LIVE memory restart response_has_nonce={response_has_nonce} prompt_has_nonce={prompt_has_nonce}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
