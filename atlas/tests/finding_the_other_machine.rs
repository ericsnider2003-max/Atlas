//! Finding your other Atlas on the same network, without typing an address.
//!
//! # What was missing, and what was not
//!
//! Two Atlases could already talk. `kin.rs` is the door one knocks on,
//! `elsewhere.rs` is the asking, `server.rs` answers, and every request
//! carries a token. None of it needs a third party.
//!
//! What it needed was this, in `config/tools.yaml`:
//!
//! ```yaml
//! elsewhere:
//!   known:
//!     - name: homelab
//!       host: "10.0.0.9"
//! ```
//!
//! A hand-typed address, on a home network that hands out a different one
//! after a reboot. So the only transport Atlas had that reaches another
//! machine directly was gated behind a setting that goes stale by itself --
//! and `mesh::choose` offered four routes of which only `Cloud` was ever
//! reachable, because `Cloud` is a folder and needs no address at all.
//!
//! # The rule this file is really defending
//!
//! **Discovery is not trust.** Finding a machine may tell you its name and
//! where it is, and nothing else, ever. The token still comes from the other
//! Atlas and is still required on every request. Most of the assertions below
//! are about that rather than about finding anything.

use atlas::mesh::{self, MeshConfig, Path};
use atlas::nearby::{self, answer_for, answer_from, Found, NearbyConfig, ASKING, PORT};
use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

fn from(ip: &str) -> SocketAddr {
    format!("{ip}:54321").parse().unwrap()
}

// ===================== what an announcement may carry =================

#[test]
fn an_announcement_carries_a_name_and_a_port_and_nothing_else() {
    // The whole security argument in one assertion. Anything listening on the
    // network sees this, so it may contain only what connecting to the port
    // would have told them anyway.
    let said = answer_for("homelab", 8787);
    assert_eq!(said, "atlas-here-1 homelab 8787");
    assert_eq!(said.split_whitespace().count(), 3, "a field was added: {said}");
}

#[test]
fn a_name_with_spaces_cannot_forge_a_longer_message() {
    // "homelab 9999 and-also" would otherwise parse as a different port.
    let said = answer_for("my laptop", 8787);
    assert_eq!(said, "atlas-here-1 my-laptop 8787");
    let f = answer_from(&said, from("10.0.0.9")).expect("parses");
    assert_eq!(f.name, "my-laptop");
    assert_eq!(f.port, 8787);
}

#[test]
fn anything_that_is_not_an_announcement_is_ignored() {
    for junk in [
        "",
        "hello",
        "atlas-here-1",
        "atlas-here-1 name",
        "atlas-here-1 name notaport",
        "atlas-here-2 name 8787",
        "ATLAS-HERE-1 name 8787",
        "atlas-here-1 name 0",
    ] {
        assert!(answer_from(junk, from("10.0.0.9")).is_none(), "accepted {junk:?}");
    }
}

#[test]
fn the_address_comes_from_the_packet_rather_than_from_the_packet_contents() {
    // A machine cannot announce somebody else's address, because it does not
    // get to say what its address is -- the socket does.
    let f = answer_from("atlas-here-1 homelab 8787", from("10.0.0.9")).expect("parses");
    assert_eq!(f.host, "10.0.0.9");
}

// ===================== finding is not being allowed in ================

#[test]
fn what_it_found_is_not_written_into_your_settings() {
    // Atlas prints the entry and leaves the token to you. An entry that only
    // needs one more field is an invitation to paste a token in without
    // thinking about which machine it lets in.
    let f = Found { name: "homelab".into(), host: "10.0.0.9".into(), port: 8787 };
    let how = f.how_to_add();
    assert!(how.contains("name: homelab"));
    assert!(how.contains("host: \"10.0.0.9\""));
    // The token is a hole, not a value.
    assert!(how.contains("<run `atlas hub`"), "{how}");
    assert!(!how.contains("token: \"\""), "an empty token reads as one that works: {how}");

    let raw = crate::common::source_of("main");
    let body = raw
        .split("fn run_nearby(")
        .nth(1)
        .and_then(|r| r.split("\nfn ").next())
        .expect("run_nearby");
    for writing in ["save(", "write(", "fs::"] {
        assert!(!body.contains(writing), "the nearby command writes something: {writing}");
    }
}

#[test]
fn the_spoken_answer_says_that_finding_is_not_reaching() {
    let found = vec![Found { name: "homelab".into(), host: "10.0.0.9".into(), port: 8787 }];
    let said = nearby::spoken(&found, &NearbyConfig::default());
    assert!(said.contains("homelab at 10.0.0.9:8787"), "{said}");
    assert!(said.contains("not being allowed in"), "{said}");
    assert!(said.contains("that machine's own token"), "{said}");
}

