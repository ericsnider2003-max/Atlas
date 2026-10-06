//! The automatic sync never holds Atlas up waiting for another device
//! (5 Oct 2026 audit, Q5, the outbound half).
//!
//! Every quarter hour the automatic sync dialled each device you've named
//! and waited up to 4 s for an answer. A phone asleep on the tailnet answers
//! the connection and then nothing, so Atlas -- the hub, your keys, your
//! voice -- stood still for those seconds, every time. The sends are made on
//! a thread now and their answers taken on a later pass.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::Path;
use std::time::{Duration, Instant};

fn tmp(tag: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-q5-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// A device that takes the connection and never says anything back.
fn asleep() -> (u16, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let dialled = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = dialled.clone();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for s in l.incoming().flatten() {
            seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            held.push(s); // kept open, never answered
        }
    });
    (port, dialled)
}

#[test]
fn a_device_that_never_answers_doesnt_stop_atlas() {
    let (port, dialled) = asleep();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let mut c = Config::load(Path::new("config")).unwrap();
    let tools = c.tools.as_mut().unwrap();
    tools.sync.enabled = true;
    tools.sync.automatic = true;
    tools.sync.folder = String::new();
    tools.elsewhere.known = vec![atlas::elsewhere::Elsewhere {
        name: "phone".into(),
        host: "127.0.0.1".into(),
        port: 1,
        sync_port: Some(port),
        ..Default::default()
    }];
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("asleep")), Proactive::new(ProactiveConfig::default()));
    let t = 1_000 + atlas::daemon::AUTO_SYNC_EVERY_SECS;
    let started = Instant::now();
    d.tick(t);
    let took = started.elapsed();
    assert!(took < Duration::from_secs(2), "the automatic sync held the tick {took:?} waiting on a device");

    // The send was still made, off the loop.
    let until = Instant::now() + Duration::from_secs(5);
    while dialled.load(std::sync::atomic::Ordering::SeqCst) == 0 && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(dialled.load(std::sync::atomic::Ordering::SeqCst) >= 1, "the named device was never dialled");

    // Later passes take the (missing) answer without waiting either.
    let started = Instant::now();
    d.tick(t + 5);
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn saying_sync_yourself_still_waits_for_the_answer() {
    // You asked, so the answer is in the reply -- only the automatic pass
    // sends off the loop.
    let src = std::fs::read_to_string("src/daemon/tick.rs").unwrap();
    let auto = &src[src.find("self.last_auto_sync = t;").expect("the automatic pass")..];
    let auto = &auto[..auto.find("take_dial_answers").unwrap()];
    assert!(auto.contains("self.dial_later = true;") && auto.contains("self.dial_later = false;"));
    assert!(auto.contains("dial_off_the_loop("));
}
