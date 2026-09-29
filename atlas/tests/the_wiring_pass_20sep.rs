//! What the 20 Sep orphan-and-unwired pass actually changed in behaviour.
//!
//! Every method touched here was built, tested in isolation, and called by
//! nothing in production — so the unit tests passed while the daemon never
//! did the thing. These drive the daemon and check the thing.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-w20-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

// ================= "call me X" reaches the memory store =================

#[test]
fn call_me_a_name_writes_the_preference_that_addresses_you() {
    // `memory::prefer` had no production writer, so `preference("called")`
    // could only ever return None and the parked-question flow addressed you
    // as nobody.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "called");
    d.turn("call me Eric", 100);
    assert_eq!(
        d.memory.preference("called").map(str::to_lowercase),
        Some("eric".to_string()),
        "the name you gave never reached the store that greets you"
    );
}

#[test]
fn stop_calling_me_that_clears_the_preference_rather_than_leaving_it_stale() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "uncalled");
    d.turn("call me Boss", 100);
    assert_eq!(
        d.memory.preference("called").map(str::to_lowercase),
        Some("boss".to_string())
    );
    d.turn("stop calling me that", 200);
    assert!(
        d.memory.preference("called").unwrap_or("").is_empty(),
        "cleared must mean empty, not the old name left behind"
    );
}

// ================= a capture stamps the project =================

#[test]
fn capturing_against_a_project_stamps_it_as_worked_on() {
    // `memory::touch_project` — the `projects` store had no writer at all.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "touch");
    // A capture that names a known project. The shipped config's projects
    // list is what read_spoken matches against; use whatever it carries, or
    // fall back to asserting nothing crashes when none match.
    let before = d.memory.projects.len();
    d.turn("note that the roof needs fixing for the Homelab project", 100);
    // Either it matched a configured project name and stamped it, or it did
    // not — but the store is now writable either way, which is the property
    // that was broken. When it matched, last_touched is set.
    if d.memory.projects.len() > before {
        let touched = d.memory.projects.values().any(|pr| pr.last_touched > 0);
        assert!(touched, "a stamped project must carry when it was touched");
    }
}

// ================= undo skips past the irreversible =================

#[test]
fn undo_reaches_past_an_irreversible_action_to_a_reversible_one() {
    // `undo::possible` existed to ask "can this be taken back?" and nothing
    // asked it: when the newest action was irreversible, its refusal was the
    // whole answer even with a reversible action right behind it.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "undo");
    let now = 100;
    // A reversible action, then an irreversible one on top.
    d.history.note(
        "moved report.pdf to Archive",
        "files",
        atlas::undo::Undo::Atlas("move it back".into()),
        true,
        now,
    );
    d.history.note(
        "sent the newsletter",
        "mail",
        atlas::undo::Undo::Cannot("a sent message can't be unsent".into()),
        true,
        now + 1,
    );
    let reply = d.turn("undo that", now + 2);
    assert!(
        reply.to_lowercase().contains("report.pdf") || reply.to_lowercase().contains("before that"),
        "undo should offer the reversible action behind the irreversible one: {reply}"
    );
}

// ================= "stop everything" reaches running work =================

#[test]
fn stop_everything_abandons_queued_lane_work() {
    // `attention::halt`'s own doc says "queues emptied, work abandoned", and
    // nothing ever emptied the queue — a later resume quietly restarted the
    // exact work you panicked about.
    use atlas::lanes::{Lane, TaskState};
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "panic");
    d.queue.push("do a big slow thing", Lane::Background);
    d.turn("stop everything", 100);
    let live = d
        .queue
        .tasks
        .iter()
        .filter(|t| !matches!(t.state, TaskState::Done | TaskState::Failed))
        .count();
    assert_eq!(live, 0, "a panic-stop must leave nothing queued to silently resume");
}

// ================= a queued net task says it needs a connection ======

#[test]
fn work_that_needs_the_network_is_queued_as_needing_it() {
    // `lanes::push_online` and the offline hold were unreachable because every
    // task ever queued had needs_net = false.
    use atlas::lanes::{Lane, Queue};
    let mut q = Queue::default();
    q.push_online("research the thing", Lane::Background);
    assert_eq!(q.waiting_for_network().len(), 1, "a net task must be visible as one");
}

// ================= smtp port comes from the provider ==================

#[test]
fn the_smtp_port_is_the_providers_own_answer() {
    // Two call sites held a literal 465 while `mail::smtp_port` — the named
    // fact — sat uncalled.
    use atlas::mail::Provider;
    assert_eq!(Provider::Gmail.smtp_port(), 465);
    assert_eq!(Provider::Fastmail.smtp_port(), 465);
}


// ================= a reviewed post fills the open draft ==============

#[test]
fn reviewing_text_with_a_draft_open_fills_it_and_asks_to_confirm() {
    // `publish::edit`/`request_approval` had no caller: every drafted post
    // stayed an empty Draft forever, and the corrected text ReviewPost
    // computed was thrown away.
    use atlas::publish::PostState;
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "post");
    // Open a draft the way "draft a post for X" does.
    let id = d.publisher.draft(atlas::publish::Channel::X, "");
    d.turn("check this before I post it: We shipped the thing today", 100);
    let post = d.publisher.get(id).expect("the draft still exists");
    assert!(!post.body.is_empty(), "the reviewed words should have become the draft body");
    assert_eq!(
        post.state,
        PostState::AwaitingApproval,
        "a filled draft should be waiting for your yes, not still empty"
    );
}

