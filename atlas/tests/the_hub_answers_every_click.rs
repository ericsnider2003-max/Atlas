//! Clicking around the hub is answered at once (27 Sep 2026).
//!
//! Eric: "Atlas felt slow when clicking around and trying to talk to it and
//! that is not acceptable." The hub was served by the daemon's loop, one
//! connection per pass, and a pass also waited on the keyboard, the wake
//! word and a nap of up to two seconds. A click is several requests, the page
//! refetches itself, and the setup window probed the port with empty
//! connections -- each of those used up a pass.
//!
//! Now connections are read on their own threads (`Server::threaded`), and
//! the loop answers every authenticated request waiting each time it looks
//! (`HubDoor::wait_and_answer`). Everything here goes over real sockets.
//!
//! `before_and_after` measures both shapes and prints the numbers.

use atlas::server::{HubDoor, Reply, Server, ServerConfig};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const TOKEN: &str = "abcdef-ghjkmn-pqrstu-vwxyz2";

fn a_server() -> Server {
    let cfg = ServerConfig { enabled: true, port: 0, ..ServerConfig::default() };
    Server::bind(&cfg, TOKEN).expect("bind")
}

fn ask(port: u16, raw: &str) -> String {
    let mut c = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    c.set_read_timeout(Some(Duration::from_secs(20))).expect("timeout");
    c.write_all(raw.as_bytes()).expect("write");
    c.flush().expect("flush");
    // Bytes, not a string: an icon is not UTF-8.
    let mut out = Vec::new();
    let _ = c.read_to_end(&mut out);
    String::from_utf8_lossy(&out).into_owned()
}

fn page(port: u16) -> String {
    ask(port, &format!("GET /hub HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Atlas-Token: {TOKEN}\r\n\r\n"))
}

/// A stand-in for the daemon's loop: answers the door in 50ms slices, the
/// way `Daemon::nap_awake` does, until told to stop. `busy_ms` is how long
/// each answer takes to work out.
fn a_loop(door: HubDoor, stop: Arc<AtomicBool>, busy_ms: u64) -> std::thread::JoinHandle<usize> {
    std::thread::spawn(move || {
        let mut answered = 0;
        while !stop.load(Ordering::SeqCst) {
            answered += door
                .wait_and_answer(50, &mut |_a| {
                    std::thread::sleep(Duration::from_millis(busy_ms));
                    Reply::html("<p>the page</p>")
                })
                .len();
        }
        answered
    })
}

/// Fire `n` page requests at once; how long until the last one came back,
/// and whether every one was a 200.
fn burst(port: u16, n: usize) -> (Duration, bool) {
    let started = Instant::now();
    let clients: Vec<_> = (0..n).map(|_| std::thread::spawn(move || page(port))).collect();
    let all_ok = clients.into_iter().all(|c| c.join().unwrap().starts_with("HTTP/1.1 200"));
    (started.elapsed(), all_ok)
}

#[test]
fn a_burst_of_requests_is_answered_together_not_one_per_pass() {
    let server = a_server();
    let port = server.port();
    let door = server.threaded().expect("threaded");
    let stop = Arc::new(AtomicBool::new(false));
    let lp = a_loop(door, stop.clone(), 2);

    let (took, ok) = burst(port, 12);
    stop.store(true, Ordering::SeqCst);
    let answered = lp.join().unwrap();

    assert!(ok, "a request in the burst was not answered with the page");
    assert_eq!(answered, 12, "the loop did not answer every request");
    crate::common::assert_prompt(took, Duration::from_millis(1500), "twelve requests took");
}

#[test]
fn empty_connections_cost_the_loop_nothing() {
    // The setup window's "is Atlas running?" probe: connect, say nothing,
    // close. And a client that connects and holds the socket open silent.
    let server = a_server();
    let port = server.port();
    let door = server.threaded().expect("threaded");
    for _ in 0..10 {
        drop(TcpStream::connect(("127.0.0.1", port)).unwrap());
    }
    let silent: Vec<TcpStream> = (0..5).map(|_| TcpStream::connect(("127.0.0.1", port)).unwrap()).collect();

    // Nothing reached the loop: none of those was a request.
    std::thread::sleep(Duration::from_millis(200));
    let reached = door.answer_waiting(&mut |_| unreachable!("an empty connection reached the daemon"));
    assert!(reached.is_empty());

    // And a real click alongside them is answered at once.
    let stop = Arc::new(AtomicBool::new(false));
    let lp = a_loop(door, stop.clone(), 0);
    let started = Instant::now();
    let r = page(port);
    let took = started.elapsed();
    stop.store(true, Ordering::SeqCst);
    lp.join().unwrap();
    drop(silent);
    assert!(r.starts_with("HTTP/1.1 200"), "got: {}", &r[..r.len().min(80)]);
    crate::common::assert_prompt(took, Duration::from_millis(500), "a click behind silent connections took");
}

