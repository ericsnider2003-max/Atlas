//! **Talking with Atlas, not at a menu.**
//!
//! Eric, 27 Sep 2026: *"I need to be able to freely speak with Atlas not just
//! scripted lines but full conversations so we may need to increase what
//! Atlas knows and its capabilities in response to make it more dynamic,
//! free flowing and smart."*
//!
//! What stood in the way, and what these pin:
//!
//! - The conversation reached the model pasted into one user message; now it
//!   goes as real turns (`Thread::messages`, `brain::Turn`).
//! - Greedy phrases answered conversation: "how's it going" asked for an
//!   Atlas called "it going", "wait, what do you mean" paused, "what should I
//!   eat" became a decision to work through.
//! - The model could choose 13 actions; now every command in
//!   `commands.yaml` is a tool, and a model-chosen consequential one asks.
//! - The prompt's first lines changed every turn, so the model server read
//!   the whole thing again each time.
//! - Talk page replies vanished; parked questions never expired; a new
//!   sentence after "Go ahead?" was taken as a no and lost.

use atlas::brain::{ChatReply, ChatRequest, Llm, Msg, Role, ToolCall};
use atlas::daemon::Daemon;
use atlas::intent::{Intent, Parser, ToolBook};
use atlas::platform::mock::MockPlatform;
use atlas::platform::{ActiveWindow, Monitor};
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-talking-freely-{tag}-{}", std::process::id()));
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

/// A model with a chat endpoint that records every request and answers with
/// whatever the test set.
struct ChatSpy {
    asked: Mutex<Vec<ChatRequest>>,
    reply: Mutex<ChatReply>,
    /// How long each answer takes.
    slow_ms: u64,
}

impl ChatSpy {
    fn saying(text: &str) -> Arc<ChatSpy> {
        Arc::new(ChatSpy {
            asked: Mutex::new(Vec::new()),
            reply: Mutex::new(ChatReply { text: text.into(), tool_calls: vec![] }),
            slow_ms: 0,
        })
    }
    fn calling(name: &str, arg: &str) -> Arc<ChatSpy> {
        Arc::new(ChatSpy {
            asked: Mutex::new(Vec::new()),
            reply: Mutex::new(ChatReply {
                text: String::new(),
                tool_calls: vec![ToolCall { name: name.into(), arguments: serde_json::json!({ "arg": arg }) }],
            }),
            slow_ms: 0,
        })
    }
    fn calls(&self) -> usize {
        self.asked.lock().unwrap().len()
    }
    fn last(&self) -> ChatRequest {
        self.asked.lock().unwrap().last().cloned().expect("the model was never asked")
    }
    fn last_user(&self) -> String {
        self.last().messages.last().map(|m| m.content.clone()).unwrap_or_default()
    }
}

impl Llm for ChatSpy {
    fn complete(&self, _: &str, _: &str) -> atlas::error::Result<String> {
        Ok(r#"{"action":"say","arg":null,"say":"(the one-prompt path)"}"#.into())
    }
    fn native_chat(&self) -> bool {
        true
    }
    fn chat(&self, req: &ChatRequest, on_text: &mut dyn FnMut(&str) -> bool) -> atlas::error::Result<ChatReply> {
        self.asked.lock().unwrap().push(req.clone());
        let r = self.reply.lock().unwrap().clone();
        for word in r.text.split_inclusive(' ') {
            if !on_text(word) {
                break;
            }
        }
        // Written, and still "thinking" for a while after.
        if self.slow_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(self.slow_ms));
        }
        Ok(r)
    }
}

fn daemon<'a>(c: &'a atlas::config::Config, p: &'a MockPlatform, llm: Arc<ChatSpy>, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, Some(llm), Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

// ================= the conversation reaches the model as a conversation =================

#[test]
fn a_real_multi_turn_exchange_reaches_the_model_as_separate_messages() {
    let (c, p) = (cfg(), plat());
    let spy = ChatSpy::saying("Mostly basalt, and it cools fast.");
    let mut d = daemon(&c, &p, spy.clone(), "turns");

    let first = d.turn("what are volcanic islands made of", 100);
    assert!(first.contains("basalt"), "the model's answer is the reply: {first}");
    *spy.reply.lock().unwrap() = ChatReply { text: "Because the ocean cools the lava quickly.".into(), tool_calls: vec![] };
    let _ = d.turn("why does it cool so fast", 130);

    let m = spy.last().messages;
    let roles: Vec<Role> = m.iter().map(|x| x.role).collect();
    assert_eq!(roles, vec![Role::System, Role::User, Role::Assistant, Role::User], "{m:#?}");
    assert_eq!(m[1].content, "what are volcanic islands made of");
    assert!(m[2].content.contains("basalt"), "Atlas's own last line is its own turn: {:?}", m[2]);
    assert!(m[3].content.ends_with("User said: why does it cool so fast"), "{:?}", m[3]);
    // Not pasted into the user message any more.
    assert!(!m[3].content.contains("Conversation so far"), "{:?}", m[3]);
}

