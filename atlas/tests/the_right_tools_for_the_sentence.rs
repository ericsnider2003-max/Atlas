//! **The prompt diet and the tool router (30 Sep 2026).**
//!
//! Eric, on his laptop (Qwen3-VL-4B Q4_K_M, llama.cpp Vulkan on the Arc
//! 140V): with a clean ~400-token system prompt and four tool schemas the
//! same model answered eight of his real requests right in 1.0-3.0 s --
//! "organize my desktop" -> the desktop tool, "look at my screen" -> the
//! screen tool, a reminder with its what and when, "do a diagnosis on
//! yourself" -> the self-check. Inside Atlas it read 2,500-3,000-token
//! prompts with every core tool on every turn and took 17-40 s. The plumbing,
//! not the model.
//!
//! Measured here before the change, by replaying his evening through the
//! prompt builder (`one_evening_on_the_laptop.rs`): 5,652 characters of
//! messages and 4,218 of tool schemas on the average turn -- about 2,820
//! tokens; 13 to 18 tools every turn, small talk included. These tests hold
//! the new shape: a normal turn under ~900 tokens, the right tool in the
//! shortlist for each of his requests, none for his small talk, and the
//! start of the prompt the same bytes turn after turn.

use atlas::brain::{ChatReply, ChatRequest, Llm, Role};
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::router::Router;
use atlas::store::Store;
use atlas::thread::{Exchange, Thread};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

fn cfg() -> atlas::config::Config {
    atlas::config::Config::load(Path::new("config")).unwrap()
}

fn router() -> Router {
    Router::new(&atlas::intent::ToolBook::new(&cfg().commands))
}

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-right-tools-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// His requests, from the two evenings' threads and the laptop
/// measurement, each with the tool that does it.
pub const HIS_REQUESTS: &[(&str, &str)] = &[
    ("I guess I want you to organize my desktop.", "tidy_desktop"),
    ("This means to organize my desktop.", "tidy_desktop"),
    ("organize my desktop", "tidy_desktop"),
    ("look at my screen", "view_display"),
    ("I asked you to look at my screen and tell me about the traits that are either a vibe or a set.", "view_display"),
    ("do a diagnosis on yourself", "self_check"),
    ("I want you to go and do a diagnosis on yourself.", "self_check"),
    ("Can you do some work and generate a report on yourself?", "self_check"),
    ("Testing setup... what still needs to be set up", "finish_setup"),
    ("Try to listen with my webcam mic", "use_mic"),
    ("you need to be using my webcam mic rightnow not my laptop mic", "use_mic"),
    ("calm down with being a smart ass", "wit"),
    ("Research ways to improve in house language models, increase response times, allowing it to do better in house analysis for personally built AI systems.", "research"),
    ("I want you to use the internet and do the research i asked for.", "research"),
    ("Please use my camera and look at me.", "whats_there"),
    ("Atlas, can you see me?", "whats_there"),
    ("what's on my calendar tomorrow", "agenda"),
    ("put the dentist on my calendar for friday at 3", "schedule"),
    ("find the tax pdf from last year", "find_file"),
    ("check my email", "mail"),
    ("my laptop is running slow, what's eating the memory", "machine_health"),
    ("note that the plumber comes on thursday", "capture"),
    ("open chrome and discord", "open_app"),
];

/// His small talk and mishearings: nothing to do, so no tool but the one
/// that is always there.
pub const HIS_SMALL_TALK: &[&str] = &[
    "Thank you.",
    "Thanks, have a good day.",
    "At this. Are you smart?",
    "Keep talking about it.",
    "At this you're repeating yourself gone.",
    "Be, you're, soup, come on, come on.",
    "That's not what I...",
    "What does that mean?",
    "Because you actually care about it. At least you do things just because I asked you to",
    "You're here to do what I asked you to do, because that's your purpose.",
    "There we go, now I can hear you.",
    "how's your day going",
    "tell me a story",
];

