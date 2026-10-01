//! What Atlas took from wshobson/agents (MIT, commit 4236bb91), and proof
//! each piece does something in Atlas rather than sitting in a document.
//!
//! Eric, 25 Sep 2026: "look at a very specific GitHub repo and see how we can
//! use it in our Atlas system." The repo is prompt content for coding
//! harnesses; nothing of it is installed or run. Four ideas were rebuilt into
//! Atlas's own code (THIRD_PARTY_NOTICES.md), and these tests hold them:
//!
//! - two council rooms: "should I build this?" and "is this safe?"
//! - replies written as you are read back for a chatbot's voice and blanks
//! - a research figure must be in the pages it came from
//! - the research brief is told to copy figures exactly and name sources

use atlas::brain::{Llm, LlmConfig};
use atlas::config::Config;
use atlas::council::{build_room, is_build_question, is_security_question, room_for, security_room};
use atlas::daemon::Daemon;
use atlas::draft::{blanks, critique, for_replies, DraftConfig, Fault};
use atlas::error::Result;
use atlas::intent::Intent;
use atlas::platform::mock::{Action, MockPlatform};
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::research::figures_not_in;
use atlas::store::Store;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-wsagents-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

// ---------------------------------------------------------------- council

#[test]
fn a_should_i_build_question_gets_the_build_room() {
    assert!(is_build_question("Should I build the hollow CLI as a paid tool?"));
    assert!(is_build_question("would anyone pay for a colony viewer"));
    assert!(!is_build_question("should I upgrade the RAM"));
    let (name, room) = room_for("Is it worth building a phone app for Atlas?").expect("a room");
    assert_eq!(name, "build");
    assert!(room.is_quorate() && room.covers_different_ground(), "a room that can't disagree");
    let ids: Vec<&str> = room.seats.iter().map(|s| s.id.as_str()).collect();
    for risk in ["demand", "first user", "money", "reach", "trust"] {
        assert!(ids.contains(&risk), "no seat for {risk}: {ids:?}");
    }
    assert!(room.seats.iter().filter(|s| s.tiebreak).count() == 1);
}

#[test]
fn an_is_it_safe_question_gets_the_security_room() {
    assert!(is_security_question("Is it safe to let the phone reach Atlas over Tailscale?"));
    assert!(is_security_question("could someone use a guest handover to get my passwords"));
    let (name, room) = room_for("is it safe to open the hub to my phone").expect("a room");
    assert_eq!(name, "security");
    assert!(room.is_quorate() && room.covers_different_ground());
    let ids: Vec<&str> = room.seats.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, vec!["impostor", "tamperer", "eavesdropper", "wrecker", "climber"], "STRIDE's five ways in");
    // Anything else keeps the general room.
    assert!(room_for("should we eat out tonight").is_none());
}

#[test]
fn every_seat_in_the_new_rooms_answers_blind() {
    for room in [build_room(), security_room()] {
        let prompts = room.blind_prompts("Should I build it?");
        assert_eq!(prompts.len(), room.seats.len());
        for (_, p) in &prompts {
            assert!(p.contains("Should I build it?") && p.contains("Do not hedge"));
            assert!(!p.contains("rest of the room"), "a blind prompt showed the others");
        }
    }
}

/// A model that answers every seat the same, and counts what it was asked.
struct Seats(Mutex<Vec<String>>);
impl Llm for Seats {
    fn complete(&self, _s: &str, user: &str) -> Result<String> {
        self.0.lock().unwrap().push(user.to_string());
        Ok("Against. Nobody has asked for it yet.".into())
    }
}

#[test]
fn asking_the_room_about_building_something_convenes_the_build_room() {
    let cfg: &'static Config = Box::leak(Box::new(Config::load(Path::new("config")).unwrap()));
    let p = plat();
    let seats = Arc::new(Seats(Mutex::new(Vec::new())));
    let mut d = Daemon::new(cfg, &p, Some(seats.clone()), Store::new(scratch("room")), Proactive::new(ProactiveConfig::default()));
    let said = d.ask_the_room("should I build the hollow CLI as a product?");
    assert!(!said.contains("hasn't enough seats"), "{said}");
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut t = atlas::store::now() + 50;
    while Instant::now() < deadline && seats.0.lock().unwrap().len() < 5 {
        let _ = d.tick(t);
        t += 1;
        std::thread::sleep(Duration::from_millis(10));
    }
    let asked = seats.0.lock().unwrap().clone();
    assert!(asked.len() >= 5, "only {} seats were asked", asked.len());
    assert!(asked.iter().any(|p| p.contains("urgently wants this")), "the demand seat wasn't asked: {asked:?}");
    assert!(asked.iter().any(|p| p.contains("who pays")), "the money seat wasn't asked");
}