#[test]
fn the_folded_summary_is_one_earlier_line_in_the_system_message() {
    let mut t = atlas::thread::Thread::default();
    t.summary = "Planning a trip to Lisbon in May.".into();
    t.append("hotels near the river?", "Alfama or Cais do Sodré.", None, 1);
    let m = t.messages(6, 1200);
    assert_eq!(m[0].role, Role::System);
    assert!(m[0].content.starts_with("Earlier: Planning a trip"));
    let turn = atlas::brain::Turn { system: "S".into(), history: m, said: "and food?".into(), ..Default::default() };
    let msgs = turn.messages();
    assert_eq!(msgs.iter().filter(|x| x.role == Role::System).count(), 1, "one system message: {msgs:#?}");
    assert!(msgs[0].content.contains("Earlier: Planning a trip"));
}

#[test]
fn the_history_keeps_to_its_budget_and_always_keeps_the_last_exchange() {
    let mut t = atlas::thread::Thread::default();
    for i in 0..20 {
        t.append(&format!("question {i} {}", "x".repeat(400)), &format!("answer {i}"), None, i);
    }
    let m = t.messages(6, 300);
    assert!(!m.is_empty());
    assert!(m.last().unwrap().content.starts_with("answer 19"));
    assert!(m.len() <= 12);
    let tiny = t.messages(6, 1);
    assert_eq!(tiny.len(), 2, "the newest exchange is kept whole even over budget");
}

// ================= conversation isn't hijacked by phrases =================

#[test]
fn conversation_reaches_the_model_rather_than_a_phrase() {
    for said in ["how's it going", "wait, what do you mean", "what should I eat tonight", "how are you doing", "what is that supposed to mean", "continue the story"] {
        let (c, p) = (cfg(), plat());
        let spy = ChatSpy::saying("Fair question. Here's what I think.");
        let mut d = daemon(&c, &p, spy.clone(), "convo");
        let reply = d.turn(said, 100);
        assert_eq!(spy.calls(), 1, "{said:?} never reached the model; got {reply:?}");
        assert!(spy.last_user().ends_with(&format!("User said: {said}")), "{said:?}: {}", spy.last_user());
        assert!(!d.attention.is_paused(), "{said:?} paused Atlas");
    }
}

#[test]
fn the_bare_commands_still_work_without_the_model() {
    let (c, p) = (cfg(), plat());
    let spy = ChatSpy::saying("should not be asked");
    let mut d = daemon(&c, &p, spy.clone(), "bare");
    let paused = d.turn("pause", 100);
    assert!(d.attention.is_paused(), "pause no longer pauses: {paused}");
    let _ = d.turn("resume", 110);
    assert!(!d.attention.is_paused());
    let opened = d.turn("open chrome", 120);
    assert!(opened.to_lowercase().contains("chrome"), "{opened}");
    assert_eq!(spy.calls(), 0, "a bare command went to the model");

    let mut parser = Parser::new(&c.commands);
    // As the daemon tells it before every turn: no other Atlases named.
    parser.know_names(atlas::intent::KnownNames { apps: vec!["chrome".into()], ..Default::default() });
    assert_eq!(parser.parse("wait"), Intent::Pause);
    assert_eq!(parser.parse("wait please"), Intent::Pause);
    assert_eq!(parser.parse("continue"), Intent::Resume);
    assert!(matches!(parser.parse("wait, what do you mean"), Intent::Unknown(_)));
    assert!(matches!(parser.parse("how's it going"), Intent::Unknown(_)));
    assert!(matches!(parser.parse("what is this"), Intent::Unknown(_)));
    assert!(matches!(parser.parse("can you tell me a joke"), Intent::Unknown(_)));
}

