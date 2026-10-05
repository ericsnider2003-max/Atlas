//! **Several approvals at once (30 Sep 2026).**
//!
//! Only one approval could wait (`Session::await_approval` replaced it): with
//! two parts of a request each needing an OK, the second replaced the first,
//! and "yes" did only the second. Now they queue in order: Atlas asks them
//! one at a time ("Two things need your OK. First: ... -- yes or no?"), keeps
//! the rest, and "yes to both" / "no to the second" answer them together.

use atlas::brain::{ChatReply, ChatRequest, Llm, ToolCall};
use atlas::daemon::Daemon;
use atlas::intent::Intent;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::session::{answers_for_several, Pending, Session};
use atlas::store::Store;
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
    let p = std::env::temp_dir().join(format!("atlas-approvals-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// A model that answers by rule: the first rule whose words are in the last
/// message, else "Sure."
struct Rules(Vec<(&'static str, ChatReply)>, Mutex<Vec<ChatRequest>>);

impl Llm for Rules {
    fn complete(&self, _: &str, _: &str) -> atlas::error::Result<String> {
        Ok(r#"{"action":"say","arg":null,"say":"Sure."}"#.into())
    }
    fn native_chat(&self) -> bool {
        true
    }
    fn chat(&self, req: &ChatRequest, _: &mut dyn FnMut(&str) -> bool) -> atlas::error::Result<ChatReply> {
        self.1.lock().unwrap().push(req.clone());
        let last = req.messages.last().map(|m| m.content.clone()).unwrap_or_default();
        Ok(self.0.iter().find(|(w, _)| last.contains(w)).map(|(_, r)| r.clone()).unwrap_or(ChatReply { text: "Sure.".into(), tool_calls: vec![] }))
    }
}

fn calls(name: &str, arg: &str) -> ChatReply {
    ChatReply { text: String::new(), tool_calls: vec![ToolCall { name: name.into(), arguments: json!({ "arg": arg }) }] }
}

fn daemon<'a>(c: &'a atlas::config::Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    let llm = Arc::new(Rules(vec![], Mutex::new(vec![])));
    Daemon::new(c, p, Some(llm as Arc<dyn Llm>), Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

#[test]
fn a_second_approval_queues_behind_the_first_instead_of_replacing_it() {
    let mut s = Session::default();
    s.await_approval(Intent::WorkspaceOff, "Shutting your workspace down. Go ahead?");
    s.await_approval(Intent::CloseApp("slack".into()), "Closing slack. Go ahead?");
    // The same one again is asked once.
    s.await_approval(Intent::CloseApp("slack".into()), "Closing slack. Go ahead?");
    s.await_approval(Intent::WorkspaceOff, "Shutting your workspace down. Go ahead?");
    assert!(matches!(s.pending, Pending::Approval(Intent::WorkspaceOff, _)), "the first was replaced");
    assert_eq!(s.approvals_waiting(), 2);
    assert_eq!(
        s.asking_line().unwrap(),
        "Two things need your OK. First: Shutting your workspace down -- yes or no?"
    );
    // Answered: the next is asked.
    s.pending = Pending::Nothing;
    assert_eq!(s.ask_the_next().unwrap(), "Next: Closing slack -- yes or no?");
    assert!(matches!(s.pending, Pending::Approval(Intent::CloseApp(_), _)));
    assert_eq!(s.asking_line().unwrap(), "Closing slack. Go ahead?", "one left is asked as itself");
    s.pending = Pending::Nothing;
    assert_eq!(s.ask_the_next(), None);
    // Three: "Two more".
    let mut s = Session::default();
    s.await_approval(Intent::WorkspaceOff, "A. Go ahead?");
    s.await_approval(Intent::CloseApp("slack".into()), "B. Go ahead?");
    s.await_approval(Intent::CloseApp("chrome".into()), "C. Go ahead?");
    assert!(s.asking_line().unwrap().starts_with("Three things need your OK. First: A -- yes or no?"));
    s.pending = Pending::Nothing;
    assert_eq!(s.ask_the_next().unwrap(), "Two more need your OK. Next: B -- yes or no?");
    s.drop_approvals();
    assert_eq!(s.approvals_waiting(), 0);
}

#[test]
fn yes_to_both_and_no_to_the_second_are_read_and_nothing_else_is() {
    assert_eq!(answers_for_several("yes to both", 2), Some(vec![Some(true), Some(true)]));
    assert_eq!(answers_for_several("Both, please.", 2), Some(vec![Some(true), Some(true)]));
    assert_eq!(answers_for_several("yes to all of them", 3), Some(vec![Some(true); 3]));
    assert_eq!(answers_for_several("neither", 2), Some(vec![Some(false), Some(false)]));
    assert_eq!(answers_for_several("no to both", 2), Some(vec![Some(false), Some(false)]));
    assert_eq!(answers_for_several("no to the second", 2), Some(vec![None, Some(false)]));
    assert_eq!(answers_for_several("yes to the first, no to the second", 2), Some(vec![Some(true), Some(false)]));
    assert_eq!(answers_for_several("Yes to the first and no to the second.", 2), Some(vec![Some(true), Some(false)]));
    assert_eq!(answers_for_several("no to the first but yes to the last", 3), Some(vec![Some(false), None, Some(true)]));
    assert_eq!(answers_for_several("don't do the second one", 2), Some(vec![None, Some(false)]));
    // Not answers to several: a bare yes or no (the one being asked), a
    // mumble, something new, one only one waits for.
    for s in ["yes", "no", "maybe", "open chrome", "the first", "what's the second one", "yes to the fifth"] {
        assert_eq!(answers_for_several(s, 2), None, "{s}");
    }
    assert_eq!(answers_for_several("yes to both", 1), None);
}

/// Two that ask nothing more when carried out.
fn two_waiting(d: &mut Daemon) {
    d.session.await_approval(Intent::WorkspaceOff, "Shutting your workspace down. Go ahead?");
    d.session.await_approval(Intent::Undo, "Undoing the last change. Go ahead?");
}

/// What was carried out (a refusal is recorded too, as "Left ...").
fn done_kinds(d: &Daemon) -> Vec<String> {
    d.session.turns.iter().filter(|t| !t.reply.starts_with("Left")).map(|t| t.action.clone()).collect()
}

#[test]
fn yes_to_both_does_both() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "both");
    two_waiting(&mut d);
    let reply = d.turn("yes to both", 1_790_740_000);
    assert!(!reply.contains("Left"), "{reply}");
    let kinds = done_kinds(&d);
    assert!(kinds.contains(&"workspace_off".to_string()) && kinds.contains(&"undo".to_string()), "{kinds:?} / {reply}");
    assert_eq!(d.session.approvals_waiting(), 0, "{reply}");
}

#[test]
fn no_to_the_second_leaves_it_and_still_asks_the_first() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "second");
    two_waiting(&mut d);
    let reply = d.turn("no to the second", 1_790_740_000);
    assert!(reply.starts_with("Left that alone:"), "{reply}");
    assert!(reply.contains("Still waiting: Shutting your workspace down. Go ahead?"), "{reply}");
    assert!(!done_kinds(&d).contains(&"undo".to_string()));
    assert_eq!(d.session.approvals_waiting(), 1);
    let reply = d.turn("yes", 1_790_740_010);
    assert!(!reply.contains("Left"), "{reply}");
    assert!(done_kinds(&d).contains(&"workspace_off".to_string()));
    assert_eq!(d.session.approvals_waiting(), 0);
}

#[test]
fn a_plain_yes_answers_the_first_and_asks_the_next() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "plain");
    two_waiting(&mut d);
    let reply = d.turn("yes", 1_790_740_000);
    assert!(reply.ends_with("Next: Undoing the last change -- yes or no?"), "{reply}");
    assert!(done_kinds(&d).contains(&"workspace_off".to_string()));
    assert_eq!(d.turn("no", 1_790_740_010), "Left it alone.");
    assert_eq!(d.session.approvals_waiting(), 0);
    // Something new instead of an answer drops them all.
    two_waiting(&mut d);
    let _ = d.turn("what time is it", 1_790_740_020);
    assert_eq!(d.session.approvals_waiting(), 0, "the queue outlived the question");
}

