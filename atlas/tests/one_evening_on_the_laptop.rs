//! **One evening on the laptop, replayed.**
//!
//! Eric, 29 Sep 2026, on his own laptop (Qwen3-VL 4B through llama.cpp on
//! the Arc graphics): Atlas answered almost everything with the same reply
//! -- "if you want me to stop, I'll stop ... What's your next move? A joke?
//! A memory? ... Either way, I'm tuned in" -- talked about the Discord
//! server on his screen instead of what he said, didn't do what he asked
//! ("organize my desktop", "look at my screen", "do a diagnosis on
//! yourself", "what still needs to be set up", "use my webcam mic"), said
//! "Paused." after nearly every reply, and took 26-40 seconds a turn.
//!
//! `tests/fixtures/one_evening/thread.json` is that evening's thread as
//! Atlas stored it, with two things changed so it can ship: the Discord
//! server's name, and the calendar reminders in the summary, made neutral.
//! Everything else -- every reply, the polluted summary -- is as it was.

use atlas::brain::{ChatReply, ChatRequest, Llm, Role};
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::{ActiveWindow, Monitor};
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use atlas::thread::{Exchange, Thread};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-one-evening-{tag}-{}", std::process::id()));
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

/// The evening's thread, as stored.
fn evening() -> Thread {
    let text = std::fs::read_to_string("tests/fixtures/one_evening/thread.json").unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    serde_json::from_value(v["data"].clone()).unwrap()
}

/// A model that answers each sentence with what the real one answered that
/// evening, and records every request.
struct Replay {
    replies: Vec<(String, String)>,
    asked: Mutex<Vec<ChatRequest>>,
}

impl Replay {
    fn of(t: &Thread) -> Arc<Replay> {
        Arc::new(Replay {
            replies: t.recent.iter().map(|e| (e.said.clone(), e.reply.clone())).collect(),
            asked: Mutex::new(Vec::new()),
        })
    }
}

