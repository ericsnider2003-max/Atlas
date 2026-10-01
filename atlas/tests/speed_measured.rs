//! Speed, measured (28 Sep 2026, round three).
//!
//! Eric's complaint was that Atlas is slow. Round two fixed the hub, the
//! wake word, the graphics card and streaming. These measure what was left
//! and pin the fixes:
//!
//! - **Each turn reads only what is new.** llama.cpp reuses a prompt only up
//!   to its first changed token (the shipped Qwen3-VL can't reuse a moved
//!   middle: `--cache-reuse` is off for it). The tools picked for each
//!   sentence sat in the system turn ahead of the conversation, and the
//!   conversation's window slid by one exchange a turn, so every turn read
//!   the whole conversation again: 818 tokens a turn on average here, 821 on
//!   the real server. Now the picked tools come after the conversation
//!   (`models::tools_late_template`) and the window moves in steps
//!   (`Thread::messages`): 494 here, 514 on the real server.
//! - **A call beside the conversation leaves its slot alone** (`aside`), and
//!   llama.cpp is told not to clear idle slots: on the real server, a
//!   rewording call on the other slot wiped the conversation's, and the next
//!   turn read 1,946 tokens instead of 601.
//! - **The model reads the start of the conversation while Atlas starts**
//!   (`warm_the_model`), so the first turn reads about 450 tokens, not 1,700.
//! - **The hub is answered while Atlas speaks**, between sentences: a page
//!   waited 2.7s for a four-sentence reply, now one sentence (0.8s).
//! - **Saving after a turn doesn't write out an unchanged file index**
//!   (545ms for 40,000 files, debug build, on every save).
//! - **Checking the connection doesn't hold the loop** (808ms every 30s on
//!   a network that drops the probe).
//!
//! Each test prints its numbers (`cargo test -- --nocapture`).

