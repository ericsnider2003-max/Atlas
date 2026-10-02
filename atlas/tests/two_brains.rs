//! **Two brains, and better answers (30 Sep 2026).**
//!
//! A deep model (Qwen3.5 9B on Eric's laptop) for work nobody waits on word
//! by word, run as a second llama-server beside the talking one: started when
//! such work comes, stopped when idle, never started without the memory,
//! giving way to every turn -- and the talking model's other slot when it
//! can't be had. And "better answers": Qwen3.5 4B talking instead of the
//! Qwen3-VL 4B.
//!
//! The servers here are scripted llama-servers on real sockets: `/health`
//! and a streamed `/v1/chat/completions`, a word at a time.

use atlas::brain::{ChatReply, ChatRequest, Llm, Msg};
use atlas::daemon::Daemon;
use atlas::deepbrain::{DeepBrain, Engine, State};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// A scripted llama-server.
// ---------------------------------------------------------------------------

type Replier = Arc<dyn Fn(&Value) -> String + Send + Sync>;

/// What a scripted server saw: each chat request's body, and when each word
/// went out.
#[derive(Default)]
struct Seen {
    requests: Vec<Value>,
    words_at: Vec<Instant>,
    /// When a client hung up while the server was still "reading the
    /// prompt" (before its first word).
    hung_up_at: Vec<Instant>,
}

struct Scripted {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn read_request(s: &mut TcpStream) -> Option<(String, String, String)> {
    let mut raw = Vec::new();
    let mut buf = [0u8; 4096];
    let head_end = loop {
        if let Some(i) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        let n = s.read(&mut buf).ok()?;
        if n == 0 {
            return None;
        }
        raw.extend_from_slice(&buf[..n]);
    };
    let head = String::from_utf8_lossy(&raw[..head_end]).to_string();
    let len: usize = head
        .lines()
        .find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().to_string()))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut body = raw[head_end + 4..].to_vec();
    while body.len() < len {
        let n = s.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&buf[..n]);
    }
    let mut first = head.lines().next().unwrap_or("").split_whitespace();
    let method = first.next().unwrap_or("").to_string();
    let path = first.next().unwrap_or("").to_string();
    Some((method, path, String::from_utf8_lossy(&body).to_string()))
}

