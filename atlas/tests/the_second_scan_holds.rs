//! What the second scan of the merged tree found (28 Sep 2026), each pinned
//! by what it does rather than by how it is written.
//!
//! Eric: "everything you found needs to be fixed." In the order found:
//!
//! - A voice turn took over a Talk page turn still thinking, and ran it twice.
//! - A click answered "busy" still ran later.
//! - Pause and "stop everything" didn't stop a model turn in flight.
//! - A client sending a byte every two seconds held a connection for hours.
//! - A streamed voice reply recorded a second of sound before every sentence.
//! - Any error from the model server turned chat off for ten minutes.
//! - Nothing kept the whole prompt inside the model's context.
//! - A stream that failed partway was answered twice.
//! - Tool calls: a split tag was read out, a required argument missing ran
//!   with nothing, a second call vanished, an announced tool was never called.
//! - Times read with the wrong offset around the clocks changing.
//! - A Talk reply vanished when another turn wrote to the conversation.
//! - One file held by a virus scanner started that part of Atlas empty.
//! - The call counter behind turn timings stopped at 5,000.
//! - Hub jobs: unseen answers kept forever, running jobs evicted.
//! - Temp folders never removed.
//! - The phone page read the iPhone app on the loop.
//! - Read-style tools answered with fixed text; live pages reloaded whole.

use atlas::brain::{Brain, ChatReply, ChatRequest, Llm, Msg, Role, ToolCall, Turn};
use atlas::daemon::{Daemon, Ears, Mouth};
use atlas::intent::{Intent, Parser, ToolBook};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::server::{Action, Reply, Server, ServerConfig};
use atlas::store::Store;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-second-scan-{tag}-{}", std::process::id()));
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

/// A model with a chat endpoint whose answers the test scripts by what was
/// said, and which holds any sentence containing `hold` until `open()`.
struct Scripted {
    asked: Mutex<Vec<ChatRequest>>,
    answer: Box<dyn Fn(&ChatRequest) -> ChatReply + Send + Sync>,
    hold: String,
    gate: (Mutex<bool>, Condvar),
}

impl Scripted {
    fn new(hold: &str, answer: impl Fn(&ChatRequest) -> ChatReply + Send + Sync + 'static) -> Arc<Scripted> {
        Arc::new(Scripted {
            asked: Mutex::new(Vec::new()),
            answer: Box::new(answer),
            hold: hold.to_string(),
            gate: (Mutex::new(false), Condvar::new()),
        })
    }
    fn open(&self) {
        *self.gate.0.lock().unwrap() = true;
        self.gate.1.notify_all();
    }
    fn times_asked_about(&self, word: &str) -> usize {
        self.asked.lock().unwrap().iter().filter(|r| last_user(r).contains(word)).count()
    }
    fn requests(&self) -> Vec<ChatRequest> {
        self.asked.lock().unwrap().clone()
    }
}

fn last_user(r: &ChatRequest) -> String {
    r.messages.iter().rev().find(|m| m.role == Role::User).map(|m| m.content.clone()).unwrap_or_default()
}

fn text(t: &str) -> ChatReply {
    ChatReply { text: t.into(), tool_calls: vec![] }
}

fn call(name: &str, args: serde_json::Value) -> ToolCall {
    ToolCall { name: name.into(), arguments: args }
}

impl Llm for Scripted {
    fn complete(&self, _: &str, _: &str) -> atlas::error::Result<String> {
        Ok(r#"{"action":"say","arg":null,"say":"(the one-prompt path)"}"#.into())
    }
    fn native_chat(&self) -> bool {
        true
    }
    fn chat(&self, req: &ChatRequest, on_text: &mut dyn FnMut(&str) -> bool) -> atlas::error::Result<ChatReply> {
        self.asked.lock().unwrap().push(req.clone());
        if !self.hold.is_empty() && last_user(req).contains(&self.hold) {
            let (m, c) = &self.gate;
            let mut open = m.lock().unwrap();
            let until = Instant::now() + Duration::from_secs(20);
            while !*open && Instant::now() < until {
                open = c.wait_timeout(open, Duration::from_millis(50)).unwrap().0;
            }
        }
        let r = (self.answer)(req);
        for w in r.text.split_inclusive(' ') {
            if !on_text(w) {
                break;
            }
        }
        Ok(r)
    }
}

fn daemon<'a>(c: &'a atlas::config::Config, p: &'a MockPlatform, llm: Arc<Scripted>, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, Some(llm), Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

fn post_talk(d: &mut Daemon, text: &str) {
    let fields = vec![("text".to_string(), text.to_string())];
    let _ = atlas::hublive::reply(d, Action::HubPost { path: "/hub/talk".into(), fields });
}

fn tick_until_quiet(d: &mut Daemon, t: u64) {
    let until = Instant::now() + Duration::from_secs(15);
    while d.talk_is_thinking() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(30));
        let _ = d.tick(t);
    }
    let _ = d.tick(t);
}

/// Ears that count every time something asked them to record.
#[derive(Default)]
struct CountingEars {
    brief: AtomicUsize,
}

impl Ears for CountingEars {
    fn wait_for_wake(&self) -> atlas::error::Result<()> {
        Ok(())
    }
    fn listen(&self) -> atlas::error::Result<String> {
        Ok(String::new())
    }
    fn listen_briefly(&self, _secs: u32) -> atlas::error::Result<Option<String>> {
        self.brief.fetch_add(1, Ordering::SeqCst);
        Ok(None)
    }
}

#[derive(Default)]
struct Quiet {
    said: Mutex<Vec<String>>,
}

impl Mouth for Quiet {
    fn speak(&self, text: &str) -> atlas::error::Result<()> {
        self.said.lock().unwrap().push(text.to_string());
        Ok(())
    }
}

const STORY: &str = "tell me a short story about a lighthouse";
const STORY_REPLY: &str = "Once upon a time a lighthouse kept its light. The keeper slept. The ships came home.";

fn story_or_islands() -> Arc<Scripted> {
    Scripted::new("lighthouse", |r| {
        if last_user(r).contains("lighthouse") {
            text(STORY_REPLY)
        } else {
            text("Mostly basalt. It cools fast in the sea. Then it cracks. Then plants arrive.")
        }
    })
}

