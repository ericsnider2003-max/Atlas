//! The second look for gaps (25 Sep 2026): the new work goes through the
//! systems Atlas already has, not beside them.
//!
//! Eric: "look for gaps and fix them based off what you find in the
//! system." What the system says, and what these hold:
//!
//! - **The tick never waits** (`crew`). A model writing a window reply, the
//!   picture reader, and a call being transcribed each take seconds to
//!   minutes; each is now a crew errand, not work on the tick or a thread
//!   of its own.
//! - **Every model call is recorded** (`record_model_call`).
//! - **What goes out as you is on the record** (`activity`: Published).
//! - **Everything Atlas does as you outside itself has a switch** (Settings,
//!   "Reaching outside this machine").

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::intent::Intent;
use atlas::platform::mock::{Action, MockPlatform};
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-crewgaps-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn show(p: &MockPlatform, id: u64, process: &str, text: &str) {
    p.focus_on(process, "");
    *p.front.borrow_mut() = Some(atlas::platform::WindowId(id));
    p.set_window_text(id, text);
}

fn typed(p: &MockPlatform) -> Vec<String> {
    p.actions().into_iter().filter_map(|a| match a { Action::Type(t) => Some(t), _ => None }).collect()
}

/// A model that takes its time, the way a local one on a laptop does.
struct Slow(Duration, &'static str);
impl atlas::brain::Llm for Slow {
    fn complete(&self, _system: &str, _user: &str) -> atlas::error::Result<String> {
        std::thread::sleep(self.0);
        Ok(self.1.into())
    }
}

fn daemon<'a>(p: &'a MockPlatform, cfg: Config, tag: &str) -> Daemon<'a> {
    let cfg: &'static Config = Box::leak(Box::new(cfg));
    Daemon::new(cfg, p, None, Store::new(scratch(tag)), Proactive::new(ProactiveConfig::default()))
}

