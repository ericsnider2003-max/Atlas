//! Finding your other Atlas on the same network, without typing an address.
//!
//! # What was missing
//!
//! Two Atlases can already talk. `kin.rs` is the door one knocks on,
//! `elsewhere.rs` is the asking, `server.rs` answers, and every request
//! carries a token. All of it works and none of it needs a third party.
//!
//! What it needs is this, in `config/tools.yaml`:
//!
//! ```yaml
//! elsewhere:
//!   known:
//!     - name: homelab
//!       host: "10.0.0.9"
//! ```
//!
//! A hand-typed address, on a home network that hands out a different one
//! after a reboot. So the one transport Atlas has that reaches another
//! machine directly was gated behind a setting that goes stale on its own,
//! and `mesh.rs` carried four routes -- `SameNetwork`, `Mesh`, `Cable`,
//! `Cloud` -- of which only `Cloud` was ever reachable, because `Cloud` is a
//! folder and needs no address at all.
//!
//! This is the missing half of `Path::SameNetwork`: a shout on the local
//! network and whatever answers.
//!
//! # What this is not
//!
//! **Discovery is not trust, and nothing here grants access.** Finding a
//! machine tells you its name and where to reach it, and that is all it may
//! ever do. The token still comes from the other Atlas -- `atlas hub` prints
//! it there -- and is still required on every request. An announcement
//! carries no secret, so anything listening on the network learns only that a
//! machine calling itself "homelab" has a door open, which it could learn
//! by connecting to the port anyway.
//!
//! Answering is **off by default**, and that is deliberate rather than
//! cautious-by-habit. The network you are on is not always your own: a café,
//! an office, a hotel. Announcing your machine's name to it is a small thing
//! that you should choose rather than inherit. Looking is harmless and is on.
//!
//! # Why UDP and nothing else
//!
//! A broadcast to the local network is the only way to ask "is anyone there"
//! without already knowing where there is, which is the entire problem. mDNS
//! would be the conventional answer and is a large specification, a
//! dependency, and a second name-resolution system to be wrong in; this is
//! forty lines of `std::net` with no third party at all, and it answers the
//! one question being asked.
//!
//! Broadcast does not cross a router, which is the property that makes it
//! safe: "the same network" is exactly the set of machines it can reach.

use serde::{Deserialize, Serialize};
use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

/// The port the shouting happens on.
///
/// Distinct from the hub's 8787 and `kin`'s door: three ports, three
/// different things, and sharing one would mean a probe and a request
/// arriving at the same listener.
pub const PORT: u16 = 8789;

/// What a probe says.
///
/// A fixed word rather than an empty packet, so a stray datagram on this port
/// is ignored rather than answered.
pub const ASKING: &str = "atlas-who-is-there-1";

/// An Atlas that answered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Found {
    /// What it calls itself.
    pub name: String,
    /// Where to reach its door.
    pub host: String,
    pub port: u16,
}

impl Found {
    /// The `elsewhere.known` entry to add, with the token left for you.
    ///
    /// Deliberately not written into your config by Atlas. The token is the
    /// whole of the access decision, and a half-filled entry that only needs
    /// one more field is an invitation to paste a token in without thinking
    /// about which machine it lets in.
    pub fn how_to_add(&self) -> String {
        format!(
            "  - name: {}\n    host: \"{}\"\n    port: {}\n    token: \"<run `atlas hub` on {} \
             and copy the token it prints>\"",
            self.name, self.host, self.port, self.name
        )
    }
}

/// The line an answering Atlas sends back.
///
/// Name and port, nothing else. Not the token, not what is on the machine,
/// not who is using it.
pub fn answer_for(name: &str, door: u16) -> String {
    format!("atlas-here-1 {} {}", name.trim().replace(char::is_whitespace, "-"), door)
}