impl Llm for Replay {
    fn complete(&self, _: &str, _: &str) -> atlas::error::Result<String> {
        Ok(r#"{"action":"say","arg":null,"say":"(the one-prompt path)"}"#.into())
    }
    fn native_chat(&self) -> bool {
        true
    }
    fn chat(&self, req: &ChatRequest, on_text: &mut dyn FnMut(&str) -> bool) -> atlas::error::Result<ChatReply> {
        self.asked.lock().unwrap().push(req.clone());
        let last = req.messages.last().map(|m| m.content.clone()).unwrap_or_default();
        let text = self
            .replies
            .iter()
            .find(|(said, _)| last.ends_with(said.as_str()))
            .map(|(_, r)| r.clone())
            .unwrap_or_else(|| "Sure.".into());
        for w in text.split_inclusive(' ') {
            if !on_text(w) {
                break;
            }
        }
        Ok(ChatReply { text, tool_calls: vec![] })
    }
}

/// Characters in a request: every message, and every tool as sent.
fn request_chars(r: &ChatRequest) -> (usize, usize) {
    let msgs: usize = r.messages.iter().map(|m| m.content.chars().count()).sum();
    let tools: usize = r.tools.iter().map(|t| t.to_string().chars().count()).sum();
    (msgs, tools)
}

/// Every sentence of the evening through the prompt builder, with the
/// evening's own replies, the Discord window in front the whole time. The
/// sizes are printed (`--nocapture`) for the before/after in the report.
#[test]
fn the_evening_replayed_through_the_prompt_builder() {
    let (c, p) = (cfg(), plat());
    *p.active.borrow_mut() = Some(ActiveWindow { process: "Discord.exe".into(), title: "#general | Chaos Crew - Discord".into() });
    let t = evening();
    let dir = tmp("replay");
    let store = Store::new(dir.clone());
    // The thread as it stood before the first of these sentences: the
    // summary and what was folded, nothing recent.
    let start = Thread { summary: t.summary.clone(), recent: Vec::new(), folded: t.folded, last_active: t.recent[0].at, current_topic: None };
    store.save("thread", &start).unwrap();
    let spy = Replay::of(&t);
    let mut d = Daemon::new(&c, &p, Some(spy.clone()), Store::new(dir), Proactive::new(ProactiveConfig::default()));
    let mut rows = Vec::new();
    for e in &t.recent {
        let before = spy.asked.lock().unwrap().len();
        let _ = d.turn(&e.said, e.at);
        let asked = spy.asked.lock().unwrap();
        if asked.len() > before {
            let r = &asked[before];
            // The window's name only for a sentence about the screen.
            // (This turn's part: the evening's own replies, in the history,
            // talk about the server by name.)
            if !atlas::doing::refers_to_screen(&e.said, "Discord") {
                let this_turn = &r.messages.last().unwrap().content;
                assert!(!this_turn.contains("Chaos Crew"), "the Discord window went in for {:?}", e.said);
            }
            let (m, tl) = request_chars(r);
            rows.push((e.said.clone(), m, tl, r.tools.len(), asked.len() - before));
        } else {
            rows.push((e.said.clone(), 0, 0, 0, 0));
        }
    }
    let asked: Vec<&(String, usize, usize, usize, usize)> = rows.iter().filter(|r| r.4 > 0).collect();
    let n = asked.len().max(1);
    let avg_msgs = asked.iter().map(|r| r.1).sum::<usize>() / n;
    let avg_tools = asked.iter().map(|r| r.2).sum::<usize>() / n;
    for r in &rows {
        println!("REPLAY chars msgs={:>6} tools={:>6} ({:>2} tools) calls={} | {}", r.1, r.2, r.3, r.4, r.0);
    }
    println!(
        "REPLAY {} of {} sentences reached the model; average {} chars of messages + {} of tools = about {} tokens",
        asked.len(),
        rows.len(),
        avg_msgs,
        avg_tools,
        (avg_msgs + avg_tools) * 2 / 7
    );
    if let Some(last) = spy.asked.lock().unwrap().last() {
        let n = last.messages.len();
        let hist: usize = last.messages[1..n - 1].iter().map(|m| m.content.chars().count()).sum();
        println!(
            "REPLAY last request: system {} chars, {} history messages {} chars, this turn {} chars",
            last.messages[0].content.chars().count(),
            n - 2,
            hist,
            last.messages[n - 1].content.chars().count()
        );
    }
    // After 29 Sep 2026 -- the evening's fixes -- a sentence that asks for
    // one of Atlas's own commands never reaches the model, and the prompt
    // the rest get is smaller: past replies cut to their first sentences,
    // and the window's name only when the screen was mentioned. Before:
    // all 23 reached the model, 6,168 characters of messages on average
    // (the tools were 4,263: the same 18 schemas, unchanged).
    assert!(asked.len() + 8 <= rows.len(), "the commands the evening asked for still went to the model: {} of {}", asked.len(), rows.len());
    // 30 Sep 2026 (merge): 5,000 -> 5,800. The other chat's fix for the
    // model making things up about itself (`persona::who_and_what`: whose
    // assistant Atlas is, its job, what it can do) put about 800 more
    // characters at the top of every request -- fixed text, the same every
    // turn, so the model server reads it once and reuses it. Measured after
    // the merge: 5,656. The evening's own fixes are still what this holds:
    // before them the average was 6,168 with the shorter character.
    assert!(avg_msgs < 5_800, "the messages are still {avg_msgs} characters on average");
    for r in spy.asked.lock().unwrap().iter() {
        // The conversation as shown (the summary and the turns), not the
        // character, which names the closers it forbids.
        let shown: String = r.messages[0].content.split("Earlier:").nth(1).unwrap_or("").to_string()
            + &r.messages[1..].iter().map(|m| m.content.as_str()).collect::<Vec<_>>().join("\n");
        assert!(!shown.contains("next move") && !shown.contains("tuned in"), "a stock closer went back to the model: {shown}");
    }
}

// ================= 1. saying the same thing again =================

fn the_replies(t: &Thread) -> Vec<String> {
    t.recent.iter().map(|e| e.reply.clone()).collect()
}

/// The evening's commonest reply, said again after four of its cousins: the
/// sentences they already said, and the closers, are not said again.
#[test]
fn a_reply_that_says_the_same_again_is_cut_to_what_is_new() {
    let t = evening();
    let replies = the_replies(&t);
    let earlier: Vec<&str> = replies[3..7].iter().map(|s| s.as_str()).collect();
    let mut f = atlas::repeating::SentenceFilter::new(&earlier);
    let new = "Okay, I\u{2019}m here. You want me to organize your desktop? But hey, if you want me to *stop*, I\u{2019}ll stop. \
               If you want me to *try*, I\u{2019}ll try. Either way, I\u{2019}m here. What\u{2019}s your next move? A joke? A memory?";
    let kept: Vec<String> = atlas::repeating::sentences(new).into_iter().filter(|s| f.pass(s)).collect();
    assert_eq!(kept, vec!["Okay, I\u{2019}m here.".to_string(), "You want me to organize your desktop?".to_string()], "{kept:?}");
    assert!(f.looping(), "most of it was the loop");
    // Every closer of the evening, on its own.
    for c in [
        "What\u{2019}s your next move?",
        "A joke?",
        "A memory?",
        "Either way, I\u{2019}m tuned in.",
        "Or maybe you\u{2019}re testing if I can still hear you when you\u{2019}re not talking?",
    ] {
        assert!(atlas::repeating::is_stock_closer(c), "{c}");
    }
    // An ordinary answer is not a closer, and not a repeat.
    assert!(!atlas::repeating::is_stock_closer("I'm here."));
    assert!(!atlas::repeating::said_before("Paris is the capital of France.", &earlier));
    // And the whole replies are near copies of each other.
    assert!(atlas::repeating::near_copy(&replies[5], &replies[6]), "{} / {}", replies[5], replies[6]);
    assert!(!atlas::repeating::near_copy(&replies[4], &replies[16]));
}

/// A model that says `first` the first time and `then` after -- recording
/// every request.
struct Loops {
    asked: Mutex<Vec<ChatRequest>>,
    first: String,
    then: String,
}

impl Llm for Loops {
    fn complete(&self, _: &str, _: &str) -> atlas::error::Result<String> {
        Ok(r#"{"action":"say","arg":null,"say":"(the one-prompt path)"}"#.into())
    }
    fn native_chat(&self) -> bool {
        true
    }
    fn chat(&self, req: &ChatRequest, on_text: &mut dyn FnMut(&str) -> bool) -> atlas::error::Result<ChatReply> {
        let n = {
            let mut a = self.asked.lock().unwrap();
            a.push(req.clone());
            a.len()
        };
        let text = if n == 1 { self.first.clone() } else { self.then.clone() };
        for w in text.split_inclusive(' ') {
            if !on_text(w) {
                break;
            }
        }
        Ok(ChatReply { text, tool_calls: vec![] })
    }
}

#[test]
fn a_reply_that_opens_with_the_loop_is_asked_again_harder_and_never_said() {
    let t = evening();
    let replies = the_replies(&t);
    let c = cfg();
    let parser = atlas::intent::Parser::new(&c.commands);
    let persona = atlas::persona::Persona::default();
    let llm = Loops {
        asked: Mutex::new(Vec::new()),
        // "You're not wrong -- I'm here. But hey, ..." after its cousins.
        first: replies[7].clone(),
        then: "I can't reach your desktop's files yet. Should I turn on System changes?".into(),
    };
    let brain = atlas::brain::Brain { llm: &llm, fallback: &parser, voice: Some((&persona, atlas::register::Register::Chatting)) };
    let turn = atlas::brain::Turn {
        said: "Use the engine now. Understand. Stay.".into(),
        system: "S".into(),
        tools: atlas::intent::ToolBook::new(&c.commands).for_sentence("", 0),
        max_tokens: 200,
        skip_phrases: true,
        recent_replies: Some(replies[4..7].to_vec()),
        ..Default::default()
    };
    let mut spoken = String::new();
    let d = brain.converse(&turn, &mut |w| {
        spoken.push_str(w);
        true
    });
    let asked = llm.asked.lock().unwrap();
    assert_eq!(asked.len(), 2, "not asked again");
    assert!(asked[1].stronger, "asked again with the same penalties");
    assert!(asked[1].tools.is_empty(), "asked again with tools");
    assert!(asked[1].messages.last().unwrap().content.contains(atlas::brain::ANSWER_AFRESH));
    assert!(!spoken.contains("next move") && !spoken.contains("Either way"), "the loop was said: {spoken}");
    assert!(spoken.contains("System changes"), "{spoken}");
    assert!(!d.say.contains("Either way"), "{}", d.say);
}

#[test]
fn a_reply_with_the_loop_after_a_new_opening_says_only_the_new_part() {
    let t = evening();
    let replies = the_replies(&t);
    let c = cfg();
    let parser = atlas::intent::Parser::new(&c.commands);
    let persona = atlas::persona::Persona::default();
    let llm = Loops {
        asked: Mutex::new(Vec::new()),
        first: "Filing the desktop needs System changes on. But hey, if you want me to *stop*, I\u{2019}ll stop. \
                If you want me to *try*, I\u{2019}ll try. What\u{2019}s your next move? A joke? A memory?"
            .into(),
        then: "Filing the desktop needs System changes on. Turn it on in Settings and I'll file them.".into(),
    };
    let brain = atlas::brain::Brain { llm: &llm, fallback: &parser, voice: Some((&persona, atlas::register::Register::Chatting)) };
    let turn = atlas::brain::Turn {
        said: "why not".into(),
        system: "S".into(),
        max_tokens: 200,
        skip_phrases: true,
        recent_replies: Some(replies[4..10].to_vec()),
        ..Default::default()
    };
    let mut spoken = String::new();
    let d = brain.converse(&turn, &mut |w| {
        spoken.push_str(w);
        true
    });
    // The new opening is said; the loop after it isn't; asked again, and
    // what that says is said -- without the opening a second time.
    let want = "Filing the desktop needs System changes on. Turn it on in Settings and I'll file them.";
    assert_eq!(spoken.split_whitespace().collect::<Vec<_>>().join(" "), want);
    assert_eq!(d.say.trim(), want, "what's remembered isn't what was said");
    assert_eq!(llm.asked.lock().unwrap().len(), 2);
}

/// Every chat request carries the sampling from the settings -- Qwen's
/// published settings and llama.cpp's DRY -- and a loop's asking-again the
/// stronger penalties. Before 29 Sep 2026 there was no DRY, and the laptop's
/// older build sent no sampling at all.
#[test]
fn every_request_says_how_to_sample_and_the_settings_can_change_it() {
    let req = ChatRequest { messages: vec![atlas::brain::Msg::user("hi")], max_tokens: 20, ..Default::default() };
    let body: serde_json::Value = serde_json::from_str(&atlas::models::chat_body(&req, true)).unwrap();
    assert_eq!(body["top_k"], 20);
    assert!((body["temperature"].as_f64().unwrap() - 0.7).abs() < 1e-6);
    assert!((body["top_p"].as_f64().unwrap() - 0.8).abs() < 1e-6);
    assert_eq!(body["min_p"].as_f64(), Some(0.0));
    assert!(body["presence_penalty"].as_f64().unwrap() >= 1.0, "{body}");
    assert!(body["dry_multiplier"].as_f64().unwrap() > 0.0, "no DRY: {body}");
    let again = ChatRequest { stronger: true, ..req.clone() };
    let harder: serde_json::Value = serde_json::from_str(&atlas::models::chat_body(&again, true)).unwrap();
    assert!(harder["presence_penalty"].as_f64() > body["presence_penalty"].as_f64());
    assert!(harder["dry_multiplier"].as_f64() > body["dry_multiplier"].as_f64());

    // The one-prompt body too, on the other slot.
    let one: serde_json::Value = serde_json::from_str(&atlas::models::completion_body("P", atlas::models::Template::ChatMl, 64, &Default::default())).unwrap();
    assert!(one["dry_multiplier"].as_f64().unwrap() > 0.0);
    assert_eq!(one["id_slot"], 1, "a side call can land in the conversation's slot");

    // From the settings.
    let m: atlas::models::ModelsConfig = serde_yaml::from_str("sampling:\n  dry_multiplier: 0.5\n  presence_penalty: 0.2\n").unwrap();
    assert!((m.sampling.dry_multiplier - 0.5).abs() < 1e-6);
    assert_eq!(m.sampling.top_k, 20, "an unset field keeps its default");
    let one: serde_json::Value =
        serde_json::from_str(&atlas::models::completion_body("P", atlas::models::Template::ChatMl, 64, &m.sampling)).unwrap();
    assert!((one["presence_penalty"].as_f64().unwrap() - 0.2).abs() < 1e-6);
}

/// What the model is shown of the evening: each past reply cut to its first
/// sentences, closers out, and a reply that repeats one already shown left
/// out -- the thread itself keeps every word.
#[test]
fn the_model_is_not_shown_its_own_loop() {
    let t = evening();
    let msgs = t.messages(24, 100_000);
    let replies: Vec<&str> = msgs.iter().filter(|m| m.role == Role::Assistant).map(|m| m.content.as_str()).collect();
    assert!(!replies.is_empty());
    for (i, r) in replies.iter().enumerate() {
        assert!(atlas::repeating::sentences(r).len() <= atlas::thread::HISTORY_SENTENCES, "{r}");
        assert!(!atlas::repeating::carries_boilerplate(r), "{r}");
        for other in &replies[..i] {
            assert!(!atlas::repeating::near_copy(r, other), "shown twice: {r} / {other}");
        }
    }
    let users = msgs.iter().filter(|m| m.role == Role::User).count();
    assert_eq!(users, t.recent.len(), "what the user said is all there");
    assert!(replies.len() < t.recent.len(), "every repeat was shown");
    // The thread is unchanged.
    assert!(t.recent[7].reply.contains("next move"));
}

// ================= 2. the running summary =================

#[test]
fn the_evening_s_summary_loses_the_reply_and_the_commentary() {
    let t = evening();
    let replies = the_replies(&t);
    let r: Vec<&str> = replies.iter().map(|s| s.as_str()).collect();
    let clean = atlas::thread::clean_summary(&t.summary, &r);
    assert!(!clean.contains("next move") && !clean.contains("tuned in") && !clean.contains("frustrated"), "{clean}");
    assert!(!clean.contains("Atlas responded"), "{clean}");
    assert!(clean.contains("put it on my desktop"), "a request of the user's was lost: {clean}");
    assert!(clean.contains("Open: Calendar tasks unexecuted"), "{clean}");
    // And what goes to the model says so.
    let msgs = t.messages(6, 100_000);
    assert!(msgs[0].content.contains("Earlier:") && !msgs[0].content.contains("next move"), "{}", msgs[0].content);
}

#[test]
fn a_new_summary_that_copies_a_reply_is_not_kept() {
    let t = evening();
    let old: Vec<Exchange> = t.recent[..10].to_vec();
    // What the model wrote that evening, more or less.
    let copied = "- User wants the desktop organized.\n- User is frustrated, hey, if you want me to *stop*, I\u{2019}ll stop. \
                  If you want me to *try*, I\u{2019}ll try. Either way, I\u{2019}m tuned in. What\u{2019}s your next move?\n\
                  - Atlas responded with jokes.\n- Atlas kept looping.";
    let kept = atlas::thread::accepted_summary(copied, &old, "");
    assert!(!kept.contains("next move") && !kept.contains("frustrated") && !kept.contains("Atlas responded"), "{kept}");
    // What the user asked for survives, in their words.
    assert!(kept.contains("organize my desktop") || kept.contains("desktop organized"), "{kept}");
    // A clean summary is kept as written.
    let good = "- Wants the desktop organized.\n- Wants Atlas to listen with the webcam mic.";
    let kept = atlas::thread::accepted_summary(good, &old, "");
    assert!(kept.starts_with(good), "{kept}");
    // And the model is only ever given what the user said.
    let th = Thread { summary: t.summary.clone(), recent: t.recent.clone(), ..Default::default() };
    let input = th.fold_input(&atlas::thread::ThreadConfig { verbatim: 4, fold_after: 8, ..Default::default() });
    assert!(!input.contains("tuned in") && !input.contains("next move") && !input.contains("server in reverse"), "{input}");
    assert!(input.contains("organize my desktop"), "{input}");
}

// ================= 3. what's on the screen =================

#[test]
fn the_window_in_front_goes_in_only_when_the_screen_is_mentioned() {
    let (c, p) = (cfg(), plat());
    *p.active.borrow_mut() = Some(ActiveWindow { process: "Discord.exe".into(), title: "#general | Chaos Crew - Discord".into() });
    let t = evening();
    let spy = Replay::of(&t);
    let mut d = Daemon::new(&c, &p, Some(spy.clone()), Store::new(tmp("screen")), Proactive::new(ProactiveConfig::default()));
    let _ = d.turn("Use the engine now. Understand. Stay.", 100);
    let _ = d.turn("How do we make this cause?", 160);
    for r in spy.asked.lock().unwrap().iter() {
        let all: String = r.messages.iter().map(|m| m.content.as_str()).collect::<Vec<_>>().join("\n");
        assert!(!all.contains("Chaos Crew") && !all.contains("#general"), "{all}");
    }
    let _ = d.turn("what do you make of this chat", 220);
    let last = spy.asked.lock().unwrap().last().cloned().unwrap();
    let user = &last.messages.last().unwrap().content;
    assert!(user.contains("Chaos Crew"), "{user}");
    assert!(user.contains("never the topic"), "not labelled as background: {user}");
    // The program named counts as the screen too.
    assert!(atlas::doing::refers_to_screen("keep talking about discord", "Discord"));
    assert!(!atlas::doing::refers_to_screen("This means to organize my desktop.", "Discord"));
}

// ================= 4. doing, not talking =================

#[test]
fn what_the_evening_asked_for_is_done_not_talked_about() {
    use atlas::intent::Intent;
    let c = cfg();
    let parser = atlas::intent::Parser::new(&c.commands);
    let cases: Vec<(&str, Intent)> = vec![
        ("I guess I want you to organize my desktop.", Intent::TidyDesktop),
        ("This means to organize my desktop.", Intent::TidyDesktop),
        ("I asked you to look at my screen and tell me about the traits that are either a vibe or a set.", Intent::ViewDisplay),
        ("You're built to be you. Can you do some work and generate a report on yourself?", Intent::SelfCheck),
        (
            "I want you to go and do a diagnosis on yourself. You can complete proof meeting in that website. And basically, good luck.",
            Intent::SelfCheck,
        ),
        ("Testing setup... what still needs to be set up", Intent::SelfCheck),
        ("Try to listen with my webcam mic", Intent::UseMic("webcam".into())),
        (
            "Atlas are you only talking with push to talk you need to be using my webcam mic rightnow not my laptop mic",
            Intent::UseMic("webcam".into()),
        ),
        ("switch microphone to headset", Intent::UseMic("headset".into())),
    ];
    for (said, want) in cases {
        assert_eq!(parser.parse(said), want, "{said}");
    }
    // The smart-ass setting, misheard.
    for said in ["Atlas, calm down with being our smart apps.", "I'm not calm. It can't be calm. You can tune it down. It's in the setting."] {
        match parser.parse(said) {
            Intent::Wit(w) => assert_eq!(atlas::wit::level_asked(&w), Some(atlas::wit::Asked::Down), "{said}"),
            other => panic!("{said} -> {other:?}"),
        }
    }
    // Left for the model: nothing here it can read for sure.
    for said in [
        "Hey, Brad. How many, what do you see in the setup on my phone?",
        "Asking about a server in Robert, I want you to look at my audit server and tell me what we see in the great setup.",
        "set up a meeting with Jordan tomorrow",
        "tone it down in the email to Maya",
        "Keep talking about it.",
        "my screen is cracked, what should I do",
    ] {
        assert!(matches!(parser.parse(said), Intent::Unknown(_)), "{said} -> {:?}", parser.parse(said));
    }
    // The ordinary phrases are as they were.
    assert_eq!(parser.parse("look at my screen"), Intent::ViewDisplay);
    assert_eq!(parser.parse("run a self check"), Intent::SelfCheck);
}

#[test]
fn a_microphone_is_found_by_the_kind_you_name() {
    use atlas::audio::{Device, Kind};
    use atlas::hearing::{mic_by_kind, MicFit};
    let devices = vec![
        Device::new("Microphone Array (Realtek(R) Audio)", Kind::Input),
        Device::new("Microphone (HD Pro Webcam C920)", Kind::Input),
        Device::new("Speakers (Realtek(R) Audio)", Kind::Output),
    ];
    match mic_by_kind(&devices, "webcam", "HD Pro Webcam C920") {
        MicFit::One(d) => assert_eq!(d.name, "Microphone (HD Pro Webcam C920)"),
        other => panic!("{other:?}"),
    }
    match mic_by_kind(&devices, "laptop", "") {
        MicFit::One(d) => assert!(d.name.contains("Array")),
        other => panic!("{other:?}"),
    }
    assert_eq!(mic_by_kind(&devices, "headset", ""), MicFit::None);
    // A webcam whose microphone goes by the camera's own name.
    let brio = vec![Device::new("Microphone (Logitech BRIO)", Kind::Input), Device::new("Microphone Array (Intel Smart Sound)", Kind::Input)];
    assert!(matches!(mic_by_kind(&brio, "webcam", ""), MicFit::One(d) if d.name.contains("BRIO")));

    // Chosen, and kept to by the re-pick while it's plugged in.
    let mut h = atlas::hearing::Hearing::default();
    h.choose("Microphone (HD Pro Webcam C920)");
    let tc = atlas::voice::ToolsConfig::default();
    let w = atlas::hearing::Where { at_desk: true, presence_unknown: false, headset_connected: false, phone_active: false, audio_playing: false };
    let p = atlas::hearing::pick_microphone(&devices, &mut h, &tc, &w, true, 1_000).unwrap();
    assert_eq!(p.name, "Microphone (HD Pro Webcam C920)");
    assert_eq!(p.why, "you asked for this one");
    // Unplugged: the usual pick.
    let without: Vec<Device> = devices.iter().filter(|d| !d.name.contains("Webcam")).cloned().collect();
    let p = atlas::hearing::pick_microphone(&without, &mut h, &tc, &w, true, 2_000).unwrap();
    assert!(p.name.contains("Array"), "{}", p.name);
}

#[test]
fn tidying_a_desktop_says_the_plan_and_moves_only_on_the_rules() {
    let home = tmp("desk");
    let desk = home.join("Desktop");
    std::fs::create_dir_all(desk.join("a folder")).unwrap();
    for f in ["notes.md", "report.docx", "manual.pdf", "untitled.txt", "Chrome.lnk", "desktop.ini", "photo.png"] {
        std::fs::write(desk.join(f), "x").unwrap();
    }
    let root = home.join("Documents").join("Filed");
    let plan = atlas::filing::plan_folder(&desk, &root, atlas::store::now());
    let names: Vec<String> = plan.iter().map(|(p, _)| p.file_name().unwrap().to_string_lossy().to_string()).collect();
    assert!(!names.iter().any(|n| n.ends_with(".lnk") || n.ends_with(".ini") || n == "a folder"), "{names:?}");
    // What "organize my desktop" says is the organizer's plan since 2 Oct
    // 2026 (`organize::plan_said`, replacing `filing::tidy_plan_words`):
    // shortcuts and the folder's settings file are left out of it too, and
    // it ends with the question.
    let sorting = atlas::organize::plan_folders(&[desk.clone()], None, false, atlas::store::now() + 3600, std::time::Duration::from_secs(5));
    let words = atlas::organize::plan_said(&sorting);
    assert!(!sorting.moves.iter().any(|m| m.from.ends_with("Chrome.lnk") || m.from.ends_with("desktop.ini")), "{words}");
    // (On Windows this test's folder is in AppData, under the system's
    // temporary folder, which is never sorted -- and the plan says that.)
    if sorting.refused.is_empty() {
        assert!(words.ends_with("Go ahead?"), "{words}");
    } else {
        assert!(cfg!(windows) && words.contains("won't sort"), "{words}");
    }

    // Switched off: nothing moves, and it says what to change.
    let off = atlas::system::SystemConfig { enabled: false, ..Default::default() };
    let (from, s) = plan.iter().find(|(p, _)| p.ends_with("notes.md")).unwrap();
    assert!(atlas::filing::file_one(from, s, &off).unwrap_err().contains("switched off"));
    assert!(from.exists());
    // On, inside the folders it may work in: moved, and never over another.
    let on = atlas::system::SystemConfig { enabled: true, file_roots: vec![home.display().to_string()], ..Default::default() };
    let to = atlas::filing::file_one(from, s, &on).unwrap();
    assert!(to.exists() && !from.exists());
    std::fs::write(from, "again").unwrap();
    assert!(atlas::filing::file_one(from, s, &on).unwrap_err().contains("won't write over"));
    assert!(from.exists());
}

// ================= 5. "Paused." =================

#[test]
fn taking_the_turn_over_a_reply_is_not_answered_with_paused() {
    use atlas::speech::{acknowledge, Delivery, YOUR_TURN};
    let cut = |by: &str| Delivery { spoken: vec!["One.".into()], unspoken: vec!["Two.".into()], interrupted_by: Some(by.into()) };
    assert_eq!(acknowledge(&cut(YOUR_TURN)), "", "the talk key over a reply said something");
    assert_eq!(acknowledge(&cut("pause")), "Paused.");
    assert_eq!(acknowledge(&cut("hold on")), "Paused.");
    assert_eq!(acknowledge(&cut("stop")), "Stopped.");
    // The rest is still kept for "carry on".
    assert_eq!(cut(YOUR_TURN).remaining_text(), "Two.");

    // Said through the real speaker loop, with the talk key pressed after
    // the first sentence: stopped there, and nothing said about it.
    struct Say;
    impl atlas::daemon::Mouth for Say {
        fn speak(&self, _: &str) -> atlas::error::Result<()> {
            Ok(())
        }
    }
    struct Nothing;
    impl atlas::speakthread::Host for Nothing {
        fn line(&mut self, _chunk: &str) {}
        fn between(&mut self) {}
    }
    let mut heard = vec![None, Some(YOUR_TURN.to_string())];
    let mut s = atlas::speakthread::Saying::start(&Say, false, None);
    s.add("One. Two. Three.");
    s.wait(&mut Nothing, &mut || if heard.is_empty() { None } else { heard.remove(0) });
    let d = s.finish().delivery;
    assert_eq!(d.spoken, vec!["One.".to_string()]);
    assert_eq!(d.remaining_text(), "Two. Three.");
    assert_eq!(acknowledge(&d), "");
}

// ================= 6. how it talks =================

#[test]
fn the_character_answers_first_briefly_and_without_closers() {
    use atlas::register::Register;
    let p = atlas::persona::Persona::default();
    let ch = p.character();
    assert!(ch.contains("Answer the latest thing they said first"), "{ch}");
    assert!(ch.contains("one to three short"), "{ch}");
    assert!(ch.contains("One question at most"), "{ch}");
    assert!(ch.contains("What's your next move?"), "the closers aren't named: {ch}");
    let turn = p.for_this_turn_on(Register::Chatting, 3, "what do you think", false);
    assert!(!turn.contains("tangent"), "{turn}");
    assert!(turn.contains("One to 3 short sentences"), "{turn}");
    // Dry is subtle, not a bit.
    let dry = atlas::persona::Persona { wit: atlas::wit::Wit::Dry, ..Default::default() }.for_this_turn_on(Register::Chatting, 3, "", false);
    assert!(dry.contains("light, dry touch") && dry.contains("never theatrical"), "{dry}");
    assert!(atlas::persona::asks_for_more("tell me a story about the sea"));
    assert!(!atlas::persona::asks_for_more("are you smart"));
}

#[test]
fn what_he_is_trying_to_get_done_is_kept_in_front_of_the_model() {
    let t = evening();
    let upto: Vec<Exchange> = t.recent[..9].to_vec(); // through "That's it."
    let th = Thread { recent: upto, ..Default::default() };
    assert_eq!(th.current_goal_where(|_| true).as_deref(), Some("I guess I want you to organize my desktop."));
    // A request one of Atlas's own commands took was done, and isn't the goal.
    assert_eq!(th.current_goal_where(|s| !s.contains("desktop")).as_deref(), Some("Try to listen with my webcam mic"));
    assert!(!atlas::thread::is_a_request("That's it."));
}

// ================= 7. how long the model took =================

#[test]
fn the_server_s_own_count_of_a_call_is_read() {
    let mut s = atlas::models::ChatStream::default();
    s.line(r#"data: {"choices":[{"delta":{"content":"Hi."}}]}"#);
    s.line(r#"data: {"choices":[{"finish_reason":"stop","index":0,"delta":{}}],"timings":{"cache_n":1500,"prompt_n":212,"prompt_ms":700.5,"predicted_n":40,"predicted_ms":1800.2}}"#);
    let t = s.timings.expect("the timings were not read");
    assert_eq!((t.cached, t.read, t.wrote), (1500, 212, 40));
    assert!(t.line().contains("read 212 new tokens (1500 from cache)"), "{}", t.line());
}

// ================= 8. hearing =================

#[test]
fn the_sharper_listening_model_is_fetched_and_preferred_when_it_is_there() {
    let p = atlas::getpieces::setup_pieces();
    let sharper = p
        .iter()
        .find(|x| x.lands == atlas::getpieces::Lands::File(atlas::getpieces::SHARPER_LISTENING_MODEL))
        .expect("setup doesn't fetch it");
    assert!(sharper.url.starts_with("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/"));
    assert_eq!(sharper.sha256.len(), 64);
    let root = tmp("stt");
    assert_eq!(atlas::language::speech_model_for("models/ggml-base.en.bin", &root), "models/ggml-base.en.bin");
    std::fs::create_dir_all(root.join("models")).unwrap();
    std::fs::write(root.join(atlas::getpieces::SHARPER_LISTENING_MODEL), "x").unwrap();
    assert!(atlas::language::speech_model_for("models/ggml-base.en.bin", &root).ends_with("ggml-small.en-q5_1.bin"));
    // One you chose yourself stays.
    assert_eq!(atlas::language::speech_model_for("models/ggml-medium.bin", &root), "models/ggml-medium.bin");
    // "Atlas" is always in the prompt, your words after it.
    let (opt, val) = atlas::improve::hint_args(&atlas::improve::Vocabulary::default(), atlas::language::SPEECH_PRIMER);
    assert_eq!((opt.as_str(), val.as_str()), ("--prompt", "Atlas."));
}

/// Through the daemon: "organize my desktop" and "listen with my webcam mic"
/// reach Atlas's own commands, and the model is never asked.
#[test]
fn the_desktop_and_the_webcam_mic_are_atlas_s_own_commands_through_the_daemon() {
    let (c, p) = (cfg(), plat());
    let t = evening();
    let spy = Replay::of(&t);
    let mut d = Daemon::new(&c, &p, Some(spy.clone()), Store::new(tmp("own")), Proactive::new(ProactiveConfig::default()));
    let r = d.turn("I guess I want you to organize my desktop.", 100);
    // The shipped settings have moving files switched off: said, with the
    // fix. (With it on, the plan and "Go ahead?".)
    assert!(r.contains("switched off") || r.contains("Go ahead?") || r.contains("loose file"), "{r}");
    let r = d.turn("Try to listen with my webcam mic", 110);
    assert!(r.to_lowercase().contains("microphone"), "{r}");
    assert_eq!(spy.asked.lock().unwrap().len(), 0, "the model was asked instead");
}