// ---------------------------------------------------------------- writing as you

#[test]
fn a_chatbots_voice_and_a_blank_are_faults() {
    let cfg = DraftConfig::default();
    let bot = critique("Certainly! I hope this helps. Feel free to reach out.", None, &cfg);
    assert!(bot.iter().any(|n| n.fault == Fault::Chatbot), "{bot:?}");
    let blank = critique("Thanks, see you Friday. [Your Name]", None, &cfg);
    assert!(blank.iter().any(|n| n.fault == Fault::Blank && n.evidence.contains("[Your Name]")), "{blank:?}");
    assert_eq!(blanks("See [1] and [x] and [Company Name] and [date]"), vec!["[Company Name]", "[date]"]);
    // Plain human replies pass, including a question back and a "maybe".
    let fine = critique("Maybe Friday? I can do 3pm if that works for Sam.", None, &for_replies());
    assert!(fine.is_empty(), "a normal chat reply was faulted: {fine:?}");
}

/// Writes like a chatbot the first time, like a person when asked to fix it.
struct Chatty(Mutex<u32>);
impl Llm for Chatty {
    fn complete(&self, system: &str, _user: &str) -> Result<String> {
        let mut n = self.0.lock().unwrap();
        *n += 1;
        if system.starts_with("You are tightening") {
            Ok("Friday at 3 works. See you then.".into())
        } else {
            Ok("Certainly! Friday at 3 works. I hope this helps!".into())
        }
    }
}

fn show(p: &MockPlatform, id: u64, process: &str, text: &str) {
    p.focus_on(process, "");
    *p.front.borrow_mut() = Some(atlas::platform::WindowId(id));
    p.set_window_text(id, text);
}

fn typed(p: &MockPlatform) -> Vec<String> {
    p.actions().into_iter().filter_map(|a| match a { Action::Type(t) => Some(t), _ => None }).collect()
}

fn settle(d: &mut Daemon, from: u64) -> Vec<String> {
    let mut out = Vec::new();
    for k in 0..300u64 {
        out.extend(d.tick(from + k));
        if k > 0 && d.working_for_you.iter().all(|w| w.composing.is_none() && w.ready.is_none()) {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    out
}

#[test]
fn a_reply_written_as_you_is_rewritten_when_it_sounds_like_a_chatbot() {
    let cfg: &'static Config = Box::leak(Box::new(Config::load(Path::new("config")).unwrap()));
    let p = plat();
    let model = Arc::new(Chatty(Mutex::new(0)));
    let mut d = Daemon::new(cfg, &p, Some(model.clone()), Store::new(scratch("chatty")), Proactive::new(ProactiveConfig::default()));
    show(&p, 7, "SomeChat.exe", "Sam: does Friday at 3 work?");
    let _ = d.execute_timed(&Intent::Delegate("draft a reply to this".into()), "draft a reply to this");
    let _ = settle(&mut d, atlas::store::now() + 50);
    assert_eq!(typed(&p), vec!["Friday at 3 works. See you then.".to_string()]);
    assert_eq!(*model.0.lock().unwrap(), 2, "the reply and one rewrite, both asked");
    // Graded from what happened: the first draft needed fixing, the rewrite
    // was kept.
    let calls: Vec<_> = d.trace.calls.iter().filter(|c| c.asked_by == "conversation-reply").collect();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].graded, Some(false));
    assert!(calls[0].why.as_deref().unwrap_or("").len() > 3, "a bad grade says why: {:?}", calls[0].why);
    assert_eq!(calls[1].graded, Some(true));
}

/// Always leaves a blank.
struct Blanky;
impl Llm for Blanky {
    fn complete(&self, _s: &str, _u: &str) -> Result<String> {
        Ok("Sounds good, see you then. [Your Name]".into())
    }
}

#[test]
fn a_reply_with_a_blank_in_it_is_never_sent() {
    let cfg: &'static Config = Box::leak(Box::new(Config::load(Path::new("config")).unwrap()));
    let p = plat();
    let mut d = Daemon::new(cfg, &p, Some(Arc::new(Blanky)), Store::new(scratch("blank")), Proactive::new(ProactiveConfig::default()));
    show(&p, 7, "SomeChat.exe", "Sam: see you Friday?");
    let _ = d.execute_timed(&Intent::Delegate("take over this conversation".into()), "take over this conversation");
    let out = settle(&mut d, atlas::store::now() + 50);
    assert_eq!(typed(&p).len(), 1, "the draft should still be put in the box for you");
    assert!(!p.actions().iter().any(|a| matches!(a, Action::Press(k) if k == "enter")), "sent a reply with a blank in it");
    assert!(out.iter().any(|l| l.contains("blank to fill in") && l.contains("[Your Name]")), "{out:?}");
    assert!(d.working_for_you[0].held, "carried on after leaving a blank for you");
}