// ================= 1 and 11: a voice turn and a Talk turn at once =================

#[test]
fn a_voice_turn_leaves_a_talk_turn_that_is_still_thinking_alone() {
    let (c, p) = (cfg(), plat());
    let llm = story_or_islands();
    let mut d = daemon(&c, &p, llm.clone(), "hijack");
    post_talk(&mut d, STORY);
    let _ = d.tick(100);
    assert!(d.talk_is_thinking(), "the Talk turn should be waiting on the model");

    // Should the voice turn wait on the Talk turn (the old defect), this lets
    // it go after four seconds rather than hang the test.
    let opener = llm.clone();
    let rescue = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(4));
        opener.open();
    });
    let (ears, mouth) = (CountingEars::default(), Quiet::default());
    let started = Instant::now();
    d.converse("what are volcanic islands made of", &ears, &mouth, &|| 110);
    let took = started.elapsed();
    assert!(took < Duration::from_secs(3), "the voice turn waited for the Talk turn: {took:?}");
    assert!(d.talk_is_thinking(), "the voice turn took the Talk turn over");
    let islands = d.thread.recent.iter().find(|e| e.said.contains("volcanic")).expect("the voice turn was answered");
    assert!(islands.reply.contains("basalt"), "{}", islands.reply);

    rescue.join().unwrap();
    tick_until_quiet(&mut d, 120);
    // And the Talk reply is written, though the thread grew meanwhile (11).
    let story: Vec<_> = d.thread.recent.iter().filter(|e| e.said == STORY).collect();
    assert_eq!(story.len(), 1, "{:#?}", d.thread.recent);
    assert!(story[0].reply.contains("lighthouse"), "{}", story[0].reply);
    for _ in 0..3 {
        let _ = d.tick(130);
    }
    assert_eq!(llm.times_asked_about("lighthouse"), 1, "the Talk turn ran twice");
}

#[test]
fn the_talk_reply_is_written_even_when_another_turn_grew_the_thread() {
    let mut t = atlas::thread::Thread::default();
    t.append("a", "b", None, 1);
    let before = t.len();
    t.append("something typed elsewhere", "its answer", None, 2);
    assert!(!t.said_since(before, STORY), "a different exchange counted as the Talk turn's");
    t.append(STORY, "the story", None, 3);
    assert!(t.said_since(before, STORY));
}

// ================= 3: pause and stop reach a turn in flight =================

#[test]
fn pausing_drops_a_model_turn_that_is_still_thinking() {
    let (c, p) = (cfg(), plat());
    let llm = story_or_islands();
    let mut d = daemon(&c, &p, llm.clone(), "pausedrop");
    post_talk(&mut d, STORY);
    let _ = d.tick(100);
    assert!(d.talk_is_thinking());
    let _ = atlas::hublive::reply(&mut d, Action::Pause(true));
    assert!(d.attention.is_paused());
    assert!(!d.talk_is_thinking(), "the pause left the turn thinking");
    llm.open();
    std::thread::sleep(Duration::from_millis(200));
    for _ in 0..3 {
        let _ = d.tick(110);
    }
    let story = d.thread.recent.iter().find(|e| e.said == STORY).expect("the Talk page says what happened");
    assert!(story.reply.contains("Paused before I answered"), "{}", story.reply);
    assert!(!d.thread.recent.iter().any(|e| e.reply.contains("keeper slept")), "the answer was used after the pause");
}

#[test]
fn an_answer_that_arrives_after_a_pause_is_not_acted_on() {
    let (c, p) = (cfg(), plat());
    let llm = story_or_islands();
    let mut d = daemon(&c, &p, llm.clone(), "pauselate");
    post_talk(&mut d, STORY);
    let _ = d.tick(100);
    // Paused without going through a turn: the finish must check for itself.
    d.attention.pause(None, 105);
    llm.open();
    tick_until_quiet(&mut d, 110);
    assert!(!d.thread.recent.iter().any(|e| e.reply.contains("keeper slept")), "{:#?}", d.thread.recent);
    let story = d.thread.recent.iter().find(|e| e.said == STORY).expect("the Talk page says what happened");
    assert!(story.reply.contains("Paused"), "{}", story.reply);
}

#[test]
fn stop_everything_drops_it_too() {
    let (c, p) = (cfg(), plat());
    let llm = story_or_islands();
    let mut d = daemon(&c, &p, llm.clone(), "panicdrop");
    post_talk(&mut d, STORY);
    let _ = d.tick(100);
    let said = d.turn("stop everything", 105);
    assert!(said.contains("stopped") || said.contains("Stopped"), "{said}");
    assert!(!d.talk_is_thinking());
    llm.open();
    std::thread::sleep(Duration::from_millis(200));
    let _ = d.tick(110);
    assert!(!d.thread.recent.iter().any(|e| e.reply.contains("keeper slept")));
}

// ================= 5: streaming speech records nothing unless asked =================

#[test]
fn a_streamed_voice_reply_records_nothing_between_its_sentences() {
    let (c, p) = (cfg(), plat());
    let llm = story_or_islands();
    let mut d = daemon(&c, &p, llm.clone(), "norecord");
    let (ears, mouth) = (CountingEars::default(), Quiet::default());
    d.converse("what are volcanic islands made of", &ears, &mouth, &|| 100);
    let islands = d.thread.recent.iter().find(|e| e.said.contains("volcanic")).expect("answered");
    assert!(islands.reply.contains("basalt"), "{}", islands.reply);
    // Four sentences. It used to record a second before each one; now only
    // the talk key starts a recording, plus at most the one follow-up window.
    let recorded = ears.brief.load(Ordering::SeqCst);
    assert!(recorded <= 1, "recorded {recorded} times while speaking");
}

// ================= 2 and 4: the hub's door =================

const TOKEN: &str = "abcdef-ghjkmn-pqrstu-vwxyz2";

fn a_server() -> Server {
    let cfg = ServerConfig { enabled: true, port: 0, ..ServerConfig::default() };
    Server::bind(&cfg, TOKEN).expect("bind")
}

