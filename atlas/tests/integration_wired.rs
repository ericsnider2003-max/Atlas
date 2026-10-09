//! Does the assembled system actually behave, as opposed to its parts?

use atlas::config::Config;
use atlas::daemon::{Autonomy, Daemon};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::publish::Channel;
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-wired-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}
fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}
fn plat() -> MockPlatform {
    MockPlatform::new(vec![
        Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true },
        Monitor { id: 3, x: 1920, y: 0, width: 1920, height: 1040, primary: false },
    ])
}
fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

// ============ the thing that was missing ============

#[test]
fn a_scheduled_post_fires_at_its_time_with_nobody_present() {
    // Approving at schedule time IS the consent to send at its time. Needing
    // you awake at 7am would make scheduling pointless.
    let (c, p) = (cfg(), plat());
    // The send runs as an errand (Eric's ruling G2, 25 Sep 2026): a browser
    // is slow, and the tick must not wait on it. Pointed at a browser that
    // isn't there, so the errand ends quickly and without the network, and
    // what matters is that it fired at its time and came back with nobody
    // asked anything.
    let mut c = c;
    c.tools.as_mut().unwrap().browser.launch = Some(atlas::tools::ExternalTool {
        command: "no-such-browser-atlas-test".into(),
        ..Default::default()
    });
    let mut d = daemon(&c, &p, "sched-post-2");
    d.autonomy = Autonomy::Unattended;
    let id = d.publisher.draft(Channel::X, "Shipping today.");
    d.publisher.schedule(id, 10_000);
    d.publisher.approve(id);
    d.connectivity.set(atlas::connectivity::Reach::Online, 0);

    assert!(d.tick(9_000).iter().all(|m| !m.contains("Shipping")), "not yet its time");
    let mut out = d.tick(10_000);
    let mut t = 10_001;
    while !out.iter().any(|m| m.contains("Shipping")) && t < 10_200 {
        std::thread::sleep(std::time::Duration::from_millis(25));
        out = d.tick(t);
        t += 1;
    }
    assert!(out.iter().any(|m| m.contains("Shipping")), "should have fired and reported back: {out:?}");
}

#[test]
fn a_post_edited_after_approval_does_not_fire_and_you_are_warned() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "sched-edited");
    let id = d.publisher.draft(Channel::X, "Original text.");
    d.publisher.schedule(id, 100);
    d.publisher.approve(id);
    d.publisher.edit(id, "Swapped after the fact.");

    let out = d.tick(200);
    assert!(!out.iter().any(|m| m.contains("Ready to send")), "must not send: {out:?}");
}

#[test]
fn a_post_that_cannot_go_is_flagged_once_not_every_hour() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "sched-warn");
    let id = d.publisher.draft(Channel::X, &"x".repeat(400));
    d.publisher.schedule(id, 100);
    d.publisher.approve(id);

    let first = d.tick(200);
    assert!(first.iter().any(|m| m.contains("Heads up")), "got {first:?}");
    let second = d.tick(300);
    assert!(!second.iter().any(|m| m.contains("Heads up")), "must not nag: {second:?}");
}

// ============ pause reaches the whole system ============

#[test]
fn saying_pause_stops_scheduled_work_not_just_the_talking() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "pause-all");
    d.scheduler.at("boot workspace", 50);
    assert_eq!(d.turn("hold on", 100), "Paused. 1 job on hold.");
    assert!(d.tick(200).is_empty(), "nothing runs while paused");

    d.turn("i am ready", 300);
    assert!(!d.tick(400).is_empty(), "and it picks back up");
}

#[test]
fn while_paused_ordinary_speech_is_ignored_but_resume_is_not() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "pause-speech");
    d.turn("pause", 100);
    assert_eq!(d.turn("boot workspace", 110), "", "ignored while paused");
    assert!(d.turn("carry on", 120).contains("Go ahead") || !d.turn("open chrome", 130).is_empty());
}

// ============ overheard speech ============

#[test]
fn a_phone_call_does_not_interrupt_a_running_job() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "overheard");
    d.scheduler.at("boot workspace", 10);
    // Through the open microphone, not the wake word. `turn` now defaults to
    // `Directed` because the wake word, push-to-talk and the typed prompt all
    // reach it — and defaulting the other way was the bug that made Atlas
    // ignore "hello". Ambient audio is the one case that is still judged.
    let reply = d.turn_from(
        "yeah no I told him we would sort it out",
        100,
        atlas::daemon::Arrival::OpenMic,
    );
    assert_eq!(reply, "", "must stay quiet and keep working");
}

