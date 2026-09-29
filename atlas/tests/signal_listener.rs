//! `SignalListener` is the piece that was missing after the first `kin`
//! build: `Door` and `route_signal` were fully proven, but nothing was
//! actually listening while the daemon ran. This proves the listener itself
//! — a real socket, a real HTTP request, over the network stack, not just
//! function calls in the same process.
//!
//! The first version of this file wrote a request and tried to read the
//! reply synchronously, before anything had called `poll_once` to accept
//! and service the connection — which cannot work with a non-blocking
//! listener that only progresses when polled. A real peer and the daemon's
//! own tick genuinely run concurrently; the test has to as well.

use atlas::kin::Peer;
use atlas::server::SignalListener;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

const TOKEN: &str = "a-real-peer-token-0123456789ab";

fn free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

/// Sends the request on its own thread, exactly as a real peer would --
/// nothing on the server side of this test blocks waiting for it.
fn send_signal(port: u16, token: &str, what: &str, urgency: &str) -> std::thread::JoinHandle<u16> {
    let token = token.to_string();
    let what = what.to_string();
    let urgency = urgency.to_string();
    std::thread::spawn(move || {
        let body = format!(r#"{{"what":"{what}","urgency":"{urgency}"}}"#);
        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let req = format!(
            "POST /signal HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\n\
             Content-Length: {}\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(req.as_bytes()).unwrap();
        let mut resp = String::new();
        let _ = stream.read_to_string(&mut resp);
        resp.lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
    })
}

/// Poll until something arrives or the deadline passes. A single `poll_once`
/// racing a client thread that has not reached `connect()` yet is a real
/// possibility, not a flake to paper over -- the daemon's loop handles this
/// the same way, by simply checking again on its next tick.
fn poll_until<T>(l: &SignalListener, deadline: Duration, mut want: impl FnMut(&SignalListener) -> Option<T>) -> Option<T> {
    let start = Instant::now();
    loop {
        if let Some(v) = want(l) {
            return Some(v);
        }
        if start.elapsed() > deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_real_request_over_a_real_socket_is_accepted_and_delivered() {
    let port = free_port();
    let l = SignalListener::bind(port, vec![Peer::new("Homelab", TOKEN)]).unwrap();

    let client = send_signal(port, TOKEN, "margin call on the leveraged position", "urgent");
    let arrived = poll_until(&l, Duration::from_secs(2), |l| l.poll_once(1))
        .expect("the daemon's tick should eventually see the connection and accept it");
    // The variant is part of the assertion, not a formality. `/signal` and
    // `/handoff` share the door, the token check and the socket, and share
    // nothing else -- a signal that came back as a handoff would mean a
    // peer's interrupt had quietly turned into a note in a list.
    let atlas::kin::Arrived::Signal(incoming) = arrived else {
        panic!("a signal came back as something else: {arrived:?}")
    };
    assert_eq!(incoming.from, "Homelab");
    assert!(incoming.what.contains("margin call"));

    assert_eq!(client.join().unwrap(), 200, "the peer should see a 200 once the daemon serviced it");
}

#[test]
fn checking_with_nobody_calling_returns_immediately_rather_than_waiting() {
    let port = free_port();
    let l = SignalListener::bind(port, vec![Peer::new("Homelab", TOKEN)]).unwrap();
    let start = Instant::now();
    assert!(l.poll_once(1).is_none());
    assert!(
        start.elapsed() < Duration::from_millis(50),
        "poll_once blocked for {:?} with nobody connected -- the one guarantee this type exists \
         to provide is that it never stalls the daemon's loop",
        start.elapsed()
    );
}

#[test]
fn a_token_nobody_registered_is_refused_over_the_real_socket() {
    let port = free_port();
    let l = SignalListener::bind(port, vec![Peer::new("Homelab", TOKEN)]).unwrap();

    let client = send_signal(port, "not-a-registered-token", "hello", "info");
    // The connection is still accepted and answered -- refused, not ignored
    // -- so a poll that only looks for a delivered Incoming would time out
    // here by design. Give the daemon side one tick to service it, then
    // confirm nothing was delivered and the peer saw a real refusal.
    let delivered = poll_until(&l, Duration::from_millis(300), |l| l.poll_once(1));
    assert!(delivered.is_none(), "a refused request must not surface as a delivered signal");
    assert_eq!(client.join().unwrap(), 401);
}

#[test]
fn two_ports_do_not_interfere_with_each_other() {
    let (pa, pb) = (free_port(), free_port());
    let a = SignalListener::bind(pa, vec![Peer::new("A", TOKEN)]).unwrap();
    let b = SignalListener::bind(pb, vec![Peer::new("B", "different-token")]).unwrap();

    send_signal(pa, TOKEN, "for a", "info");
    let got = poll_until(&a, Duration::from_secs(2), |a| a.poll_once(1)).unwrap();
    let atlas::kin::Arrived::Signal(got) = got else { panic!("not a signal: {got:?}") };
    assert_eq!(got.from, "A");
    assert!(b.poll_once(1).is_none(), "nothing was ever sent to b's port");
}

#[test]
fn a_second_peer_on_the_same_door_is_told_apart_from_the_first() {
    let port = free_port();
    let l = SignalListener::bind(
        port,
        vec![Peer::new("Homelab", TOKEN), Peer::new("SomeOtherAtlas", "second-token")],
    )
    .unwrap();

    send_signal(port, "second-token", "from the other one", "info");
    let got = poll_until(&l, Duration::from_secs(2), |l| l.poll_once(1)).unwrap();
    let atlas::kin::Arrived::Signal(got) = got else { panic!("not a signal: {got:?}") };
    assert_eq!(got.from, "SomeOtherAtlas");
}

// ================= the second endpoint, over the same real socket =========

/// The same as `send_signal`, to `/handoff`. Deliberately a separate helper
/// rather than a `path` parameter on the first: the two endpoints share a
/// socket and a token check and nothing else, and a test helper that can
/// swap between them with one argument is the shape of thing that ends up
/// proving neither.
fn send_handoff(port: u16, token: &str, what: &str) -> std::thread::JoinHandle<u16> {
    let token = token.to_string();
    let what = what.to_string();
    std::thread::spawn(move || {
        let body = format!(r#"{{"what":"{what}","from":"whoever they say"}}"#);
        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let req = format!(
            "POST /handoff HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\n\
             Content-Length: {}\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(req.as_bytes()).unwrap();
        let mut resp = String::new();
        let _ = stream.read_to_string(&mut resp);
        resp.lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
    })
}

#[test]
fn a_note_handed_over_arrives_as_a_handoff_and_not_as_a_signal() {
    let port = free_port();
    let l = SignalListener::bind(port, vec![Peer::new("Priya", TOKEN)]).unwrap();

    let client = send_handoff(port, TOKEN, "the spreadsheet template");
    let arrived = poll_until(&l, Duration::from_secs(2), |l| l.poll_once(1))
        .expect("the daemon's tick should see it");

    let atlas::kin::Arrived::Handoff(d) = arrived else {
        panic!("a note came back as something else: {arrived:?}")
    };
    assert_eq!(d.what, "the spreadsheet template");
    // The registered name, not the one in the body.
    assert_eq!(d.from, "Priya");
    assert_eq!(client.join().unwrap(), 200);
}

#[test]
fn an_unregistered_token_is_refused_at_the_handoff_door_too() {
    let port = free_port();
    let l = SignalListener::bind(port, vec![Peer::new("Priya", TOKEN)]).unwrap();

    let client = send_handoff(port, "not-a-token-at-all-0123456789", "have this");
    let got = poll_until(&l, Duration::from_millis(300), |l| l.poll_once(1));
    assert!(got.is_none(), "an unregistered token handed something over: {got:?}");
    assert_eq!(client.join().unwrap(), 401, "refused, not ignored");
}

#[test]
fn an_endpoint_nobody_serves_is_refused_rather_than_falling_through() {
    // Two peer-reachable paths now exist on this socket. A third that
    // happens to parse must not be answered by either.
    let port = free_port();
    let l = SignalListener::bind(port, vec![Peer::new("Priya", TOKEN)]).unwrap();

    let token = TOKEN.to_string();
    let client = std::thread::spawn(move || {
        let body = r#"{"what":"open chrome"}"#;
        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let req = format!(
            "POST /say HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\n\
             Content-Length: {}\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(req.as_bytes()).unwrap();
        let mut resp = String::new();
        let _ = stream.read_to_string(&mut resp);
        resp.lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(0)
    });

    let got = poll_until(&l, Duration::from_millis(300), |l| l.poll_once(1));
    assert!(got.is_none(), "a peer token reached /say on the signal socket: {got:?}");
    assert_eq!(client.join().unwrap(), 401);
}