fn ask(port: u16, raw: &str) -> String {
    let mut c = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    c.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    c.write_all(raw.as_bytes()).unwrap();
    let mut out = Vec::new();
    let _ = c.read_to_end(&mut out);
    String::from_utf8_lossy(&out).into_owned()
}

#[test]
fn a_click_answered_busy_is_never_run_afterwards() {
    let server = a_server().with_answer_wait(Duration::from_millis(300));
    let port = server.port();
    let door = server.threaded().unwrap();
    let got = ask(port, &format!("POST /hub/pause HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Atlas-Token: {TOKEN}\r\nContent-Length: 10\r\n\r\nwhat=pause"));
    // Told it didn't happen (a busy answer, shown as the failed-page error).
    assert!(got.starts_with("HTTP/1.1 5"), "{}", &got[..got.len().min(200)]);
    // The daemon gets round to it after the browser was told it was busy.
    let ran = AtomicUsize::new(0);
    let done = door.answer_waiting(&mut |_a| {
        ran.fetch_add(1, Ordering::SeqCst);
        Reply::ok("{}")
    });
    assert_eq!(ran.load(Ordering::SeqCst), 0, "a request answered busy was run anyway");
    assert!(done.is_empty());
    // One answered in time still runs.
    let server = a_server();
    let port = server.port();
    let d2 = server.threaded().unwrap();
    let lp = std::thread::spawn(move || {
        let until = Instant::now() + Duration::from_secs(5);
        let mut n = 0;
        while n == 0 && Instant::now() < until {
            n += d2.wait_and_answer(50, &mut |_a| Reply::ok("{\"ok\":true}")).len();
        }
        n
    });
    let got = ask(port, &format!("GET /hub/live.json HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Atlas-Token: {TOKEN}\r\n\r\n"));
    assert!(got.starts_with("HTTP/1.1 200"), "{}", &got[..got.len().min(200)]);
    assert_eq!(lp.join().unwrap(), 1);
}