#[test]
fn a_real_command_still_gets_through_while_working() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "real-cmd");
    d.scheduler.at("boot workspace", 10);
    // The command got through -- which is what this test is about. Not the
    // exact wording: the acknowledgement goes through
    // `Persona::acknowledge`, which carries the configured form of address
    // and varies the phrasing so the same command twice does not come back
    // identical. `tests/a_conversation_not_a_script.rs` owns those rules.
    let reply = d.turn("open chrome", 100);
    assert!(reply.starts_with("Chrome is up"), "got: {reply}");
}

// ============ follow-ups ============

#[test]
fn close_it_resolves_to_the_app_atlas_just_opened() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "pronoun");
    d.turn("open chrome", 100);
    d.memory.record_approval("close_app", true, None);
    let reply = d.turn("close it", 110);
    assert!(reply.contains("chrome"), "got: {reply}");
}

#[test]
fn a_pronoun_with_nothing_behind_it_asks_rather_than_guessing() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "pronoun-none");
    assert_eq!(d.turn("close it", 100), "Which app?");
}

// ============ nothing is forgotten ============

#[test]
fn work_blocked_by_being_offline_lands_on_the_outstanding_list() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "backlog");
    d.connectivity.set(atlas::connectivity::Reach::Offline, 100);

    d.turn("research the quic v1 spec", 100);
    assert_eq!(d.backlog.outstanding().len(), 1, "must be kept, not dropped");
    let pending = d.turn("what's outstanding", 110);
    assert!(pending.contains("preparing"), "{pending}");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let brief = loop {
        if let Some(result) = d.poll_brief() { break result.expect("the actual outstanding brief must be prepared"); }
        assert!(std::time::Instant::now() < deadline, "outstanding preparation never finished");
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    assert!(brief.yours.iter().any(|item| item.subject.contains("quic v1")), "the prepared brief lost the offline request: {brief:?}");
    assert_eq!(d.backlog.outstanding().len(), 1, "preparing the brief must not consume unfinished offline work");
}

#[test]
fn you_can_ask_what_is_queued_to_go_out() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "queued");
    assert!(d.turn("what's queued", 100).contains("Nothing queued"));
    let id = d.publisher.draft(Channel::LinkedIn, "a draft");
    d.publisher.schedule(id, 5000);
    d.publisher.approve(id);
    let said = d.turn("what's scheduled", 110);
    assert!(said.contains("1 scheduled"), "got: {said}");
}

// ============ it all survives a restart ============

#[test]
fn posts_backlog_and_learning_all_come_back_after_a_restart() {
    let (c, p) = (cfg(), plat());
    let dir = tmp("restart");
    {
        let mut d = Daemon::new(&c, &p, None, Store::new(&dir), Proactive::new(ProactiveConfig::default()));
        let id = d.publisher.draft(Channel::X, "scheduled for later");
        d.publisher.schedule(id, 99_999);
        d.publisher.approve(id);
        d.backlog.record("research something", atlas::backlog::Blocker::Offline, 100);
        d.turn("open chrome", 100);
    }
    let d2 = Daemon::new(&c, &p, None, Store::new(&dir), Proactive::new(ProactiveConfig::default()));
    assert_eq!(d2.publisher.pending().len(), 1, "the scheduled post survived");
    assert_eq!(d2.backlog.outstanding().len(), 1, "the outstanding item survived");
}

#[test]
fn housekeeping_runs_on_a_schedule_and_does_not_thrash() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "tidy");
    for _ in 0..600 {
        d.memory.record_approval("close_app", true, None);
    }
    d.tick(10_000);
    assert!(d.memory.approvals.len() <= 500);
    d.tick(10_001);
    d.tick(10_002);
    assert!(d.memory.approvals.len() <= 500, "still bounded, and no churn");
}

// ============ everything now reachable by voice ============

