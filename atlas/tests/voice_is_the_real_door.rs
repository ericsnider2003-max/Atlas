//! Both front doors reach the same Atlas.
//!
//! `atlas voice` (and `atlas voice --wake`) went to `voice_loop`, which
//! called `handle`. `handle` covers **five** of the 56 `Intent` variants —
//! `WorkspaceOn`, `WorkspaceOff`, `ViewDisplay`, `CaptureWebcam`, and
//! `Say`/`Ask` — and answered everything else with:
//!
//! ```text
//! parsed ReviewPost("…") — not wired to an action yet
//! ```
//!
//! `Daemon::execute` has 89 `Intent::` arms. So the command whose name most
//! obviously means "talk to Atlas" was a legacy stub that answered about a
//! tenth of what Atlas can do, in Rust — while `prompt_line`, the *typed*
//! door, had been building a real daemon for weeks and documenting `handle`
//! as the no-`tools.yaml` fallback. `voice_loop` never built a daemon at
//! all, and nothing said so at the point someone would type the wrong
//! command.
//!
//! These are source-shape assertions because the thing that was wrong was
//! the wiring. Running `atlas voice` for real needs a microphone, which is
//! named in the report as unverifiable here.


fn main_rs() -> String {
    crate::common::source_of("main")
}

/// The body of a named function in `main.rs`, by brace matching.
fn body_of(src: &str, signature_starts_with: &str) -> String {
    let start = src
        .find(signature_starts_with)
        .unwrap_or_else(|| panic!("no function starting `{signature_starts_with}`"));
    let rest = &src[start..];
    let open = rest.find('{').expect("no body");
    let mut depth = 0i32;
    for (i, c) in rest[open..].char_indices() {
        if c == '{' {
            depth += 1;
        } else if c == '}' {
            depth -= 1;
            if depth == 0 {
                return rest[open..open + i + 1].to_string();
            }
        }
    }
    panic!("unbalanced braces after `{signature_starts_with}`");
}

#[test]
fn the_voice_door_builds_a_daemon() {
    let body = body_of(&main_rs(), "fn voice_loop(");
    assert!(
        body.contains("Daemon::new("),
        "`atlas voice` still answers without a daemon — five intents out of \
         fifty-six"
    );
    assert!(
        body.contains("execute_timed("),
        "`atlas voice` builds a daemon and then doesn't ask it anything"
    );
}

#[test]
fn the_voice_door_no_longer_falls_through_to_the_five_intent_stub() {
    let body = body_of(&main_rs(), "fn voice_loop(");
    assert!(
        !body.contains("handle(cfg, plat, parser, approver"),
        "`voice_loop` is calling `handle` again — that is the stub"
    );
}

#[test]
fn the_mic_it_just_picked_is_the_one_the_daemon_gets() {
    // The subtle way this fix goes wrong: `voice_loop` works out which
    // microphone can actually hear you and writes `mic_device` into a
    // *clone* of the tools config. Hand `Daemon::new` the original `cfg` and
    // the daemon holds a stale view — two configurations, one of them
    // missing the measurement that was the whole point of the code above it.
    let body = body_of(&main_rs(), "fn voice_loop(");
    assert!(
        body.contains("cfg_owned.tools = Some(tc_owned)"),
        "the measured microphone is not being carried into the config the \
         daemon reads"
    );
    assert!(
        body.contains("Daemon::new(\n        &cfg_owned,") || body.contains("&cfg_owned,"),
        "the daemon is built from a config that does not have the picked mic"
    );
}

#[test]
fn the_voice_door_takes_the_single_instance_lock() {
    // It reads and writes `data/state` now. Two long-running Atlases both
    // loading state, changing it in memory and writing the whole thing back
    // means the second silently erasing what the first learned — which is
    // exactly why `run_daemon` has taken this lock all along.
    let body = body_of(&main_rs(), "fn voice_loop(");
    assert!(
        body.contains("OnlyOne::at("),
        "`atlas voice` can now clobber the daemon's state and does not take \
         the lock"
    );
}

#[test]
fn the_gate_is_applied_on_the_voice_path_too() {
    let body = body_of(&main_rs(), "fn voice_loop(");
    assert!(
        body.contains("gate_with_identity("),
        "spoken commands bypass the identity gate that typed ones go through"
    );
}

#[test]
fn both_doors_work_out_the_model_connection_the_same_way() {
    // They did not: the derived-model logic lived inline in `run_daemon`, so
    // the other door could not reach it without copying fifty lines — and
    // so it didn't.
    let src = main_rs();
    assert!(src.contains("fn model_connection("), "the shared helper is gone");
    let voice = body_of(&src, "fn voice_loop(");
    let daemon = body_of(&src, "fn run_daemon(");
    assert!(voice.contains("model_connection("), "the voice door has no model");
    assert!(daemon.contains("model_connection("), "the daemon door stopped using the helper");
}

#[test]
fn handle_is_only_the_no_tools_yaml_fallback_and_says_so_in_english() {
    // It stays — `prompt_line` documents it as the fallback for a machine
    // with no `config/tools.yaml`, and a prompt that refuses to start is
    // worse than one that can only do six things. But its fallback arm used
    // to print a Rust variant name.
    let body = body_of(&main_rs(), "fn handle(");
    assert!(!body.contains(":?}"), "`handle` still debug-formats an intent onto the screen");
    assert!(
        body.contains("tools.yaml"),
        "`handle`'s fallback should say why it can only do a few things"
    );
}

#[test]
fn prompt_line_still_prefers_the_daemon() {
    // The door that was already right must stay right.
    //
    // This used to assert `execute_timed(` as the proxy for "reaches the
    // daemon". On 18 Sep 2026 the door got better rather than worse:
    // `prompt_line` now calls `d.turn(...)`, which is `execute_timed` plus
    // the model, the assembled context and the conversation thread. The
    // proxy went stale; the property it stood for is unchanged and is
    // asserted directly here. `tests/typing_is_a_conversation_too.rs` pins
    // the new call site on its own terms.
    let body = body_of(&main_rs(), "fn prompt_line(");
    assert!(
        body.contains("d.turn(") || body.contains("d.execute_timed("),
        "prompt_line no longer reaches the daemon at all"
    );
    assert!(body.contains("handle(cfg, plat, parser, approver"), "the fallback is gone");
}

#[test]
fn the_daemon_covers_far_more_than_the_fallback_ever_did() {
    // The number that made this finding worth acting on. If `handle` ever
    // grows toward `execute`, someone is rebuilding the daemon by hand and
    // should be told to stop.
    let daemon = crate::common::source_of("daemon");
    let arms = daemon.matches("Intent::").count();
    let handle_arms = body_of(&main_rs(), "fn handle(").matches("Intent::").count();
    assert!(arms > 60, "the daemon only handles {arms} intent references");
    assert!(
        handle_arms < 12,
        "`handle` now names {handle_arms} intents — it is growing into a \
         second daemon instead of staying the no-config fallback"
    );
}