use atlas::brain::{ChatReply, ChatRequest, Llm, Role};
use atlas::daemon::{Daemon, Ears, Mouth};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-speed-measured-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn cfg() -> atlas::config::Config {
    atlas::config::Config::load(Path::new("config")).unwrap()
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

/// A model that records what it was asked, and writes its reply a word at a
/// time after `first_ms`, `word_ms` apart.
struct Talker {
    asked: Mutex<Vec<ChatRequest>>,
    reply: String,
    first_ms: u64,
    word_ms: u64,
}

impl Talker {
    fn new(reply: &str, first_ms: u64, word_ms: u64) -> Arc<Talker> {
        Arc::new(Talker { asked: Mutex::new(Vec::new()), reply: reply.into(), first_ms, word_ms })
    }
}

impl Llm for Talker {
    fn complete(&self, _: &str, _: &str) -> atlas::error::Result<String> {
        Ok(r#"{"action":"say","arg":null,"say":"(the one-prompt path)"}"#.into())
    }
    fn native_chat(&self) -> bool {
        true
    }
    fn chat(&self, req: &ChatRequest, on_text: &mut dyn FnMut(&str) -> bool) -> atlas::error::Result<ChatReply> {
        self.asked.lock().unwrap().push(req.clone());
        // Never the same answer twice: Atlas asks again when a reply
        // repeats the last one word for word.
        let about = req.messages.last().map(|m| m.content.rsplit("User said: ").next().unwrap_or("").to_string()).unwrap_or_default();
        // 29 Sep 2026: and never the same sentences twice either. Atlas now
        // leaves out a sentence an earlier reply already said, and asks
        // again when most of a reply is repeats (`repeating`); a talker
        // saying the same three sentences after a new opening every turn is
        // the loop Eric's laptop fell into, not a conversation.
        let n = self.asked.lock().unwrap().len();
        let body = if self.reply == REPLY { REPLIES[n % REPLIES.len()] } else { self.reply.as_str() };
        let reply = format!("On {about}: {body}");
        std::thread::sleep(Duration::from_millis(self.first_ms));
        for w in reply.split_inclusive(' ') {
            if !on_text(w) {
                break;
            }
            std::thread::sleep(Duration::from_millis(self.word_ms));
        }
        Ok(ChatReply { text: reply, tool_calls: vec![] })
    }
}

/// The request as the model reads it on Atlas's own model server: Qwen's
/// chat template (what llama-server's `--jinja` applies for the shipped
/// model), with the tools picked for this sentence after the conversation
/// when the request says which tools are the every-turn ones
/// (`models::tools_late_template`, checked against the real server). With
/// `late` false, the model's own template: every tool at the top.
fn render_as(req: &ChatRequest, late: bool) -> String {
    let early = if late && req.stable_tools > 0 && req.stable_tools < req.tools.len() { req.stable_tools } else { req.tools.len() };
    let mut out = String::new();
    let mut rest: &[atlas::brain::Msg] = &req.messages;
    if !req.tools.is_empty() {
        out.push_str("<|im_start|>system\n");
        if let Some(first) = req.messages.first().filter(|m| m.role == Role::System) {
            out.push_str(&first.content);
            out.push_str("\n\n");
            rest = &req.messages[1..];
        }
        out.push_str("# Tools\n\n<tools>");
        for t in &req.tools[..early] {
            out.push('\n');
            out.push_str(&t.to_string());
        }
        out.push_str("\n</tools><|im_end|>\n");
    }
    for (i, m) in rest.iter().enumerate() {
        if i + 1 == rest.len() && m.role == Role::User && early < req.tools.len() {
            out.push_str("<|im_start|>system\n# Tools for this request\n<tools>");
            for t in &req.tools[early..] {
                out.push('\n');
                out.push_str(&t.to_string());
            }
            out.push_str("\n</tools><|im_end|>\n");
        }
        out.push_str(&format!("<|im_start|>{}\n{}<|im_end|>\n", m.role.name(), m.content));
    }
    out.push_str("<|im_start|>assistant\n");
    out
}

fn render(req: &ChatRequest) -> String {
    render_as(req, true)
}

/// About four characters to a token, near enough for Qwen on English and
/// JSON; the same rule for before and after.
fn tokens(s: &str) -> usize {
    s.len().div_ceil(4)
}

fn common_prefix(a: &str, b: &str) -> usize {
    a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count()
}

/// A conversation, the way people talk to it: follow-ups, a change of
/// subject, and back.
const CONVERSATION: &[&str] = &[
    "tell me something interesting about octopuses",
    "why do they have three hearts",
    "what should I cook tonight with rice and eggs",
    "how long does that take",
    "what is the capital of australia",
    "is it bigger than sydney",
    "tell me a joke about computers",
    "explain why the sky is blue",
    "and why are sunsets red then",
    "what's a good book about the ocean",
    "who wrote it",
    "thanks, that's all for now",
];

/// A reply the length a conversation's usually are: three sentences.
const REPLY: &str = "Fair question, and there's more to it than it looks. The short version is that it \
    depends on a few things, mostly timing and what you already have to hand. If you tell me a bit more \
    about what you're after, I can be a lot more specific.";

/// Replies the length of `REPLY`, each different, so a conversation of them
/// is not a loop (`Talker`).
const REPLIES: &[&str] = &[
    "Fair question, and there's more to it than it looks. The short version is that it depends on a few things, \
     mostly timing and what you already have to hand. If you tell me a bit more, I can be a lot more specific.",
    "Most people get this one backwards at first. What matters is the order you do things in, not how fast. \
     Start small and the rest tends to follow on its own.",
    "It's simpler than it sounds once you see the trick behind it. Everything else is detail layered on top of \
     one idea. Ask me about any part and I'll unpack that bit.",
    "There are two camps on this and both have a point. One side cares about speed, the other about getting it \
     right the first time. I lean towards the second, for what that's worth.",
    "Short answer: yes, but with a caveat worth knowing. The caveat only bites in unusual cases, so you'll \
     rarely hit it. When you do, it's obvious straight away.",
    "That one surprised me when I first read about it. The explanation involves a bit of physics and a bit of \
     history. I can go into either if you like.",
    "Honestly, it comes down to taste more than anything. There's no wrong choice among the usual options. \
     Pick the one you'd enjoy doing on a tired evening.",
    "The numbers are closer than people expect. Neither is dramatically ahead once you account for the edges. \
     It's the kind of thing that flips depending on who's counting.",
    "Good one, and the usual explanation is only half right. The missing half is about how light scatters on \
     its way through the air. Once you see that, the colours make sense.",
    "I'd start with something short and well written rather than the famous doorstop. You'll finish it and \
     want more. Then the big classic reads twice as well.",
    "That was written by someone better known for something else entirely. It came out early in their career. \
     Their later work is more polished but less fun.",
    "Any time. I'll be around when you want to pick it up again. Enjoy the rest of your evening.",
];

/// Sentences that need a tool the phrases don't catch, for trying a real
/// model's tool choice (`ATLAS_DUMP_TOOL_ASKS=<file>`).
const NEEDS_A_TOOL: &[&str] = &[
    "prepare me for my next meeting",
    "what am I still waiting to hear back on",
    "the wifi keeps dropping, can you figure out why",
    "recap what happened in the last hour",
    "find the invoice pdf from march",
    "who should I catch up with this week",
    "how's my computer doing",
    "what did you get done overnight",
    "is anything left dangling from yesterday",
    "how much do you know about me by now",
];

#[test]
fn dump_tool_asks_for_a_real_model() {
    let Ok(path) = std::env::var("ATLAS_DUMP_TOOL_ASKS") else { return };
    let (c, p) = (cfg(), plat());
    let mut out = String::new();
    for (i, said) in NEEDS_A_TOOL.iter().enumerate() {
        let llm = Talker::new(REPLY, 0, 0);
        let mut d = Daemon::new(&c, &p, Some(llm.clone() as Arc<dyn Llm>), Store::new(tmp(&format!("toolask{i}"))), Proactive::new(ProactiveConfig::default()));
        let _ = d.turn(said, 1_790_000_000);
        let asked = llm.asked.lock().unwrap();
        match asked.first() {
            Some(r) => {
                let msgs: Vec<serde_json::Value> = r.messages.iter().map(|m| serde_json::json!({"role": m.role.name(), "content": m.content})).collect();
                out.push_str(&serde_json::json!({"said": said, "messages": msgs, "tools": r.tools}).to_string());
                out.push('\n');
            }
            None => println!("{said:?} never reached the model"),
        }
    }
    std::fs::write(path, out).unwrap();
}

/// What each turn made the model read again, in tokens: (whole prompt,
/// read again).
fn read_again(asked: &[ChatRequest], late: bool) -> Vec<(usize, usize)> {
    let mut prev = String::new();
    asked
        .iter()
        .map(|r| {
            let p = render_as(r, late);
            let kept = common_prefix(&prev, &p);
            let row = (tokens(&p), tokens(&p) - tokens(&p[..kept]));
            prev = p;
            row
        })
        .collect()
}

/// The part of a rendered request that the next turn can reuse: everything
/// before this turn's own tools and words.
fn reusable_part(rendered: &str) -> &str {
    let end = rendered
        .rfind("<|im_start|>system\n# Tools for this request")
        .or_else(|| rendered.rfind("<|im_start|>user\n"))
        .unwrap_or(0);
    &rendered[..end]
}

#[test]
fn a_conversation_rereads_only_what_is_new() {
    let (c, p) = (cfg(), plat());
    let llm = Talker::new(REPLY, 0, 0);
    let mut d = Daemon::new(&c, &p, Some(llm.clone() as Arc<dyn Llm>), Store::new(tmp("prefix")), Proactive::new(ProactiveConfig::default()));
    let mut t = 1_790_000_000u64;
    let mut asked = Vec::new();
    for said in CONVERSATION {
        let n = llm.asked.lock().unwrap().len();
        let _ = d.turn(said, t);
        // The loop ticks between turns, as it does in the running Atlas.
        for _ in 0..3 {
            t += 20;
            let _ = d.tick(t);
        }
        let all = llm.asked.lock().unwrap();
        println!("{said:?}: {} call(s)", all.len() - n);
        asked.push(all.get(n).cloned().expect("every sentence reaches the model"));
    }
    // For replaying against a real llama-server (`ATLAS_DUMP_REQUESTS=<file>`).
    if let Ok(path) = std::env::var("ATLAS_DUMP_REQUESTS") {
        let mut out = String::new();
        for r in &asked {
            let msgs: Vec<serde_json::Value> = r.messages.iter().map(|m| serde_json::json!({"role": m.role.name(), "content": m.content})).collect();
            out.push_str(&serde_json::json!({"messages": msgs, "tools": r.tools}).to_string());
            out.push('\n');
        }
        std::fs::write(path, out).unwrap();
    }
    let own = read_again(&asked, false);
    let ours = read_again(&asked, true);
    println!("turn | prompt tokens | read again, model's own template | read again, Atlas's");
    for (i, (o, a)) in own.iter().zip(&ours).enumerate() {
        println!("{:>4} | {:>13} | {:>32} | {:>5}", i + 1, a.0, o.1, a.1);
    }
    let mean = |rows: &[(usize, usize)]| rows.iter().skip(1).map(|r| r.1).sum::<usize>() / (rows.len() - 1);
    println!("read again, turns 2-{}: mean {} with the model's own template, {} with Atlas's", ours.len(), mean(&own), mean(&ours));

    // The pin: each turn starts with everything the turn before could keep --
    // who Atlas is, the core tools and the whole conversation -- except
    // when the window's start steps forward, at most one turn in three.
    let rendered: Vec<String> = asked.iter().map(render).collect();
    let mut kept = 0;
    for i in 1..rendered.len() {
        if rendered[i].starts_with(reusable_part(&rendered[i - 1])) {
            kept += 1;
        }
    }
    let turns = rendered.len() - 1;
    println!("the earlier conversation was reused on {kept} of {turns} turns");
    assert!(kept * 3 >= turns * 2, "the conversation was read again on {} of {turns} turns", turns - kept);
    assert!(mean(&ours) * 10 <= mean(&own) * 7, "{} vs {}", mean(&ours), mean(&own));
}

// ================= the template that puts the picked tools last =================

const QWEN3_VL_TEMPLATE: &str = include_str!("fixtures/models/qwen3-vl-chat-template.jinja");

#[test]
fn the_template_moves_only_the_picked_tools() {
    let t = atlas::models::tools_late_template(QWEN3_VL_TEMPLATE).expect("the shipped model's template is one it knows");
    // The core tools stay in the system turn, where the model was trained to
    // find them; how many is said with each request, and a request that
    // doesn't say gets every tool there, as the model's own template does.
    assert!(t.starts_with("{%- set atlas_early = (atlas_stable_tools | int) if atlas_stable_tools is defined else (tools | length if tools else 0) %}"), "{}", &t[..200]);
    assert_eq!(t.matches("{%- for tool in tools[:atlas_early] %}").count(), 1);
    assert!(!t.contains("{%- for tool in tools %}"));
    // The rest in a system turn of their own, just before the last thing you said.
    let late = t.find("# Tools for this request").expect("the picked tools have a place");
    let user = t.find("{%- if message.role == \"user\" %}").unwrap();
    assert!(late < user && t[late..].contains("{%- for tool in tools[atlas_early:] %}"));
    assert!(t.contains("message.role == \"user\" and loop.last"));
    // Everything else as it was.
    assert_eq!(t.matches("<tool_call>").count(), QWEN3_VL_TEMPLATE.matches("<tool_call>").count());
    assert!(t.contains("<|vision_start|><|image_pad|><|vision_end|>"));
    // A template it wasn't written against keeps the model's own; so does
    // one it already changed.
    assert_eq!(atlas::models::tools_late_template("<|im_start|>system {{ messages }}"), None);
    assert_eq!(atlas::models::tools_late_template(&t), None);
}

/// A model server that takes one request and answers it; returns what it
/// was sent.
fn one_request_server() -> (String, std::sync::mpsc::Receiver<String>) {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let Ok((mut c, _)) = l.accept() else { return };
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut buf = Vec::new();
        let mut chunk = [0u8; 65536];
        loop {
            let n = c.read(&mut chunk).unwrap_or(0);
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            let t = String::from_utf8_lossy(&buf).to_string();
            if let Some(i) = t.find("\r\n\r\n") {
                let len: usize = t
                    .lines()
                    .find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0)))
                    .unwrap_or(0);
                if buf.len() >= i + 4 + len {
                    break;
                }
            }
        }
        let t = String::from_utf8_lossy(&buf).to_string();
        let _ = tx.send(t.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or_default());
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"Hi.\"}}]}\n\ndata: [DONE]\n\n";
        let _ = c.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes());
    });
    (format!("http://127.0.0.1:{port}/v1/chat/completions"), rx)
}

