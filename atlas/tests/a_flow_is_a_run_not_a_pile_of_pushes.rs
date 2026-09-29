//! A triggered workflow is a `flow::Run`, not a pile of queue pushes.
//!
//! What the flatten-into-the-queue shape threw away, measured: `on_fail` and
//! `produces` were discarded at trigger time, so an optional step stopped
//! the chain, a retrying step never retried, `{name}` substitution ran with
//! the braces still in the command — and `Run`, `needs_approval` and `deny`
//! sat fully built with tests and no caller. These tests hold the wiring:
//! the daemon drives a `Run`, a consequential step pauses the WHOLE chain
//! for a yes, and a no abandons the rest rather than half-doing it.
//!
//! The mind is wired through the same seam, because this is the first work
//! the daemon has ever actually handed it: before this, `Mind::begin` had no
//! caller, so "what are you doing?" was structurally incapable of any answer
//! but "nothing".

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-flowrun-{tag}"));
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

/// Save a chain of benign steps under a trigger phrase.
fn save_flow(d: &mut Daemon, name: &str, steps: &[&str], trigger: &str) {
    let cmds: Vec<String> = steps.iter().map(|s| s.to_string()).collect();
    d.flows.record(name, &cmds, Some(trigger));
}

#[test]
fn a_benign_flow_runs_to_done_in_the_triggering_turn() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "done");
    save_flow(&mut d, "morning", &["resume", "what can you do"], "start the morning");

    let reply = d.turn("start the morning", 1_000);
    assert!(reply.contains("Running morning, 2 steps"), "{reply}");
    assert!(reply.contains("done"), "a two-step benign flow should finish in the turn: {reply}");
    // Finished means finished: nothing left in flight for the tick to move.
    assert!(
        d.mind.focus().is_none(),
        "the mind should have nothing active after the flow completed"
    );
}

#[test]
fn the_mind_finally_has_something_true_to_say_while_a_flow_runs() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "mind");
    // "close chrome" requires approval, so the run pauses there — which is
    // exactly when "what are you doing?" has a real answer.
    save_flow(&mut d, "shutdown", &["resume", "close chrome"], "wind down");

    let reply = d.turn("wind down", 1_000);
    assert!(reply.contains("go ahead?"), "should have paused to ask: {reply}");

    let w = d.mind.focus().expect("a flow in flight is work the mind knows about");
    assert_eq!(w.asked, "shutdown");
    let (done, total) = w.progress();
    assert_eq!(total, 2, "the plan is the flow's own steps");
    assert_eq!(done, 1, "the benign step is done, the paused one is not");
    assert!(
        !w.recent_thinking(3).is_empty(),
        "the pause was thought about, in words a person can be shown"
    );
}

#[test]
fn a_consequential_step_pauses_the_chain_and_yes_resumes_it() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "yes");
    save_flow(&mut d, "shutdown", &["resume", "close chrome", "what can you do"], "wind down");

    let asked = d.turn("wind down", 1_000);
    assert!(asked.contains("close chrome"), "the question names the step: {asked}");

    let after = d.turn("yes", 2_000);
    assert!(
        after.contains("done"),
        "yes resumes the chain and the remaining steps run: {after}"
    );
}

#[test]
fn no_abandons_the_rest_rather_than_half_doing_it() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "no");
    save_flow(&mut d, "shutdown", &["resume", "close chrome", "what can you do"], "wind down");

    let asked = d.turn("wind down", 1_000);
    assert!(asked.contains("go ahead?"), "{asked}");

    let after = d.turn("no", 2_000);
    assert!(
        after.contains("stopped") && after.contains("declined"),
        "a refusal mid-chain abandons the rest and says so: {after}"
    );
    // And the next turn is not haunted by it.
    let quiet = d.turn("resume", 3_000);
    assert!(
        !quiet.contains("go ahead?"),
        "the declined flow must not re-ask on a later turn: {quiet}"
    );
}

#[test]
fn a_paused_flow_stays_paused_across_ticks_rather_than_re_asking() {
    // The scheduled-post shape of this bug: a thing awaiting an answer that
    // comes round every two-second tick and asks again, indefinitely.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "tick");
    save_flow(&mut d, "shutdown", &["close chrome"], "wind down");

    let asked = d.turn("wind down", 1_000);
    assert!(asked.contains("go ahead?"), "{asked}");
    for i in 0..5u64 {
        let out = d.tick(2_000 + i * 2_000);
        assert!(
            out.iter().all(|m| !m.contains("go ahead?")),
            "tick {i} re-asked the pending question: {out:?}"
        );
    }
}

#[test]
fn what_are_you_doing_reports_background_work_from_the_panel() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "bg");
    // A flow pauses on approval, then a second one arrives: the first is
    // still active work the mind is holding, and `take_on` records the
    // hand-over rather than losing it.
    save_flow(&mut d, "first", &["close chrome"], "do the first thing");
    save_flow(&mut d, "second", &["resume", "close chrome"], "do the second thing");

    let one = d.turn("do the first thing", 1_000);
    assert!(one.contains("go ahead?"), "{one}");
    let two = d.turn("do the second thing", 10_000);
    assert!(two.contains("Running second"), "{two}");

    assert!(
        d.mind.active().len() >= 2,
        "both jobs are live work: {:?}",
        d.mind.active().iter().map(|w| w.asked.clone()).collect::<Vec<_>>()
    );
}
