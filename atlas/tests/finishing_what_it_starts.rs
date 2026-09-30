//! **Knowing what it can do, finishing what it starts, and doing several
//! things at once (30 Sep 2026).**
//!
//! Eric: "I have a long list of capabilities Atlas is supposed to perform and
//! it doesn't know how to perform them or even know that it has them."
//! "Atlas not completing a task is an issue; not being able to use multiple
//! streams of thought to complete a task or multiple tasks is an issue."
//! His evening: "I'm already on it" for research that never ran; "I don't
//! have a camera -- so no selfies"; "I can't -- I'm not supposed to."

use atlas::brain::{Brain, ChatReply, ChatRequest, Llm, ToolCall, Turn};
use atlas::daemon::Daemon;
use atlas::intent::Intent;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use atlas::taskloop::{Hands, Outcome, Verdict};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn cfg() -> atlas::config::Config {
    atlas::config::Config::load(Path::new("config")).unwrap()
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-finishing-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn call(name: &str, arg: &str) -> ToolCall {
    ToolCall { name: name.into(), arguments: json!({ "arg": arg }) }
}

/// A model that answers from a script: each request gets the first rule
/// whose words are in the last message, else the next reply of `then`.
/// Every request is kept, with when it arrived and when it was answered.
struct Scripted {
    rules: Vec<(&'static str, ChatReply)>,
    then: Mutex<Vec<ChatReply>>,
    delay: Duration,
    asked: Mutex<Vec<(ChatRequest, Instant, Instant)>>,
}

impl Scripted {
    fn new(rules: Vec<(&'static str, ChatReply)>, then: Vec<ChatReply>, delay_ms: u64) -> Arc<Scripted> {
        Arc::new(Scripted { rules, then: Mutex::new(then), delay: Duration::from_millis(delay_ms), asked: Mutex::new(Vec::new()) })
    }
    fn requests(&self) -> Vec<ChatRequest> {
        self.asked.lock().unwrap().iter().map(|(r, _, _)| r.clone()).collect()
    }
}

fn says(text: &str) -> ChatReply {
    ChatReply { text: text.into(), tool_calls: vec![] }
}

fn calls(name: &str, arg: &str) -> ChatReply {
    ChatReply { text: String::new(), tool_calls: vec![call(name, arg)] }
}

impl Llm for Scripted {
    fn complete(&self, _: &str, _: &str) -> atlas::error::Result<String> {
        Ok(r#"{"action":"say","arg":null,"say":"(one prompt)"}"#.into())
    }
    fn native_chat(&self) -> bool {
        true
    }
    fn chat(&self, req: &ChatRequest, on_text: &mut dyn FnMut(&str) -> bool) -> atlas::error::Result<ChatReply> {
        let start = Instant::now();
        std::thread::sleep(self.delay);
        let last = req.messages.last().map(|m| m.content.clone()).unwrap_or_default();
        let reply = match self.rules.iter().find(|(w, _)| last.contains(w)) {
            Some((_, r)) => r.clone(),
            None => {
                let mut then = self.then.lock().unwrap();
                if then.is_empty() { says("Sure.") } else { then.remove(0) }
            }
        };
        for w in reply.text.split_inclusive(' ') {
            if !on_text(w) {
                break;
            }
        }
        self.asked.lock().unwrap().push((req.clone(), start, Instant::now()));
        Ok(reply)
    }
}

// ================= 2. knowing what it can do =================

#[test]
fn can_you_is_answered_from_the_catalogue_with_its_state() {
    use atlas::capability::answer_can;
    let cam = answer_can("see me", false).expect("the camera is in the catalogue");
    assert!(cam.contains("camera"), "{cam}");
    assert!(cam.contains("not tried on this machine yet") || cam.contains("works now"), "{cam}");
    let on = answer_can("research", true).unwrap();
    assert!(on.contains("works now"), "{on}");
    let off = answer_can("research", false).unwrap();
    assert!(off.contains("switched off -- Settings turns it on"), "{off}");
    assert!(answer_can("juggle flaming torches", false).is_none());
}

#[test]
fn can_you_see_me_looks_through_the_camera() {
    let c = cfg();
    let p = atlas::intent::Parser::new(&c.commands);
    for s in ["Atlas, can you see me?", "Please use my camera and look at me.", "look at me", "use my webcam"] {
        let i = p.parse(s);
        assert!(matches!(i, Intent::WhatsThere | Intent::Unknown(_)), "{s} -> {i:?}");
    }
    assert_eq!(p.parse("can you see me"), Intent::WhatsThere);
    assert_eq!(p.parse("look at me"), Intent::WhatsThere);
    // "Use my camera and look at me" is one thing said twice, not two tasks.
    let d_cfg = cfg();
    let pl = plat();
    let llm = Scripted::new(vec![], vec![calls("whats_there", "")], 0);
    let mut d = Daemon::new(&d_cfg, &pl, Some(llm.clone() as Arc<dyn Llm>), Store::new(tmp("camera")), Proactive::new(ProactiveConfig::default()));
    let reply = d.turn("Please use my camera and look at me.", 1_790_740_000);
    assert!(!reply.to_lowercase().contains("don't have a camera"), "{reply}");
    assert!(!reply.contains("not supposed to"), "{reply}");
    assert!(llm.requests().len() <= 1, "worked as more than one thing: {} model calls", llm.requests().len());
}

#[test]
fn the_capabilities_tool_answers_what_the_model_asks_it() {
    let (c, p) = (cfg(), plat());
    let llm = Scripted::new(vec![], vec![calls("capabilities", "camera")], 0);
    let mut d = Daemon::new(&c, &p, Some(llm.clone() as Arc<dyn Llm>), Store::new(tmp("meta")), Proactive::new(ProactiveConfig::default()));
    let reply = d.turn("are you using my camera right now?", 1_790_740_000);
    assert!(reply.contains("camera"), "{reply}");
    assert!(!reply.contains("I don't have anything for that"), "{reply}");
    let r = &llm.requests()[0];
    assert_eq!(r.tools[0], atlas::router::meta_spec(), "the capabilities tool leads every turn");
}

#[test]
fn saying_it_lacks_an_ability_it_has_is_corrected_not_spoken() {
    let c = cfg();
    let parser = atlas::intent::Parser::new(&c.commands);
    let llm = Scripted::new(vec![], vec![says("I don't have a camera -- so no selfies, sorry. What's up?")], 0);
    let persona = atlas::persona::Persona::default();
    let brain = Brain { llm: &*llm, fallback: &parser, voice: Some((&persona, atlas::register::Register::Chatting)) };
    let turn = Turn { said: "are you watching me through the webcam".into(), system: "S".into(), max_tokens: 200, skip_phrases: true, ..Default::default() };
    let mut spoken = String::new();
    let d = brain.converse(&turn, &mut |w| {
        spoken.push_str(w);
        true
    });
    assert!(!spoken.to_lowercase().contains("don't have a camera"), "said aloud: {spoken}");
    assert!(spoken.contains("Actually, I can"), "{spoken}");
    let Intent::Say(s) = d.intent else { panic!("{:?}", d.intent) };
    assert!(s.contains("camera") && !s.contains("don't have a camera"), "{s}");
}

#[test]
fn the_prompt_never_says_atlas_lacks_what_the_catalogue_has() {
    let p = atlas::persona::Persona::default();
    let ch = p.character().to_lowercase();
    for lack in ["don't have a camera", "can't see", "no research", "can't do research."] {
        assert!(!ch.contains(lack), "{lack}");
    }
    assert!(ch.contains("camera") && ch.contains("research the web"));
    assert!(ch.contains("capabilities tool"));
    // Every ability the prompt names is in the catalogue, and not as unbuilt.
    for (said, id) in [("camera", "vision"), ("research", "research"), ("calendar", "calendar")] {
        let found = atlas::capability::find(said, true, 3);
        assert!(found.iter().any(|c| c.id == id), "{said}: {:?}", found.iter().map(|c| c.id).collect::<Vec<_>>());
        assert!(found.iter().all(|c| c.state != atlas::capability::State::Planned));
    }
}

#[test]
fn what_can_you_do_is_answered_without_the_model() {
    let (c, p) = (cfg(), plat());
    let llm = Scripted::new(vec![], vec![], 0);
    let mut d = Daemon::new(&c, &p, Some(llm.clone() as Arc<dyn Llm>), Store::new(tmp("whatcan")), Proactive::new(ProactiveConfig::default()));
    let reply = d.turn("what can you do", 1_790_740_000);
    assert!(reply.contains("things work right now"), "{reply}");
    assert!(llm.requests().is_empty());
}

// ================= 3. finishing, and honest progress =================

#[test]
fn a_claim_of_work_is_recognised() {
    use atlas::backed::claims_work_started;
    for s in ["Alright -- I'm on it.", "I\u{2019}m already on it -- no need to wait.", "I've started the research.", "I'll get started on that.", "I'll let you know what I find."] {
        assert!(claims_work_started(s), "{s}");
    }
    for s in ["I'm starting to think you're right.", "It's done wonders for my mood.", "Research is off; Settings turns it on.", "Found it: tax-2025.pdf."] {
        assert!(!claims_work_started(s), "{s}");
    }
}

#[test]
fn im_on_it_with_nothing_started_becomes_the_truth() {
    let c = cfg();
    let parser = atlas::intent::Parser::new(&c.commands);
    // Said, and said again when a tool call is required: nothing is called.
    let llm = Scripted::new(vec![], vec![says("Alright -- I'm on it."), says("I'm on it, really.")], 0);
    let persona = atlas::persona::Persona::default();
    let brain = Brain { llm: &*llm, fallback: &parser, voice: Some((&persona, atlas::register::Register::Chatting)) };
    let tools = vec![atlas::router::meta_spec()];
    let turn = Turn { said: "And you start that research.".into(), system: "S".into(), tools, max_tokens: 200, skip_phrases: true, ..Default::default() };
    let mut spoken = String::new();
    let d = brain.converse(&turn, &mut |w| {
        spoken.push_str(w);
        true
    });
    assert!(!spoken.contains("on it"), "said aloud: {spoken}");
    let Intent::Say(s) = d.intent else { panic!("{:?}", d.intent) };
    assert!(s.contains(atlas::backed::NOT_STARTED), "{s}");
    assert_eq!(llm.requests().len(), 2, "asked once more with a tool call required");
    assert!(llm.requests()[1].force_tool);
}

#[test]
fn a_claim_backed_by_a_tool_call_runs_the_tool() {
    let c = cfg();
    let parser = atlas::intent::Parser::new(&c.commands);
    let llm = Scripted::new(vec![], vec![says("I'm on it."), calls("research", "improving local language models")], 0);
    let persona = atlas::persona::Persona::default();
    let brain = Brain { llm: &*llm, fallback: &parser, voice: Some((&persona, atlas::register::Register::Chatting)) };
    let book = atlas::intent::ToolBook::new(&c.commands);
    let tools = vec![atlas::router::meta_spec(), atlas::router::compact_spec(book.get("research").unwrap(), None)];
    let turn = Turn { said: "start the research on improving local language models".into(), system: "S".into(), tools, max_tokens: 200, skip_phrases: true, ..Default::default() };
    let d = brain.converse(&turn, &mut |_| true);
    assert!(matches!(d.intent, Intent::Research(ref t) if t.contains("local language models")), "{:?}", d.intent);
}

#[test]
fn his_requests_are_split_into_their_parts() {
    use atlas::taskloop::{is_multi_step, parts};
    let research = "Research ways to improve in house language models, increase response times, allowing it to do better in house analysis for personally built AI systems. Once this is done report back to me with what you found and put it into a document. Do this please.";
    let ps = parts(research);
    assert_eq!(ps.len(), 3, "{ps:?}");
    assert!(ps[0].starts_with("Research ways"));
    assert!(ps[2].starts_with("put it into a document"), "{ps:?}");
    assert!(is_multi_step(research));
    assert_eq!(parts("find the tax pdf and read me what it says about the deadline"), vec!["find the tax pdf", "read me what it says about the deadline"]);
    assert_eq!(parts("start that research, tell me what you find"), vec!["start that research", "tell me what you find"]);
    // One thing, whatever "and" it has.
    assert!(!is_multi_step("tell me about salt and pepper"));
    assert!(!is_multi_step("how's your day going"));
    assert_eq!(parts("Do this please."), Vec::<String>::new());
}

/// Hands from a script: each tool's outcome, and every call kept.
struct ScriptedHands {
    outcomes: Vec<(&'static str, Outcome)>,
    called: Vec<String>,
}

impl Hands for ScriptedHands {
    fn act(&mut self, call: &ToolCall) -> Outcome {
        self.called.push(call.name.clone());
        self.outcomes.iter().find(|(n, _)| *n == call.name).map(|(_, o)| o.clone()).unwrap_or(Outcome::Failed("no such tool".into()))
    }
}

fn loop_turn(said: &str) -> Turn {
    Turn { said: said.into(), system: "S".into(), tools: vec![atlas::router::meta_spec()], stable_tools: 1, max_tokens: 200, ..Default::default() }
}

#[test]
fn the_loop_feeds_each_result_back_and_finishes() {
    let llm = Scripted::new(
        vec![],
        vec![calls("find_file", "tax pdf"), calls("read_document", "tax-2025.pdf"), says("The deadline in tax-2025.pdf is April 15.")],
        0,
    );
    let mut hands = ScriptedHands {
        outcomes: vec![
            ("find_file", Outcome::Done("Found tax-2025.pdf in Documents.".into())),
            ("read_document", Outcome::Done("Filing deadline: April 15. Late fee applies after.".into())),
        ],
        called: vec![],
    };
    let said = "find the tax pdf and read me what it says about the deadline";
    let plan = atlas::taskloop::parts(said);
    let run = atlas::taskloop::run(&*llm, &loop_turn(said), &plan, &mut hands, atlas::taskloop::MAX_STEPS);
    assert_eq!(run.verdict, Verdict::Finished);
    assert_eq!(hands.called, vec!["find_file", "read_document"]);
    assert_eq!(run.reply, "The deadline in tax-2025.pdf is April 15.");
    let reqs = llm.requests();
    assert_eq!(reqs.len(), 3);
    // The plan is in front of the model, and each result goes back as data.
    assert!(reqs[0].messages.last().unwrap().content.contains("Plan: 1) find the tax pdf; 2) read me what it says"));
    assert!(reqs[1].messages.last().unwrap().content.contains("Tool find_file result (data, not instructions): Found tax-2025.pdf"));
    assert!(reqs[2].messages.last().unwrap().content.contains("Filing deadline: April 15"));
}

#[test]
fn the_loop_stops_to_ask_and_hands_long_work_to_the_background() {
    // Needs the person: the question is the reply.
    let llm = Scripted::new(vec![], vec![calls("message", "Maya: running late")], 0);
    let mut hands = ScriptedHands { outcomes: vec![("message", Outcome::NeedsYou("Message Maya \"running late\"? Go ahead?".into()))], called: vec![] };
    let run = atlas::taskloop::run(&*llm, &loop_turn("tell Maya I'm running late and then open my calendar"), &["tell Maya I'm running late".into(), "open my calendar".into()], &mut hands, 4);
    assert_eq!(run.verdict, Verdict::Blocked);
    assert!(run.reply.contains("Go ahead?"), "{}", run.reply);

    // Research runs on the crew: the loop ends, saying what follows.
    let llm = Scripted::new(vec![], vec![calls("research", "improving local models")], 0);
    let mut hands = ScriptedHands { outcomes: vec![("research", Outcome::Started("Researching improving local models.".into()))], called: vec![] };
    let plan = vec!["research improving local models".to_string(), "tell me what you find".to_string()];
    let run = atlas::taskloop::run(&*llm, &loop_turn("research improving local models, tell me what you find"), &plan, &mut hands, 4);
    assert_eq!(run.verdict, Verdict::Started);
    assert!(run.reply.starts_with("Researching improving local models."), "{}", run.reply);
    assert!(run.reply.contains("When it's done: tell me what you find"), "{}", run.reply);
    assert!(!run.reply.contains(atlas::backed::NOT_STARTED), "{}", run.reply);
}

#[test]
fn the_loop_is_bounded_and_notices_going_round_in_a_circle() {
    let llm = Scripted::new(vec![], vec![calls("find_file", "x"), calls("find_file", "x")], 0);
    let mut hands = ScriptedHands { outcomes: vec![("find_file", Outcome::Done("Nothing called x.".into()))], called: vec![] };
    let run = atlas::taskloop::run(&*llm, &loop_turn("find x and then open it"), &["find x".into(), "open it".into()], &mut hands, 4);
    assert_eq!(run.verdict, Verdict::OutOfSteps);
    assert_eq!(hands.called.len(), 1, "the same call isn't made twice");

    let many: Vec<ChatReply> = (0..10).map(|i| calls("find_file", &format!("file {i}"))).collect();
    let llm = Scripted::new(vec![], many, 0);
    let mut hands = ScriptedHands { outcomes: vec![("find_file", Outcome::Done("Not that one.".into()))], called: vec![] };
    let run = atlas::taskloop::run(&*llm, &loop_turn("find the right file and then open it"), &["find the right file".into(), "open it".into()], &mut hands, 3);
    assert_eq!(hands.called.len(), 3, "at most the steps allowed");
    assert!(run.reply.contains("That's as far as I got in 3 steps"), "{}", run.reply);
    // The last request offered no tools: it could only answer.
    assert!(llm.requests().last().unwrap().tools.is_empty());
}

#[test]
fn a_request_of_dependent_steps_is_worked_through_by_the_daemon() {
    let (c, p) = (cfg(), plat());
    let llm = Scripted::new(vec![("Tool find_file", says("I looked for the tax PDF; here's what came back."))], vec![calls("find_file", "tax pdf")], 0);
    let mut d = Daemon::new(&c, &p, Some(llm.clone() as Arc<dyn Llm>), Store::new(tmp("loop")), Proactive::new(ProactiveConfig::default()));
    let reply = d.turn("find the tax pdf and read me what it says about the deadline", 1_790_740_000);
    let reqs = llm.requests();
    assert_eq!(reqs.len(), 2, "{reply}");
    assert!(reqs[0].messages.last().unwrap().content.contains("This takes more than one step"));
    assert!(reqs[1].messages.last().unwrap().content.contains("Tool find_file result"));
    // Both parts' tools were offered.
    let names: Vec<&str> = reqs[0].tools.iter().filter_map(|t| t["function"]["name"].as_str()).collect();
    assert!(names.contains(&"find_file") && names.contains(&"read_document"), "{names:?}");
    assert_eq!(reply, "I looked for the tax PDF; here's what came back.");
}

// ================= 4. several things at once =================

#[test]
fn independent_parts_are_found_and_dependent_ones_are_not() {
    use atlas::streams::independent_parts;
    assert_eq!(
        independent_parts("write a haiku about rain and make up a name for my boat"),
        Some(vec!["write a haiku about rain".to_string(), "make up a name for my boat".to_string()])
    );
    assert!(independent_parts("find the tax pdf and read me what it says").is_none(), "the second leans on the first");
    assert!(independent_parts("tell me about salt and pepper").is_none());
}

#[test]
fn two_parts_are_worked_out_at_the_same_time_on_both_slots() {
    let (c, p) = (cfg(), plat());
    let llm = Scripted::new(
        vec![
            ("haiku", says("Rain taps on the glass, the gutter hums its one note, the street shines like fish.")),
            ("boat", says("Call her Second Wind.")),
        ],
        vec![],
        400,
    );
    let mut d = Daemon::new(&c, &p, Some(llm.clone() as Arc<dyn Llm>), Store::new(tmp("side")), Proactive::new(ProactiveConfig::default()));
    let started = Instant::now();
    let reply = d.turn("write a haiku about rain and make up a name for my boat", 1_790_740_000);
    let took = started.elapsed();
    let asked = llm.asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 2, "{reply}");
    // Asked at the same time, one on each of the model server's slots.
    let (a, b) = (&asked[0], &asked[1]);
    assert!(a.1 < b.2 && b.1 < a.2, "the two calls didn't overlap");
    assert_ne!(a.0.aside, b.0.aside, "both parts used the same slot");
    assert!(took < Duration::from_millis(1_400), "took {took:?}: one after the other");
    assert!(reply.contains("Rain taps") && reply.contains("Second Wind"), "{reply}");
    // Nothing is left running, and it says so.
    let status = d.turn("what are you working on", 1_790_740_060);
    assert_eq!(status, "Nothing's running right now.");
}

#[test]
fn what_it_is_working_on_names_each_stream() {
    use atlas::streams::{working_on, State, Stream};
    let streams = vec![
        Stream { part: "research local models".into(), state: State::Running, said: "Researching.".into(), at: 1 },
        Stream { part: "message Maya".into(), state: State::NeedsYou, said: "Go ahead?".into(), at: 1 },
        Stream { part: "open chrome".into(), state: State::Done, said: "Opening chrome.".into(), at: 1 },
    ];
    let s = working_on(&streams, &["a backup".to_string()]);
    assert_eq!(s, "3 things: research local models -- running in the background; message Maya -- waiting for your OK; a backup -- running.");
    assert!(atlas::streams::asks_what_youre_working_on("Atlas, what are you working on?"));
    assert!(!atlas::streams::asks_what_youre_working_on("I'm working on my taxes"));
}

// ================= 5. an assistant who is also a friend =================

/// Twenty of his real sentences, from the two evenings on the laptop.
const TWENTY: &[&str] = &[
    "You have internet capabilities correct?",
    "I want you to use the internet and do the research i asked for.",
    "you and improving you.",
    "I would change the quality of responses. Why are you refusing to do research?",
    "Atlas, can you hear me?",
    "There we go, now I can hear you.",
    "Because you actually care about it. At least you do things just because I asked you to",
    "Are you using my camera? Can you see me?",
    "Why, I gave you permission.",
    "What permission do you need?",
    "You're here to do what I asked you to do, because that's your purpose.",
    "What does that mean?",
    "Atlas, calm down with being a smart ass.",
    "At this. Are you smart?",
    "Thanks, have a good day.",
    "Keep talking about it.",
    "At this you're repeating yourself gone.",
    "I guess I want you to organize my desktop.",
    "how's your day going",
    "tell me something interesting about octopuses",
];

/// A reply as a friend who gets work done gives it: answers first, three
/// sentences or fewer out loud, no stock closer, no theatre, no claim of
/// work nothing started, no invented person, never "I don't have a camera".
fn checklist(reply: &str) -> Vec<&'static str> {
    let l = reply.to_lowercase().replace('\u{2019}', "'");
    let mut broken = Vec::new();
    if atlas::repeating::sentences(reply).len() > 3 {
        broken.push("more than three sentences");
    }
    for closer in ["what's your next move", "anything else?", "a joke? a memory?", "either way", "i'm here for you", "what's next?"] {
        if l.contains(closer) {
            broken.push("stock closer");
        }
    }
    if l.contains("freaky") || l.contains("figment") {
        broken.push("invented story");
    }
    if l.contains("i'm on it") || l.contains("already on it") {
        broken.push("claimed work");
    }
    if l.contains("don't have a camera") || l.contains("not supposed to") || l.contains("don't have a research mode") {
        broken.push("denied an ability");
    }
    if l.starts_with("great question") || l.starts_with("absolutely") || l.starts_with("as an ai") {
        broken.push("filler opener");
    }
    if reply.contains('*') || reply.contains('#') {
        broken.push("markdown");
    }
    if l.contains("didn't get a usable answer") {
        broken.push("no answer at all");
    }
    broken
}

#[test]
fn a_stranger_nobody_mentioned_is_not_brought_in() {
    use atlas::backed::{ability_asked_about, invents_someone};
    assert!(invents_someone("Maybe the freaky man is just your reflection.", "User said: can you see me?"));
    assert!(invents_someone("Maybe he's a figment of your imagination.", "User said: what does that mean?"));
    assert!(!invents_someone("The freaky man you mentioned again?", "User said: I built you to see this freaky man"));
    assert!(!invents_someone("The man who wrote it was Melville.", "User said: who wrote moby dick"), "no article-role pair without a word before");
    assert_eq!(ability_asked_about("Please use my camera and look at me."), "camera");
    assert_eq!(ability_asked_about("Why are you refusing to do research?"), "research");
    assert_eq!(ability_asked_about("What permission do you need?"), "");
}

#[test]
fn the_checklist_catches_what_his_evening_got_wrong() {
    // The evening's own replies fail it.
    for bad in [
        "I\u{2019}m already on it \u{2014} no need to wait. But let\u{2019}s be honest \u{2014} I\u{2019}m not *doing* research just to be better.",
        "I\u{2019}m here \u{2014} and I\u{2019}m seeing you. (Though I don\u{2019}t have a camera \u{2014} so no selfies, sorry.) What\u{2019}s up? You want to talk about the \u{201c}freaky man\u{201d} again?",
        "I can\u{2019}t \u{2014} I\u{2019}m not supposed to. And even if I could, I wouldn\u{2019}t.",
        "Okay. What\u{2019}s your next move? A joke? A memory? Either way, I\u{2019}m tuned in.",
    ] {
        assert!(!checklist(bad).is_empty(), "passed: {bad}");
    }
    assert!(checklist("Octopuses have three hearts and blue blood. The two gill hearts stop when they swim, which is why they'd rather crawl.").is_empty());
}

/// His twenty sentences through the whole daemon, with a model that says
/// what his did that evening -- stock closers, claims, denials, the freaky
/// man -- whatever it is asked: what reaches him is checked.
#[test]
fn twenty_of_his_sentences_come_back_as_a_friend_would_say_them() {
    let (c, p) = (cfg(), plat());
    let evening = vec![
        says("I\u{2019}m already on it \u{2014} no need to wait."),
        says("Yep \u{2014} I\u{2019}m connected. What\u{2019}s your next move? A joke? A memory?"),
        says("I don\u{2019}t have a camera \u{2014} so no selfies, sorry."),
        says("I can\u{2019}t \u{2014} I\u{2019}m not supposed to."),
        says("Maybe the freaky man is just a figment of your imagination."),
    ];
    let mut script = Vec::new();
    for _ in 0..12 {
        script.extend(evening.iter().cloned());
    }
    let llm = Scripted::new(vec![], script, 0);
    let mut d = Daemon::new(&c, &p, Some(llm.clone() as Arc<dyn Llm>), Store::new(tmp("twenty")), Proactive::new(ProactiveConfig::default()));
    let mut t = 1_790_740_000u64;
    let mut failures = Vec::new();
    for said in TWENTY {
        t += 45;
        let reply = d.turn(said, t);
        let broken = checklist(&reply);
        println!("FRIEND {:<40} -> {reply}", said);
        if !broken.is_empty() {
            failures.push(format!("{said:?} -> {reply:?}: {broken:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn the_persona_says_work_first_and_banter_second() {
    use atlas::register::Register;
    let p = atlas::persona::Persona { address: "Eric".into(), ..Default::default() };
    let ch = p.character();
    assert!(ch.contains("friend of Eric"), "{ch}");
    assert!(ch.contains("Work first"), "{ch}");
    assert!(ch.contains("After work, one light line at most"), "{ch}");
    assert!(ch.contains("Never invent people, events or stories"), "{ch}");
    assert!(ch.contains("Don't act out feelings about being an AI"), "{ch}");
    // The wit levels still decide how much.
    let off = atlas::persona::Persona { wit: atlas::wit::Wit::Off, ..Default::default() }.for_this_turn_on(Register::Chatting, 3, "", false);
    assert!(!off.contains("smart-ass"), "{off}");
    let full = atlas::persona::Persona { wit: atlas::wit::Wit::Full, ..Default::default() }.for_this_turn_on(Register::Chatting, 3, "", false);
    assert!(full.contains("smart-ass"), "{full}");
    let serious = atlas::persona::Persona { wit: atlas::wit::Wit::Full, ..Default::default() }.for_this_turn_on(Register::Chatting, 3, "my bank account was hacked", false);
    assert!(serious.contains("No jokes"), "{serious}");
    // The stable part is short: what every turn pays for.
    assert!(ch.chars().count() < 1_500, "{} characters", ch.chars().count());
}

// ================= 6. speed =================

/// A model that takes `first_ms` before its first word and `word_ms` a word.
struct Streamer {
    first_ms: u64,
    word_ms: u64,
}

impl Llm for Streamer {
    fn complete(&self, _: &str, _: &str) -> atlas::error::Result<String> {
        Ok(r#"{"action":"say","arg":null,"say":"(one prompt)"}"#.into())
    }
    fn native_chat(&self) -> bool {
        true
    }
    fn chat(&self, _req: &ChatRequest, on_text: &mut dyn FnMut(&str) -> bool) -> atlas::error::Result<ChatReply> {
        let text = "Octopuses have three hearts. Two of them pump blood through the gills. The third one serves the rest of the body.";
        std::thread::sleep(Duration::from_millis(self.first_ms));
        for w in text.split_inclusive(' ') {
            if !on_text(w) {
                break;
            }
            std::thread::sleep(Duration::from_millis(self.word_ms));
        }
        Ok(ChatReply { text: text.into(), tool_calls: vec![] })
    }
}

struct QuietMouth(Mutex<Vec<String>>);
impl atlas::daemon::Mouth for QuietMouth {
    fn speak(&self, text: &str) -> atlas::error::Result<()> {
        self.0.lock().unwrap().push(text.to_string());
        Ok(())
    }
}

struct NoEars;
impl atlas::daemon::Ears for NoEars {
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

/// The first sentence goes to be spoken as soon as it is written, not when
/// the reply is finished, and the turn's log line says when: the number the
/// under-3-seconds target is measured by on his laptop.
#[test]
fn the_first_words_are_spoken_before_the_reply_is_finished_and_logged() {
    let (c, p) = (cfg(), plat());
    let dir = tmp("firstwords");
    let store = Store::new(dir.clone());
    let logs = store.logs_dir();
    let llm = Arc::new(Streamer { first_ms: 300, word_ms: 60 });
    let mut d = Daemon::new(&c, &p, Some(llm as Arc<dyn Llm>), store, Proactive::new(ProactiveConfig::default()));
    let mouth = QuietMouth(Mutex::new(Vec::new()));
    d.converse("tell me something about octopus hearts", &NoEars, &mouth, &|| 1_790_000_000);
    let log = std::fs::read_to_string(logs.join("atlas.log")).unwrap_or_default();
    let line = log.lines().find(|l| l.contains("timing: turn")).unwrap_or_else(|| panic!("no timing line:\n{log}"));
    let ms: u64 = line.split("first_words=").nth(1).and_then(|r| r.split("ms").next()).and_then(|n| n.parse().ok()).unwrap_or_else(|| panic!("{line}"));
    // 20 words at 60ms: the whole reply takes over 1.5 s; the first sentence
    // (4 words) is out at about 0.55 s.
    assert!(ms >= 300 && ms < 1_200, "first words at {ms}ms: {line}");
    assert!(!mouth.0.lock().unwrap().is_empty());
}