/// And which slot it belongs in (`ChatRequest::aside`).
#[test]
fn each_request_says_which_tools_are_the_every_turn_ones() {
    let tool = |n: &str| serde_json::json!({"type": "function", "function": {"name": n, "description": "d", "parameters": {"type": "object", "properties": {}}}});
    let req = |stable: usize, aside: bool| ChatRequest {
        messages: vec![atlas::brain::Msg::system("S"), atlas::brain::Msg::user("User said: hello")],
        tools: vec![tool("a"), tool("b"), tool("picked")],
        max_tokens: 20,
        force_tool: false,
        stable_tools: stable,
        aside,
        stronger: false,
    };
    let sent_as = |stable: usize, aside: bool| {
        let (url, rx) = one_request_server();
        atlas::models::chat_call(&url, &req(stable, aside), &mut |_| true).expect("answered");
        let body: serde_json::Value = serde_json::from_str(&rx.recv_timeout(Duration::from_secs(5)).unwrap()).unwrap();
        body
    };
    let sent = |stable: usize| sent_as(stable, false);
    let b = sent(2);
    assert_eq!(b["id_slot"], 0, "the conversation keeps its slot");
    assert_eq!(b["chat_template_kwargs"][atlas::models::STABLE_TOOLS_KWARG], 2, "{b}");
    assert_eq!(b["tools"].as_array().unwrap().len(), 3, "every tool is still offered");
    // Not said when it isn't known, or when every tool is an every-turn one.
    // 30 Sep 2026: every request now carries `enable_thinking: false` (Qwen3
    // thinks before answering otherwise), so the kwargs are always there;
    // the stable-tools count still only when it is known and not all of them.
    assert!(sent(0)["chat_template_kwargs"].get(atlas::models::STABLE_TOOLS_KWARG).is_none());
    assert!(sent(3)["chat_template_kwargs"].get(atlas::models::STABLE_TOOLS_KWARG).is_none());
    assert_eq!(sent(3)["chat_template_kwargs"]["enable_thinking"], false);
    // A call beside the conversation goes to the other slot, so the
    // conversation's slot keeps what it has read.
    assert_eq!(sent_as(2, true)["id_slot"], 1);
}