#[test]
fn names_only_phrases_take_only_names_atlas_knows() {
    let c = cfg();
    let mut p = Parser::new(&c.commands);
    p.know_names(atlas::intent::KnownNames {
        apps: vec!["chrome".into(), "notepad".into()],
        modes: vec!["focus".into()],
        peers: vec!["homelab".into()],
    });
    assert_eq!(p.parse("go to chrome"), Intent::FocusApp("chrome".into()));
    assert!(matches!(p.parse("go to sleep"), Intent::Unknown(_)));
    assert_eq!(p.parse("close notepad"), Intent::CloseApp("notepad".into()));
    assert!(matches!(p.parse("quit smoking"), Intent::Unknown(_)));
    assert_eq!(p.parse("go into focus"), Intent::SetMode("focus".into()));
    assert!(matches!(p.parse("go into detail"), Intent::Unknown(_)));
    assert_eq!(p.parse("how is homelab"), Intent::BriefOn("homelab".into()));
    assert!(matches!(p.parse("how is your day"), Intent::Unknown(_)));
    // "open" still takes any name: an app you haven't set up is asked about.
    assert_eq!(p.parse("open frobnicator"), Intent::OpenApp("frobnicator".into()));
}

#[test]
fn a_note_that_shares_a_word_is_a_hint_not_the_answer() {
    let (c, p) = (cfg(), plat());
    let spy = ChatSpy::saying("Something warm -- a curry, maybe.");
    let mut d = daemon(&c, &p, spy.clone(), "hint");
    // A stated fact, which used to answer anything that shared a word.
    let _ = d.turn("actually my favourite food is ramen", 90);
    let reply = d.turn("what should I eat tonight", 100);
    assert!(reply.contains("curry"), "the model answered: {reply}");
    let system = spy.last().messages[0].content.clone();
    let user = spy.last_user();
    assert!(
        system.contains("ramen") || user.contains("ramen"),
        "what you told Atlas reaches the model as something it knows\nsystem: {system}\nuser: {user}"
    );
}

#[test]
fn a_correction_word_only_counts_at_the_start() {
    let (c, p) = (cfg(), plat());
    let spy = ChatSpy::saying("Mostly pork and beef trimmings.");
    let mut d = daemon(&c, &p, spy.clone(), "actually");
    let reply = d.turn("what is actually in a hot dog", 100);
    assert!(reply.contains("pork"), "a question with 'actually' in it was filed as a fact: {reply}");
    let learned = d.turn("actually my car is a Honda", 110);
    assert!(learned.starts_with("Got it"), "a correction that opens with it is still learned: {learned}");
}

// ================= every command is a tool =================

#[test]
fn every_offered_command_round_trips_through_the_tool_parser() {
    let c = cfg();
    let book = ToolBook::new(&c.commands);
    let parser = Parser::new(&c.commands);
    let mut unreachable = Vec::new();
    for e in book.entries() {
        if !book.offered(&e.name) {
            assert!(atlas::intent::from_tool(&e.name, &serde_json::json!({"arg": "x"}), "x").is_none(), "{} is offered nowhere but callable", e.name);
            continue;
        }
        // A sentence the phrases read as this command, and the tool call
        // for it, must come out as the same command.
        let args = ["", "chrome", "homelab", "focus", "the quarterly report", "sam", "sam to family", "sam to the family group"];
        let mut ok = false;
        'found: for phrase in &e.phrases {
            for a in args {
                let said = format!("{phrase} {a}").trim().to_string();
                let parsed = parser.parse(&said);
                if matches!(parsed, Intent::Unknown(_)) {
                    continue;
                }
                let arg = if a.is_empty() { serde_json::json!({}) } else { serde_json::json!({ "arg": a }) };
                if let Some(called) = atlas::intent::from_tool(&e.name, &arg, &said) {
                    if std::mem::discriminant(&called) == std::mem::discriminant(&parsed) {
                        ok = true;
                        break 'found;
                    }
                }
            }
        }
        if !ok {
            unreachable.push(e.name.clone());
        }
        // And its definition is valid.
        let spec = e.spec();
        assert_eq!(spec["function"]["name"], e.name.as_str());
        assert!(!spec["function"]["description"].as_str().unwrap_or("").is_empty(), "{} has no description", e.name);
    }
    assert!(unreachable.is_empty(), "offered but not reachable by a tool call: {unreachable:?}");
}

#[test]
fn every_command_in_commands_yaml_is_described() {
    let c = cfg();
    for cmd in &c.commands.commands {
        assert!(cmd.describe.as_deref().is_some_and(|d| !d.trim().is_empty()), "{} has no describe:", cmd.intent);
        if let Some(x) = &cmd.expose {
            assert!(["core", "retrieved", "never"].contains(&x.as_str()), "{}: expose: {x}", cmd.intent);
        }
    }
    let book = ToolBook::new(&c.commands);
    for never in atlas::intent::NEVER_FOR_THE_MODEL {
        assert!(!book.offered(never), "{never} is offered to the model");
    }
    // The file and the code say the same thing about what the model may
    // never choose.
    let mut in_yaml: Vec<&str> =
        c.commands.commands.iter().filter(|x| x.expose.as_deref() == Some("never")).map(|x| x.intent.as_str()).collect();
    in_yaml.sort();
    in_yaml.dedup();
    let mut in_code: Vec<&str> = atlas::intent::NEVER_FOR_THE_MODEL.to_vec();
    in_code.sort();
    assert_eq!(in_yaml, in_code);
}