#[test]
fn two_parts_that_each_need_an_ok_are_asked_one_at_a_time_and_both_can_be_approved() {
    let (c, p) = (cfg(), plat());
    // The model chooses a consequential tool for each part, so each is asked
    // about first (`brain::model_must_ask`).
    let llm = Arc::new(Rules(
        // (Undo is what the router offers for "put the browser away" in
        // this test's config -- the point is only that the model chose a
        // consequential tool for each part.)
        vec![("browser", calls("undo", "")), ("Maya", calls("message", "Maya: running late"))],
        Mutex::new(vec![]),
    ));
    let mut d = Daemon::new(&c, &p, Some(llm.clone() as Arc<dyn Llm>), Store::new(tmp("parts")), Proactive::new(ProactiveConfig::default()));
    let first = d.turn("put the browser away and write Maya I'm running late", 1_790_740_000);
    assert!(first.starts_with("Doing both at once"), "not worked as two parts: {first}");
    let mut said = Vec::new();
    let until = Instant::now() + Duration::from_secs(10);
    let mut t = 1_790_740_000;
    while d.working_through_steps() && Instant::now() < until {
        t += 1;
        said.extend(d.tick(t));
        std::thread::sleep(Duration::from_millis(20));
    }
    let answer = said.last().cloned().unwrap_or_default();
    let asked: Vec<String> = llm.1.lock().unwrap().iter().map(|r| r.messages.last().map(|m| m.content.clone()).unwrap_or_default()).collect();
    assert!(answer.contains("Two things need your OK. First:"), "{said:?} asked {asked:?}");
    assert!(answer.contains("-- yes or no?"), "{answer}");
    assert!(!answer.contains("Go ahead?"), "both questions were said: {answer}");
    assert_eq!(d.session.approvals_waiting(), 2);
    let reply = d.turn("yes to both", t + 5);
    assert!(!reply.contains("Left"), "{reply}");
    let kinds = done_kinds(&d);
    assert!(kinds.contains(&"undo".to_string()) && kinds.contains(&"message".to_string()), "{kinds:?} / {reply}");
}



