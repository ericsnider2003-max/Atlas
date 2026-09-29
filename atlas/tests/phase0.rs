//! Phase 0: things that were built and tested but not reachable.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::hub::Page;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::server::{render, route, Action, Body, Reply, Request};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-p0-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}
fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}
fn plat() -> MockPlatform {
    MockPlatform::new(vec![
        Monitor { id: 1, x: 0, y: 0, width: 2560, height: 1392, primary: true },
        Monitor { id: 2, x: 2560, y: 0, width: 2560, height: 1392, primary: false },
    ])
}
fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}
fn req(method: &str, path: &str, body: &str) -> Request {
    Request {
        method: method.into(),
        path: path.into(),
        // `path` and `query` are separate now: `parse_request` splits on the
        // `?` before routing, so a route can no longer be missed because the
        // browser appended a query string to it.
        query: String::new(),
        token: Some("t".into()),
        // Not from the URL — a token in the query string gets one redirect to
        // set a cookie and is never accepted as a working credential for a
        // request that does something.
        token_from_url: false,
        body: body.into(),
    }
}

// ================= 0.4 the hub is actually served =================

#[test]
fn hub_pages_are_reachable_over_the_api() {
    assert_eq!(route(&req("GET", "/hub", "")), Some(Action::Hub(Page::Dashboard)));
    assert_eq!(route(&req("GET", "/hub/status", "")), Some(Action::Hub(Page::Status)));
    assert_eq!(route(&req("GET", "/hub/settings", "")), Some(Action::Hub(Page::Settings)));
    assert_eq!(route(&req("GET", "/hub/permissions", "")), Some(Action::Hub(Page::Permissions)));
}

#[test]
fn changing_a_setting_from_the_hub_is_a_routed_action() {
    let r = route(&req("POST", "/hub/set", "key=ocr.enabled&value=on"));
    assert_eq!(r, Some(Action::HubSet { key: "ocr.enabled".into(), value: "on".into() }));
}

#[test]
fn the_hub_and_the_api_do_not_leak_into_each_other() {
    // A page path must not resolve to a command, and vice versa.
    assert!(route(&req("GET", "/hub/../say", "")).is_none());
    assert!(route(&req("GET", "/say", "")).is_none(), "commands are POST only");
    assert!(route(&req("POST", "/hub/settings", "")).is_none());
}

#[test]
fn a_page_is_served_as_html_and_a_command_answers_as_json() {
    let page = render(&Reply::html("<html></html>"));
    assert!(page.contains("Content-Type: text/html"));
    assert!(page.contains("X-Content-Type-Options: nosniff"));

    let api = render(&Reply::ok("{}"));
    assert!(api.contains("Content-Type: application/json"));
    assert_eq!(Reply::ok("{}").kind, Body::Json);
}

#[test]
fn saving_a_setting_sends_you_back_with_a_get() {
    // Otherwise refreshing the page applies the change a second time.
    let r = render(&Reply::redirect("/hub/settings"));
    assert!(r.starts_with("HTTP/1.1 303 See Other"));
    assert!(r.contains("Location: /hub/settings"));
    assert!(r.contains("Content-Length: 0"));
}

#[test]
fn the_hub_is_behind_the_same_token_as_everything_else() {
    // Routing happens after authentication, so an unauthenticated request
    // never reaches a page either.
    let mut r = req("GET", "/hub/settings", "");
    r.token = None;
    assert!(route(&r).is_some(), "routing itself doesn't authenticate");
    // The server checks the token before routing — covered in server tests.
}

// ================= 0.5 clipboard and rehearsal by voice =================

#[test]
fn explain_this_reaches_the_clipboard() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "clip");
    d.clipboard_text = Some("thread 'main' panicked at src/lib.rs:12".into());
    let reply = d.turn("explain this", 100);
    assert!(reply.contains("an error"), "got: {reply}");
    assert!(d.pending_clipboard.is_some(), "the question is ready for the model");
}

#[test]
fn atlas_says_what_it_picked_up_so_you_know_it_got_the_right_thing() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "clip2");
    d.clipboard_text = Some("fn main() { let x = 1; }".into());
    // "explain what I copied", not "what does this mean" (changed 27 Sep
    // 2026): in conversation "what does this mean" is about what was just
    // said, so it goes to the model; naming the copy still reaches it.
    assert!(d.turn("explain what i copied", 100).contains("some code"));
}

#[test]
fn an_empty_clipboard_says_so_rather_than_guessing() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "clip3");
    d.clipboard_text = Some("   ".into());
    assert!(d.turn("explain this", 100).contains("nothing on the clipboard"));
}

#[test]
fn what_you_said_is_carried_into_the_clipboard_question() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "clip4");
    d.clipboard_text = Some("some prose here".into());
    d.turn("summarise this", 100);
    let q = d.pending_clipboard.clone().unwrap();
    assert!(q.starts_with("summarise this"), "your instruction leads: {q}");
    assert!(q.contains("some prose here"));
}