#[test]
fn each_of_his_requests_gets_the_tool_that_does_it() {
    let r = router();
    let mut missed = Vec::new();
    for (said, want) in HIS_REQUESTS {
        let got = r.names_for(said, atlas::router::SHORTLIST);
        println!("ROUTE {:<16} <- {said:?}: {got:?}", want);
        if !got.iter().any(|g| g == want) {
            missed.push(format!("{said:?} wanted {want}, got {got:?}"));
        }
    }
    assert!(missed.is_empty(), "the right tool wasn't shortlisted:\n{}", missed.join("\n"));
}

#[test]
fn his_small_talk_is_offered_no_tools() {
    let r = router();
    for said in HIS_SMALL_TALK {
        let got = r.names_for(said, atlas::router::SHORTLIST);
        assert!(got.is_empty(), "small talk offered tools: {said:?} -> {got:?}");
    }
}

#[test]
fn the_shortlist_is_best_first_and_nothing_weak_gets_in() {
    let r = router();
    let got = r.shortlist("find the tax pdf from last year", atlas::router::SHORTLIST);
    assert_eq!(got.first().map(|(e, _)| e.name.as_str()), Some("find_file"));
    for w in got.windows(2) {
        assert!(w[0].1 >= w[1].1, "not best first: {} {} / {} {}", w[0].0.name, w[0].1, w[1].0.name, w[1].1);
    }
    let best = got[0].1;
    assert!(got.iter().all(|(_, s)| *s >= atlas::router::FLOOR && *s >= best * atlas::router::RELATIVE));
    // A sentence that starts with a command's own phrase leads with it.
    let got = r.shortlist("research the best small language models for a laptop", 2);
    assert_eq!(got[0].0.name, "research");
}

#[test]
fn what_he_asked_earlier_is_not_offered_to_small_talk() {
    let r = router();
    let goal = Some("Research ways to improve in house language models");
    assert!(r.for_turn("hey, how's it going", goal, atlas::router::SHORTLIST).is_empty());
    assert!(r.for_turn("thanks", goal, atlas::router::SHORTLIST).is_empty());
    // Nor to a question about the last answer (30 Sep 2026: "why not?").
    assert!(r.for_turn("Why not?", goal, atlas::router::SHORTLIST).is_empty());
    assert!(r.for_turn("what do you mean", goal, atlas::router::SHORTLIST).is_empty());
    // "That's it" leans on it.
    let names: Vec<String> = r.for_turn("That's it.", goal, atlas::router::SHORTLIST).iter().map(|e| e.name.clone()).collect();
    assert!(names.contains(&"research".to_string()), "{names:?}");
}

#[test]
fn a_tool_is_described_in_one_line() {
    let book = atlas::intent::ToolBook::new(&cfg().commands);
    for e in book.entries() {
        let spec = atlas::router::compact_spec(e, None);
        let chars = spec.to_string().chars().count();
        assert!(chars <= 330, "{} is {chars} characters as sent: {spec}", e.name);
    }
    // The meta tool is the same bytes every time.
    assert_eq!(atlas::router::meta_spec(), atlas::router::meta_spec());
}

/// A model that answers "Sure." and records every request.
struct Spy {
    asked: Mutex<Vec<ChatRequest>>,
}