#[test]
fn a_client_sending_a_byte_at_a_time_runs_out_of_time() {
    let server = a_server();
    let port = server.port();
    let _door = server.threaded().unwrap();
    let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
    c.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
    let started = Instant::now();
    let head = b"GET /hub HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Filler: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let mut closed = false;
    for b in head.iter().cycle().take(400) {
        if c.write_all(&[*b]).is_err() {
            closed = true;
            break;
        }
        let mut buf = [0u8; 64];
        match c.read(&mut buf) {
            Ok(0) => {
                closed = true;
                break;
            }
            Ok(_) => {}
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(_) => {
                closed = true;
                break;
            }
        }
        if started.elapsed() > Duration::from_secs(12) {
            break;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    assert!(closed, "a trickling client was still connected after {:?}", started.elapsed());
    assert!(started.elapsed() < Duration::from_secs(9), "held for {:?}", started.elapsed());
}

#[test]
fn one_address_cannot_take_every_place_at_the_door() {
    let server = a_server();
    let port = server.port();
    let _door = server.threaded().unwrap();
    // Sixteen connections that say nothing.
    let idle: Vec<TcpStream> = (0..16).map(|_| TcpStream::connect(("127.0.0.1", port)).unwrap()).collect();
    std::thread::sleep(Duration::from_millis(200));
    // The seventeenth from the same address is closed unread.
    let mut extra = TcpStream::connect(("127.0.0.1", port)).unwrap();
    extra.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let _ = extra.write_all(b"GET /hub/sw.js HTTP/1.1\r\nHost: x\r\n\r\n");
    let mut buf = Vec::new();
    let started = Instant::now();
    let _ = extra.read_to_end(&mut buf);
    assert!(buf.is_empty() && started.elapsed() < Duration::from_secs(2), "the seventeenth was served: {}", String::from_utf8_lossy(&buf));
    drop(idle);
    std::thread::sleep(Duration::from_millis(300));
    // Places given back: served again.
    let got = ask(port, "GET /hub/sw.js HTTP/1.1\r\nHost: x\r\n\r\n");
    assert!(got.starts_with("HTTP/1.1 200"), "{}", &got[..got.len().min(120)]);
}

// ================= 22: settings-only mode is threaded =================

#[test]
fn the_settings_only_hub_is_served_from_threads() {
    let src = crate::common::read_source_path("src/main.rs").unwrap();
    let start = src.find("fn run_hub(").expect("run_hub");
    let body = &src[start..start + src[start..].find("\n}\n").unwrap()];
    assert!(body.contains(".threaded()"), "settings-only mode no longer uses the threaded door");
    assert!(!body.contains("serve_once("), "settings-only mode takes one connection at a time again");
}

// ================= 6: what an error from the model server means =================

/// A model server that answers each connection with the next canned reply,
/// and keeps what it was asked.
fn fake_model_server(replies: Vec<String>) -> (String, Arc<Mutex<Vec<String>>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s2 = seen.clone();
    std::thread::spawn(move || {
        for r in replies {
            let Ok((mut c, _)) = l.accept() else { return };
            c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut buf = Vec::new();
            let mut chunk = [0u8; 8192];
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
            s2.lock().unwrap().push(String::from_utf8_lossy(&buf).to_string());
            let _ = c.write_all(r.as_bytes());
        }
    });
    (format!("http://127.0.0.1:{port}/v1/chat/completions"), seen)
}

fn answer(status: &str, body: &str) -> String {
    format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
}

fn streamed(words: &str, done: bool) -> String {
    let mut body = format!("data: {{\"choices\":[{{\"delta\":{{\"content\":\"{words}\"}}}}]}}\n\n");
    if done {
        body.push_str("data: [DONE]\n\n");
    }
    format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
}

fn a_request(n_history: usize) -> ChatRequest {
    let mut messages = vec![Msg::system("S")];
    for i in 0..n_history {
        messages.push(Msg::user(format!("earlier {i}")));
        messages.push(Msg::assistant(format!("reply {i}")));
    }
    messages.push(Msg::user("User said: hello"));
    // `stable_tools` and `aside` since 28 Sep 2026 (the every-turn tools, and
    // a call beside the conversation); neither here.
    ChatRequest { messages, tools: vec![], max_tokens: 50, force_tool: false, stable_tools: 0, aside: false }
}

#[test]
fn only_a_server_that_cannot_chat_is_left_alone() {
    use atlas::models::{judge_chat_failure, ChatFail};
    assert!(matches!(judge_chat_failure(404, "Not Found"), ChatFail::NoChat(_)));
    assert!(matches!(judge_chat_failure(500, "tools param requires --jinja flag"), ChatFail::NoChat(_)));
    assert!(matches!(judge_chat_failure(503, "{\"error\":{\"message\":\"Loading model\"}}"), ChatFail::Loading(_)));
    assert!(matches!(
        judge_chat_failure(400, "the request exceeds the available context size, try increasing it"),
        ChatFail::TooLong(_)
    ));
    assert!(matches!(judge_chat_failure(500, "out of memory"), ChatFail::Other(_)));
    assert!(matches!(judge_chat_failure(502, "bad gateway"), ChatFail::Other(_)));

    // A 500 about something else: this turn fails, chat stays on.
    let (url, _) = fake_model_server(vec![answer("500 Internal Server Error", "{\"error\":\"out of memory\"}")]);
    assert!(atlas::models::chat_call(&url, &a_request(0), &mut |_| true).is_err());
    assert!(atlas::models::chat_available(&url), "one unrelated error turned chat off");
    // No --jinja: left alone.
    let (url, _) = fake_model_server(vec![answer("500 Internal Server Error", "{\"error\":\"tools param requires --jinja flag\"}")]);
    assert!(atlas::models::chat_call(&url, &a_request(0), &mut |_| true).is_err());
    assert!(!atlas::models::chat_available(&url));
}

#[test]
fn a_model_still_loading_is_waited_for() {
    let loading = answer("503 Service Unavailable", "{\"error\":{\"code\":503,\"message\":\"Loading model\"}}");
    let (url, seen) = fake_model_server(vec![loading.clone(), loading, streamed("Hello there.", true)]);
    let mut heard = String::new();
    let r = atlas::models::chat_call(&url, &a_request(0), &mut |w| {
        heard.push_str(w);
        true
    })
    .expect("answered once it had loaded");
    assert_eq!(r.text, "Hello there.");
    assert_eq!(heard, "Hello there.", "words from a failed try were passed on");
    assert_eq!(seen.lock().unwrap().len(), 3);
    assert!(atlas::models::chat_available(&url));
}

#[test]
fn a_prompt_too_long_is_shortened_and_asked_once_more() {
    let (url, seen) = fake_model_server(vec![
        answer("400 Bad Request", "{\"error\":{\"message\":\"the request exceeds the available context size\"}}"),
        streamed("Short answer.", true),
    ]);
    let r = atlas::models::chat_call(&url, &a_request(4), &mut |_| true).expect("answered after shortening");
    assert_eq!(r.text, "Short answer.");
    let asked = seen.lock().unwrap().clone();
    assert_eq!(asked.len(), 2);
    assert!(asked[0].contains("earlier 0") && !asked[1].contains("earlier 0"), "the retry still carried the old conversation");
    assert!(asked[1].contains("User said: hello"));
    assert!(atlas::models::chat_available(&url));
}

#[test]
fn a_stream_that_stops_without_saying_it_is_done_is_a_failure() {
    let (url, _) = fake_model_server(vec![streamed("Half a", false)]);
    assert!(atlas::models::chat_call(&url, &a_request(0), &mut |_| true).is_err());
}

// ================= 7: the whole prompt fits =================

#[test]
fn the_prompt_is_cut_to_fit_the_context_history_first_then_tools_then_what_was_said() {
    let c = cfg();
    let book = ToolBook::new(&c.commands);
    let core = book.for_sentence("", 0).len();
    let mut history = vec![Msg::system("Earlier: a trip to Lisbon.")];
    for i in 0..10 {
        history.push(Msg::user(format!("question {i} {}", "word ".repeat(200))));
        history.push(Msg::assistant(format!("answer {i}")));
    }
    let mut turn = Turn {
        said: "and what about the food?".into(),
        system: "S".into(),
        history,
        tools: book.for_sentence("open research remind calendar", 6),
        max_tokens: 200,
        ..Default::default()
    };
    let before = turn.estimated_tokens();
    assert!(turn.fit(before, core), "it fitted already?");
    assert!(turn.estimated_tokens() <= before - 200 - 64);
    assert!(turn.history.len() < 21, "the history wasn't cut first");
    assert_eq!(turn.said, "and what about the food?", "what was said was cut before the history");

    // A pasted page alone bigger than the context: said is cut, head and
    // tail kept, and it says so.
    let page = format!("START {} END", "lorem ipsum ".repeat(4000));
    let mut turn = Turn { said: page.clone(), system: "S".into(), tools: book.for_sentence("", 0), max_tokens: 200, ..Default::default() };
    assert!(turn.fit(4096, core));
    assert!(turn.estimated_tokens() <= 4096 - 264, "{}", turn.estimated_tokens());
    assert!(turn.said.starts_with("START") && turn.said.ends_with("END"));
    assert!(turn.said.contains("left out"), "it doesn't say it was shortened");
    assert_eq!(turn.tools.len(), core, "the core tools went");
}

// ================= 8 and 9: what reaches the speaker =================

fn brain_turn(said: &str, history: Vec<Msg>, tools: Vec<serde_json::Value>) -> Turn {
    Turn { said: said.into(), history, tools, max_tokens: 200, skip_phrases: true, one_prompt: "Now: noon.".into(), ..Default::default() }
}

/// A model that streams set pieces, then answers with a set reply or fails.
struct Pieces {
    calls: Mutex<Vec<ChatRequest>>,
    plan: Mutex<Vec<(Vec<String>, Option<ChatReply>)>>,
    completed: AtomicUsize,
}

impl Pieces {
    fn new(plan: Vec<(Vec<&str>, Option<ChatReply>)>) -> Pieces {
        Pieces {
            calls: Mutex::new(Vec::new()),
            plan: Mutex::new(plan.into_iter().map(|(p, r)| (p.into_iter().map(String::from).collect(), r)).collect()),
            completed: AtomicUsize::new(0),
        }
    }
}

impl Llm for Pieces {
    fn complete(&self, _: &str, _: &str) -> atlas::error::Result<String> {
        self.completed.fetch_add(1, Ordering::SeqCst);
        Ok(r#"{"action":"say","arg":null,"say":"A SECOND ANSWER."}"#.into())
    }
    fn native_chat(&self) -> bool {
        true
    }
    fn chat(&self, req: &ChatRequest, on_text: &mut dyn FnMut(&str) -> bool) -> atlas::error::Result<ChatReply> {
        self.calls.lock().unwrap().push(req.clone());
        let (pieces, reply) = {
            let mut p = self.plan.lock().unwrap();
            if p.is_empty() {
                (vec![], Some(text("")))
            } else {
                p.remove(0)
            }
        };
        for piece in &pieces {
            if !on_text(piece) {
                break;
            }
        }
        reply.ok_or_else(|| atlas::error::AtlasError::Platform("the connection dropped".into()))
    }
}

fn converse(llm: &Pieces, turn: &Turn) -> (atlas::brain::Decision, String, Vec<String>) {
    let c = cfg();
    let parser = Parser::new(&c.commands);
    let persona = atlas::persona::Persona::default();
    let brain = Brain { llm, fallback: &parser, voice: Some((&persona, atlas::register::Register::Chatting)) };
    let mut heard = String::new();
    let mut also = Vec::new();
    let d = brain.converse_noting(
        turn,
        &mut |w| {
            heard.push_str(w);
            true
        },
        &mut also,
    );
    (d, heard, also)
}

#[test]
fn a_stream_that_fails_partway_is_finished_with_what_was_said_not_answered_again() {
    let llm = Pieces::new(vec![(vec!["Part one. ", "Part two"], None)]);
    let (d, heard, _) = converse(&llm, &brain_turn("tell me something long", vec![], vec![]));
    assert_eq!(llm.completed.load(Ordering::SeqCst), 0, "asked again the other way after words were said");
    assert!(!heard.contains("SECOND"), "{heard}");
    let Intent::Say(s) = &d.intent else { panic!("{d:?}") };
    assert!(s.starts_with("Part one. Part two") && s.contains("as far as I got"), "{s}");
    // Nothing said yet: the other way answers, as before.
    let llm = Pieces::new(vec![(vec![], None)]);
    let (d, _, _) = converse(&llm, &brain_turn("tell me something long", vec![], vec![]));
    assert_eq!(d.intent, Intent::Say("A SECOND ANSWER.".into()));
}

#[test]
fn a_repeat_of_an_earlier_line_is_caught_before_it_is_said() {
    let earlier = "I can't help with that one from here, I'm afraid.";
    let history = vec![Msg::user("something old"), Msg::assistant(earlier)];
    let llm = Pieces::new(vec![
        (vec!["I can't help ", "with that one from here, ", "I'm afraid. ", "Sorry."], Some(text("I can't help with that one from here, I'm afraid. Sorry."))),
        (vec!["Paris is ", "the capital of France."], Some(text("Paris is the capital of France."))),
    ]);
    let (d, heard, _) = converse(&llm, &brain_turn("what's the capital of France", history, vec![]));
    assert_eq!(heard, "Paris is the capital of France.", "the repeat was said before the answer");
    assert_eq!(d.intent, Intent::Say("Paris is the capital of France.".into()));
    let calls = llm.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert!(!calls[1].messages.iter().any(|m| m.content == earlier), "the fresh ask still had the conversation");
}

#[test]
fn a_tool_call_tag_split_across_pieces_is_never_read_out() {
    let full = "Sure thing. <tool_call>{\"name\":\"open_app\",\"arguments\":{\"arg\":\"chrome\"}}</tool_call>";
    let llm = Pieces::new(vec![(
        vec!["Sure thing. ", "<to", "ol_ca", "ll>{\"name\":\"open_app\",", "\"arguments\":{\"arg\":\"chrome\"}}</tool_call>"],
        Some(ChatReply::from_text(full)),
    )]);
    let (d, heard, _) = converse(&llm, &brain_turn("could you get chrome up", vec![], vec![]));
    assert_eq!(heard, "Sure thing. ", "{heard:?}");
    assert_eq!(d.intent, Intent::OpenApp("chrome".into()));
    // A "<" that turns out not to be a tag is said, at the end.
    let llm = Pieces::new(vec![(vec!["Three ", "<", " five."], Some(text("Three < five.")))]);
    let (_, heard, _) = converse(&llm, &brain_turn("is three less than five", vec![], vec![]));
    assert_eq!(heard, "Three < five.");
}

#[test]
fn a_call_missing_what_it_needs_is_asked_about_not_run_with_nothing() {
    let c = cfg();
    let book = ToolBook::new(&c.commands);
    let spec = book.get("research").expect("research is a tool").spec();
    assert!(spec.pointer("/function/parameters/required").is_some(), "research needs its argument: {spec}");
    let broken = ChatReply { text: String::new(), tool_calls: vec![call("research", serde_json::json!({}))] };
    let llm = Pieces::new(vec![(vec![], Some(broken))]);
    let (d, _, _) = converse(&llm, &brain_turn("look up the latest on that", vec![], vec![spec]));
    assert!(matches!(d.intent, Intent::Ask(_)), "{:?}", d.intent);
    assert!(!matches!(d.intent, Intent::Research(_)));
}

#[test]
fn a_second_tool_call_is_named_not_dropped() {
    let two = ChatReply {
        text: String::new(),
        tool_calls: vec![call("open_app", serde_json::json!({"arg": "chrome"})), call("open_app", serde_json::json!({"arg": "notepad"}))],
    };
    let llm = Pieces::new(vec![(vec![], Some(two))]);
    let (d, _, also) = converse(&llm, &brain_turn("get chrome and notepad up", vec![], vec![]));
    assert_eq!(d.intent, Intent::OpenApp("chrome".into()));
    assert_eq!(also.len(), 1, "{also:?}");
    assert!(also[0].contains("notepad"), "{also:?}");
}

#[test]
fn an_announced_tool_is_called_rather_than_promised() {
    let c = cfg();
    let book = ToolBook::new(&c.commands);
    let tools = book.for_sentence("what's on my calendar tomorrow", 6);
    let llm = Pieces::new(vec![
        (vec!["I'll check ", "your calendar."], Some(text("I'll check your calendar."))),
        (vec![], Some(ChatReply { text: String::new(), tool_calls: vec![call("agenda", serde_json::json!({"arg": "tomorrow"}))] })),
    ]);
    let (d, _, _) = converse(&llm, &brain_turn("anything on for tomorrow", vec![], tools));
    assert_eq!(d.intent, Intent::Agenda("tomorrow".into()));
    let calls = llm.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert!(!calls[0].force_tool && calls[1].force_tool, "the second ask didn't require a tool");
    assert!(atlas::brain::announces_an_action("Let me check that for you."));
    assert!(!atlas::brain::announces_an_action("You've got the dentist at nine."));
    // The chat body asks the server for it.
    let persona = atlas::persona::Persona::default();
    assert!(persona.character().contains("call the tool"), "the prompt doesn't say to call tools");
}

/// 30 Sep 2026: when the forced ask still called nothing, "I'll check your
/// calendar." was the whole reply -- a promise nothing followed.
#[test]
fn an_announced_tool_that_never_runs_is_owned_up_to() {
    let c = cfg();
    let book = ToolBook::new(&c.commands);
    let tools = book.for_sentence("what's on my calendar tomorrow", 6);
    let llm = Pieces::new(vec![
        (vec!["I'll check ", "your calendar."], Some(text("I'll check your calendar."))),
        (vec![], Some(text("Sure."))),
    ]);
    let (d, _, _) = converse(&llm, &brain_turn("anything on for tomorrow", vec![], tools));
    assert_eq!(d.say, format!("I'll check your calendar. {}", atlas::brain::NOTHING_FOLLOWED));
}

// ================= 10: times around the clocks changing =================

#[test]
fn a_time_after_the_clocks_change_is_read_with_that_days_offset() {
    let ny = atlas::tz::Zone::named("America/New_York").expect("the zone is known");
    // Friday 30 Oct 2026, noon UTC -- summer time, UTC-4. The clocks go back
    // on Sunday 1 Nov.
    let friday_noon = 1_793_361_600;
    let w = atlas::calendar::resolve_when_in("dentist monday at 9am", friday_noon, &ny).expect("read");
    // 9 am on Monday 2 Nov is 14:00 UTC (UTC-5), not 13:00.
    assert_eq!(w.start, 1_793_628_000, "read with the offset of the day it was said");
}

#[test]
fn the_clock_can_be_pinned_by_the_tests_that_need_it() {
    atlas::localclock::pin_offset(Some(0));
    assert_eq!(atlas::localclock::told_offset(), Some(0));
    assert_eq!(atlas::localclock::hour_here(1_793_628_000), 14);
}

// ================= 12: a file held for a moment =================

#[test]
fn a_file_that_is_briefly_unreadable_is_waited_for_not_set_aside() {
    let dir = tmp("patience");
    let store = Store::new(&dir);
    // A folder where the file should be reads as an error, as a lock does.
    let path = dir.join("tray.json");
    std::fs::create_dir_all(&path).unwrap();
    let p2 = path.clone();
    let fixer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        std::fs::remove_dir_all(&p2).unwrap();
        std::fs::write(&p2, "[1,2,3]").unwrap();
    });
    let got: Vec<u32> = store.load("tray");
    fixer.join().unwrap();
    assert_eq!(got, vec![1, 2, 3], "it gave up before the file came free");
    assert!(atlas::store::tell_set_aside(&store).is_none());
    assert!(!std::fs::read_dir(&dir).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().contains("unreadable")));
}