#[test]
fn a_turn_says_its_core_tools_lead() {
    let (c, p) = (cfg(), plat());
    let llm = Talker::new(REPLY, 0, 0);
    let mut d = Daemon::new(&c, &p, Some(llm.clone() as Arc<dyn Llm>), Store::new(tmp("core")), Proactive::new(ProactiveConfig::default()));
    // 30 Sep 2026 (the prompt diet, `router`): the one tool offered every
    // turn is the capabilities tool; the rest are picked for the sentence,
    // and small talk ("octopuses") gets none, so a request is used here.
    let _ = d.turn("find the tax pdf from last year and the receipts", 1_790_000_000);
    let r = llm.asked.lock().unwrap()[0].clone();
    assert!(r.stable_tools > 0 && r.stable_tools < r.tools.len(), "{} of {}", r.stable_tools, r.tools.len());
    assert_eq!(&r.tools[..r.stable_tools], &[atlas::router::meta_spec()][..], "the leading tool is the every-turn one");
}

// ================= the model reads ahead while Atlas starts =================

#[test]
fn the_first_turn_is_read_on_top_of_what_was_read_at_start() {
    let (c, p) = (cfg(), plat());
    let llm = Talker::new(REPLY, 0, 0);
    let mut d = Daemon::new(&c, &p, Some(llm.clone() as Arc<dyn Llm>), Store::new(tmp("warm")), Proactive::new(ProactiveConfig::default()));
    // Something from before this start, as there usually is.
    d.thread.append("what's a good name for a boat", "Something short you can shout: Tern, or Kestrel.", None, 1_789_999_000);
    let t = 1_790_000_000;
    d.warm_the_model(t).expect("a model that chats is read ahead to").join().unwrap();
    let _ = d.turn("why do they have three hearts", t + 5);
    let asked = llm.asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 2);
    assert!(asked[0].max_tokens <= 16, "reading ahead asks for next to nothing back");
    let (warm, first) = (render(&asked[0]), render(&asked[1]));
    let shared = reusable_part(&warm);
    assert!(shared.contains("Tern, or Kestrel"), "the conversation so far is part of what's read ahead");
    assert!(first.starts_with(shared), "the first turn doesn't start with what was read ahead");
    println!(
        "first turn: {} tokens, {} of them read at start, {} left to read",
        tokens(&first),
        tokens(shared),
        tokens(&first) - tokens(shared)
    );
    // Nothing was said or remembered by reading ahead.
    assert_eq!(d.thread.recent.len(), 2);
}