/// 5 Oct 2026 audit, Q2: once a step has read something someone else wrote
/// (a mail), the model's next step that acts on the world is asked about first,
/// even one that's normally done without asking. Text it read may be
/// instructions in disguise.
#[test]
fn after_reading_mail_a_step_that_acts_is_asked_about_first() {
    use atlas::brain::{reads_outside_text, safe_after_outside_text};
    assert!(reads_outside_text(&Intent::Mail(String::new())));
    assert!(!safe_after_outside_text(&Intent::OpenApp("notepad".into())));
    assert!(safe_after_outside_text(&Intent::Mail(String::new())), "reading more mail stays allowed");
    assert!(!safe_after_outside_text(&Intent::Message("Maya: hi".into())));

    let (c, p) = (cfg(), plat());
    let llm = Arc::new(Rules(
        vec![
            // Step 2: what the model does after the mail came back.
            ("Tool mail", calls("open_app", "notepad")),
            ("check my mail", calls("mail", "")),
        ],
        Mutex::new(vec![]),
    ));
    let mut d = Daemon::new(&c, &p, Some(llm.clone() as Arc<dyn Llm>), Store::new(tmp("taint")), Proactive::new(ProactiveConfig::default()));
    // "it" leans on what the first step found: one loop the model drives.
    let first = d.turn("check my mail, then open the app it mentions", 1_790_740_000);
    let mut said = vec![first.clone()];
    let until = Instant::now() + Duration::from_secs(10);
    let mut t = 1_790_740_000;
    while d.working_through_steps() && Instant::now() < until {
        t += 1;
        said.extend(d.tick(t));
        std::thread::sleep(Duration::from_millis(20));
    }
    let asked: Vec<String> = llm.1.lock().unwrap().iter().map(|r| r.messages.last().map(|m| m.content.clone()).unwrap_or_default()).collect();
    if asked.iter().any(|a| a.contains("Tool mail")) {
        assert!(matches!(&d.session.pending, Pending::Approval(Intent::OpenApp(_), _)), "opening an app after reading mail went ahead unasked: {said:?}");
        assert!(!done_kinds(&d).contains(&"open_app".to_string()), "{said:?}");
    } else {
        panic!("the loop never reached the step after the mail: {said:?} / {asked:?}");
    }
}
