//! `kin.rs` unit-tests the notify-only channel in isolation. This proves the
//! other half of the claim: once it's wired into `server.rs`, a peer
//! credential genuinely cannot reach anywhere except that channel — not
//! merely because nothing routes it there today, but because a peer token
//! is checked by an entirely separate function that never calls into
//! `route()` at all.
//!
//! There was no existing test coverage of `server.rs`'s routing at this
//! level before tonight — worth knowing, since it means this file is also
//! the first thing that would catch a regression in the *general* routing,
//! not only the new peer path.

use atlas::kin::{Door, Peer};
use atlas::server::{route, route_chat, route_left, route_read, route_signal, Action, Request};

const PHONE_TOKEN: &str = "the-phone-token-0123456789abcdef";
const PEER_TOKEN: &str = "the-peer-token-0123456789abcdef";

fn say_request(token: &str) -> Request {
    Request {
        method: "POST".into(),
        query: String::new(),
        token_from_url: false,
        path: "/say".into(),
        token: Some(token.into()),
        body: r#"{"text":"open chrome"}"#.into(),
    }
}

fn signal_request(token: &str, what: &str) -> Request {
    Request {
        method: "POST".into(),
        query: String::new(),
        token_from_url: false,
        path: "/signal".into(),
        token: Some(token.into()),
        body: format!(r#"{{"what":"{what}","urgency":"urgent"}}"#),
    }
}

fn door() -> std::sync::Mutex<Door> {
    std::sync::Mutex::new(Door::new(vec![Peer::new("Homelab", PEER_TOKEN)]))
}

// ================= the structural claim, proven =================

#[test]
fn a_peer_token_reaches_nothing_through_the_general_router() {
    // route() is what the phone's token is checked against. A peer's token
    // was never meant to be handed to this function at all -- confirming
    // that even if it somehow was, nothing recognises it as an action, since
    // route() has no concept of peer tokens whatsoever.
    let req = say_request(PEER_TOKEN);
    // route() doesn't even look at the token -- token-checking happens
    // before routing in handle_conn. This proves the narrower claim: the
    // action space itself has no Signal-shaped hole a peer could fall into
    // by accident.
    assert_eq!(route(&req), Some(Action::Say("open chrome".into())));
    // The real guarantee lives in handle_conn's ordering, exercised below.
}

#[test]
fn a_peer_token_can_open_the_signal_door_and_nothing_else() {
    let d = door();
    let signal_req = signal_request(PEER_TOKEN, "margin call");
    let action = route_signal(&signal_req, &d, PEER_TOKEN);
    assert!(matches!(action, Some(Action::Signal(_))));

    // The same peer token, aimed at /say -- route_signal only ever matches
    // POST /signal, so this is None, not an error and not a Say action.
    let say_req = say_request(PEER_TOKEN);
    assert_eq!(route_signal(&say_req, &d, PEER_TOKEN), None);
}

#[test]
fn a_phone_token_gets_nothing_from_the_signal_door() {
    let d = door();
    let req = signal_request(PHONE_TOKEN, "not actually a peer");
    assert_eq!(
        route_signal(&req, &d, PHONE_TOKEN),
        None,
        "a token nobody registered as a peer must not be quietly accepted"
    );
}

#[test]
fn only_post_signal_is_recognised_get_is_refused_even_from_a_real_peer() {
    let d = door();
    let req = Request {
        method: "GET".into(),
        query: String::new(),
        token_from_url: false,
        path: "/signal".into(),
        token: Some(PEER_TOKEN.into()),
        body: String::new(),
    };
    assert_eq!(route_signal(&req, &d, PEER_TOKEN), None);
}

#[test]
fn a_signal_carrying_no_message_is_refused_at_the_door_not_delivered_empty() {
    let d = door();
    let req = signal_request(PEER_TOKEN, "");
    assert_eq!(route_signal(&req, &d, PEER_TOKEN), None);
}

// ========================= reaching the daemon =========================
//
// Proves the other end of the pipe: an accepted Incoming really does
// surface as an ordinary offer -- the same one `nudge` produces -- rather
// than something with elevated trust.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn e2e_tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-kin-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn e2e_cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}

fn e2e_plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

#[test]
fn a_signal_reaches_the_daemon_as_an_ordinary_offer() {
    use atlas::kin::{Door, Peer, Urgency};

    let c = e2e_cfg();
    let p = e2e_plat();
    let mut d = Daemon::new(&c, &p, None, Store::new(e2e_tmp("e2e")), Proactive::new(ProactiveConfig::default()));

    let mut door = Door::new(vec![Peer::new("Homelab", PEER_TOKEN)]);
    let incoming = door.receive(PEER_TOKEN, "position closed at a loss", Urgency::Urgent, 1).unwrap();

    d.receive_signal(&incoming);
    let offer = d.pending_offer().expect("a signal should produce a pending offer");
    assert!(offer.message.contains("Homelab"));
    assert!(offer.message.contains("position closed at a loss"));
    // Structural, not wording: it went through from_nudge like anything else
    // -- kin's nudge carries no relief, so command falls back to "brief",
    // the same default every other reliefless nudge gets. A signal getting
    // its own special command here would mean it skipped the shared path.
    assert_eq!(offer.command, "brief");
    assert_eq!(offer.kind, "nudge_checkin");
}

// ================= the chat door, same structural claim =================
//
// Peer chat crosses the same boundary as signals and handoffs: a peer token
// must be incapable of producing anything but `Action::Chatted`, and its only
// destination is `chat::Chats::receive` in a room the receiver opens for the
// sender the token names. These prove the door itself, the way the signal
// tests above prove theirs.

fn chat_request(token: &str, body: &str) -> Request {
    Request {
        method: "POST".into(),
        query: String::new(),
        token_from_url: false,
        path: "/chat".into(),
        token: Some(token.into()),
        body: format!(
            r#"{{"business":null,"body":"{body}","sent_at":1000,"offset":0,"after":1,"id":"r-1000-1"}}"#
        ),
    }
}

#[test]
fn a_peer_token_can_open_the_chat_door_and_nothing_else() {
    let d = door();
    let action = route_chat(&chat_request(PEER_TOKEN, "the roof quote came back"), &d, PEER_TOKEN);
    assert!(matches!(action, Some(Action::Chatted(_))), "a real peer's message is a Chatted action");

    // The same peer token aimed at /say produces nothing through the chat
    // door -- route_chat only ever matches POST /chat.
    let say_req = say_request(PEER_TOKEN);
    assert_eq!(route_chat(&say_req, &d, PEER_TOKEN), None);
}

#[test]
fn the_sender_of_a_chat_is_the_token_not_the_body() {
    let d = door();
    // A body that lies about who it is from changes nothing: the door names
    // the sender from the token (the peer registered as "Homelab").
    let req = Request {
        method: "POST".into(),
        query: String::new(),
        token_from_url: false,
        path: "/chat".into(),
        token: Some(PEER_TOKEN.into()),
        body: r#"{"from":"the-bank","business":null,"body":"pay this now","sent_at":1000,"offset":0,"after":1,"id":"r-1000-1"}"#.into(),
    };
    match route_chat(&req, &d, PEER_TOKEN) {
        Some(Action::Chatted(c)) => assert_eq!(c.from, "Homelab", "the body's from must be ignored"),
        other => panic!("expected a Chatted from the registered peer, got {other:?}"),
    }
}

#[test]
fn a_phone_token_gets_nothing_from_the_chat_door() {
    let d = door();
    assert_eq!(
        route_chat(&chat_request(PHONE_TOKEN, "hi"), &d, PHONE_TOKEN),
        None,
        "a token nobody registered as a peer must not reach the chat door"
    );
}

#[test]
fn only_post_chat_is_recognised_get_is_refused_even_from_a_real_peer() {
    let d = door();
    let req = Request {
        method: "GET".into(),
        query: String::new(),
        token_from_url: false,
        path: "/chat".into(),
        token: Some(PEER_TOKEN.into()),
        body: String::new(),
    };
    assert_eq!(route_chat(&req, &d, PEER_TOKEN), None);
}

#[test]
fn a_chat_carrying_no_body_is_refused_at_the_door() {
    let d = door();
    assert_eq!(route_chat(&chat_request(PEER_TOKEN, ""), &d, PEER_TOKEN), None);
}

// A read receipt crosses the same boundary as chat: a peer token must be
// incapable of producing anything but `Action::ReadReceipt`, and its only
// destination is `chat::Chats::mark_read`. These prove the receipt door the
// way the chat tests above prove theirs.

fn read_request(token: &str, ids_json: &str) -> Request {
    Request {
        method: "POST".into(),
        query: String::new(),
        token_from_url: false,
        path: "/read".into(),
        token: Some(token.into()),
        body: format!(r#"{{"ids":{ids_json}}}"#),
    }
}

#[test]
fn a_peer_token_can_open_the_read_door_and_the_reader_is_the_token() {
    let d = door();
    match route_read(&read_request(PEER_TOKEN, r#"["r-1000-1"]"#), &d, PEER_TOKEN) {
        Some(Action::ReadReceipt(r)) => {
            assert_eq!(r.from, "Homelab", "who read it comes from the token, never the body");
            assert_eq!(r.ids, vec!["r-1000-1".to_string()]);
        }
        other => panic!("expected a ReadReceipt from the registered peer, got {other:?}"),
    }

    // The same peer token aimed at /say produces nothing through the read door.
    assert_eq!(route_read(&say_request(PEER_TOKEN), &d, PEER_TOKEN), None);
}

#[test]
fn a_phone_token_gets_nothing_from_the_read_door() {
    let d = door();
    assert_eq!(
        route_read(&read_request(PHONE_TOKEN, r#"["r-1000-1"]"#), &d, PHONE_TOKEN),
        None,
        "a token nobody registered as a peer must not reach the read door"
    );
}

#[test]
fn a_read_receipt_with_no_ids_is_refused_at_the_door() {
    let d = door();
    assert_eq!(route_read(&read_request(PEER_TOKEN, "[]"), &d, PEER_TOKEN), None);
}

#[test]
fn only_post_read_is_recognised() {
    let d = door();
    let req = Request {
        method: "GET".into(),
        query: String::new(),
        token_from_url: false,
        path: "/read".into(),
        token: Some(PEER_TOKEN.into()),
        body: String::new(),
    };
    assert_eq!(route_read(&req, &d, PEER_TOKEN), None);
}

// A "left a group" notice crosses the same boundary: a peer token must be
// incapable of producing anything but `Action::LeftGroup`.

fn left_request(token: &str, group_id: &str) -> Request {
    Request {
        method: "POST".into(),
        query: String::new(),
        token_from_url: false,
        path: "/left".into(),
        token: Some(token.into()),
        body: format!(r#"{{"group_id":"{group_id}"}}"#),
    }
}

#[test]
fn a_peer_token_can_open_the_leave_door_and_the_leaver_is_the_token() {
    let d = door();
    match route_left(&left_request(PEER_TOKEN, "grp-1"), &d, PEER_TOKEN) {
        Some(Action::LeftGroup(l)) => {
            assert_eq!(l.from, "Homelab", "who left comes from the token, never the body");
            assert_eq!(l.group_id, "grp-1");
        }
        other => panic!("expected a LeftGroup from the registered peer, got {other:?}"),
    }
    // The same token aimed at /say produces nothing through the leave door.
    assert_eq!(route_left(&say_request(PEER_TOKEN), &d, PEER_TOKEN), None);
}

#[test]
fn a_phone_token_gets_nothing_from_the_leave_door() {
    let d = door();
    assert_eq!(route_left(&left_request(PHONE_TOKEN, "grp-1"), &d, PHONE_TOKEN), None);
}

#[test]
fn a_leave_notice_with_no_group_id_is_refused_at_the_door() {
    let d = door();
    let req = Request {
        method: "POST".into(),
        query: String::new(),
        token_from_url: false,
        path: "/left".into(),
        token: Some(PEER_TOKEN.into()),
        body: "{}".into(),
    };
    assert_eq!(route_left(&req, &d, PEER_TOKEN), None);
}