#[test]
fn the_core_tools_come_first_in_a_fixed_order_and_the_rest_by_the_sentence() {
    let c = cfg();
    let book = ToolBook::new(&c.commands);
    let names = |v: Vec<serde_json::Value>| -> Vec<String> { v.iter().map(|t| t["function"]["name"].as_str().unwrap().to_string()).collect() };
    let a = names(book.for_sentence("remind me what's on my flashcards", 6));
    let b = names(book.for_sentence("translate this into French", 6));
    let core = a.iter().take_while(|n| book.get(n).unwrap().exposure == atlas::intent::Exposure::Core).count();
    assert!(core >= 10, "{a:?}");
    assert_eq!(a[..core], b[..core], "the core tools are the same, in the same order");
    assert!(a.contains(&"cards".to_string()), "{a:?}");
    assert!(b.contains(&"translate".to_string()), "{b:?}");
    assert!(a.len() <= core + 6 && b.len() <= core + 6);
}

#[test]
fn brief_on_works_from_the_model_and_the_schema() {
    let d = atlas::brain::parse_decision(r#"{"action":"brief_on","arg":"homelab","say":""}"#).unwrap();
    assert_eq!(d.intent, Intent::BriefOn("homelab".into()));
    assert_eq!(atlas::intent::from_tool("brief_on", &serde_json::json!({"arg": "homelab"}), "how's homelab doing"), Some(Intent::BriefOn("homelab".into())));
    // Any command by its name, in the one-prompt schema too.
    let d = atlas::brain::parse_decision(r#"{"action":"agenda","arg":"tomorrow","say":""}"#).unwrap();
    assert_eq!(d.intent, Intent::Agenda("tomorrow".into()));
    // Never the vault.
    assert!(atlas::brain::parse_decision(r#"{"action":"unlock","arg":"hunter2","say":""}"#).is_err());
}

#[test]
fn a_tool_call_written_inline_is_read_as_a_call() {
    let r = ChatReply::from_text("Sure.\n<tool_call>\n{\"name\": \"open_app\", \"arguments\": {\"arg\": \"chrome\"}}\n</tool_call>");
    assert_eq!(r.text, "Sure.");
    assert_eq!(r.tool_calls.len(), 1);
    let d = atlas::brain::decision_from_chat(&r, "open chrome for me");
    assert_eq!(d.intent, Intent::OpenApp("chrome".into()));
    // An app nothing in the sentence named is not opened.
    let d = atlas::brain::decision_from_chat(&r, "what's the capital of France");
    assert_eq!(d.intent, Intent::Say("Sure.".into()));
}

#[test]
fn a_consequential_command_the_model_chose_is_asked_about_first() {
    let (c, p) = (cfg(), plat());
    let spy = ChatSpy::calling("hand_over", "");
    let mut d = daemon(&c, &p, spy.clone(), "handover");
    let reply = d.turn("my sister wants to borrow the laptop for a bit", 100);
    assert_eq!(spy.calls(), 1);
    assert!(reply.contains("Go ahead?") || reply.to_lowercase().contains("okay?") || reply.contains('?'), "it didn't ask: {reply}");
    assert!(
        matches!(d.session.pending, atlas::session::Pending::Approval(Intent::HandOver(_), _)),
        "no approval is waiting: {:?}",
        d.session.pending
    );
}

#[test]
fn a_read_tool_the_model_chose_just_runs() {
    let (c, p) = (cfg(), plat());
    let spy = ChatSpy::calling("agenda", "tomorrow");
    let mut d = daemon(&c, &p, spy.clone(), "agenda");
    let reply = d.turn("am I free at all tomorrow", 100);
    assert!(!matches!(d.session.pending, atlas::session::Pending::Approval(..)), "{reply}");
    assert!(!reply.contains("Go ahead?"), "{reply}");
}

// ================= the prompt keeps its prefix =================

#[test]
fn the_prompt_prefix_is_identical_across_turns_with_different_window_titles() {
    let (c, p) = (cfg(), plat());
    let spy = ChatSpy::saying("Sounds good.");
    let mut d = daemon(&c, &p, spy.clone(), "prefix");
    *p.active.borrow_mut() = Some(ActiveWindow { process: "chrome.exe".into(), title: "Quarterly numbers - Sheets".into() });
    let _ = d.turn("what do you think of the plan", 100);
    let first = spy.last().messages;
    *p.active.borrow_mut() = Some(ActiveWindow { process: "code.exe".into(), title: "main.rs - atlas".into() });
    let _ = d.turn("tell me a joke about compilers", 160);
    let second = spy.last().messages;

    assert_eq!(first[0].content, second[0].content, "the system message changed between turns");
    assert!(!first[0].content.contains("Quarterly numbers"), "the window title is in the stable part");
    assert!(first.last().unwrap().content.contains("Quarterly numbers"), "the window title is missing from this turn");
    assert!(second.last().unwrap().content.contains("main.rs"), "the window title is missing from this turn");
    // Neither the time nor a sentence count sits in the stable part.
    assert!(!first[0].content.contains("At most"), "{}", first[0].content);
    // The tools that are always offered come first, in the same order.
    let names = |r: &ChatRequest| -> Vec<String> { r.tools.iter().take(10).map(|t| t["function"]["name"].as_str().unwrap_or("").to_string()).collect() };
    let asked = spy.asked.lock().unwrap();
    assert_eq!(names(&asked[0]), names(&asked[1]));
}

#[test]
fn about_atlas_goes_in_only_when_the_question_is_about_atlas() {
    let (c, p) = (cfg(), plat());
    let spy = ChatSpy::saying("Paris.");
    let mut d = daemon(&c, &p, spy.clone(), "aboutatlas");
    let _ = d.turn("what's the capital of France", 100);
    assert!(!spy.last_user().contains("About Atlas (you)"), "{}", spy.last_user());
    let _ = d.turn("where do I change your voice settings", 110);
    assert!(spy.last_user().contains("About Atlas (you)"), "{}", spy.last_user());
}

#[test]
fn a_poem_gets_room_and_a_task_gets_brevity() {
    use atlas::register::{read, Moment, Register};
    assert_eq!(read("write me a poem about the sea", &Moment::default()), Register::Chatting);
    assert_eq!(read("write a script to rename my photos", &Moment::default()), Register::Working);
    assert_eq!(read("say that again", &Moment::default()), Register::Chatting, "'again' alone is not frustration");
    assert!(Register::Chatting.max_tokens() > Register::Working.max_tokens());
}

#[test]
fn the_stream_stops_at_the_sentence_cap_not_mid_sentence() {
    let mut cap = atlas::brain::SentenceCap::new(Some(2));
    assert!(cap.push("It is 3.5 metres long. "));
    assert!(!cap.push("That is big. And"), "two sentences in, it stops");
    let mut s = atlas::brain::Sentences::default();
    assert!(s.push("Hello there").is_empty());
    assert_eq!(s.push(". How are you? I"), vec!["Hello there.".to_string(), "How are you?".to_string()]);
}

// ================= the Talk page always answers =================

fn post_talk(d: &mut Daemon, text: &str) {
    let fields = vec![("text".to_string(), text.to_string())];
    let _ = atlas::hublive::reply(d, atlas::server::Action::HubPost { path: "/hub/talk".into(), fields });
}

#[test]
fn a_talk_turn_that_pauses_still_shows_a_reply() {
    let (c, p) = (cfg(), plat());
    let spy = ChatSpy::saying("unused");
    let mut d = daemon(&c, &p, spy.clone(), "talkpause");
    post_talk(&mut d, "pause");
    let _ = d.tick(100);
    let last = d.thread.recent.last().expect("nothing on the Talk page");
    assert_eq!(last.said, "pause");
    assert!(!last.reply.trim().is_empty(), "the reply vanished");
    // And while paused, what's typed still gets a line back.
    post_talk(&mut d, "are you there at all");
    let _ = d.tick(101);
    let last = d.thread.recent.last().unwrap();
    assert_eq!(last.said, "are you there at all");
    assert!(last.reply.contains("paused"), "{}", last.reply);
}

#[test]
fn a_talk_turn_does_not_hold_the_loop_while_the_model_thinks() {
    let (c, p) = (cfg(), plat());
    let spy = Arc::new(ChatSpy {
        asked: Mutex::new(Vec::new()),
        reply: Mutex::new(ChatReply { text: "Once upon a time there was a lighthouse. It kept the light.".into(), tool_calls: vec![] }),
        slow_ms: 3_000,
    });
    let mut d = daemon(&c, &p, spy.clone(), "offloop");
    post_talk(&mut d, "tell me a short story about a lighthouse");
    let started = std::time::Instant::now();
    let _ = d.tick(100);
    // The tick came back while the model was still on it.
    assert!(d.talk_is_thinking(), "the tick waited for the model ({:?})", started.elapsed());
    assert!(started.elapsed() < std::time::Duration::from_millis(3_000), "the tick waited for the model: {:?}", started.elapsed());
    // The words so far are there for the Talk page while it thinks.
    let mut seen_partial = false;
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while d.talk_is_thinking() && std::time::Instant::now() < until {
        seen_partial |= d.talk_so_far().contains("Once upon a time");
        std::thread::sleep(std::time::Duration::from_millis(50));
        let _ = d.tick(101);
    }
    assert!(seen_partial, "the words so far were never shown");
    assert!(!d.talk_is_thinking(), "the turn never finished");
    let last = d.thread.recent.last().unwrap();
    assert_eq!(last.said, "tell me a short story about a lighthouse");
    assert!(last.reply.contains("lighthouse"), "{}", last.reply);
}

// ================= a question doesn't eat the next command =================

#[test]
fn an_expired_parked_question_does_not_eat_the_next_command() {
    let (c, p) = (cfg(), plat());
    let spy = ChatSpy::saying("unused");
    let mut d = daemon(&c, &p, spy.clone(), "expired");
    d.session.await_approval(Intent::WorkspaceOff, "Shutting down. Go ahead?");
    let _ = d.tick(1_000);
    let reply = d.turn("open chrome", 1_000 + atlas::daemon::QUESTION_LIFETIME_SECS + 60);
    assert!(!reply.contains("Left it alone"), "{reply}");
    assert!(reply.to_lowercase().contains("chrome"), "{reply}");
    assert!(
        !matches!(d.session.pending, atlas::session::Pending::Approval(Intent::WorkspaceOff, _)),
        "the old question is still waiting"
    );
    // One asked a moment ago is still answered.
    d.session.await_approval(Intent::WorkspaceOff, "Shutting down. Go ahead?");
    let _ = d.tick(5_000);
    assert_eq!(d.turn("no", 5_030), "Left it alone.");
}

#[test]
fn a_new_request_instead_of_yes_or_no_is_taken_as_itself() {
    let (c, p) = (cfg(), plat());
    let spy = ChatSpy::saying("unused");
    let mut d = daemon(&c, &p, spy.clone(), "newinstead");
    d.session.await_approval(Intent::WorkspaceOff, "Shutting down. Go ahead?");
    let reply = d.turn("open chrome", 100);
    assert!(!reply.contains("Left it alone"), "{reply}");
    assert!(reply.to_lowercase().contains("chrome"), "{reply}");
    assert!(!d.session.is_waiting() || !matches!(d.session.pending, atlas::session::Pending::Approval(Intent::WorkspaceOff, _)));
    // A plain no is still a no.
    d.session.await_approval(Intent::WorkspaceOff, "Shutting down. Go ahead?");
    assert_eq!(d.turn("no", 110), "Left it alone.");
}

// ================= voice follow-ups =================

#[test]
fn a_follow_up_about_someone_else_is_still_the_conversation() {
    use atlas::addressing::{assess, respond, Directed, Response, Situation};
    let said = "yeah what did she say about them after that";
    let cold = Situation::default();
    assert_ne!(assess(said, &cold).directed, Directed::AtAtlas);
    let warm = Situation { just_spoke: true, ..Default::default() };
    let a = assess(said, &warm);
    assert_ne!(respond(&a, &warm), Response::Ignore, "a follow-up was dropped in silence: {a:?}");
}

// ================= offering to look it up =================

#[test]
fn not_knowing_or_a_changing_fact_comes_with_an_offer_to_look_it_up() {
    assert!(atlas::brain::worth_looking_up("who won the game last night?", "I don't know the result."));
    assert!(atlas::brain::worth_looking_up("what's the weather today", "Probably mild."));
    assert!(!atlas::brain::worth_looking_up("tell me a joke", "Why did the compiler cross the road?"));
}

#[test]
fn the_offer_is_made_and_answered_as_an_offer() {
    let (c, p) = (cfg(), plat());
    let spy = ChatSpy::saying("I'm not sure who won -- I don't have last night's results.");
    let mut d = daemon(&c, &p, spy.clone(), "lookup");
    let reply = d.turn("who won the game last night?", 100);
    assert!(reply.ends_with("Want me to look it up?"), "{reply}");
    // The answer is the offer's (a yes runs `research <the question>`, the
    // offer's command -- not run here, it would go to the web), not a new
    // sentence for the model.
    let before = spy.calls();
    let no = d.turn("no", 110);
    assert_eq!(no, "Alright.");
    assert_eq!(spy.calls(), before, "the answer went to the model");
    assert!(!d.session.is_waiting(), "the offer is still waiting");
    // Something else instead is taken as itself, and the offer dropped.
    let _ = d.turn("who won the game last night?", 200);
    assert!(d.session.is_waiting(), "offered again");
    *spy.reply.lock().unwrap() = ChatReply { text: "Why did the compiler cross the road?".into(), tool_calls: vec![] };
    let other = d.turn("never mind that, tell me a joke", 210);
    assert!(spy.calls() > before + 1);
    assert!(spy.last_user().ends_with("User said: never mind that, tell me a joke"), "the new request didn't reach the model: {other}");
    assert!(other.contains("compiler"), "{other}");
}

#[test]
fn a_chat_that_fails_falls_back_to_the_one_prompt_path() {
    struct Broken;
    impl Llm for Broken {
        fn complete(&self, _: &str, _: &str) -> atlas::error::Result<String> {
            Ok(r#"{"action":"say","arg":null,"say":"Answered the old way."}"#.into())
        }
        fn native_chat(&self) -> bool {
            true
        }
        fn chat(&self, _: &ChatRequest, _: &mut dyn FnMut(&str) -> bool) -> atlas::error::Result<ChatReply> {
            Err(atlas::error::AtlasError::Platform("the model server answered 500: tools not supported".into()))
        }
    }
    let c = cfg();
    let p = Parser::new(&c.commands);
    let persona = atlas::persona::Persona::default();
    let brain = atlas::brain::Brain { llm: &Broken, fallback: &p, voice: Some((&persona, atlas::register::Register::Chatting)) };
    let turn = atlas::brain::Turn { said: "tell me something interesting".into(), one_prompt: "Now: noon.".into(), ..Default::default() };
    let d = brain.converse(&turn, &mut |_| true);
    assert_eq!(d.intent, Intent::Say("Answered the old way.".into()));
}

#[test]
fn a_model_without_chat_gets_messages_flattened_into_one_prompt() {
    let msgs = vec![Msg::system("S"), Msg::user("hi"), Msg::assistant("hello"), Msg::user("User said: how are you")];
    let (s, u) = atlas::brain::flatten_messages(&msgs);
    assert_eq!(s, "S");
    assert!(u.starts_with("Conversation so far:\nyou: hi\natlas: hello\n"), "{u}");
    assert!(u.ends_with("User said: how are you"));
}

#[test]
fn the_chat_stream_is_read_as_it_arrives() {
    let mut s = atlas::models::ChatStream::default();
    assert_eq!(s.line(r#"data: {"choices":[{"delta":{"content":"Hel"}}]}"#), Some("Hel".into()));
    assert_eq!(s.line(r#"data: {"choices":[{"delta":{"content":"lo."}}]}"#), Some("lo.".into()));
    s.line(r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"open_app","arguments":"{\"arg\":"}}]}}]}"#);
    s.line(r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"chrome\"}"}}]}}]}"#);
    s.line("data: [DONE]");
    assert!(s.done);
    let r = s.reply();
    assert_eq!(r.text, "Hello.");
    assert_eq!(r.tool_calls[0].name, "open_app");
    assert_eq!(r.tool_calls[0].arguments["arg"], "chrome");
    // A server that sent an error instead.
    let mut e = atlas::models::ChatStream::default();
    e.line(r#"data: {"error":{"message":"tools param requires --jinja flag"}}"#);
    assert!(e.error.as_deref().unwrap_or("").contains("jinja"));
}