impl Llm for Spy {
    fn complete(&self, _: &str, _: &str) -> atlas::error::Result<String> {
        Ok(r#"{"action":"say","arg":null,"say":"Sure."}"#.into())
    }
    fn native_chat(&self) -> bool {
        true
    }
    fn chat(&self, req: &ChatRequest, on_text: &mut dyn FnMut(&str) -> bool) -> atlas::error::Result<ChatReply> {
        self.asked.lock().unwrap().push(req.clone());
        on_text("Sure.");
        Ok(ChatReply { text: "Sure.".into(), tool_calls: vec![] })
    }
}

/// Roughly tokens, the way `brain::Turn::estimated_tokens` counts them
/// (3.5 characters a token: errs long for English).
fn tokens(chars: usize) -> usize {
    (chars * 2).div_ceil(7)
}

fn request_tokens(r: &ChatRequest) -> (usize, usize) {
    let msgs: usize = r.messages.iter().map(|m| tokens(m.content.chars().count()) + 4).sum();
    let tools: usize = r.tools.iter().map(|t| tokens(t.to_string().chars().count())).sum();
    (msgs, tools)
}

/// Tonight's evening (the thread from his laptop, 30 Sep 2026), every
/// sentence through the whole daemon, with a long polluted summary behind
/// it: each request that reaches the model is measured.
#[test]
fn a_normal_turn_is_under_nine_hundred_tokens_and_starts_the_same_every_time() {
    let c = cfg();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let dir = tmp("diet");
    let store = Store::new(dir.clone());
    let summary = "- \u{201c}Freaky man\u{201d} unconfirmed \u{2014} user insists it exists and built Atlas to see it.\n\
        - User repeatedly asks Atlas to diagnose, audit server, look at screen \u{2014} no access without info.\n\
        - No server name or file path provided for audit.\n\
        - User says: \u{201c}I built you to see this freaky man\u{201d} \u{2014} Atlas acknowledges it as wild/cool.\n\
        - Server name reversal (\u{201c}flip channel names\u{201d}) requested \u{2014} no action taken.\n\
        - Research task: improve AI systems (response time, human-likeness, speed, model upgrades) for in-house language models, with report into document.\n\
        - System: running on laptop \u{2192} later phone; direct computer use.\n\
        - Goals: smarter, more human, faster, less error-prone, more useful.";
    let start = Thread { summary: summary.into(), recent: Vec::new(), folded: 71, last_active: 1_790_740_000, current_topic: None };
    store.save("thread", &start).unwrap();
    let spy = Arc::new(Spy { asked: Mutex::new(Vec::new()) });
    let mut d = Daemon::new(&c, &p, Some(spy.clone()), Store::new(dir), Proactive::new(ProactiveConfig::default()));
    let tonight = [
        "You have internet capabilities correct?",
        "you and improving you.",
        "I would change the quality of responses. Why are you refusing to do research? Is it because you dont want to make yourself better?",
        "Because you actually care about it. At least you do things just because I asked you to",
        "Atlas, can you see me?",
        "Are you using my camera? Can you see me?",
        "Why, I gave you permission.",
        "What permission do you need?",
        "You're here to do what I asked you to do, because that's your purpose.",
        "What does that mean?",
        "Please, comment me.",
        "All improvement.",
        "tell me something interesting about octopuses",
        "how's your day going",
    ];
    let mut t = 1_790_740_100u64;
    let mut rows = Vec::new();
    for said in tonight {
        t += 60;
        let before = spy.asked.lock().unwrap().len();
        let _ = d.turn(said, t);
        let asked = spy.asked.lock().unwrap();
        if let Some(r) = asked.get(before) {
            let (m, tl) = request_tokens(r);
            rows.push((said, m, tl, r.tools.len(), r.clone()));
        }
    }
    assert!(rows.len() >= 10, "only {} of the sentences reached the model", rows.len());
    let mut total = 0;
    for (said, m, tl, n, _) in &rows {
        let r = &rows.iter().find(|x| x.0 == *said).unwrap().4;
        let sizes: Vec<String> = r.messages.iter().map(|m| format!("{}{}", &m.role.name()[..1], m.content.chars().count())).collect();
        if std::env::var_os("DIET_SHOW").is_some() {
            println!("USER<<{}>>", r.messages.last().map(|m| m.content.clone()).unwrap_or_default());
        }
        println!("DIET ~{:>4} tokens = {:>4} messages + {:>4} tools ({n} tools) [{}] | {said}", m + tl, m, tl, sizes.join(" "));
        total += m + tl;
        // Before: about 2,800 on average. The ceiling for any one turn: a
        // question about Atlas itself also carries up to two lines of its
        // catalogue (about 100 tokens), so the ceiling is a little over the
        // ~900 an ordinary turn is held to on average below.
        assert!(m + tl <= 1_150, "{said:?} is ~{} tokens", m + tl);
    }
    let avg = total / rows.len();
    println!("DIET average ~{avg} tokens over {} turns (before the diet: ~2,820)", rows.len());
    assert!(avg <= 900, "the average turn is ~{avg} tokens");

    // The start of the prompt is the same bytes every turn -- the system
    // message and the one tool that is always offered -- so the model
    // server reads it once.
    let first = &rows[0].4;
    for (said, _, _, _, r) in &rows {
        assert_eq!(r.messages[0].role, Role::System);
        assert_eq!(r.messages[0].content, first.messages[0].content, "the system message changed for {said:?}");
        assert_eq!(r.tools.first(), Some(&atlas::router::meta_spec()), "{said:?}");
        assert_eq!(r.stable_tools, 1, "{said:?}");
    }
    // The freaky man stays folded away unless he is what's being talked about.
    for (said, _, _, _, r) in &rows {
        let all: String = r.messages.iter().map(|m| m.content.as_str()).collect::<Vec<_>>().join("\n");
        assert!(!all.to_lowercase().contains("freaky"), "the old summary's freaky man came back for {said:?}");
    }
    // The camera question (30 Sep 2026, merged with r8-senses) no longer
    // reaches the model at all: `camera_ask` sends it to the camera's own
    // command. If it ever does reach the model again, it must be told what
    // the catalogue says about the camera.
    if let Some((_, _, _, _, cam)) = rows.iter().find(|r| r.0.contains("using my camera")) {
        let last = &cam.messages.last().unwrap().content;
        assert!(last.contains("camera"), "{last}");
        assert!(last.contains("never say you lack one"), "{last}");
    }
}

#[test]
fn the_summary_comes_back_only_where_it_bears_on_what_was_said() {
    let summary = "- Freaky man unconfirmed.\n- Research task: improve in-house language models, report into a document.\n- Calendar: dentist on Friday.";
    let got = atlas::thread::summary_bearing_on(summary, "Atlas, can you see me?", 2);
    assert!(got.is_empty(), "{got:?}");
    let got = atlas::thread::summary_bearing_on(summary, "how's that research going", 2);
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(got[0].starts_with("Research task"), "{got:?}");
    let got = atlas::thread::summary_bearing_on(summary, "who was the freaky man", 2);
    assert!(got[0].contains("Freaky man"), "{got:?}");
    // The history the model reads is held to its budget, however long the
    // conversation's sentences were.
    let th = Thread {
        recent: (0..10)
            .map(|i| Exchange { at: i, said: format!("question {i} {}", "and a long run of words ".repeat(12)), reply: format!("Answer {i}."), about: None })
            .collect(),
        ..Default::default()
    };
    let msgs = th.messages(atlas::daemon::HISTORY_EXCHANGES, atlas::daemon::HISTORY_TOKENS);
    let chars: usize = msgs.iter().map(|m| m.content.chars().count()).sum();
    assert!(chars <= atlas::daemon::HISTORY_TOKENS * 4 + 400, "{chars} characters of history: {msgs:?}");
    assert!(msgs.iter().filter(|m| m.role == Role::User).count() <= atlas::daemon::HISTORY_EXCHANGES, "{msgs:?}");
}

/// `ATLAS_WRITE_CAPABILITIES=1`: write docs/CAPABILITIES.md from the
/// catalogue (what `atlas catalog --markdown` prints), for when the list
/// changed.
#[test]
fn write_the_capabilities_document_when_asked() {
    if std::env::var("ATLAS_WRITE_CAPABILITIES").is_ok() {
        std::fs::write("docs/CAPABILITIES.md", atlas::capability::as_markdown()).unwrap();
    }
}


/// Every command Atlas has, asked for in its own words (its description, as
/// the capability list and "what can you do" say it): the router must find
/// it. This is the "long list of capabilities" -- each one reachable by
/// asking for what it does, not only by its set phrases.
#[test]
fn every_command_is_found_by_what_it_does() {
    let book = atlas::intent::ToolBook::new(&cfg().commands);
    let r = router();
    let mut missed = Vec::new();
    let mut total = 0;
    for e in book.entries() {
        if e.exposure == atlas::intent::Exposure::Never || e.name == atlas::router::META_TOOL {
            continue;
        }
        total += 1;
        let asked = e.describe.split(" arg: ").next().unwrap_or("").trim().to_string();
        let got = r.names_for(&asked, atlas::router::SHORTLIST);
        if !got.contains(&e.name) {
            missed.push(format!("{}: {asked:?} -> {got:?}", e.name));
        }
    }
    println!("{} of {total} commands found by their own description", total - missed.len());
    for m in &missed {
        println!("MISSED {m}");
    }
    assert!(missed.len() * 20 <= total, "{} of {total} not found:\n{}", missed.len(), missed.join("\n"));
}