// ================= the hub while Atlas speaks =================

const HUB_TOKEN: &str = "abcdef-ghjkmn-pqrstu-vwxyz2";

struct SlowMouth {
    per_sentence_ms: u64,
    said: Mutex<Vec<String>>,
}

impl Mouth for SlowMouth {
    fn speak(&self, text: &str) -> atlas::error::Result<()> {
        self.said.lock().unwrap().push(text.to_string());
        std::thread::sleep(Duration::from_millis(self.per_sentence_ms));
        Ok(())
    }
}

struct NoMoreEars;

impl Ears for NoMoreEars {
    fn wait_for_wake(&self) -> atlas::error::Result<()> {
        Ok(())
    }
    fn listen(&self) -> atlas::error::Result<String> {
        Ok(String::new())
    }
    fn listen_briefly(&self, _secs: u32) -> atlas::error::Result<Option<String>> {
        Ok(None)
    }
}

/// A spoken turn: the hub is answered while the model thinks (the model's
/// call runs on a worker, `speak_while_thinking`) and between the sentences
/// said (28 Sep 2026: none was answered until the whole reply had been said).
#[test]
fn the_hub_is_answered_between_spoken_sentences() {
    let (c, p) = (cfg(), plat());
    // A second and a half of thinking, then four sentences written at once;
    // each takes 600ms to say.
    let llm = Talker::new("Octopuses have three hearts. Two pump blood through the gills. The third serves the body. It stops when they swim.", 1_500, 0);
    let mut d = Daemon::new(&c, &p, Some(llm.clone() as Arc<dyn Llm>), Store::new(tmp("speaking")), Proactive::new(ProactiveConfig::default()));
    let server = atlas::server::Server::bind(&atlas::server::ServerConfig { enabled: true, port: 0, ..Default::default() }, HUB_TOKEN).expect("bind");
    let port = server.port();
    d.hub_server = Some(server.threaded().expect("threaded"));
    let mouth = SlowMouth { per_sentence_ms: 600, said: Mutex::new(Vec::new()) };

    // A page asked for every 100ms while Atlas talks.
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let s2 = stop.clone();
    let asker = std::thread::spawn(move || {
        let mut waits = Vec::new();
        while !s2.load(std::sync::atomic::Ordering::SeqCst) {
            let started = Instant::now();
            let mut conn = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            conn.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
            conn.write_all(format!("GET /hub HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Atlas-Token: {HUB_TOKEN}\r\n\r\n").as_bytes()).unwrap();
            let mut out = Vec::new();
            let _ = conn.read_to_end(&mut out);
            waits.push(started.elapsed());
            std::thread::sleep(Duration::from_millis(100));
        }
        waits
    });
    std::thread::sleep(Duration::from_millis(150));
    let started = Instant::now();
    d.converse("tell me something about octopus hearts", &NoMoreEars, &mouth, &|| 1_790_000_000);
    let talked = started.elapsed();
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    // Then the loop's own answering, as `Daemon::run` does next.
    let door = d.hub_server.take().unwrap();
    while !asker.is_finished() {
        door.wait_and_answer(20, &mut |a| atlas::hublive::reply(&mut d, a));
    }
    let waits = asker.join().unwrap();
    let worst = waits.iter().max().copied().unwrap_or_default();
    println!("spoke {} sentence(s) in {talked:?}; {} page(s) asked for meanwhile, the slowest answered in {worst:?}", mouth.said.lock().unwrap().len(), waits.len());
    assert!(mouth.said.lock().unwrap().len() >= 4, "{:?}", mouth.said.lock().unwrap());
    // One sentence's worth of waiting at most, not the whole reply's.
    assert!(worst < Duration::from_millis(1_300), "a page waited {worst:?} while Atlas spoke for {talked:?}");
}