#[test]
fn saying_a_mode_name_switches_mode_and_silences_atlas() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "mode");
    for m in atlas::modes::suggested() {
        d.modes.add(m);
    }
    assert_eq!(d.turn("i'm on a call", 100), "call mode.");
    assert!(!d.modes.may_interrupt(true), "call mode means silence");
    // And a proactive tick must respect it.
    assert!(d.tick(200).is_empty());
}

#[test]
fn a_named_sequence_can_be_triggered_by_saying_its_name() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "flow");
    d.flows.record("start work", &["boot workspace".into(), "open chrome".into()], Some("start work"));
    let reply = d.turn("ok, start work", 100);
    assert!(reply.contains("2 steps"), "got: {reply}");
    // This used to assert `queue.pending() == 2`: the trigger flattened the
    // steps into the queue, discarding `on_fail` and `produces` on the way.
    // A triggered flow is a `flow::Run` now, driven in the turn — so the
    // benign steps are DONE, not queued.
    assert!(reply.contains("done"), "the benign steps ran in the turn: {reply}");
    assert_eq!(d.queue.pending(), 0, "nothing left behind in a lane");
}

#[test]
fn undo_is_reachable_by_voice_and_says_so_when_there_is_nothing() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "undo");
    let said = d.turn("undo that", 100);
    assert!(said.to_lowercase().contains("nothing to undo"), "got: {said}");
}

#[test]
fn the_conversation_is_one_thread_that_survives_a_restart() {
    let (c, p) = (cfg(), plat());
    let dir = tmp("thread");
    {
        let mut d = Daemon::new(&c, &p, None, Store::new(&dir), Proactive::new(ProactiveConfig::default()));
        d.turn("open chrome", 100);
    }
    let d2 = Daemon::new(&c, &p, None, Store::new(&dir), Proactive::new(ProactiveConfig::default()));
    assert_eq!(d2.thread.len(), 1);
    assert_eq!(d2.thread.current_topic.as_deref(), Some("chrome"));
    let resumed = d2.thread.resume_line(&Default::default(), 100 + 7200).unwrap();
    assert_eq!(resumed, "We were on chrome.", "a continuation, not a greeting");
}

#[test]
fn coming_back_after_a_gap_leads_with_where_the_thread_left_off() {
    // The whole point of `thread.rs`: a return picks the conversation up by
    // naming what it was about, rather than greeting you fresh. This drives it
    // through the daemon's real turn path, not the method in isolation.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "resume-line");
    let first = d.turn("open chrome", 100);
    assert!(!first.to_lowercase().contains("we were on"), "no resume on turn one: {first}");

    // Two hours later, the next thing said. Nothing ran while away (opening an
    // app is an `Asked` record, which no brief recites), so there is no
    // away-brief to lead with -- only the conversation gap, which is exactly
    // what `resume_line` is for.
    let back = d.turn("what can you do", 100 + 7200);
    assert!(
        back.contains("We were on chrome."),
        "the return should say where the thread left off, got: {back}"
    );

    // And it is said once. The turn just taken reset `last_active`, so a
    // follow-up a minute later is an ordinary reply with no continuation line.
    let next = d.turn("thanks", 100 + 7200 + 60);
    assert!(!next.contains("We were on"), "resume line must not repeat: {next}");
}

#[test]
fn what_atlas_says_goes_through_its_manner() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "persona");
    let reply = d.turn("open chrome", 100);
    assert!(!reply.contains('\n'), "speech, not a document");
    assert!(reply.chars().next().unwrap().is_uppercase());
}

#[test]
fn the_busy_signal_is_real_now_rather_than_a_stub() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "busy");
    // A flow paused mid-chain for a yes is work in hand: the busy signal
    // must say so even though nothing sits in a queue lane any more.
    d.flows.record("big job", &["close chrome".into(), "research b".into()], Some("big job"));
    let asked = d.turn("big job", 100);
    assert!(asked.contains("go ahead?"), "{asked}");
    assert!(d.queue_is_busy(), "a flow awaiting its yes is work in hand");
}

#[test]
fn a_complete_command_containing_a_pronoun_is_not_questioned() {
    // "undo that" and "close that window" both contain pronouns, but the
    // first is already a complete instruction. Asking "which one?" about it
    // would be absurd.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "pronoun-complete");
    assert!(!d.turn("undo that", 100).contains("Which"));
    // A genuinely incomplete one still asks.
    assert_eq!(d.turn("close it", 110), "Which app?");
}