#[test]
fn a_wrong_token_is_slowed_without_slowing_anything_else() {
    // `slow_down_a_guess` sleeps -- on the connection's own thread now, never
    // the loop's.
    let server = a_server();
    let port = server.port();
    let door = server.threaded().expect("threaded");
    let stop = Arc::new(AtomicBool::new(false));
    let lp = a_loop(door, stop.clone(), 0);

    let guesses: Vec<_> = (0..8)
        .map(|_| {
            std::thread::spawn(move || {
                ask(port, "GET /hub HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Atlas-Token: wrong-wrong-wrong-wrong\r\n\r\n")
            })
        })
        .collect();
    std::thread::sleep(Duration::from_millis(50));
    let started = Instant::now();
    let r = page(port);
    let took = started.elapsed();
    for g in guesses {
        assert!(g.join().unwrap().starts_with("HTTP/1.1 401"), "a wrong token was let in");
    }
    stop.store(true, Ordering::SeqCst);
    lp.join().unwrap();
    assert!(r.starts_with("HTTP/1.1 200"));
    crate::common::assert_prompt(took, Duration::from_millis(400), "the right token waited behind wrong ones");
}

#[test]
fn the_app_files_are_answered_with_nobody_at_the_loop() {
    // The service worker and icons need nothing of Atlas's state: answered
    // on the connection's thread, even while the loop is busy elsewhere.
    let server = a_server();
    let port = server.port();
    let _door = server.threaded().expect("threaded");
    let started = Instant::now();
    let r = ask(port, "GET /hub/icon-192.png HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
    assert!(r.starts_with("HTTP/1.1 200"), "got: {}", &r[..r.len().min(80)]);
    assert!(r.contains("max-age"), "an icon is not kept by the browser");
    crate::common::assert_prompt(started.elapsed(), Duration::from_millis(500), "took too long");
}

#[test]
fn a_page_the_loop_never_gets_to_is_not_answered_as_the_page() {
    // Nobody answering (the loop gone) must not hang the connection or let
    // the request through unanswered: with the channel's other end dropped,
    // the connection is told Atlas is busy.
    let server = a_server();
    let port = server.port();
    let door = server.threaded().expect("threaded");
    drop(door);
    let r = page(port);
    assert!(r.starts_with("HTTP/1.1 503"), "got: {}", &r[..r.len().min(80)]);
}

/// The measurement: the old shape (one connection per pass of a loop that
/// also waits half a second on the keyboard -- the shortest pass the daemon
/// had) against the new, for the same burst. Printed, and the new must be
/// faster by a wide margin.
#[test]
fn before_and_after() {
    const N: usize = 8;
    const PASS_MS: u64 = 500;

    // Before: `poll_once` once a pass.
    let old = a_server();
    let old_port = old.port();
    let stop = Arc::new(AtomicBool::new(false));
    let s2 = stop.clone();
    let old_loop = std::thread::spawn(move || {
        while !s2.load(Ordering::SeqCst) {
            old.poll_once(&mut |_| {
                std::thread::sleep(Duration::from_millis(2));
                Reply::html("<p>the page</p>")
            });
            std::thread::sleep(Duration::from_millis(PASS_MS));
        }
    });
    let (before, ok_before) = burst(old_port, N);
    stop.store(true, Ordering::SeqCst);
    old_loop.join().unwrap();

    // After.
    let new = a_server();
    let new_port = new.port();
    let door = new.threaded().expect("threaded");
    let stop = Arc::new(AtomicBool::new(false));
    let lp = a_loop(door, stop.clone(), 2);
    let (after, ok_after) = burst(new_port, N);
    stop.store(true, Ordering::SeqCst);
    lp.join().unwrap();

    eprintln!(
        "hub, {N} requests at once: one-per-pass loop {} ms; threaded door {} ms",
        before.as_millis(),
        after.as_millis()
    );
    assert!(ok_before && ok_after);
    assert!(after * 4 < before, "before {before:?}, after {after:?}");
}