// ================= saving after a turn =================

#[test]
fn a_turn_does_not_write_out_an_unchanged_file_index() {
    let (c, p) = (cfg(), plat());
    let dir = tmp("persist");
    let store = Store::new(dir.clone());
    let mut d = Daemon::new(&c, &p, None, store.clone(), Proactive::new(ProactiveConfig::default()));
    // A person's Desktop, Documents and Downloads: tens of thousands of files.
    for i in 0..40_000u64 {
        let path = format!("C:/Users/eric/Documents/project-{}/notes/file-{i}.txt", i % 300);
        d.index.entries.insert(
            path.clone(),
            atlas::index::Entry { name: format!("file-{i}.txt"), path, ext: "txt".into(), size: 1000 + i, modified: 1_790_000_000 - i, class: atlas::index::AssetClass::Other },
        );
    }
    d.index.last_scan = 1_790_000_000;
    let started = Instant::now();
    d.persist();
    let first = started.elapsed();
    let file = store.root().join("index.json");
    assert!(std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0) > 1_000_000, "a changed index is written");

    // What every save after a turn cost before (28 Sep 2026): the whole index
    // turned into text, only for the store to find it unchanged.
    let started = Instant::now();
    d.index.save(&store).unwrap();
    let as_before = started.elapsed();
    let started = Instant::now();
    d.persist();
    let now = started.elapsed();
    println!("saving after a turn with 40,000 files indexed: {as_before:?} for the index alone before, {now:?} for everything now (first write {first:?})");
    assert!(now < as_before, "{now:?} vs {as_before:?}");

    // Unchanged, it isn't written at all; changed by a scan, it is.
    std::fs::write(&file, "left alone").unwrap();
    d.persist();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "left alone");
    d.index.last_scan += 60;
    d.persist();
    assert_ne!(std::fs::read_to_string(&file).unwrap(), "left alone", "a rescanned index was not written");
}