// ---------------------------------------------------------------- research

#[test]
fn a_figure_must_be_in_the_pages_it_came_from() {
    let src = "Revenue was 1,200 in 2025, up 15% on the year. Margin 3.5.";
    assert!(figures_not_in("Revenue of 1200 in 2025 grew 15%, margin 3.5.", src).is_empty());
    assert_eq!(figures_not_in("It grew 150% last year.", src), vec!["150%"]);
    assert_eq!(figures_not_in("Margin was 3.8.", src), vec!["3.8"]);
    // Not claims: single digits, names like B2 or v3, 4B models.
    assert!(figures_not_in("Read 2 sources on the B2 plan, v3, and a 4B model.", src).is_empty());
    // A figure is matched whole: 15 isn't found inside 150.
    assert_eq!(figures_not_in("up 15 points", "grew by 150 points"), vec!["15"]);
}

/// Writes a brief with one made-up figure.
struct Inventive;
impl Llm for Inventive {
    fn complete(&self, system: &str, _user: &str) -> Result<String> {
        if system.contains("research brief") && !system.contains("Copy every number") {
            return Ok("The brief wasn't told to copy figures exactly.".into());
        }
        Ok("Ventura's high tide peaks at 12:40 and reaches 5.9 feet. Wear sandals.".into())
    }
}

#[test]
fn research_says_a_made_up_figure_is_unconfirmed_and_files_it() {
    let dir = scratch("research");
    let mut c = Config::load(Path::new("config")).unwrap();
    {
        let tools = c.tools.as_mut().unwrap();
        tools.research.enabled = true;
        tools.research.notes_dir = dir.join("notes").display().to_string();
        tools.research.search = Some(atlas::tools::ExternalTool {
            command: "sh".into(),
            args: vec!["-c".into(), "echo 'http://example.test/tides'".into()],
            ..Default::default()
        });
        tools.research.fetch = Some(atlas::tools::ExternalTool {
            command: "sh".into(),
            args: vec!["-c".into(), format!("echo '{}'", "Ventura high tide peaks at 12:40 today. ".repeat(10))],
            ..Default::default()
        });
        tools.llm = Some(LlmConfig {
            tool: Default::default(),
            request: r#"{"model":"stub","prompt":"{user}"}"#.into(),
            response_path: "response".into(),
            vision_request: None,
        });
    }
    let c: &'static Config = Box::leak(Box::new(c));
    let p = plat();
    let mut d = Daemon::new(c, &p, Some(Arc::new(Inventive)), Store::new(scratch("research-store")), Proactive::new(ProactiveConfig::default()));
    d.connectivity.set(atlas::connectivity::Reach::Online, 0);
    let _ = d.turn("research tide times at Ventura", 100);
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut said = Vec::new();
    let mut t = 101;
    while Instant::now() < deadline && !said.iter().any(|l: &String| l.contains("5.9 feet")) {
        said.extend(d.tick(t));
        t += 1;
        std::thread::sleep(Duration::from_millis(10));
    }
    let answer = said.iter().find(|l| l.contains("5.9 feet")).unwrap_or_else(|| panic!("no answer: {said:?}"));
    assert!(answer.contains("5.9 isn't in any of the pages I read"), "{answer}");
    // The write-ups; the `.last-research` mark beside them isn't a note.
    let notes: Vec<_> = std::fs::read_dir(dir.join("notes")).unwrap().flatten().filter(|e| e.path().extension().is_some_and(|x| x == "md")).collect();
    assert_eq!(notes.len(), 1, "one note per research");
    let note = &notes[0];
    let text = std::fs::read_to_string(note.path()).unwrap();
    assert!(text.contains("## Figures not found in the sources") && text.contains("- 5.9"), "{text}");
    assert!(!text.contains("- 12:40") && !text.contains("- 12\n"), "a figure that is in the source was flagged: {text}");
    let graded = d.trace.calls.iter().rev().find(|c| c.graded.is_some()).expect("the research call wasn't graded");
    assert_eq!(graded.graded, Some(false));
    assert_eq!(graded.why.as_deref(), Some("stated a figure that isn't in its sources"));
}