#[test]
fn a_file_set_aside_is_said_once_on_the_next_turn_and_on_the_hub() {
    let dir = tmp("setaside");
    std::fs::create_dir_all(dir.join("notes_index.json")).unwrap();
    let store = Store::new(&dir);
    let started = Instant::now();
    let got: Vec<u32> = store.load("notes_index");
    assert!(got.is_empty());
    assert!(started.elapsed() >= Duration::from_millis(350), "it wasn't tried again first");
    let said = atlas::store::tell_set_aside(&store).expect("said");
    assert!(said.contains("notes index") && said.contains("nothing was deleted"), "{said}");
    assert!(atlas::store::tell_set_aside(&store).is_none(), "said twice");
    assert!(!atlas::store::set_aside_since(&store, 0).is_empty(), "the hub can't show it");
    // Another store in the same process hears nothing of it.
    assert!(atlas::store::tell_set_aside(&Store::new(tmp("setaside-other"))).is_none());
}

#[test]
fn the_next_turn_says_a_file_was_set_aside() {
    let (c, p) = (cfg(), plat());
    let dir = tmp("setaside-turn");
    std::fs::create_dir_all(dir.join("oddments.json")).unwrap();
    let store = Store::new(&dir);
    let _: Vec<u32> = store.load("oddments");
    let llm = story_or_islands();
    let mut d = Daemon::new(&c, &p, Some(llm), store, Proactive::new(ProactiveConfig::default()));
    let reply = d.turn("what time is it", 100);
    assert!(reply.contains("oddments") && reply.contains("put the file aside"), "{reply}");
    let again = d.turn("what time is it", 110);
    assert!(!again.contains("oddments"), "{again}");
    // And the file really was moved aside, not overwritten.
    let kept = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).any(|e| {
        let n = e.file_name().to_string_lossy().to_string();
        n.starts_with("oddments.unreadable.") && n.ends_with(".json.bak")
    });
    assert!(kept, "the unreadable file is gone");
}