#[test]
fn the_answer_to_a_clipboard_request_goes_back_on_the_clipboard() {
    use atlas::brain::{Llm, MockLlm};
    use std::sync::Arc;
    let (c, p) = (cfg(), plat());
    let llm: Arc<dyn Llm> = Arc::new(MockLlm("It's a null-pointer panic on line 12.".into()));
    let mut d = Daemon::new(
        &c,
        &p,
        Some(llm),
        Store::new(tmp("clip-writeback")),
        Proactive::new(ProactiveConfig::default()),
    );
    d.clipboard_text = Some("thread 'main' panicked at src/lib.rs:12".into());
    let reply = d.turn("explain this", 100);
    // The model's answer reached you...
    assert!(reply.contains("null-pointer"), "the answer should be shown, got: {reply}");
    // ...and it's queued to go back on the clipboard for you to paste. The
    // platform layer takes this field after the turn and writes it out — the
    // mirror of how `clipboard_text` is set on the way in.
    let back = d.clipboard_writeback.take();
    assert_eq!(back.as_deref(), Some("It's a null-pointer panic on line 12."));
    assert!(d.clipboard_writeback.is_none(), "the write-back is taken once");
}

#[test]
fn a_rehearsal_shows_the_steps_and_moves_nothing() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "rehearse");
    let reply = d.turn("rehearse boot workspace", 100);
    assert!(reply.contains("nothing here actually happens"), "got: {reply}");
    assert!(reply.contains("steps"));
    // The real platform was never touched.
    assert!(p.actions().is_empty(), "a rehearsal must not reach the real windows");
}

#[test]
fn the_full_walk_through_is_kept_for_reading() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "rehearse2");
    d.turn("rehearse boot workspace", 100);
    let detail = d.last_rehearsal.clone().expect("the detail is kept");
    assert!(detail.contains("If you said"));
    assert!(detail.contains("open chrome"), "got:\n{detail}");
}

#[test]
fn rehearsing_something_destructive_marks_it_without_doing_it() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "rehearse3");
    let reply = d.turn("rehearse close chrome", 100);
    let detail = d.last_rehearsal.clone().unwrap();
    assert!(detail.contains("!1. close chrome"), "irreversible steps marked: {detail}");
    // The mark is in the stored walk-through; it must also reach the spoken
    // line, which is the one you hear before saying "go".
    assert!(
        reply.to_lowercase().contains("can't be undone"),
        "the spoken rehearsal warns about the irreversible step: {reply}"
    );
    assert!(p.actions().is_empty());
}

#[test]
fn a_rehearsal_with_no_irreversible_step_gives_no_undo_warning() {
    // Booting the workspace only opens, moves and focuses windows -- nothing
    // that can't be taken back -- so the warning must not fire, proving it is
    // gated on the reading and not stapled onto every rehearsal.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "rehearse5");
    let reply = d.turn("rehearse boot workspace", 100);
    assert!(
        !reply.to_lowercase().contains("can't be undone"),
        "a reversible rehearsal must not warn: {reply}"
    );
}

#[test]
fn a_rehearsal_of_something_it_cannot_rehearse_says_so() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "rehearse4");
    let reply = d.turn("rehearse what's the weather", 100);
    assert!(reply.to_lowercase().contains("don't know how to rehearse"), "got: {reply}");
}

// ================= 0.2 the queue drains =================

#[test]
fn queued_background_work_actually_runs() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "drain");
    d.queue.push("open chrome", atlas::lanes::Lane::Foreground);
    let before = d.queue.pending();
    // Screen work waits for a gap, so give it one.
    d.awareness.last_spoke = 0;
    d.tick(10_000);
    assert!(d.queue.pending() < before, "something should have run");
}

#[test]
fn queued_work_never_sneaks_past_the_approval_gate() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "drain-gate");
    d.queue.push("shutdown workspace", atlas::lanes::Lane::Foreground);
    d.awareness.last_spoke = 0;
    d.tick(10_000);
    assert!(
        d.backlog.outstanding().iter().any(|i| i.request.contains("shutdown")),
        "it should be filed as needing your go-ahead, not run"
    );
}


// ---------------------------------------------------------------------------
// Rearranging the dashboard is a routed action, and only the ones that exist.
// ---------------------------------------------------------------------------

#[test]
fn a_move_button_and_a_drop_route_to_the_same_action() {
    use atlas::dash::{Card, Move};
    let clicked = route(&req("POST", "/hub/dash", "what=up&card=machine"));
    assert_eq!(clicked, Some(Action::DashMove(Move::Up(Card::Machine))));

    let dropped = route(&req("POST", "/hub/dash", "what=to&card=machine&to=0"));
    assert_eq!(dropped, Some(Action::DashMove(Move::To(Card::Machine, 0))));
}

#[test]
fn arranging_is_a_mode_you_turn_on_and_off() {
    assert_eq!(
        route(&req("POST", "/hub/dash", "what=arrange")),
        Some(Action::DashArrange(true))
    );
    assert_eq!(
        route(&req("POST", "/hub/dash", "what=done")),
        Some(Action::DashArrange(false))
    );
}

#[test]
fn a_malformed_rearrange_does_nothing_rather_than_guessing() {
    // 27 Sep 2026: nothing moves, and it goes back to Home saying so, rather
    // than to "that isn't a page in Atlas" (which is what `None` became).
    for body in ["what=sideways&card=machine", "what=up&card=nonsense", ""] {
        match route(&req("POST", "/hub/dash", body)) {
            Some(Action::HubBack(atlas::hub::Page::Dashboard, said)) => {
                assert_eq!(said, "That didn't say which card or where to, so nothing moved.")
            }
            other => panic!("{body:?} routed to {other:?}"),
        }
    }
}

#[test]
fn rearranging_is_post_only() {
    // A GET that moves things is a link that rearranges your dashboard when a
    // browser prefetches it.
    assert!(route(&req("GET", "/hub/dash", "")).is_none());
}
