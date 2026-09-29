//! Reaching the phone when the laptop isn't with you.
//!
//! The last gap in the delivery chain. Atlas speaks, then draws its own
//! window, then holds — and holding means you hear nothing at all if you have
//! left the building. These tests cover the routing decision and the message
//! itself; the POST is exercised against a real socket at the bottom, because
//! a push client that has only ever been tested against a mock is a push
//! client nobody has tested.

use atlas::notify::{route, Note, Route, Urgency};
use atlas::phone::{body_for, configured, send, NotSet, PhoneConfig, NO_PHONE};
use atlas::presence::Presence;

fn urgent(title: &str, body: &str) -> Note {
    Note::new(title, body, Urgency::Urgent, 0)
}

// --- when the phone is the right answer -------------------------------------

#[test]
fn gone_long_enough_the_phone_beats_a_window_you_cannot_see() {
    // Drawing a panel into an empty house is not delivery.
    assert_eq!(route(Presence::Away, false, true, true, true), Route::Phone);
}

#[test]
fn briefly_away_it_uses_the_screen_rather_than_buzzing_your_pocket() {
    // A push for something you'd have seen on your own monitor two minutes
    // later is how people learn to silence an app.
    assert_eq!(route(Presence::Away, false, true, true, false), Route::Notify);
}

#[test]
fn with_no_screen_the_phone_is_used_even_soon_after_you_step_away() {
    assert_eq!(route(Presence::Away, false, false, true, false), Route::Phone);
}

#[test]
fn at_the_desk_nothing_goes_to_the_phone() {
    assert_eq!(route(Presence::AtDesk, true, true, true, true), Route::Speak);
}

#[test]
fn with_no_phone_configured_it_still_holds_rather_than_dropping() {
    assert_eq!(route(Presence::Away, false, false, false, true), Route::Hold);
}

// --- what lands on the lock screen ------------------------------------------

#[test]
fn the_detail_stays_off_the_lock_screen_by_default() {
    // A phone notification is visible to anyone who picks it up, and Atlas has
    // no way to know who that is — the desk-window problem with less control.
    let cfg = PhoneConfig::default();
    let b = body_for(&urgent("Bank alert", "balance is 12.40"), &cfg);
    assert!(!b.contains("12.40"), "the detail reached the lock screen: {b}");
    assert!(b.contains("Bank alert"), "it didn't even say what it was about: {b}");
    assert!(b.contains("Ask me"), "no way to know how to get the rest: {b}");
}

#[test]
fn a_private_note_stays_redacted_even_when_detail_is_switched_on() {
    // `include_detail` is a convenience for ordinary alerts. It must not
    // override an explicit private marking.
    let cfg = PhoneConfig { include_detail: true, ..Default::default() };
    let n = urgent("Bank alert", "balance is 12.40").private();
    let b = body_for(&n, &cfg);
    assert!(!b.contains("12.40"), "private content was pushed anyway: {b}");
}

#[test]
fn switching_detail_on_sends_an_ordinary_message_in_full() {
    let cfg = PhoneConfig { include_detail: true, ..Default::default() };
    let b = body_for(&urgent("Disk", "5 GB left"), &cfg);
    assert!(b.contains("5 GB left"), "{b}");
}

#[test]
fn urgency_travels_so_the_phone_can_treat_it_differently() {
    let cfg = PhoneConfig::default();
    let u = body_for(&urgent("a", "b"), &cfg);
    let r = body_for(&Note::new("a", "b", Urgency::Routine, 0), &cfg);
    assert!(u.contains("\"priority\":5"), "{u}");
    assert!(r.contains("\"priority\":3"), "{r}");
}

#[test]
fn a_quote_in_a_title_does_not_produce_a_broken_message() {
    // Invalid JSON is rejected by the server, which reads as "the phone is
    // unreachable" — a wrong diagnosis for a quoting bug.
    let cfg = PhoneConfig::default();
    let b = body_for(&urgent("He said \"go\"", "x"), &cfg);
    assert!(b.contains("\\\"go\\\""), "{b}");
    // And it parses.
    let v: serde_json::Value = serde_json::from_str(&b).expect("the message wasn't valid JSON");
    assert_eq!(v["title"], "He said \"go\"");
}

#[test]
fn a_newline_in_a_body_does_not_break_the_message() {
    let cfg = PhoneConfig { include_detail: true, ..Default::default() };
    let b = body_for(&urgent("t", "line one\nline two"), &cfg);
    let v: serde_json::Value = serde_json::from_str(&b).expect("not valid JSON");
    assert_eq!(v["message"], "line one\nline two");
}

// --- refusing clearly rather than failing later -----------------------------