fn serve_one(mut s: TcpStream, reply: Replier, word_ms: u64, prefill_ms: u64, ready_at: Instant, seen: Arc<Mutex<Seen>>) {
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let Some((method, path, body)) = read_request(&mut s) else { return };
    if method == "GET" && path == "/health" {
        let (code, text) = if Instant::now() >= ready_at { ("200 OK", r#"{"status":"ok"}"#) } else { ("503 Service Unavailable", r#"{"error":{"message":"Loading model"}}"#) };
        let _ = write!(s, "HTTP/1.1 {code}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len());
        return;
    }
    let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    seen.lock().unwrap().requests.push(v.clone());
    // Reading the prompt: nothing sent for `prefill_ms`, watching for the
    // client hanging up (llama-server cancels the request then).
    let until = Instant::now() + Duration::from_millis(prefill_ms);
    while Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
        let _ = s.set_nonblocking(true);
        let gone = match s.peek(&mut [0u8; 1]) {
            Ok(0) => true,
            Ok(_) => false,
            Err(e) => e.kind() != std::io::ErrorKind::WouldBlock,
        };
        let _ = s.set_nonblocking(false);
        if gone {
            seen.lock().unwrap().hung_up_at.push(Instant::now());
            return;
        }
    }
    let text = reply(&v);
    if s.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n").is_err() {
        return;
    }
    for w in text.split_inclusive(' ') {
        std::thread::sleep(Duration::from_millis(word_ms));
        let line = format!("data: {}\n\n", json!({"choices":[{"delta":{"content": w}}]}));
        if s.write_all(line.as_bytes()).and_then(|_| s.flush()).is_err() {
            return; // the client stopped reading: llama-server stops generating
        }
        seen.lock().unwrap().words_at.push(Instant::now());
    }
    let _ = s.write_all(b"data: [DONE]\n\n");
}

impl Scripted {
    fn start(port: u16, reply: Replier, word_ms: u64, load_ms: u64, seen: Arc<Mutex<Seen>>) -> Scripted {
        Scripted::start_reading(port, reply, word_ms, 0, load_ms, seen)
    }

    /// One that takes `prefill_ms` to read each prompt before its first word.
    fn start_reading(port: u16, reply: Replier, word_ms: u64, prefill_ms: u64, load_ms: u64, seen: Arc<Mutex<Seen>>) -> Scripted {
        let stop = Arc::new(AtomicBool::new(false));
        let l = TcpListener::bind(("127.0.0.1", port)).expect("bind the scripted server");
        l.set_nonblocking(true).unwrap();
        let ready_at = Instant::now() + Duration::from_millis(load_ms);
        let (st, sn) = (stop.clone(), seen.clone());
        let thread = std::thread::spawn(move || {
            while !st.load(Ordering::SeqCst) {
                match l.accept() {
                    Ok((s, _)) => {
                        let _ = s.set_nonblocking(false);
                        let (r, sn) = (reply.clone(), sn.clone());
                        std::thread::spawn(move || serve_one(s, r, word_ms, prefill_ms, ready_at, sn));
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
        });
        Scripted { stop, thread: Some(thread) }
    }
    fn halt(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Scripted {
    fn drop(&mut self) {
        self.halt();
    }
}

/// The deep server's process, as a scripted server started and stopped on
/// its port.
struct ScriptedEngine {
    port: u16,
    reply: Replier,
    word_ms: u64,
    prefill_ms: u64,
    load_ms: u64,
    seen: Arc<Mutex<Seen>>,
    running: Option<Scripted>,
    starts: Arc<AtomicUsize>,
}

impl Engine for ScriptedEngine {
    fn start(&mut self) -> Result<(), String> {
        self.starts.fetch_add(1, Ordering::SeqCst);
        self.running = Some(Scripted::start_reading(self.port, self.reply.clone(), self.word_ms, self.prefill_ms, self.load_ms, self.seen.clone()));
        Ok(())
    }
    fn stop(&mut self) {
        if let Some(mut s) = self.running.take() {
            s.halt();
        }
    }
    fn running(&mut self) -> bool {
        self.running.is_some()
    }
    fn healthy(&mut self) -> bool {
        atlas::deepbrain::healthy_at(self.port, Duration::from_millis(300))
    }
}

/// A chat connection to a server on this machine.
struct HttpChat(u16);

impl Llm for HttpChat {
    fn complete(&self, _: &str, _: &str) -> atlas::error::Result<String> {
        Err(atlas::error::AtlasError::Platform("chat only".into()))
    }
    fn native_chat(&self) -> bool {
        true
    }
    fn chat(&self, req: &ChatRequest, on_text: &mut dyn FnMut(&str) -> bool) -> atlas::error::Result<ChatReply> {
        atlas::models::chat_call(&format!("http://127.0.0.1:{}/v1/chat/completions", self.0), req, on_text)
    }
    fn chat_until(&self, req: &ChatRequest, on_text: &mut dyn FnMut(&str) -> bool, keep_going: &dyn Fn() -> bool) -> atlas::error::Result<ChatReply> {
        atlas::models::chat_call_until(&format!("http://127.0.0.1:{}/v1/chat/completions", self.0), req, on_text, keep_going)
    }
}

/// The talking model, as a spy: what it was asked, and whether beside the
/// conversation.
#[derive(Default)]
struct TalkSpy {
    asked: Mutex<Vec<ChatRequest>>,
    completes: AtomicUsize,
}

impl Llm for TalkSpy {
    fn complete(&self, _: &str, _: &str) -> atlas::error::Result<String> {
        self.completes.fetch_add(1, Ordering::SeqCst);
        Ok("from the talking model".into())
    }
    fn native_chat(&self) -> bool {
        true
    }
    fn chat(&self, req: &ChatRequest, on_text: &mut dyn FnMut(&str) -> bool) -> atlas::error::Result<ChatReply> {
        self.asked.lock().unwrap().push(req.clone());
        on_text("from the talking model");
        Ok(ChatReply { text: "from the talking model".into(), tool_calls: vec![] })
    }
}

fn fixed(text: &'static str) -> Replier {
    Arc::new(move |_| text.to_string())
}

/// `all`, or -- asked to carry on from words already written (the last
/// message is the assistant's) -- the rest of it.
fn continuing(all: &'static str) -> Replier {
    Arc::new(move |v: &Value| {
        let msgs = v["messages"].as_array().cloned().unwrap_or_default();
        match msgs.last() {
            Some(m) if m["role"] == "assistant" => {
                let so_far = m["content"].as_str().unwrap_or("");
                all.strip_prefix(so_far).unwrap_or(all).to_string()
            }
            _ => all.to_string(),
        }
    })
}

struct Rig {
    deep: DeepBrain,
    seen: Arc<Mutex<Seen>>,
    starts: Arc<AtomicUsize>,
    port: u16,
}

fn rig(reply: Replier, word_ms: u64, load_ms: u64, idle: Duration, free_mb: u64) -> Rig {
    rig_reading(reply, word_ms, 0, load_ms, idle, free_mb)
}

fn rig_reading(reply: Replier, word_ms: u64, prefill_ms: u64, load_ms: u64, idle: Duration, free_mb: u64) -> Rig {
    let port = free_port();
    let seen = Arc::new(Mutex::new(Seen::default()));
    let starts = Arc::new(AtomicUsize::new(0));
    let engine = ScriptedEngine { port, reply, word_ms, prefill_ms, load_ms, seen: seen.clone(), running: None, starts: starts.clone() };
    let deep = DeepBrain::new(Box::new(engine), Arc::new(HttpChat(port)), "Qwen_Qwen3.5-9B-IQ4_XS", 6_400, idle, Box::new(move || free_mb));
    Rig { deep, seen, starts, port }
}

/// Call `f` on a thread while the loop keeps the deep brain, as the
/// daemon's tick does; its answer.
fn while_kept<T: Send + 'static>(deep: &mut DeepBrain, f: impl FnOnce() -> T + Send + 'static) -> (T, Vec<String>) {
    let h = std::thread::spawn(f);
    let mut log = Vec::new();
    let until = Instant::now() + Duration::from_secs(20);
    while !h.is_finished() && Instant::now() < until {
        log.extend(deep.keep());
        std::thread::sleep(Duration::from_millis(20));
    }
    (h.join().expect("the call panicked"), log)
}

// ---------------------------------------------------------------------------
// 1. Routing, starting on demand, stopping when idle.
// ---------------------------------------------------------------------------

#[test]
fn background_work_starts_the_deep_brain_goes_to_it_and_it_stops_when_idle() {
    let mut r = rig(fixed("A careful draft from the deep model."), 5, 300, Duration::from_millis(600), 16_000);
    let talk = Arc::new(TalkSpy::default());
    assert_eq!(r.deep.gate.state(), State::Off);
    // Nothing asked: nothing started.
    for _ in 0..5 {
        r.deep.keep();
    }
    assert_eq!(r.starts.load(Ordering::SeqCst), 0, "started with no work");

    let llm = r.deep.for_background(talk.clone() as Arc<dyn Llm>);
    let (got, log) = while_kept(&mut r.deep, move || llm.complete("You draft.", "Draft a note to Northwind about the invoice."));
    assert_eq!(got.unwrap(), "A careful draft from the deep model.");
    assert_eq!(r.starts.load(Ordering::SeqCst), 1);
    assert!(log.iter().any(|l| l.contains("starting the deep model")), "{log:?}");
    assert!(log.iter().any(|l| l.contains("the deep model is up")), "{log:?}");
    assert_eq!(talk.completes.load(Ordering::SeqCst) + talk.asked.lock().unwrap().len(), 0, "the talking model was asked");
    // What the deep server was sent: the prompt as a conversation, no thinking.
    let reqs = r.seen.lock().unwrap().requests.clone();
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0]["messages"][0]["role"], "system");
    assert!(reqs[0]["messages"][1]["content"].as_str().unwrap().contains("Northwind"));
    assert_eq!(reqs[0]["chat_template_kwargs"]["enable_thinking"], false);

    // Idle past its time: stopped, and the port answers no more.
    let until = Instant::now() + Duration::from_secs(5);
    while r.deep.gate.state() != State::Off && Instant::now() < until {
        r.deep.keep();
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(r.deep.gate.state(), State::Off, "never stopped");
    assert!(!atlas::deepbrain::healthy_at(r.port, Duration::from_millis(200)), "still answering after being stopped");

    // More work: started again.
    let llm = r.deep.for_background(talk.clone() as Arc<dyn Llm>);
    let (got, _) = while_kept(&mut r.deep, move || llm.complete("You draft.", "And another."));
    assert!(got.unwrap().contains("careful draft"));
    assert_eq!(r.starts.load(Ordering::SeqCst), 2);
    assert_eq!(r.deep.gate.served.load(Ordering::SeqCst), 2);
}

#[test]
fn it_is_not_stopped_while_work_is_waiting_on_it() {
    // Idle time shorter than one slow call: the call in progress keeps it.
    let mut r = rig(fixed("one two three four five six seven eight nine ten"), 60, 0, Duration::from_millis(100), 16_000);
    let talk = Arc::new(TalkSpy::default());
    let llm = r.deep.for_background(talk as Arc<dyn Llm>);
    let (got, log) = while_kept(&mut r.deep, move || llm.complete("s", "u"));
    assert_eq!(got.unwrap(), "one two three four five six seven eight nine ten");
    assert!(!log.iter().any(|l| l.contains("stopped the deep model")), "stopped mid-call: {log:?}");
    assert_eq!(r.starts.load(Ordering::SeqCst), 1);
}

// ---------------------------------------------------------------------------
// 2. Memory: refused rather than paging the machine.
// ---------------------------------------------------------------------------

#[test]
fn without_the_memory_it_is_never_started_and_the_talking_models_other_slot_answers() {
    let mut r = rig(fixed("never"), 5, 0, Duration::from_secs(60), 3_000);
    let talk = Arc::new(TalkSpy::default());
    let llm = r.deep.for_background(talk.clone() as Arc<dyn Llm>);
    let req = ChatRequest { messages: vec![Msg::system("s"), Msg::user("summarise the conversation")], max_tokens: 200, ..Default::default() };
    let (got, log) = while_kept(&mut r.deep, move || llm.chat(&req, &mut |_| true));
    assert_eq!(got.unwrap().text, "from the talking model");
    assert_eq!(r.starts.load(Ordering::SeqCst), 0, "started without the memory");
    assert_eq!(r.deep.gate.state(), State::Unavailable);
    let why = r.deep.gate.why_not();
    assert!(why.contains("7424 MB") && why.contains("3000 MB is free"), "{why}");
    assert!(log.iter().any(|l| l.contains("needs about")), "{log:?}");
    // Beside the conversation, not in its slot.
    let asked = talk.asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 1);
    assert!(asked[0].aside, "the fallback used the conversation's slot");
    assert_eq!(r.deep.gate.fell_back.load(Ordering::SeqCst), 1);
    // And said on the Connections page's line.
    assert!(r.deep.describe().contains("not now"), "{}", r.deep.describe());
}

#[test]
fn a_deep_server_that_wont_start_hands_the_work_back() {
    struct Broken;
    impl Engine for Broken {
        fn start(&mut self) -> Result<(), String> {
            Err("llama-server isn't there".into())
        }
        fn stop(&mut self) {}
        fn running(&mut self) -> bool {
            false
        }
        fn healthy(&mut self) -> bool {
            false
        }
    }
    let mut deep = DeepBrain::new(Box::new(Broken), Arc::new(HttpChat(free_port())), "deep", 100, Duration::from_secs(60), Box::new(|| 16_000));
    let talk = Arc::new(TalkSpy::default());
    let llm = deep.for_background(talk.clone() as Arc<dyn Llm>);
    let (got, log) = while_kept(&mut deep, move || llm.complete("s", "u"));
    assert_eq!(got.unwrap(), "from the talking model");
    assert!(log.iter().any(|l| l.contains("couldn't start: llama-server isn't there")), "{log:?}");
}

// ---------------------------------------------------------------------------
// 3. Giving way to the talking model.
// ---------------------------------------------------------------------------

#[test]
fn a_turn_cuts_in_and_the_deep_work_carries_on_from_its_words() {
    // First request: ten words, slowly. The carry-on request (its words so
    // far as the start of the answer) gets the rest.
    const ALL: &str = "one two three four five six seven eight nine ten";
    let mut r = rig(continuing(ALL), 80, 0, Duration::from_secs(60), 16_000);
    let talk = Arc::new(TalkSpy::default());
    let gate = r.deep.gate.clone();
    let llm = r.deep.for_background(talk as Arc<dyn Llm>);
    let h = std::thread::spawn(move || {
        let req = ChatRequest { messages: vec![Msg::system("s"), Msg::user("write it up")], max_tokens: 300, ..Default::default() };
        llm.chat(&req, &mut |_| true)
    });
    // Up, and a few words written...
    let until = Instant::now() + Duration::from_secs(10);
    while r.seen.lock().unwrap().words_at.len() < 3 && Instant::now() < until {
        r.deep.keep();
        std::thread::sleep(Duration::from_millis(10));
    }
    // ... then a turn begins, and lasts 600 ms.
    let turn = gate.talk();
    let began = Instant::now();
    std::thread::sleep(Duration::from_millis(600));
    let during: Vec<Instant> = r.seen.lock().unwrap().words_at.iter().filter(|w| **w > began).cloned().collect();
    let ended = Instant::now();
    drop(turn);
    let got = h.join().unwrap().unwrap();
    assert_eq!(got.text, ALL, "the words weren't carried on whole");
    // Nothing written while the turn was answered, bar the word already on
    // its way when it began and one more: the connection is closed at the
    // first word that arrives, and a server -- llama-server as this one --
    // finds out when its next write fails, the one after that.
    assert!(during.len() <= 2, "the deep model kept writing through the turn: {} words", during.len());
    assert!(during.iter().all(|w| *w < began + Duration::from_millis(250)), "a word came long after the turn began");
    let after: usize = r.seen.lock().unwrap().words_at.iter().filter(|w| **w > ended).count();
    assert!(after > 0, "it never carried on");
    assert_eq!(gate.yields.load(Ordering::SeqCst), 1);
    let reqs = r.seen.lock().unwrap().requests.clone();
    assert_eq!(reqs.len(), 2, "{reqs:?}");
    let last = reqs[1]["messages"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["role"], "assistant", "the carry-on didn't send its words so far");
    assert!(ALL.starts_with(last["content"].as_str().unwrap()) && last["content"].as_str().unwrap().starts_with("one two three"));
}

#[test]
fn a_turn_cuts_in_while_the_deep_model_is_still_reading_its_prompt() {
    // A long prompt (research sources) takes seconds to read before the
    // first word: the turn can't wait for a word to cut in at.
    let mut r = rig_reading(fixed("The write-up."), 5, 1_500, 0, Duration::from_secs(60), 16_000);
    let talk = Arc::new(TalkSpy::default());
    let gate = r.deep.gate.clone();
    let llm = r.deep.for_background(talk as Arc<dyn Llm>);
    let h = std::thread::spawn(move || llm.complete("s", "write up these sources"));
    let until = Instant::now() + Duration::from_secs(10);
    while r.seen.lock().unwrap().requests.is_empty() && Instant::now() < until {
        r.deep.keep();
        std::thread::sleep(Duration::from_millis(10));
    }
    std::thread::sleep(Duration::from_millis(200));
    let turn = gate.talk();
    let began = Instant::now();
    std::thread::sleep(Duration::from_millis(700));
    let hung_up = r.seen.lock().unwrap().hung_up_at.clone();
    assert_eq!(hung_up.len(), 1, "the deep request wasn't dropped while the turn was answered");
    assert!(hung_up[0].duration_since(began) < Duration::from_millis(500), "it took {:?} to give way", hung_up[0].duration_since(began));
    assert_eq!(r.seen.lock().unwrap().requests.len(), 1, "asked again during the turn");
    drop(turn);
    let until = Instant::now() + Duration::from_secs(10);
    while !h.is_finished() && Instant::now() < until {
        r.deep.keep();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(h.join().unwrap().unwrap(), "The write-up.");
    assert_eq!(r.seen.lock().unwrap().requests.len(), 2, "asked again once the turn was answered");
    assert_eq!(gate.yields.load(Ordering::SeqCst), 1);
}

#[test]
fn deep_work_waits_for_a_turn_in_progress_before_it_starts() {
    let mut r = rig(fixed("done"), 5, 0, Duration::from_secs(60), 16_000);
    let gate = r.deep.gate.clone();
    let turn = gate.talk();
    let talk = Arc::new(TalkSpy::default());
    let llm = r.deep.for_background(talk as Arc<dyn Llm>);
    let h = std::thread::spawn(move || llm.complete("s", "u"));
    // Not even started (loading competes too) while the turn is answered.
    for _ in 0..20 {
        r.deep.keep();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(r.starts.load(Ordering::SeqCst), 0, "started during a turn");
    assert!(r.seen.lock().unwrap().requests.is_empty());
    drop(turn);
    let until = Instant::now() + Duration::from_secs(10);
    while !h.is_finished() && Instant::now() < until {
        r.deep.keep();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(h.join().unwrap().unwrap(), "done");
}

#[test]
fn the_talking_server_answers_at_full_speed_while_deep_work_is_under_way() {
    // Two servers on two ports, as on the laptop.
    let talk_seen = Arc::new(Mutex::new(Seen::default()));
    let talk_port = free_port();
    let _talk_server = Scripted::start(talk_port, fixed("Here you go."), 20, 0, talk_seen.clone());
    let talk: Arc<dyn Llm> = Arc::new(HttpChat(talk_port));
    let mut r = rig(continuing("a b c d e f g h i j k l m n o p q r s t"), 50, 0, Duration::from_secs(60), 16_000);
    let gate = r.deep.gate.clone();
    let bg = r.deep.for_background(talk.clone());
    let h = std::thread::spawn(move || bg.complete("s", "research write-up"));
    let until = Instant::now() + Duration::from_secs(10);
    while r.seen.lock().unwrap().words_at.len() < 2 && Instant::now() < until {
        r.deep.keep();
        std::thread::sleep(Duration::from_millis(10));
    }
    // A turn, the way the daemon answers one: holding the guard.
    let t0 = Instant::now();
    let reply = {
        let _turn = gate.talk();
        talk.chat(&ChatRequest { messages: vec![Msg::user("hi")], max_tokens: 50, ..Default::default() }, &mut |_| true).unwrap()
    };
    let took = t0.elapsed();
    let t1 = Instant::now();
    assert_eq!(reply.text, "Here you go.");
    assert!(took < Duration::from_millis(1_500), "the turn took {took:?}");
    // The deep server wrote nothing while the turn was answered (bar one
    // word already on its way).
    let overlapping = r.seen.lock().unwrap().words_at.iter().filter(|w| **w > t0 && **w < t1).count();
    assert!(overlapping <= 1, "{overlapping} deep words written during the turn");
    let until = Instant::now() + Duration::from_secs(20);
    while !h.is_finished() && Instant::now() < until {
        r.deep.keep();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(h.join().unwrap().unwrap(), "a b c d e f g h i j k l m n o p q r s t");
    // The talking server was asked once: the turn. None of the deep work.
    assert_eq!(talk_seen.lock().unwrap().requests.len(), 1);
}

// ---------------------------------------------------------------------------
// 4. The daemon: background call sites go to the deep brain.
// ---------------------------------------------------------------------------

fn cfg() -> atlas::config::Config {
    atlas::config::Config::load(Path::new("config")).unwrap()
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-two-brains-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn a_decision_drafted_in_the_background_is_written_by_the_deep_brain() {
    let (c, p) = (cfg(), plat());
    let talk = Arc::new(TalkSpy::default());
    let mut d = Daemon::new(&c, &p, Some(talk.clone() as Arc<dyn Llm>), Store::new(tmp("decide")), Proactive::new(ProactiveConfig::default()));
    let r = rig(fixed("OPTION: take the contract\nOPTION: keep the retainer\nLEAN: keep the retainer"), 2, 100, Duration::from_secs(60), 16_000);
    let seen = r.seen.clone();
    d.use_deep_brain_for_test(r.deep);
    let reply = d.turn("Should I take the contract or keep the retainer?", 1_790_740_000);
    assert!(reply.contains("think that through"), "{reply}");
    let mut news = Vec::new();
    let until = Instant::now() + Duration::from_secs(20);
    let mut t = 1_790_740_001;
    while news.is_empty() && Instant::now() < until {
        news.extend(d.tick(t));
        t += 1;
        std::thread::sleep(Duration::from_millis(30));
    }
    let reqs = seen.lock().unwrap().requests.clone();
    assert_eq!(reqs.len(), 1, "the deep model wasn't asked: {news:?}");
    assert!(reqs[0]["messages"][1]["content"].as_str().unwrap().contains("contract"));
    assert_eq!(talk.completes.load(Ordering::SeqCst), 0, "the talking model drafted it");
    assert_eq!(d.deep_gate_for_test().served.load(Ordering::SeqCst), 1);
}

#[test]
fn every_background_call_site_asks_for_the_background_model() {
    // Research, the running summary, drafted mail, a decision, code drafts,
    // the council, a call's notes, keeping at a build: each takes
    // `background_llm`, not the talking model.
    let src = |p: &str| std::fs::read_to_string(p).unwrap();
    for (file, fun) in [
        ("src/daemon/reading.rs", "fn research("),
        ("src/daemon/late.rs", "fn fold_if_due("),
        ("src/daemon/late.rs", "fn keep_at_it("),
        ("src/daemon/inbox.rs", "fn check_mail("),
        ("src/daemon/inbox.rs", "fn draft_outreach("),
        ("src/daemon/helping.rs", "fn decision_help("),
        ("src/daemon/making.rs", "fn build_from_description("),
        ("src/daemon/making.rs", "fn code_writers("),
        ("src/daemon/making.rs", "fn improve_project("),
        ("src/daemon/model.rs", "fn ask_the_room("),
        ("src/daemon/hands.rs", "fn carry_out("),
    ] {
        let s = src(file);
        let at = s.find(fun).unwrap_or_else(|| panic!("{fun} is gone from {file}"));
        let body = &s[at..];
        let end = body[3..].find("\n    pub").or_else(|| body[3..].find("\n    fn ")).map(|e| e + 3).unwrap_or(body.len());
        let body = &body[..end];
        // `code_writers` (2 Oct 2026) is the background model first, then the
        // stronger ones -- checked in this list on its own.
        assert!(
            body.contains("self.background_llm()") || body.contains("self.code_writers("),
            "{fun} in {file} doesn't use the background model"
        );
        assert!(!body.contains("self.llm.clone()"), "{fun} in {file} still takes the talking model");
    }
    // And every talking call holds the guard the deep model gives way to.
    let conv = src("src/daemon/conversation.rs");
    assert_eq!(conv.matches("let talking = self.talking_guard();").count(), 2, "the turn's worker and the rewording");
    assert!(src("src/daemon/turn.rs").contains("let _talking = self.talking_guard();"));
}

// ---------------------------------------------------------------------------
// 5. Choosing the models.
// ---------------------------------------------------------------------------

fn model(id: &str, params_b: f64, bytes: u64) -> atlas::models::Model {
    atlas::models::Model {
        path: PathBuf::from(format!("models/{id}.gguf")),
        id: id.into(),
        architecture: "qwen35".into(),
        quant: "Q4_K".into(),
        parameters: (params_b * 1e9) as u64,
        weight_bytes: bytes,
        max_context: 262_144,
        chat_template: None,
    }
}

fn eric_s_folder() -> atlas::models::Registry {
    atlas::models::Registry {
        models: vec![
            model("Qwen3VL-4B-Instruct-Q4_K_M", 4.0, 2_497_281_664),
            model("Qwen_Qwen3.5-4B-Q4_K_M", 4.2, 3_013_027_808),
            model("Qwen_Qwen3.5-9B-IQ4_XS", 9.0, 5_501_202_464),
            model("mmproj-Qwen3VL-4B-Instruct-Q8_0", 0.4, 453_974_304),
        ],
    }
}

#[test]
fn the_talking_model_is_the_faster_one_unless_better_is_asked_for() {
    let reg = eric_s_folder();
    let mut mc = atlas::models::ModelsConfig::default();
    let budget = 8u64 << 30;
    // Shipped: faster. Qwen3.5 4B in the folder, bigger, doesn't take over.
    assert_eq!(reg.choose_for(&mc, budget).unwrap().id, "Qwen3VL-4B-Instruct-Q4_K_M");
    mc.talk = "faster".into();
    assert_eq!(reg.choose_for(&mc, budget).unwrap().id, "Qwen3VL-4B-Instruct-Q4_K_M");
    mc.talk = "better".into();
    assert_eq!(reg.choose_for(&mc, budget).unwrap().id, "Qwen_Qwen3.5-4B-Q4_K_M");
    assert!(atlas::models::talks_better(&mc));
    // Better asked for and not here: the faster one, not nothing.
    let without = atlas::models::Registry { models: reg.models.iter().filter(|m| !m.id.contains("3.5-4B")).cloned().collect() };
    assert_eq!(without.choose_for(&mc, budget).unwrap().id, "Qwen3VL-4B-Instruct-Q4_K_M");
    // A model named in `prefer` still wins over the shipped default.
    let mut named = atlas::models::ModelsConfig { prefer: "Qwen_Qwen3.5-4B-Q4_K_M".into(), ..Default::default() };
    named.talk = "faster".into();
    assert_eq!(reg.choose_for(&named, budget).unwrap().id, "Qwen_Qwen3.5-4B-Q4_K_M");
}

#[test]
fn the_deep_model_is_the_9b_when_it_is_there_and_never_the_talking_one() {
    let reg = eric_s_folder();
    let mut mc = atlas::models::ModelsConfig::default();
    assert_eq!(atlas::deepbrain::deep_model_among(&reg, &mc).unwrap().id, "Qwen_Qwen3.5-9B-IQ4_XS");
    mc.deep = "off".into();
    assert!(atlas::deepbrain::deep_model_among(&reg, &mc).is_none());
    mc.deep = "Qwen_Qwen3.5-4B-Q4_K_M.gguf".into();
    assert_eq!(atlas::deepbrain::deep_model_among(&reg, &mc).unwrap().id, "Qwen_Qwen3.5-4B-Q4_K_M");
    mc.talk = "better".into();
    assert!(atlas::deepbrain::deep_model_among(&reg, &mc).is_none(), "the talking model doubled as the deep one");
    let none_here = atlas::models::Registry { models: vec![model("Qwen3VL-4B-Instruct-Q4_K_M", 4.0, 2_497_281_664)] };
    assert!(atlas::deepbrain::deep_model_among(&none_here, &atlas::models::ModelsConfig::default()).is_none());
    // Its server: its own port and context, no helper model.
    let d = atlas::deepbrain::deep_settings(&atlas::models::ModelsConfig { draft: "Qwen3-0.6B-Q8_0.gguf".into(), ..Default::default() });
    assert_eq!((d.port, d.context, d.draft.as_str()), (8081, 8192, ""));
    // Neither model is chosen as the other when nothing fits in memory.
    let mc = atlas::models::ModelsConfig::default();
    assert_ne!(reg.choose_for(&mc, 8u64 << 30).unwrap().id, "Qwen_Qwen3.5-9B-IQ4_XS");
}

#[test]
fn qwen35_s_cache_is_sized_for_its_attention_layers_only() {
    // Real GGUF bytes with Qwen3.5 9B's own numbers (read from the published
    // file's header, 30 Sep 2026): 33 layers, one in four attention.
    fn kv(key: &str, v: u32) -> Vec<u8> {
        let mut o = (key.len() as u64).to_le_bytes().to_vec();
        o.extend_from_slice(key.as_bytes());
        o.extend(4u32.to_le_bytes());
        o.extend(v.to_le_bytes());
        o
    }
    let mut kvs: Vec<Vec<u8>> = Vec::new();
    let mut s = ("general.architecture".len() as u64).to_le_bytes().to_vec();
    s.extend_from_slice(b"general.architecture");
    s.extend(8u32.to_le_bytes());
    s.extend((6u64).to_le_bytes());
    s.extend_from_slice(b"qwen35");
    kvs.push(s);
    for (k, v) in [
        ("qwen35.block_count", 33),
        ("qwen35.embedding_length", 4096),
        ("qwen35.attention.head_count", 16),
        ("qwen35.attention.head_count_kv", 4),
        ("qwen35.attention.key_length", 256),
        ("qwen35.full_attention_interval", 4),
    ] {
        kvs.push(kv(k, v));
    }
    let mut bytes = b"GGUF".to_vec();
    bytes.extend(3u32.to_le_bytes());
    bytes.extend(0u64.to_le_bytes());
    bytes.extend((kvs.len() as u64).to_le_bytes());
    for k in kvs {
        bytes.extend(k);
    }
    let g = atlas::gguf::Gguf::read(std::io::Cursor::new(bytes)).unwrap();
    // 9 attention layers x 4 heads x 256 x (key + value) x 2 bytes x 8192.
    assert_eq!(g.kv_cache_bytes(8192), 2 * 2 * 9 * 4 * 256 * 8192);
}

#[test]
fn the_real_qwen35_template_thinks_unless_told_and_takes_the_tools_late() {
    // Read out of bartowski's Qwen3.5 4B and 9B GGUF files (identical in
    // both, 30 Sep 2026): it thinks unless `enable_thinking` is false.
    let t = include_str!("fixtures/models/qwen3.5-4b-9b-chat-template.jinja");
    assert!(t.contains("enable_thinking is defined and enable_thinking is false"));
    // The chat path sends the switch...
    let body: Value = serde_json::from_str(&atlas::models::chat_body(&ChatRequest { messages: vec![Msg::user("hi")], max_tokens: 20, ..Default::default() }, true)).unwrap();
    assert_eq!(body["chat_template_kwargs"]["enable_thinking"], false);
    // ...and the one-prompt path writes the empty thought itself.
    let chatml = atlas::models::Template::ChatMl.render("S", "U");
    let p = atlas::models::no_thinking_prompt(chatml.clone(), Some(t));
    assert!(p.ends_with("<|im_start|>assistant\n<think>\n\n</think>\n\n"), "{p}");
    let vl = include_str!("fixtures/models/qwen3-vl-chat-template.jinja");
    assert_eq!(atlas::models::no_thinking_prompt(chatml.clone(), Some(vl)), chatml, "the VL model has no thinking to switch off");
    // Atlas's tools-late template applies to it.
    let late = atlas::models::tools_late_template(t).expect("the real Qwen3.5 template is one it knows");
    assert_eq!(late.matches("{%- for tool in tools[:atlas_early] %}").count(), 1);
    let elif = late.find("{%- elif message.role == \"user\" %}").unwrap();
    assert!(elif < late.find("# Tools for this request").unwrap());
}

#[test]
fn better_answers_is_a_setting_the_hub_and_voice_share() {
    let t = atlas::voice::ToolsConfig::default();
    let reg = atlas::settings::registry(&t);
    let s = reg.get("models.talk").expect("Better answers is on the settings page");
    assert_eq!(s.name, "Better answers");
    assert!(!atlas::settings::needs_a_restart("models.talk"), "it switches live");
    let shipped = atlas::config::Config::load(Path::new("config")).unwrap().tools.unwrap();
    assert_eq!(shipped.models.talk, "faster");
    assert!(!atlas::models::talks_better(&shipped.models));
    let dir = tmp("setting");
    let mut reg = atlas::settings::registry(&t);
    let said = reg.set_and_keep("models.talk", "better", &dir);
    assert!(!said.contains("couldn't"), "{said}");
    let prefs = atlas::preferences::Preferences::load(&dir);
    let mut root: serde_yaml::Value = serde_yaml::from_str("models: {}").unwrap();
    prefs.apply_to(&mut root);
    assert_eq!(root["models"]["talk"].as_str(), Some("better"));
    assert!(reg.set("models.talk", "cleverest").is_err(), "only faster or better");

    use atlas::deepbrain::asks_for_talk_model;
    assert_eq!(asks_for_talk_model("Atlas, use the better model."), Some(true));
    assert_eq!(asks_for_talk_model("switch to the faster model please"), Some(false));
    assert_eq!(asks_for_talk_model("use the smarter model"), Some(true));
    assert_eq!(asks_for_talk_model("which model is better for coding"), None);
    assert_eq!(asks_for_talk_model("the better model of car"), None);
}

#[test]
fn asking_for_the_better_model_when_it_isnt_here_says_how_to_get_it() {
    let (c, p) = (cfg(), plat());
    let talk = Arc::new(TalkSpy::default());
    let mut d = Daemon::new(&c, &p, Some(talk.clone() as Arc<dyn Llm>), Store::new(tmp("better")), Proactive::new(ProactiveConfig::default()));
    let reply = d.turn("use the better model", 1_790_740_000);
    assert!(reply.contains("isn't on this computer yet") && reply.contains("2.8 GB"), "{reply}");
    assert!(talk.asked.lock().unwrap().is_empty(), "the model was asked");
}

#[test]
fn the_models_are_pinned_and_the_hub_offers_them() {
    let deep = atlas::getpieces::deep_model();
    assert_eq!(deep.bytes, 5_501_202_464);
    assert_eq!(deep.sha256, "7d977cc96c2e08616016d967f232083e354691a8a16f345b26f7d782ee5c9601");
    assert!(deep.url.contains("bartowski/Qwen_Qwen3.5-9B-GGUF/resolve/182be2fd") && deep.url.ends_with("Qwen_Qwen3.5-9B-IQ4_XS.gguf"));
    assert_eq!(atlas::getpieces::gib_label(deep.bytes), "5.1 GB");
    let better = atlas::getpieces::better_talk_model();
    assert_eq!(better.bytes, 3_013_027_808);
    assert_eq!(better.sha256, "13c16f426047e2de38cd075bdade4a7bcbc8c774384876f677740cda65f8a983");
    assert_eq!(atlas::getpieces::gib_label(better.bytes), "2.8 GB");
    // The file names land where the model folder is scanned, under the ids
    // `deepbrain` looks for.
    assert!(deep.key_path().ends_with(&format!("{}.gguf", atlas::deepbrain::DEEP_DEFAULT)));
    assert!(better.key_path().ends_with(&format!("{}.gguf", atlas::deepbrain::BETTER_TALK)));
    assert!(atlas::getpieces::faster_talk_model().key_path().ends_with(&format!("{}.gguf", atlas::deepbrain::FASTER_TALK)));

    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, Some(Arc::new(TalkSpy::default()) as Arc<dyn Llm>), Store::new(tmp("hub")), Proactive::new(ProactiveConfig::default()));
    let html = atlas::hublive::reply(&mut d, atlas::server::Action::Hub(atlas::hub::Page::Connections)).body;
    assert!(html.contains("Two brains"), "no section: {}", &html[html.len().saturating_sub(2500)..]);
    assert!(html.contains("Get the deep brain \u{b7} 5.1 GB"), "no deep button");
    assert!(html.contains("action=/hub/brains"));
    assert!(atlas::server::route(&atlas::server::parse_request("POST /hub/brains HTTP/1.1\r\n", "what=get-deep").unwrap()).is_some());
}

#[test]
fn a_model_already_on_this_computer_is_taken_in_after_its_hash_is_checked() {
    let root = tmp("takein");
    let bench = root.join("model-bench");
    std::fs::create_dir_all(&bench).unwrap();
    let data = b"not really a model, but checked like one".to_vec();
    let piece = atlas::getpieces::Piece {
        name: "a test model",
        for_what: "a test",
        url: "https://example.invalid/x.gguf",
        sha256: Box::leak(atlas::digest::sha256_hex(&data).into_boxed_str()),
        bytes: data.len() as u64,
        lands: atlas::getpieces::Lands::File("models/x.gguf"),
    };
    // Wrong bytes of the right size: left where they are.
    std::fs::write(bench.join("x.gguf"), vec![b'z'; data.len()]).unwrap();
    assert_eq!(atlas::getpieces::take_in(&piece, &root, &atlas::getpieces::places_it_may_be(&root)).unwrap(), None);
    assert!(bench.join("x.gguf").exists());
    std::fs::write(bench.join("x.gguf"), &data).unwrap();
    let from = atlas::getpieces::take_in(&piece, &root, &atlas::getpieces::places_it_may_be(&root)).unwrap();
    assert_eq!(from, Some(bench.clone()));
    assert!(atlas::getpieces::have(&piece, &root));
    assert_eq!(std::fs::read(root.join("models/x.gguf")).unwrap(), data);
    let _ = std::fs::remove_dir_all(&root);
}

/// The laptop, 1 Oct 2026: 2.3 GB free at start, the 4B over the budget,
/// and the 0.6B helper model Atlas had just fetched for drafts took over
/// talking. The helper never talks; the talking model is used over budget.
#[test]
fn the_helper_model_never_talks_even_when_memory_is_short() {
    let mut reg = eric_s_folder();
    reg.models.push(model(atlas::models::DRAFT_ONLY, 0.6, 639_446_688));
    let mc = atlas::models::ModelsConfig::default();
    assert_eq!(reg.choose_for(&mc, 1u64 << 30).unwrap().id, "Qwen3VL-4B-Instruct-Q4_K_M");
    assert_eq!(reg.choose_for(&mc, 8u64 << 30).unwrap().id, "Qwen3VL-4B-Instruct-Q4_K_M");
}