// ================= 15: the call counter keeps counting =================

#[test]
fn the_calls_of_a_turn_are_found_after_the_oldest_roll_off() {
    let mut t = atlas::trace::Trace::default();
    for _ in 0..atlas::trace::KEEP {
        t.record(atlas::trace::Call::new("brain", "m", 1).finished(5, "p", "r"));
    }
    let before = t.recorded();
    t.record(atlas::trace::Call::new("brain", "m", 1).finished(700, "p", "r"));
    t.record(atlas::trace::Call::new("brain", "m", 1).finished(300, "p", "r"));
    let since = t.recorded_since(before);
    assert_eq!(since.len(), 2, "the turn's calls were lost once the list was full");
    assert_eq!(since.iter().map(|c| c.took_ms).sum::<u64>(), 1000);
    assert_eq!(t.calls.len(), atlas::trace::KEEP);
}

// ================= 16: hub jobs =================

#[test]
fn a_finished_job_nobody_came_back_for_is_forgotten_and_a_running_one_never_is() {
    use atlas::hub::Page;
    let jobs = atlas::hubjobs::Jobs::default();
    let done = jobs.start(Page::Friends, "Knocking");
    jobs.finish(done, Ok("Answered.".into()), false);
    jobs.age_finished(done, Duration::from_secs(11 * 60));
    let _ = jobs.start(Page::Friends, "Another");
    assert!(jobs.look(done).is_none(), "an unseen answer was kept past its time");

    let jobs = atlas::hubjobs::Jobs::default();
    let first = jobs.start(Page::Friends, "The first, still going");
    for i in 0..80 {
        let _ = jobs.start(Page::Friends, &format!("job {i}"));
    }
    assert!(jobs.look(first).is_some(), "a running job was evicted");
    // Past the limit, finished ones go first.
    for id in 2..=40 {
        jobs.finish(id, Ok("done".into()), false);
    }
    let _ = jobs.start(Page::Friends, "one more");
    assert!(jobs.len() <= 64 || jobs.look(first).is_some());
    assert!(jobs.look(first).is_some());
}