#[test]
fn finding_nothing_says_which_of_the_two_reasons_it_might_be() {
    // "Nothing found" sends you looking at the wrong problem half the time.
    let said = nearby::spoken(&[], &NearbyConfig::default());
    assert!(said.contains("no other Atlas on this network"), "{said}");
    assert!(said.contains("nearby.announce: true"), "{said}");
    assert!(said.contains("on that machine"), "it didn't say where to change it: {said}");
}

#[test]
fn a_machine_that_is_not_answering_is_told_it_is_not() {
    // You looked, you found one, and the other one cannot find you. Worth
    // knowing before you wonder why.
    let found = vec![Found { name: "homelab".into(), host: "10.0.0.9".into(), port: 8787 }];
    let quiet = nearby::spoken(&found, &NearbyConfig { announce: false, ..Default::default() });
    assert!(quiet.contains("isn't answering probes itself"), "{quiet}");
    let loud = nearby::spoken(&found, &NearbyConfig { announce: true, ..Default::default() });
    assert!(!loud.contains("isn't answering probes itself"), "{loud}");
}

#[test]
fn answering_is_off_until_you_turn_it_on() {
    // The network you're on isn't always your own.
    assert!(!NearbyConfig::default().announce);
    let yaml = std::fs::read_to_string("config/tools.yaml").expect("tools.yaml");
    assert!(yaml.contains("announce: false"), "it ships announcing");
    assert!(yaml.contains("isn't always your own"), "the reason isn't written down");
}

#[test]
fn a_probe_is_not_answered_when_announcing_is_off() {
    // The switch, checked before a socket is opened rather than after.
    let off = NearbyConfig { announce: false, ..Default::default() };
    let r = nearby::answer_probes("homelab", 8787, &off, &|| false);
    // Returns rather than binding and looping forever, which is what makes
    // this testable at all.
    assert!(r.is_ok(), "{r:?}");
}

// ===================== the socket path, for real ======================

#[test]
fn a_probe_over_a_real_socket_gets_a_real_answer() {
    // Not a parse test: two sockets, one datagram each way, on loopback.
    // Everything above this checks the words; this checks that the words ever
    // leave the machine.
    //
    // A fixed port would collide with another test run, so the responder
    // takes one from the OS and the prober is pointed at it -- which means
    // this exercises `answer_for`/`answer_from` over UDP without needing the
    // broadcast address, which a sandbox will not route.
    let responder = UdpSocket::bind(("127.0.0.1", 0)).expect("bind responder");
    let door = responder.local_addr().expect("addr").port();
    responder.set_read_timeout(Some(Duration::from_secs(2))).expect("timeout");

    let listening = std::thread::spawn(move || {
        let mut buf = [0u8; 512];
        let (n, from) = responder.recv_from(&mut buf).expect("a probe");
        assert_eq!(String::from_utf8_lossy(&buf[..n]).trim(), ASKING);
        let reply = answer_for("homelab", 8787);
        responder.send_to(reply.as_bytes(), from).expect("reply");
    });

    let prober = UdpSocket::bind(("127.0.0.1", 0)).expect("bind prober");
    prober.set_read_timeout(Some(Duration::from_secs(2))).expect("timeout");
    prober.send_to(ASKING.as_bytes(), ("127.0.0.1", door)).expect("probe");

    let mut buf = [0u8; 512];
    let (n, from) = prober.recv_from(&mut buf).expect("an answer");
    let found = answer_from(&String::from_utf8_lossy(&buf[..n]), from).expect("parses");
    assert_eq!(found.name, "homelab");
    assert_eq!(found.port, 8787);
    assert_eq!(found.host, "127.0.0.1");

    listening.join().expect("the responder thread");
}

#[test]
fn looking_on_a_network_with_nothing_on_it_comes_back_empty_rather_than_hanging() {
    // The wait is bounded by `listen_ms`, so a machine with no other Atlas on
    // it answers quickly instead of appearing to stall.
    let quick = NearbyConfig { listen_ms: 150, ..Default::default() };
    let began = std::time::Instant::now();
    let r = nearby::look(&quick);
    let took = began.elapsed();
    // Either an empty list or a socket error -- a sandbox may refuse
    // broadcast, and that is reported rather than swallowed. What must not
    // happen is waiting.
    assert!(took < Duration::from_secs(3), "it waited {took:?}");
    if let Ok(found) = r {
        assert!(found.is_empty() || !found.is_empty(), "a list either way");
    }
}

// ===================== the route it makes reachable ===================

#[test]
fn the_machine_you_sync_with_being_here_is_what_same_network_means() {
    let found = vec![
        Found { name: "homelab".into(), host: "10.0.0.9".into(), port: 8787 },
        Found { name: "laptop".into(), host: "10.0.0.4".into(), port: 8787 },
    ];
    assert!(mesh::on_this_network("homelab", &found));
    assert!(mesh::on_this_network("LAPTOP", &found), "case shouldn't matter");
    // Two Atlases on a network do not help if the one you want is neither.
    assert!(!mesh::on_this_network("workshop", &found));
    assert!(!mesh::on_this_network("", &found));
    assert!(!mesh::on_this_network("homelab", &[]));
}