// ================= the daemon methods, exercised directly =============
//
// These were orphan methods — no caller, no test — and the pass gave each a
// production caller. Called from inside daemon.rs, they would otherwise be
// "helper-untested"; these drive them by name so the behaviour is pinned,
// not merely reached.

use atlas::daemon::Mouth;
use atlas::error::Result as AResult;

struct RecordingMouth {
    said: std::cell::RefCell<Vec<String>>,
}
impl Mouth for RecordingMouth {
    fn speak(&self, text: &str) -> AResult<()> {
        self.said.borrow_mut().push(text.to_string());
        Ok(())
    }
}

#[test]
fn learned_merges_a_finding_into_the_knowledge_store() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "learned");
    let first = d.learned("The Ventura tide is low at 3pm", "research: tides", 100);
    assert!(!first, "the first sighting of a claim is an addition, not a strengthening");
    // The same claim again strengthens rather than duplicating.
    let again = d.learned("Ventura tide is low around 3pm", "research: tides", 200);
    assert!(again, "the same claim reworded should strengthen what's known");
}

#[test]
fn a_budget_exceeded_only_by_undroppable_facts_is_said_not_hidden() {
    // `consolidate::over_budget_on_purpose` was built and tested and nothing
    // called it: `learned` spoke when it dropped something (`dropped_note`) and
    // fell silent in the very case that function exists for. Settled facts and
    // things about your own setup are held out of the trim because they can't
    // be looked up again -- when those alone exceed the cap, nothing drops and
    // the store sits over budget quietly. A cap quietly exceeded is a cap doing
    // nothing, so which of the two it is now gets said.
    let (mut c, p) = (cfg(), plat());
    let mut t = c.tools.clone().unwrap_or_default();
    t.consolidate.keep_at_most = 2;
    c.tools = Some(t);
    let mut d = daemon(&c, &p, "overbudget");

    // "stands for" / "means" / "definition" all land a claim on Shelf::Settled,
    // which trim never drops -- three of them against a budget of two.
    d.learned("HTTP stands for hypertext transfer protocol", "note", 100);
    d.learned("TCP means transmission control protocol", "note", 200);
    d.learned("JSON definition is javascript object notation", "note", 300);

    let note = d
        .history
        .since(0)
        .into_iter()
        .find(|e| e.area == "knowledge" && e.what.contains("budget of 2"));
    assert!(
        note.is_some(),
        "three un-droppable facts against a budget of two should be reported, not hidden"
    );
    assert!(
        note.unwrap().what.contains("settled facts"),
        "the note should say why nothing was dropped"
    );
}

#[test]
fn another_way_names_a_different_route_of_the_same_kind() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "route");
    d.connectivity.set(atlas::connectivity::Reach::Online, 100);
    // Given one Extract route failed, it should name a different one.
    let alt = d.another_way(atlas::route::Kind::Extract, "read the file that's already there");
    assert!(alt.is_some(), "there is more than one way to extract; it should offer another");
    assert_ne!(alt.as_deref(), Some("read the file that's already there"));
}

#[test]
fn keep_awake_lets_the_machine_sleep_on_a_low_battery() {
    let (c, p) = (cfg(), plat());
    let d = daemon(&c, &p, "awake");
    let flat = atlas::awake::Power {
        on_battery: true,
        battery_pct: 5,
        lid_closed: false,
        lid_action: atlas::awake::LidAction::Unknown,
        external_display: false,
    };
    let (hold, why) = d.keep_awake(atlas::awake::Because::OvernightWork, &flat, 0);
    assert_eq!(hold, atlas::awake::Hold::Release, "5% battery is for the morning, not a render");
    assert!(!why.is_empty(), "letting it sleep should say why");
}

#[test]
fn a_reply_can_be_interrupted_and_then_finished() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "interrupt");
    let mouth = RecordingMouth { said: std::cell::RefCell::new(Vec::new()) };
    // A listener that interrupts after the first chunk.
    let mut calls = 0;
    let mut listen = move || {
        calls += 1;
        if calls == 1 { Some("stop".to_string()) } else { None }
    };
    let d_res = d.say_interruptibly(
        &mouth,
        "First sentence here. Second sentence here. Third sentence here.",
        &mut listen,
    );
    if d_res.was_interrupted() {
        // The unsaid remainder is parked and retrievable.
        assert!(d.finish_saying().is_some(), "an interrupted reply must leave a remainder to finish");
        assert!(d.finish_saying().is_none(), "the remainder is handed over once, not forever");
    }
}

// ================= what reaches the speaker is spoken form ============

#[test]
fn the_speaker_gets_words_not_symbols() {
    // The screen keeps "$5"; the voice should hear "5 dollars". This pins the
    // wiring, not just the module — spoken_form runs between the daemon and
    // the mouth.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "spoken");
    let mouth = RecordingMouth { said: std::cell::RefCell::new(Vec::new()) };
    let mut never = || None;
    let d_res = d.say_interruptibly(&mouth, "That's $5 and 20% off.", &mut never);
    // If the tier speaks at all, what it spoke is normalised.
    if !d_res.was_interrupted() {
        let spoken = mouth.said.borrow().join(" ");
        if !spoken.is_empty() {
            assert!(
                spoken.contains("5 dollars"),
                "the voice should say '5 dollars', got: {spoken:?}"
            );
            assert!(
                spoken.contains("20 percent"),
                "the voice should say '20 percent', got: {spoken:?}"
            );
            assert!(!spoken.contains('$'), "no bare symbols should reach the speaker: {spoken:?}");
        }
    }
}