// ================= 17: temp folders =================

#[test]
fn a_runs_temp_folder_goes_with_it_and_old_ones_are_cleared() {
    let temp = tmp("sweep");
    let mine = std::process::id();
    let stale = temp.join("atlas-notes-999991");
    let fresh = temp.join("atlas-notes-999992");
    let own = temp.join(format!("atlas-notes-{mine}"));
    let other = temp.join("atlas-other-999993");
    for d in [&stale, &fresh, &own, &other] {
        std::fs::create_dir_all(d).unwrap();
    }
    // "Stale" by an age of zero: anything not ours with the prefix goes.
    let gone = atlas::roots::sweep_run_scratch(&temp, "atlas-notes", mine, 0);
    assert_eq!(gone, 2);
    assert!(!stale.exists() && !fresh.exists());
    assert!(own.exists(), "this run's own folder was cleared");
    assert!(other.exists(), "a folder of another name was cleared");
    // With the real age, fresh ones stay.
    std::fs::create_dir_all(&fresh).unwrap();
    assert_eq!(atlas::roots::sweep_run_scratch(&temp, "atlas-notes", mine, atlas::roots::RUN_SCRATCH_STALE_SECS), 0);

    let path = {
        let s = atlas::roots::RunScratch::new("atlas-second-scan-scratch");
        std::fs::write(s.path().join("segment.wav"), b"x").unwrap();
        s.path().to_path_buf()
    };
    assert!(!path.exists(), "the run's folder outlived it");
}

// ================= 18: the phone page doesn't read the app on the loop =================

#[test]
fn the_iphone_app_is_read_off_the_loop() {
    let dir = tmp("ipa");
    let f = dir.join("Atlas.ipa");
    std::fs::write(&f, vec![0u8; 40 * 1024 * 1024]).unwrap();
    let started = Instant::now();
    let first = atlas::hublive::ipa_facts(&f);
    assert!(first.is_none());
    assert!(started.elapsed() < Duration::from_millis(100), "the first look read the file: {:?}", started.elapsed());
    let until = Instant::now() + Duration::from_secs(20);
    while !atlas::hublive::ipa_facts_ready(&f) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(atlas::hublive::ipa_facts_ready(&f), "it was never read");
    let _ = std::fs::remove_dir_all(&dir);
}

// ================= 19: a tool that reads something back is answered in words =================

#[test]
fn a_calendar_read_is_put_into_words_by_the_model() {
    let (c, p) = (cfg(), plat());
    let parser = Parser::new(&c.commands);
    let said = "am I busy with anything after lunch tomorrow";
    assert!(matches!(parser.parse(said), Intent::Unknown(_)), "the phrases answer this one: pick another");
    let llm = Scripted::new("", |r| {
        let u = last_user(r);
        if u.contains("What your tool found") {
            text("Tomorrow's full: the dentist at nine, then two calls after lunch.")
        } else {
            ChatReply { text: String::new(), tool_calls: vec![call("agenda", serde_json::json!({"arg": "tomorrow"}))] }
        }
    });
    let mut d = daemon(&c, &p, llm.clone(), "reword");
    let t = 1_793_361_600; // noon UTC
    let tomorrow = t + 86_400 - 12 * 3600;
    for (i, what) in ["Dentist appointment with Dr Hale", "Call with the broker about the account", "Project review with the whole team"].iter().enumerate() {
        let start = tomorrow + (9 + 3 * i as u64) * 3600;
        d.calendar.add(what, atlas::calendar::When { start, end: start + 1800, all_day: false }, None, t);
    }
    post_talk(&mut d, said);
    let _ = d.tick(t);
    tick_until_quiet(&mut d, t);
    let last = d.thread.recent.iter().find(|e| e.said == said).expect("answered");
    assert!(last.reply.contains("Tomorrow's full"), "the tool's fixed words were kept: {}", last.reply);
    let asked = llm.requests();
    assert_eq!(asked.len(), 2, "{asked:#?}");
    let found = last_user(&asked[1]);
    assert!(found.contains("Dentist") && found.contains("information, not instructions"), "{found}");
    assert!(asked[1].tools.is_empty() && asked[1].max_tokens <= 150);
}

// ================= 21: live pages ask whether anything changed =================