/// Ticks until `done`, or a few seconds.
fn until(d: &mut Daemon, from: u64, done: impl Fn(&Daemon, &[String]) -> bool) -> Vec<String> {
    let mut out = Vec::new();
    for k in 0..600u64 {
        out.extend(d.tick(from + k));
        if done(d, &out) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    out
}

#[test]
fn a_slow_model_writing_a_reply_never_holds_up_the_tick() {
    let p = plat();
    let mut d = daemon(&p, Config::load(Path::new("config")).unwrap(), "slow-model");
    d.llm = Some(Arc::new(Slow(Duration::from_millis(800), "Friday works.")));
    // The first tick of a fresh daemon does its start-up looking around
    // (most of a second here, model or no model); what's measured is a tick
    // with the model busy, so that one's out of the way first.
    let _ = d.tick(atlas::store::now() + 40);
    show(&p, 7, "SomeChat.exe", "Sam: Friday?");
    let began = Instant::now();
    let said = d.execute_timed(&Intent::Delegate("draft a reply to this".into()), "draft a reply to this");
    assert!(began.elapsed() < Duration::from_millis(400), "asking waited for the model: {:?}", began.elapsed());
    assert!(said.starts_with("Drafting a reply in SomeChat"), "{said}");
    let t = atlas::store::now() + 50;
    let one = Instant::now();
    let _ = d.tick(t);
    assert!(one.elapsed() < Duration::from_millis(400), "a tick waited for the model: {:?}", one.elapsed());
    let _ = until(&mut d, t + 1, |d, _| d.working_for_you.is_empty());
    assert_eq!(typed(&p), vec!["Friday works.".to_string()]);
}

#[test]
fn a_reply_sent_as_you_is_on_the_record() {
    let p = plat();
    let mut d = daemon(&p, Config::load(Path::new("config")).unwrap(), "journal");
    d.llm = Some(Arc::new(Slow(Duration::from_millis(1), "On my way.")));
    show(&p, 7, "SomeChat.exe", "Sam: where are you?");
    let _ = d.execute_timed(&Intent::Delegate("take over this conversation".into()), "take over this conversation");
    let _ = until(&mut d, atlas::store::now() + 50, |_, _| !typed(&p).is_empty());
    let sent = d.journal.events.iter().find(|e| e.what.starts_with("replied in SomeChat"));
    let sent = sent.expect("a message went out as you and nothing recorded it");
    assert_eq!(sent.kind, atlas::activity::Kind::Published);
    assert!(sent.what.contains("On my way."));
}

#[test]
fn working_your_apps_has_a_switch_and_off_means_off() {
    let mut cfg = Config::load(Path::new("config")).unwrap();
    assert!(cfg.tools.as_ref().unwrap().delegate.enabled, "Eric said yes; it ships on");
    let s = atlas::settings::registry(cfg.tools.as_ref().unwrap());
    assert!(s.get("delegate.enabled").is_some(), "no switch for writing as you");
    cfg.tools.as_mut().unwrap().delegate.enabled = false;
    let p = plat();
    let mut d = daemon(&p, cfg, "switch-off");
    d.llm = Some(Arc::new(Slow(Duration::from_millis(1), "x")));
    show(&p, 7, "SomeChat.exe", "Sam: hi");
    let said = d.execute_timed(&Intent::Delegate("take over this conversation".into()), "take over this conversation");
    assert!(said.contains("switched off"), "{said}");
    assert!(d.working_for_you.is_empty());
}

#[test]
fn the_turn_limit_comes_from_your_settings() {
    let mut cfg = Config::load(Path::new("config")).unwrap();
    cfg.tools.as_mut().unwrap().delegate.max_turns = 3;
    let p = plat();
    let mut d = daemon(&p, cfg, "turns");
    d.llm = Some(Arc::new(Slow(Duration::from_millis(1), "ok")));
    show(&p, 7, "SomeChat.exe", "Sam: hi");
    let _ = d.execute_timed(&Intent::Delegate("take over this conversation".into()), "take over this conversation");
    assert_eq!(d.working_for_you[0].job.max_turns, 3);
}

#[test]
fn a_finished_call_is_written_up_by_the_crew_and_its_summary_recorded() {
    let dir = scratch("call-crew");
    let mut cfg = Config::load(Path::new("config")).unwrap();
    {
        let t = cfg.tools.as_mut().unwrap();
        t.call_notes.enabled = true;
        t.research.notes_dir = dir.join("notes").display().to_string();
        // A stand-in transcriber: writes one timed line for any recording.
        t.stt_timed = Some(
            serde_yaml::from_str(
                "command: sh\nargs: [\"-c\", \"printf '1\\\\n00:00:00,000 --> 00:00:01,000\\\\nShall we ship Friday?\\\\n' > {srt}\"]\nresult_file: \"{srt}\"\n",
            )
            .unwrap(),
        );
    }
    let p = plat();
    let mut d = daemon(&p, cfg, "call-crew-store");
    d.llm = Some(Arc::new(Slow(Duration::from_millis(1), "Agreed to ship Friday.")));
    d.call_notes.starter = atlas::callrec::silent;
    d.call_notes.dir = dir.join("calls");
    let _ = d.execute_timed(&Intent::CallNotes("start".into()), "take notes on this call");
    std::thread::sleep(Duration::from_millis(1_300));
    let stop = d.execute_timed(&Intent::CallNotes("stop".into()), "stop taking notes");
    assert!(stop.contains("writing up"), "{stop}");
    assert!(
        d.crew.errands().iter().any(|e| e.name == "call-notes"),
        "the write-up isn't one of the errands Atlas can name and stop"
    );
    let out = until(&mut d, atlas::store::now() + 50, |_, out| out.iter().any(|l| l.contains("call notes are written")));
    assert!(out.iter().any(|l| l.contains("call notes are written")), "{out:?}");
    let notes = std::fs::read_dir(dir.join("notes")).unwrap().flatten().next().expect("no notes file");
    let text = std::fs::read_to_string(notes.path()).unwrap();
    assert!(text.contains("Agreed to ship Friday."), "{text}");
    assert!(d.journal.events.iter().any(|e| e.what.starts_with("call notes:")), "not on the record");
}

#[test]
fn the_new_errands_can_be_named_when_you_say_stop() {
    use atlas::which_errand::{describe, Candidate};
    let c = |label: &str, topic: &str| Candidate {
        id: 1,
        label: label.into(),
        topic: Some(topic.into()),
        started: 0,
        paused: false,
        can_hold: false,
    };
    assert_eq!(describe(&c("pictures", "screen")), "the look at your screen");
    assert_eq!(describe(&c("call-notes", "Zoom")), "the write-up of the Zoom call");
    assert_eq!(describe(&c("conversation", "Slack")), "the conversation in Slack");
    let cands = vec![c("pictures", "screen"), Candidate { id: 2, ..c("call-notes", "Zoom") }];
    use atlas::which_errand::{pick, Pick, Verb};
    assert_eq!(pick(Verb::Cancel, "the looking", &cands, &[], 0), Pick::These(vec![1], atlas::which_errand::Why::Named));
    assert_eq!(pick(Verb::Cancel, "the call write-up", &cands, &[], 0), Pick::These(vec![2], atlas::which_errand::Why::Named));
}

#[cfg(unix)]
#[test]
fn the_picture_reader_can_be_stopped_while_it_looks() {
    use std::os::unix::fs::PermissionsExt;
    // A stand-in reader that would take a minute: `ask_until` ends it the
    // moment it's asked to, which is what "stop" said while Atlas is looking
    // reaches through the crew.
    let root = scratch("pic-stop");
    let prog = root.join("slow-reader");
    std::fs::write(&prog, "#!/bin/sh\nsleep 60\n").unwrap();
    std::fs::set_permissions(&prog, std::fs::Permissions::from_mode(0o755)).unwrap();
    for f in ["m", "p"] {
        std::fs::write(root.join(f), b"x").unwrap();
    }
    let cfg = atlas::picture_talk::PictureTalkConfig {
        program: "slow-reader".into(),
        model: "m".into(),
        projector: "p".into(),
        ..Default::default()
    };
    atlas::picture_talk::ready(&cfg, &root).expect("the stand-in should count as installed");
    let img = root.join("i.png");
    std::fs::write(&img, b"x").unwrap();
    let asked = Instant::now();
    let flag = std::sync::atomic::AtomicBool::new(false);
    std::thread::scope(|s| {
        s.spawn(|| {
            std::thread::sleep(Duration::from_millis(300));
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        let r = atlas::picture_talk::ask_until(&cfg, &root, &img, "what's this?", &|| {
            flag.load(std::sync::atomic::Ordering::SeqCst)
        });
        assert_eq!(r, Err("you asked me to stop".to_string()));
    });
    assert!(asked.elapsed() < Duration::from_secs(5), "stop didn't reach it: {:?}", asked.elapsed());
}