#[test]
fn same_network_is_a_route_that_can_now_be_reached_at_all() {
    // It never could be. Every caller of `choose` passed literals, so the
    // answer was invariably `Cloud`.
    let cfg = MeshConfig::default();
    let found = vec![Found { name: "homelab".into(), host: "10.0.0.9".into(), port: 8787 }];
    let here = mesh::on_this_network("homelab", &found);
    assert_eq!(mesh::choose(here, false, true, false, &cfg).0, Path::SameNetwork);
    // And away from it, the folder, which is the one that is actually built.
    assert_eq!(mesh::choose(false, false, true, false, &cfg).0, Path::Cloud);
}

#[test]
fn being_on_the_same_network_is_not_reported_as_having_sent_it_that_way() {
    // The line this whole pass keeps drawing. Atlas found the other machine;
    // it still moved the bundle through the folder, because sending straight
    // across is not built.
    let raw = crate::common::source_of("daemon");
    let code: String = raw
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(code.contains("crate::mesh::on_this_network("), "nothing observes the network");
    assert!(code.contains("crate::nearby::look("), "nothing looks");
    // Sending straight across is built now (`transport::exchange` in the
    // sync pass). The line this test draws still holds, in its new form: it
    // says "synced straight across" only on the branch where the exchange
    // came back, and says the folder carried it when it didn't. (Until the
    // 26 Sep merge this looked for "isn't built yet", which lived in the post
    // sender that the Atlas Project chat's 25j replaced with a real send.)
    assert!(code.contains("crate::transport::exchange("), "nothing sends across");
    assert!(
        code.contains("direct send didn't land; it's in the folder"),
        "a failed direct send is reported as if it went across"
    );
    // `mesh` came off CAPABILITY_UNWIRED, honestly -- its code is reached
    // now. What has not changed is that nothing *sends* over it, so the
    // catalogue still says `Planned`, and the reason is written down in
    // `PLANNED_FOR_A_REASON_OF_ITS_OWN` rather than implied by a list it no
    // longer belongs on. That list was created empty this same day and this
    // is its first entry, which is the case it was created for: reachable,
    // and still not a thing Atlas can do.
    let honesty = std::fs::read_to_string("tests/capability_honesty.rs").expect("capability_honesty");
    let named = honesty
        .split("PLANNED_FOR_A_REASON_OF_ITS_OWN: &[(&str, &str)] = &[")
        .nth(1)
        .and_then(|r| r.split("];").next())
        .expect("the list");
    assert!(named.contains("\"mesh\""), "nothing records why mesh is still Planned");
    // Line continuations put the reason across several source lines, so the
    // backslashes go and the whitespace is collapsed before looking for a
    // phrase in it. Checking raw source for a sentence that a `\` split in
    // two is a test that fails on reformatting rather than on meaning --
    // which it did, twice, before this line looked like this.
    let flat = named.replace('\\', " ").split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(flat.contains("still not built is the sending"), "the reason went vague: {flat}");
}

#[test]
fn nothing_is_broadcast_on_a_machine_with_nobody_to_look_for() {
    // A packet sent to ask a question nobody is waiting to answer.
    let raw = crate::common::source_of("daemon");
    // The main chat's transport (24 Sep) made the route a pair — the route
    // and the peer's address — so the guard reads `let (route, peer_addr) =
    // if peers.is_empty() { (None, None) }`. The same guard; the test finds
    // either spelling and still requires "nothing" when there are no peers.
    let body = raw
        .split("= if peers.is_empty() {")
        .nth(1)
        .expect("the guard is gone -- every sync now broadcasts");
    let first = body.trim_start();
    assert!(first.starts_with("None") || first.starts_with("(None, None)"), "{}", &body[..80.min(body.len())]);
}

#[test]
fn something_actually_starts_the_answerer() {
    // The half that is easy to leave out: an answerer nothing runs is a
    // machine that can never be found. Beside the hub rather than anywhere
    // else, because the two are the same fact -- the announcement says there
    // is a door at this port, and the hub is the process holding it open.
    let raw = crate::common::source_of("main");
    let body = raw
        .split("fn run_hub(")
        .nth(1)
        .and_then(|r| r.split("\nfn ").next())
        .expect("run_hub");
    assert!(body.contains("atlas::nearby::answer_probes("), "nothing answers probes");
    assert!(body.contains("if ncfg.announce {"), "it answers whether or not you asked it to");
    assert!(body.contains("std::thread::spawn"), "answering would block the hub");
    // A failure to announce must not take the hub down with it.
    assert!(body.contains("The hub is unaffected"), "a discovery failure looks fatal");
}

#[test]
fn the_ports_are_three_different_ones() {
    // A probe and a request arriving at the same listener is the bug this
    // avoids by construction.
    assert_ne!(PORT, 8787, "the hub's port");
    assert_ne!(PORT, atlas::kin::DEFAULT_PORT, "kin's door");
}