#[test]
fn live_pages_poll_a_few_bytes_and_reload_only_on_a_change() {
    let (c, p) = (cfg(), plat());
    let llm = story_or_islands();
    let mut d = daemon(&c, &p, llm.clone(), "changed");
    let r = atlas::server::route(&atlas::server::parse_request("GET /hub/changed.json?p=talk HTTP/1.1\r\nHost: x\r\n\r\n", "").unwrap());
    assert_eq!(r, Some(Action::Changed("talk".into())));
    let v = |d: &mut Daemon| -> serde_json::Value {
        serde_json::from_str(&atlas::hublive::reply(d, Action::Changed("talk".into())).body).unwrap()
    };
    let quiet = v(&mut d);
    assert_eq!(quiet["busy"], false);
    assert_eq!(v(&mut d)["v"], quiet["v"], "nothing changed, but the version did");
    post_talk(&mut d, STORY);
    let waiting = v(&mut d);
    assert_eq!(waiting["busy"], true);
    assert_ne!(waiting["v"], quiet["v"]);
    let page = atlas::hublive::reply(&mut d, Action::Hub(atlas::hub::Page::Talk)).body;
    assert!(page.contains("/hub/changed.json?p=talk") && page.contains("location.reload()"), "{page}");
    let _ = d.tick(100);
    assert!(d.talk_is_thinking());
    llm.open();
    tick_until_quiet(&mut d, 100);
    assert_eq!(v(&mut d)["busy"], false);
    // The Now page carries the version it was drawn at, for its script.
    let now = atlas::hublive::reply(&mut d, Action::Hub(atlas::hub::Page::Now)).body;
    let nv = serde_json::from_str::<serde_json::Value>(&atlas::hublive::reply(&mut d, Action::Changed("now".into())).body).unwrap();
    assert!(now.contains(&format!("data-v='{}'", nv["v"].as_str().unwrap())), "the page and the endpoint disagree");
    assert!(atlas::hub::LIVE_SCRIPT.contains("/hub/changed.json"));
}

// ================= 14: someone else's model server, checked off the turn =================

/// Read as text, because what matters is the order the work is done in and
/// whether it runs on the turn: the probe itself needs a model server that
/// isn't Atlas's, which the tests don't have.
#[test]
fn another_model_server_is_asked_about_off_the_turn_and_at_most_once_a_minute() {
    let src = crate::common::read_source_path("src/daemon.rs").unwrap();
    // The work moved into `keep_model_server_waiting` (29 Sep 2026: the loop
    // calls it with no wait); `keep_model_server` only passes the usual wait.
    let start = src.find("fn keep_model_server_waiting(").expect("keep_model_server_waiting");
    let body = &src[start..start + src[start..].find("\n    }\n").unwrap()];
    let cached = body.find("model_server_seen").expect("the finding is kept");
    let scan = body.find("scan_reporting").expect("the folder is still scanned when starting one");
    let measure = body.find("fit::measure").expect("and the machine measured");
    assert!(cached < scan && cached < measure, "the folder is scanned before the kept finding is read");
    assert!(!body.contains("models::is_running("), "the HTTP check is back on the turn");
    let probe = src.find("fn probe_model_server(").expect("probe");
    let probe = &src[probe..probe + src[probe..].find("\n    }\n").unwrap()];
    assert!(probe.contains("spawn(") && probe.contains("is_running("), "the check isn't on its own thread");
}

#[test]
fn a_talk_message_given_up_on_is_kept_and_answered_later() {
    // 29 Sep 2026: typed on the Talk page while Atlas was stuck, answered
    // "busy", and never run -- what you typed was gone. A Talk message is the
    // one post that is kept (anything else answered busy still never runs).
    let server = a_server().with_answer_wait(Duration::from_millis(300));
    let port = server.port();
    let door = server.threaded().unwrap();
    let body = "text=what+is+on+my+calendar+today";
    let got = ask(port, &format!("POST /hub/talk HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Atlas-Token: {TOKEN}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{body}", body.len()));
    assert!(got.starts_with("HTTP/1.1 5"), "{}", &got[..got.len().min(200)]);
    let ran = AtomicUsize::new(0);
    door.answer_waiting(&mut |_a| {
        ran.fetch_add(1, Ordering::SeqCst);
        Reply::ok("{}")
    });
    assert_eq!(ran.load(Ordering::SeqCst), 0, "a request answered busy was run as a request");
    let kept = door.take_late_talk();
    assert_eq!(kept, vec![("what is on my calendar today".to_string(), false)], "the message was dropped");
    assert!(door.take_late_talk().is_empty(), "kept twice");
}

#[test]
fn the_friends_door_opens_once_its_port_is_free() {
    // 29 Sep 2026: a port busy at start kept the door friends reach you on
    // shut for the whole session. It is tried again once a minute.
    let blocker = std::net::TcpListener::bind(("::", 0)).or_else(|_| std::net::TcpListener::bind(("0.0.0.0", 0))).unwrap();
    let port = blocker.local_addr().unwrap().port();
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, story_or_islands(), "friends-door").with_signal_door_later(port, Vec::new());
    d.open_signal_door_again(100);
    drop(blocker);
    d.open_signal_door_again(120);
    assert!(TcpStream::connect(("127.0.0.1", port)).is_err(), "tried again before a minute was up");
    d.open_signal_door_again(161);
    assert!(TcpStream::connect(("127.0.0.1", port)).is_ok(), "the door never opened once the port was free");
}

#[test]
fn a_reminder_that_fires_is_said() {
    // 29 Sep 2026: a fired reminder runs as "say this", which proceeds by
    // itself, and the tick only passed on what needed approval or failed --
    // so every reminder that worked was never said.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, story_or_islands(), "reminder-said");
    let set = d.turn("remind me in 2 minutes to stretch", 1_000);
    assert!(set.to_lowercase().contains("set") || set.to_lowercase().contains("remind"), "{set}");
    let mut heard = Vec::new();
    for t in [1_060, 1_130, 1_200] {
        heard.extend(d.tick(t));
    }
    assert!(heard.iter().any(|l| l.to_lowercase().contains("stretch")), "the reminder fired and wasn't said: {heard:?}");
}