/// Read an answer. `None` for anything that is not one.
pub fn answer_from(line: &str, from: SocketAddr) -> Option<Found> {
    let mut parts = line.trim().split_whitespace();
    if parts.next()? != "atlas-here-1" {
        return None;
    }
    let name = parts.next()?.to_string();
    let port: u16 = parts.next()?.parse().ok()?;
    if name.is_empty() || port == 0 {
        return None;
    }
    Some(Found { name, host: from.ip().to_string(), port })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct NearbyConfig {
    /// Answer other Atlases asking who is here.
    ///
    /// Off by default. See the module header: the network you are on is not
    /// always your own, and announcing your machine's name to a café is a
    /// choice rather than a default.
    pub announce: bool,
    /// How long to wait for answers. Everything on a local network replies in
    /// well under a second; the rest of the wait is for the machine that was
    /// busy.
    pub listen_ms: u64,
}

impl Default for NearbyConfig {
    fn default() -> Self {
        NearbyConfig { announce: false, listen_ms: 700 }
    }
}

/// Shout, and collect whatever answers.
///
/// Returns machines in the order they replied, with duplicates removed --
/// a machine on two interfaces answers twice, and it is one machine.
///
/// Errors are the socket's own words. A machine with no network at all fails
/// here, and saying so beats an empty list that reads as "nobody is there".
pub fn look(cfg: &NearbyConfig) -> Result<Vec<Found>, String> {
    // Port 0: the OS picks. The prober does not need a known port, only the
    // answerer does, and binding a fixed one here would stop two Atlases on
    // the same machine both looking.
    let sock = UdpSocket::bind(("0.0.0.0", 0)).map_err(|e| e.to_string())?;
    sock.set_broadcast(true).map_err(|e| e.to_string())?;
    sock.set_read_timeout(Some(Duration::from_millis(cfg.listen_ms)))
        .map_err(|e| e.to_string())?;
    sock.send_to(ASKING.as_bytes(), ("255.255.255.255", PORT)).map_err(|e| e.to_string())?;

    let mut out: Vec<Found> = Vec::new();
    let mut buf = [0u8; 512];
    let until = std::time::Instant::now() + Duration::from_millis(cfg.listen_ms);
    while std::time::Instant::now() < until {
        match sock.recv_from(&mut buf) {
            Ok((n, from)) => {
                let line = String::from_utf8_lossy(&buf[..n]);
                if let Some(f) = answer_from(&line, from) {
                    // Same name and host is the same machine, however many
                    // interfaces it answered on.
                    if !out.iter().any(|o| o.name == f.name && o.host == f.host) {
                        out.push(f);
                    }
                }
            }
            // A timeout is how the wait ends, not a failure.
            Err(_) => break,
        }
    }
    Ok(out)
}

/// Answer probes until told to stop.
///
/// Blocking, and meant for a thread of its own. Returns on a socket error
/// rather than looping on one, so a port that got taken is reported instead
/// of spun on.
///
/// `stop` is checked between datagrams, so this returns within one read
/// timeout of being asked to.
pub fn answer_probes(
    name: &str,
    door: u16,
    cfg: &NearbyConfig,
    stop: &dyn Fn() -> bool,
) -> Result<(), String> {
    if !cfg.announce {
        return Ok(());
    }
    let sock = UdpSocket::bind(("0.0.0.0", PORT)).map_err(|e| e.to_string())?;
    sock.set_read_timeout(Some(Duration::from_millis(400))).map_err(|e| e.to_string())?;
    let reply = answer_for(name, door);
    let mut buf = [0u8; 512];
    while !stop() {
        let Ok((n, from)) = sock.recv_from(&mut buf) else { continue };
        if String::from_utf8_lossy(&buf[..n]).trim() == ASKING {
            // A failed reply is not worth stopping for: the other machine
            // asks again, and one unanswered probe is a slower discovery
            // rather than a broken one.
            let _ = sock.send_to(reply.as_bytes(), from);
        }
    }
    Ok(())
}

/// What Atlas says about what it found.
pub fn spoken(found: &[Found], cfg: &NearbyConfig) -> String {
    if found.is_empty() {
        // The two reasons for an empty list are completely different
        // problems, and a single "nothing found" sends you looking at the
        // wrong one.
        return "Nothing answered. Either there's no other Atlas on this network, or the one \
                that's there hasn't been told it may answer — that's `nearby.announce: true` \
                in its settings, on that machine."
            .into();
    }
    let mut s = format!(
        "{} on this network:",
        if found.len() == 1 { "One other Atlas".to_string() } else { format!("{} Atlases", found.len()) }
    );
    for f in found {
        s.push_str(&format!("\n  {} at {}:{}", f.name, f.host, f.port));
    }
    // The part that is not automatic, said every time rather than once.
    s.push_str(
        "\n\nFinding it is not being allowed in. To actually reach one, add it under \
         `elsewhere.known` with that machine's own token:\n\n",
    );
    s.push_str(&found[0].how_to_add());
    if !cfg.announce {
        s.push_str(
            "\n\n(This machine isn't answering probes itself — `nearby.announce` is off, so \
             the others can't find you the same way.)",
        );
    }
    s
}