/// A model that calls the machine-health tool, then puts what it found into
/// words.
struct CallsThenWords {
    asked: Mutex<Vec<ChatRequest>>,
}

impl Llm for CallsThenWords {
    fn complete(&self, _: &str, _: &str) -> atlas::error::Result<String> {
        Ok(r#"{"action":"say","arg":null,"say":"(the one-prompt path)"}"#.into())
    }
    fn native_chat(&self) -> bool {
        true
    }
    fn chat(&self, req: &ChatRequest, on_text: &mut dyn FnMut(&str) -> bool) -> atlas::error::Result<ChatReply> {
        self.asked.lock().unwrap().push(req.clone());
        if req.tools.is_empty() {
            on_text("Your computer's fine. ");
            return Ok(ChatReply { text: "Your computer's fine.".into(), tool_calls: vec![] });
        }
        Ok(ChatReply {
            text: String::new(),
            tool_calls: vec![atlas::brain::ToolCall { name: "machine_health".into(), arguments: serde_json::json!({}) }],
        })
    }
}

#[test]
fn putting_a_result_into_words_leaves_the_conversation_slot_alone() {
    let (c, p) = (cfg(), plat());
    let llm = Arc::new(CallsThenWords { asked: Mutex::new(Vec::new()) });
    let mut d = Daemon::new(&c, &p, Some(llm.clone() as Arc<dyn Llm>), Store::new(tmp("aside")), Proactive::new(ProactiveConfig::default()));
    let fields = vec![("text".to_string(), "how's my computer doing".to_string())];
    let _ = atlas::hublive::reply(&mut d, atlas::server::Action::HubPost { path: "/hub/talk".into(), fields });
    let until = Instant::now() + Duration::from_secs(10);
    let _ = d.tick(1_790_000_000);
    while d.talk_is_thinking() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
        let _ = d.tick(1_790_000_001);
    }
    let asked = llm.asked.lock().unwrap().clone();
    assert!(asked.len() >= 2, "the result was never put into words: {} call(s)", asked.len());
    assert!(!asked[0].aside, "the turn itself is the conversation");
    assert!(asked.last().unwrap().aside, "putting the result into words went to the conversation's slot");
}