#[test]
fn an_https_address_is_accepted_now_and_an_empty_one_is_refused_up_front() {
    // Round 5 added TLS (`http::https_post_json`), so a public push service
    // works. An address with nothing after the scheme is still refused before
    // anything urgent needs sending.
    let ok = PhoneConfig { enabled: true, host: "https://ntfy.sh".into(), ..Default::default() };
    assert_eq!(configured(&ok), Ok(()));
    let empty = PhoneConfig { enabled: true, host: "https://".into(), ..Default::default() };
    assert_eq!(configured(&empty), Err(NotSet::NeedsTls("https://".into())));
    assert!(configured(&empty).unwrap_err().plain().contains("server name"));
    assert!(send(&urgent("x", "y"), &empty).is_err());
}

#[test]
fn being_switched_off_is_not_reported_as_a_fault() {
    // Most people won't want this. "Off" must read differently from "broken".
    assert_eq!(configured(&PhoneConfig::default()), Err(NotSet::Disabled));
    assert!(NO_PHONE.contains("held"), "it doesn't say what happens instead");
}

#[test]
fn enabled_with_no_address_says_so() {
    let cfg = PhoneConfig { enabled: true, ..Default::default() };
    assert_eq!(configured(&cfg), Err(NotSet::NoHost));
}

#[test]
fn sending_with_nothing_configured_fails_rather_than_quietly_succeeding() {
    let e = send(&urgent("x", "y"), &PhoneConfig::default()).unwrap_err();
    assert!(format!("{e}").contains("switched off"), "got: {e}");
}

// --- against a real socket --------------------------------------------------

#[test]
fn a_push_actually_goes_out_over_a_socket() {
    // Not a mock. `http.rs` was written for Chrome's debugger and `post_json`
    // had never been called by anything, so this is the first time that code
    // path has carried a real request.
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let l = TcpListener::bind("127.0.0.1:0").expect("a port");
    let addr = l.local_addr().unwrap();
    let got = std::thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        let mut buf = [0u8; 2048];
        let n = s.read(&mut buf).unwrap();
        let req = String::from_utf8_lossy(&buf[..n]).to_string();
        s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok").unwrap();
        req
    });

    let cfg = PhoneConfig {
        enabled: true,
        host: format!("127.0.0.1:{}", addr.port()),
        path: "/atlas".into(),
        ..Default::default()
    };
    send(&urgent("Disk is nearly full", "5 GB left"), &cfg).expect("the push should have gone");

    let req = got.join().unwrap();
    // ntfy's JSON publish: to the root, with the topic in the body.
    assert!(req.starts_with("POST / "), "wrong method or path:\n{req}");
    assert!(req.contains("\"topic\":\"atlas\""), "the topic didn't travel:\n{req}");
    assert!(!req.contains("Authorization"), "a token was sent with none set:\n{req}");
    assert!(req.contains("Disk is nearly full"), "the title didn't travel:\n{req}");
    assert!(!req.contains("5 GB left"), "the detail was pushed by default:\n{req}");
}

#[test]
fn a_server_that_refuses_says_which_way_it_refused() {
    // 404 means the topic is wrong; 401 means the token is. Those send you to
    // different places, and "couldn't reach your phone" sends you to neither.
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let l = TcpListener::bind("127.0.0.1:0").expect("a port");
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        let mut buf = [0u8; 2048];
        let _ = s.read(&mut buf);
        let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
    });

    let cfg = PhoneConfig {
        enabled: true,
        host: format!("127.0.0.1:{}", addr.port()),
        ..Default::default()
    };
    let e = send(&urgent("x", "y"), &cfg).unwrap_err();
    let msg = format!("{e}");
    assert!(msg.contains("404"), "the status was swallowed: {msg}");
    assert!(msg.contains("topic"), "it didn't say what 404 means here: {msg}");
}

#[test]
fn a_token_goes_in_the_authorization_header_and_detail_is_scrubbed() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    let l = TcpListener::bind("127.0.0.1:0").expect("a port");
    let addr = l.local_addr().unwrap();
    let got = std::thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        let mut buf = [0u8; 4096];
        let n = s.read(&mut buf).unwrap();
        s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok").unwrap();
        String::from_utf8_lossy(&buf[..n]).to_string()
    });
    let cfg = PhoneConfig {
        enabled: true,
        host: format!("http://127.0.0.1:{}", addr.port()),
        path: "/atlas".into(),
        token: Some("tk_abc123".into()),
        include_detail: true,
        ..Default::default()
    };
    send(&urgent("Card declined", "card 4111 1111 1111 1111 was declined, write to dana@acme-install.com"), &cfg).unwrap();
    let req = got.join().unwrap();
    println!("LIVE [phone push, ntfy format]\n{req}");
    assert!(req.contains("Authorization: Bearer tk_abc123"), "{req}");
    assert!(!req.contains("4111 1111"), "a card number reached the lock screen:\n{req}");
    assert!(!req.contains("dana@acme-install.com"), "an address reached the lock screen:\n{req}");
    assert!(req.contains("declined"), "{req}");
}
