//! The Supervisor governs something.
//!
//! `lifecycle::Supervisor` was machinery running over an empty list.
//! `daemon.rs` called `self.helpers.reap(t)` every tick, and `acquire` — the
//! only thing that ever pushes into `running` — had **zero callers anywhere
//! in `src`**. `running` was `Vec::new()` at construction and stayed empty
//! forever. So the LRU eviction, the memory budget, the
//! never-evict-something-mid-job rule and the refuse-with-a-sentence path
//! were all built, all tested, and all unreachable, and every tick reaped
//! nothing.
//!
//! Two reasons that mattered more than a typical dead module: this is
//! exactly the machinery meant to keep whisper, piper and llama-server warm
//! under a real budget on a laptop where the biggest measured cost in a turn
//! was the model server unloading between turns; and `LifecycleConfig` was
//! being parsed out of `tools.yaml` and thrown away, because the daemon
//! built `Supervisor::default()` rather than `Supervisor::new(cfg)`.
//!
//! These tests are structural on purpose — they assert the *wiring*, which
//! is what was missing, not the algorithm, which was already covered by
//! `tests/resources.rs`.

use atlas::lifecycle::{Helpers, LifecycleConfig, Order, Supervisor};
use std::collections::BTreeMap;

fn src(name: &str) -> String {
    crate::common::read_source_path(&format!("src/{name}")).unwrap_or_default()
}

#[test]
fn acquire_has_callers_in_production_code() {
    // The assertion that would have caught this on the day it was written.
    // `want` is the one wrapper that calls `acquire`; if nothing calls
    // `want` either, the Supervisor is decoration again.
    let lifecycle = src("lifecycle.rs");
    assert!(
        lifecycle.contains("self.sup.acquire("),
        "nothing in lifecycle.rs calls acquire — the Supervisor is back to \
         reaping an empty list"
    );

    // Whitespace-collapsed before counting, on purpose. Five of the six
    // source-text guards in this tree have needed a correction because they
    // were matching a substring that formatting could move — `rustfmt`
    // breaking `self.helpers\n    .want(` across two lines is exactly that
    // failure. Counting against the collapsed text takes formatting out of
    // the answer.
    let daemon = src("daemon.rs");
    let flat: String = daemon.split_whitespace().collect::<Vec<_>>().join(" ").replace(" .", ".");
    let calls = flat.matches(".helpers.want(").count();
    assert!(
        calls >= 3,
        "only {calls} production call(s) to helpers.want — the camera, the \
         model server and the panel window should each ask before they start"
    );
}

#[test]
fn the_daemon_takes_its_budget_from_the_config_not_from_default() {
    // `ToolsConfig.lifecycle` was parsed and discarded: `budget_mb`,
    // `idle_timeout_secs` and `keep_warm_secs` from tools.yaml never reached
    // the running Supervisor, because the daemon said `Supervisor::default()`.
    let daemon = src("daemon.rs");
    assert!(
        !daemon.contains("Supervisor::default()"),
        "the daemon is building its Supervisor from defaults again, so the \
         lifecycle section of tools.yaml is parsed and thrown away"
    );
    assert!(
        daemon.contains("Helpers::new("),
        "the daemon should build its helpers from the configured lifecycle"
    );
}

#[test]
fn the_model_server_is_actually_started_rather_than_described() {
    // `which_model` used to end at "Not running — I'd start it with N
    // layers", and `models::launch` had no caller anywhere in `src`. A
    // sentence describing an action nothing performs is the exact shape this
    // project keeps finding.
    let daemon = src("daemon.rs");
    assert!(
        daemon.contains("models::launch("),
        "nothing starts the model server; `which_model` is describing an \
         action again"
    );
    assert!(
        !daemon.contains("I'd start it with"),
        "`which_model` still only says it would start the server"
    );
}

#[test]
fn nothing_atlas_starts_outlives_it() {
    // `models::launch` was spawn-and-forget: no handle, so nothing could
    // stop it and Atlas exiting left a multi-gigabyte server behind.
    let models = src("models.rs");
    assert!(
        models.contains("Result<std::process::Child>"),
        "launch drops the child on the floor again — nothing can stop what it \
         started"
    );
    let lifecycle = src("lifecycle.rs");
    assert!(
        lifecycle.contains("impl Drop for Helpers"),
        "helpers are no longer killed when Atlas exits"
    );
}

#[test]
fn every_wired_helper_has_a_weight_somebody_decided_on() {
    // A helper wired in without a size would silently get the fallback and
    // the budget would mean less than it says.
    for name in ["cdp", "model-server", "camera", "hidden_desktop", "whisper", "tts"] {
        assert!(
            atlas::lifecycle::typical_mb(name) > 0,
            "{name} has no weight in the budget table"
        );
    }
    assert_eq!(atlas::lifecycle::typical_mb("something-new"), 100, "the fallback moved");
}