/// What Atlas starts its own model server with: the template that puts the
/// picked tools last, and idle slots left as they are.
#[cfg(unix)]
#[test]
fn atlas_starts_its_model_server_to_keep_what_it_read() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tmp("launch");
    let script = dir.join("llama-server");
    let seen = dir.join("seen.txt");
    std::fs::write(&script, format!("#!/bin/sh\necho \"$@\" > {0}\necho \"idle=$LLAMA_ARG_CACHE_IDLE_SLOTS\" >> {0}\n", seen.display())).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut cfg = atlas::models::ModelsConfig::default();
    cfg.server = Some(atlas::tools::ExternalTool { command: script.display().to_string(), ..Default::default() });
    let model = atlas::models::Model {
        path: dir.join("Qwen3-VL-4B-Instruct-Q4_K_M.gguf"),
        id: "Qwen3-VL-4B-Instruct-Q4_K_M".into(),
        architecture: "qwen3vl".into(),
        quant: "Q4_K".into(),
        parameters: 4_000_000_000,
        weight_bytes: 2_500_000_000,
        max_context: 32768,
        chat_template: Some(QWEN3_VL_TEMPLATE.into()),
    };
    let mut child = atlas::models::launch(&model, &cfg, 0, &Default::default()).expect("started");
    let _ = child.wait();
    let said = std::fs::read_to_string(&seen).unwrap();
    assert!(said.contains("idle=0"), "idle slots would be cleared: {said}");
    let args: Vec<&str> = said.lines().next().unwrap().split(' ').collect();
    let at = args.iter().position(|a| *a == "--chat-template-file").expect("started with the model's own template only");
    let written = std::fs::read_to_string(args[at + 1]).unwrap();
    assert_eq!(Some(written), atlas::models::tools_late_template(QWEN3_VL_TEMPLATE));
    assert!(args.contains(&"--jinja"));

    // A model whose template it doesn't know is started with its own.
    let other = atlas::models::Model { chat_template: Some("{{ messages }}".into()), ..model };
    let mut child = atlas::models::launch(&other, &cfg, 0, &Default::default()).expect("started");
    let _ = child.wait();
    assert!(!std::fs::read_to_string(&seen).unwrap().contains("--chat-template-file"));
}

// ================= the connection check =================

/// The tick asks whether the internet is there every pass; every thirty
/// seconds that was a TCP connect on the loop's own thread, up to 800ms
/// with the hub waiting (measured: 808ms every 30s on a network that drops
/// the probe). Once there's an answer, a re-check runs beside the loop.
#[test]
fn checking_the_connection_again_does_not_hold_the_loop() {
    use atlas::connectivity::{Connectivity, ConnectivityConfig, Reach};
    // An address nothing answers: refused at once or dropped until the
    // timeout, depending on the network -- either way it's a probe.
    let mut c = Connectivity::new(ConnectivityConfig { probe: "192.0.2.1:53".into(), timeout_ms: 800, cache_secs: 30, assume_offline: false });
    let answer = |c: &mut Connectivity, t: u64| {
        let until = Instant::now() + Duration::from_secs(3);
        loop {
            let started = Instant::now();
            let r = c.status_now(t);
            let took = started.elapsed();
            assert!(took < Duration::from_millis(100), "the loop waited {took:?} for the probe");
            if r != Reach::Unknown || Instant::now() > until {
                return r;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    // Even the first check doesn't wait: "don't know yet", then the answer.
    let started = Instant::now();
    assert_eq!(c.status_now(1_000), Reach::Unknown);
    println!("the first connection check took {:?} on the loop (the probe itself waits up to 800ms)", started.elapsed());
    let first = answer(&mut c, 1_000);
    assert_ne!(first, Reach::Unknown, "the probe's answer never arrived");
    // Stale: the old answer comes back at once while the probe runs beside.
    assert_eq!(c.status_now(1_100), first);
    std::thread::sleep(Duration::from_millis(1_200));
    assert_eq!(answer(&mut c, 1_101), first);
    // A pin still wins, and `status` still asks and waits, as before.
    c.set(Reach::Online, 1_200);
    assert_eq!(c.status_now(9_999), Reach::Online);
    c.unpin();
    assert_ne!(c.status(10_000), Reach::Unknown);
}