#[test]
fn the_server_is_started_for_chat_tools_and_two_slots() {
    let m = atlas::models::Model {
        path: "m.gguf".into(),
        id: "Qwen3-VL-4B-Instruct-Q4_K_M".into(),
        architecture: "qwen3".into(),
        quant: "Q4_K_M".into(),
        parameters: 4_000_000_000,
        weight_bytes: 2 << 30,
        max_context: 8192,
        chat_template: None,
    };
    let args = atlas::models::server_args(&m, &atlas::models::ModelsConfig::default(), 0);
    for flag in ["--jinja", "--kv-unified", "--cache-reuse"] {
        assert!(args.iter().any(|a| a == flag), "{flag} missing: {args:?}");
    }
    let np = args.iter().position(|a| a == "-np").map(|i| args[i + 1].clone());
    assert_eq!(np.as_deref(), Some("2"));
    // And the chat address beside Atlas's own completion address.
    let l = atlas::models::llm_config_for(&m, &atlas::models::ModelsConfig::default(), &atlas::models::server_post());
    assert_eq!(atlas::models::chat_url_beside(&l).as_deref(), Some("http://127.0.0.1:8080/v1/chat/completions"));
}

// ================= against a real llama-server, when one is running =================

/// Runs only when `ATLAS_LIVE_LLAMA_PORT` names a llama-server started with
/// `models::server_args` (the shipped build, b10456, was used on 27 Sep
/// 2026). Checks the real endpoint takes Atlas's chat request with tools,
/// streams it, and that a command comes back as a tool call.
#[test]
fn live_llama_server_smoke() {
    let Ok(port) = std::env::var("ATLAS_LIVE_LLAMA_PORT") else { return };
    let port: u16 = port.parse().unwrap();
    let c = cfg();
    let mcfg = atlas::models::ModelsConfig { port, ..Default::default() };
    let model = atlas::models::Model {
        path: "m.gguf".into(),
        id: "Qwen3-VL-4B-Instruct-Q4_K_M".into(),
        architecture: "qwen3vl".into(),
        quant: "Q4_K_M".into(),
        parameters: 4_000_000_000,
        weight_bytes: 2 << 30,
        max_context: 8192,
        chat_template: None,
    };
    let lc = atlas::models::llm_config_for(&model, &mcfg, &atlas::models::server_post());
    let llm = atlas::brain::ShellLlm { cfg: lc, vars: Default::default() };
    assert!(llm.native_chat(), "no chat address beside {port}");
    let parser = Parser::new(&c.commands);
    let persona = atlas::persona::Persona::default();
    let book = ToolBook::new(&c.commands);
    let brain = atlas::brain::Brain { llm: &llm, fallback: &parser, voice: Some((&persona, atlas::register::Register::Chatting)) };
    for (said, history) in [
        ("tell me a fun fact about octopuses", vec![]),
        ("why is that?", vec![Msg::user("tell me a fun fact about octopuses"), Msg::assistant("Octopuses have three hearts.")]),
        ("open chrome and put it on the left", vec![]),
        ("what's on my calendar tomorrow", vec![]),
        ("am I free tomorrow afternoon?", vec![]),
    ] {
        let turn = atlas::brain::Turn {
            said: said.into(),
            aside: false,
            system: format!("{}\n\nApps you can open, close or switch to by name: chrome, discord, notepad.", persona.character()),
            history,
            now: persona.for_this_turn_on(atlas::register::Register::Chatting, 3, said, false),
            tools: book.for_sentence(said, 6),
            // 28 Sep 2026: how many of the tools are the every-turn ones
            // (`ChatRequest::stable_tools`); not what this measures.
            stable_tools: 0,
            max_tokens: 200,
            max_sentences: Some(3),
            one_prompt: String::new(),
            skip_phrases: true,
        };
        let started = std::time::Instant::now();
        let mut first: Option<std::time::Duration> = None;
        let d = brain.converse(&turn, &mut |_| {
            first.get_or_insert(started.elapsed());
            true
        });
        eprintln!(
            "LIVE {said:?} -> {:?} / {:?} (first words {:?}, whole {:?})",
            d.intent,
            d.say,
            first,
            started.elapsed()
        );
    }
}

#[test]
fn the_web_is_for_when_it_was_asked_for_or_changes_by_the_day() {
    let r = ChatReply {
        text: "Octopuses have three hearts.".into(),
        tool_calls: vec![ToolCall { name: "research".into(), arguments: serde_json::json!({"arg": "octopus facts"}) }],
    };
    let d = atlas::brain::decision_from_chat(&r, "tell me a fun fact about octopuses");
    assert_eq!(d.intent, Intent::Say("Octopuses have three hearts.".into()));
    let d = atlas::brain::decision_from_chat(&r, "look up the latest octopus research");
    assert_eq!(d.intent, Intent::Research("octopus facts".into()));
}


#[test]
fn a_quote_marker_the_model_copied_is_not_read_out() {
    // Seen live against llama-server (Qwen3 0.6B, 27 Sep 2026): the model
    // copied the "> " Atlas marks quoted text with onto its own answer.
    assert_eq!(atlas::brain::spoken_text("> Octopuses have three hearts.").as_deref(), Some("Octopuses have three hearts."));
}