#[test]
fn a_helper_that_fails_to_start_does_not_keep_spending_the_budget() {
    // The failure mode that would make the whole thing worse than nothing:
    // book 4400MB for a server that never came up, then refuse everything
    // afterwards for lack of room.
    let mut h = Helpers::new(LifecycleConfig { budget_mb: 500, ..LifecycleConfig::default() });
    let failed = h.want("model-server", 400, 10, || Err("no such binary".into()));
    assert!(failed.is_err());
    assert_eq!(h.memory_mb(), 0, "a helper that never started is still on the books");

    // And the next one is not refused because of it.
    let ok = h.want("camera", 400, 11, || Ok(None));
    assert!(ok.is_ok(), "the budget is still being spent on something that isn't running");
}

#[test]
fn a_closed_camera_gives_its_memory_back_immediately() {
    // `finished` rather than `done`: the camera really is closed on the way
    // out of `one_frame`, so keeping it warm would spend 60MB on a device
    // that is not open.
    let mut h = Helpers::new(LifecycleConfig::default());
    h.want("camera", 60, 1, || Ok(None)).unwrap();
    assert_eq!(h.memory_mb(), 60);
    h.finished("camera");
    assert_eq!(h.memory_mb(), 0);
    assert!(!h.is_running("camera"));
}

#[test]
fn something_warm_is_reused_rather_than_started_twice() {
    let mut h = Helpers::new(LifecycleConfig::default());
    let mut starts = 0;
    h.want("model-server", 100, 1, || {
        starts += 1;
        Ok(None)
    })
    .unwrap();
    h.done("model-server", 1);
    h.want("model-server", 100, 2, || {
        starts += 1;
        Ok(None)
    })
    .unwrap();
    assert_eq!(starts, 1, "a warm helper was started a second time");
    assert_eq!(h.memory_mb(), 100, "it was also counted twice");
}

#[test]
fn the_budget_evicts_the_least_recently_used_and_says_so() {
    let mut h = Helpers::new(LifecycleConfig {
        budget_mb: 300,
        idle_timeout_secs: 90,
        keep_warm_secs: BTreeMap::new(),
    });
    h.want("a", 100, 1, || Ok(None)).unwrap();
    h.done("a", 1);
    h.want("b", 100, 2, || Ok(None)).unwrap();
    h.done("b", 2);

    let said = h.want("big", 200, 3, || Ok(None)).unwrap();
    assert!(!said.is_empty(), "something had to go and nothing was said about it");
    assert!(said[0].contains('a'), "the least recently used one should have gone first: {said:?}");
    assert!(!h.is_running("a"));
    assert!(h.is_running("big"));
}

#[test]
fn a_refusal_is_a_sentence_a_person_can_read() {
    let mut h = Helpers::new(LifecycleConfig { budget_mb: 100, ..LifecycleConfig::default() });
    // In use, so it cannot be evicted.
    h.want("busy", 100, 1, || Ok(None)).unwrap();

    // The behaviour first, the wording second. A refusal that still ran the
    // start closure would have spawned the process it just said there was no
    // room for -- and every assertion in this test was about the *sentence*,
    // which would not have noticed. `tests/retrospective.rs` counts a test
    // whose every assertion is `.contains(..)` as documentation with a
    // harness around it; it counted this one, and it had a point.
    let mut started = false;
    let why = h
        .want("model-server", 100, 2, || {
            started = true;
            Ok(None)
        })
        .unwrap_err();
    assert!(!started, "the refusal still ran the thing it refused to start");

    assert!(why.contains("budget"), "the refusal does not say why: {why}");
    assert!(!why.contains("Refuse("), "the refusal is a Debug format, not English");
}

#[test]
fn orders_read_as_english_not_as_variant_names() {
    assert_eq!(Order::Start("camera".into()).plain(), "starting camera");
    assert!(Order::Stop("cdp".into()).plain().contains("idle"));
    assert_eq!(Order::Reuse("tts".into()).who(), Some("tts"));
    assert_eq!(Order::Refuse("no room".into()).who(), None);
}

#[test]
fn reaping_still_works_through_the_wrapper() {
    // The one thing that was already being called every tick — it must not
    // have been broken on the way through.
    let mut sup = Supervisor::new(LifecycleConfig {
        budget_mb: 1000,
        idle_timeout_secs: 10,
        keep_warm_secs: BTreeMap::new(),
    });
    sup.acquire("x", 10, 0);
    sup.release("x", 0);
    assert!(sup.reap(5).is_empty(), "reaped something that was still inside its timeout");
    assert_eq!(sup.reap(20).len(), 1, "an idle helper was not reaped");
}
