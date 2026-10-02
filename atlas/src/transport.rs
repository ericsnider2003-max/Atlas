//! Sending a bundle straight to your other Atlas on the same network.
//!
//! `sync` says any carrier that moves a file can sync Atlas, and the folder is
//! the workhorse. But when your phone and your laptop are on the same wifi, the
//! folder is a slow way to move something across the room: it waits on a cloud
//! provider to notice, upload, and hand back down. `nearby::look` already finds
//! the other machine by shouting on the network and hearing it answer — what
//! was missing was the last step, actually handing the bundle across. The
//! daemon even said so out loud: "sending straight across isn't built yet."
//!
//! This is that. A tiny framed protocol over a plain TCP connection: connect to
//! the peer, send it the bundle, get its bundle back. One round trip is a full
//! two-way sync — you learn what it did, it learns what you did — so a phone
//! coming back onto the home wifi is caught up in the time it takes to open a
//! socket, with no folder, no cloud, no server, and nothing on the internet.
//!
//! The payload is opaque here on purpose. `sync::seal` / `sync::read_bundle`
//! decide sealed-or-plain exactly as they do for the folder, and this module
//! just moves the bytes — so the wire is protected by the same household key,
//! and the transport has no idea what a bundle is. Offline-first to the core:
//! the only thing it needs is a local network, and it degrades to the folder
//! when there isn't one.
//!
//! Style note: the receiver is a `poll`, not a thread that blocks on `accept`.
//! Atlas advances a step at a time on one clock — the same reason `run_the_night`
//! is written as "advance by one" — so the daemon asks this "is anyone waiting?"
//! once per tick and never blocks the turn that also answers you.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

/// The port a device listens on for a direct sync. Distinct from the hub
/// (8787), `kin`'s door (8788) and `nearby`'s shout — one job, one port, so a
/// stray connection on any of them is never mistaken for another.
pub const SYNC_PORT: u16 = 8790;

/// The most a single bundle frame may be. A bundle is a log of short events, so
/// this is enormous headroom; it exists so a wrong or hostile peer can't ask us
/// to allocate the machine's whole memory with a four-byte length.
pub const MAX_FRAME: u32 = 64 * 1024 * 1024;

/// The first bytes of every frame. Not security — a household key is that — but
/// it means a probe from some other protocol that lands on this port is turned
/// away as "not Atlas" instead of read as a length.
const MAGIC: &[u8; 4] = b"ATL1";

fn other(msg: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, msg.to_string())
}

/// Write one length-prefixed frame: magic, a 4-byte big-endian length, bytes.
fn write_frame(w: &mut impl Write, bytes: &[u8]) -> std::io::Result<()> {
    if bytes.len() as u64 > MAX_FRAME as u64 {
        return Err(other("bundle is larger than a sync frame allows"));
    }
    w.write_all(MAGIC)?;
    w.write_all(&(bytes.len() as u32).to_be_bytes())?;
    w.write_all(bytes)?;
    w.flush()
}

/// Read one frame, refusing a length that isn't ours or is absurd.
fn read_frame(r: &mut impl Read) -> std::io::Result<Vec<u8>> {
    let mut magic = [0u8; 4];
    r.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(other("that isn't an Atlas sync connection"));
    }
    let mut len_bytes = [0u8; 4];
    r.read_exact(&mut len_bytes)?;
    let len = u32::from_be_bytes(len_bytes);
    if len > MAX_FRAME {
        return Err(other("the other side announced a bundle too large to accept"));
    }
    // Read what actually arrives rather than allocating what was announced:
    // a stranger announcing 64 MB and sending nothing costs nothing.
    let mut buf = Vec::new();
    r.take(len as u64).read_to_end(&mut buf)?;
    if buf.len() != len as usize {
        return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "the bundle stopped part way"));
    }
    Ok(buf)
}

/// The sending side: connect to `host:port`, hand over `outgoing`, and return
/// what the peer sent back — its own bundle. One call is a complete two-way
/// sync. `timeout` bounds every step, so a peer that answered the shout but
/// then went away can't hang the turn.
pub fn exchange(
    host: &str,
    port: u16,
    outgoing: &[u8],
    timeout: Duration,
) -> std::io::Result<Vec<u8>> {
    // Resolve and connect with a bound wait, so a stale address from discovery
    // fails fast instead of blocking on a dead host.
    let addr = (host, port)
        .to_socket_addrs_first()
        .ok_or_else(|| other("couldn't resolve the peer's address"))?;
    let mut stream = TcpStream::connect_timeout(&addr, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    write_frame(&mut stream, outgoing)?;
    read_frame(&mut stream)
}

/// One-shot address resolution helper, kept as a trait so `exchange` reads
/// cleanly. Returns the first resolved socket address.
trait FirstAddr {
    fn to_socket_addrs_first(&self) -> Option<std::net::SocketAddr>;
}
impl FirstAddr for (&str, u16) {
    fn to_socket_addrs_first(&self) -> Option<std::net::SocketAddr> {
        use std::net::ToSocketAddrs;
        self.to_socket_addrs().ok()?.next()
    }
}

/// The receiving side: a bound listener the daemon polls once a tick.
pub struct Server {
    listener: TcpListener,
}

impl Server {
    /// Bind on every interface so a peer on the LAN can reach it. Non-blocking,
    /// so `poll` never waits on `accept`.
    pub fn bind(port: u16) -> std::io::Result<Server> {
        let listener = TcpListener::bind(("0.0.0.0", port))?;
        listener.set_nonblocking(true)?;
        Ok(Server { listener })
    }

    /// Bind to loopback on an OS-chosen port — for tests, and for a device that
    /// only wants to be reachable from itself.
    pub fn bind_local_ephemeral() -> std::io::Result<Server> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        listener.set_nonblocking(true)?;
        Ok(Server { listener })
    }

    pub fn local_addr(&self) -> std::io::Result<std::net::SocketAddr> {
        self.listener.local_addr()
    }

    /// Serve at most one waiting peer, without blocking. Returns `Ok(true)` if
    /// one was served, `Ok(false)` if nobody was waiting. `handle` is given the
    /// peer's bundle bytes and returns ours to send back — that's where the
    /// daemon merges what arrived and builds its reply.
    ///
    /// One peer per call on purpose: a step at a time, like everything else on
    /// this clock. A busy moment with two peers waiting is caught on the next
    /// tick, not by looping here and holding the turn.
    pub fn poll(
        &self,
        timeout: Duration,
        handle: impl FnOnce(Vec<u8>) -> Vec<u8>,
    ) -> std::io::Result<bool> {
        self.poll_from(timeout, |_, incoming| Some(handle(incoming)))
    }

    /// As `poll`, but `handle` is told who connected and may answer nothing
    /// at all (`None`): the connection is closed with no bundle sent. That is
    /// how a stranger on the same Wi-Fi is turned away (1 Oct 2026 security
    /// pass: anyone who connected used to get your notes back).
    pub fn poll_from(
        &self,
        timeout: Duration,
        handle: impl FnOnce(std::net::IpAddr, Vec<u8>) -> Option<Vec<u8>>,
    ) -> std::io::Result<bool> {
        match self.listener.accept() {
            Ok((mut stream, peer)) => {
                stream.set_nonblocking(false)?;
                stream.set_read_timeout(Some(timeout))?;
                stream.set_write_timeout(Some(timeout))?;
                let incoming = read_frame(&mut stream)?;
                if let Some(reply) = handle(peer.ip(), incoming) {
                    write_frame(&mut stream, &reply)?;
                }
                Ok(true)
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(false),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn a_frame_survives_the_round_trip_exactly() {
        let payload = b"fourteen things from your phone".to_vec();
        let mut wire = Vec::new();
        write_frame(&mut wire, &payload).unwrap();
        let mut cursor = std::io::Cursor::new(wire);
        assert_eq!(read_frame(&mut cursor).unwrap(), payload);
    }

    #[test]
    fn a_connection_that_isnt_atlas_is_turned_away() {
        let mut cursor = std::io::Cursor::new(b"GET / HTTP/1.1\r\n".to_vec());
        assert!(read_frame(&mut cursor).is_err());
    }

    #[test]
    fn one_exchange_is_a_full_two_way_sync() {
        // The server side answers with its own payload; the client gets it.
        let server = Server::bind_local_ephemeral().unwrap();
        let addr = server.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();

        let handle = std::thread::spawn(move || {
            // Wait (a few polls) for the client to arrive, then serve once.
            loop {
                let served = server
                    .poll(Duration::from_secs(2), |incoming| {
                        tx.send(incoming).unwrap();
                        b"laptop's bundle".to_vec()
                    })
                    .unwrap();
                if served {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        });

        let got = exchange(
            &addr.ip().to_string(),
            addr.port(),
            b"phone's bundle",
            Duration::from_secs(2),
        )
        .unwrap();

        assert_eq!(got, b"laptop's bundle", "client receives the server's reply");
        assert_eq!(rx.recv().unwrap(), b"phone's bundle", "server received the client's bundle");
        handle.join().unwrap();
    }

    #[test]
    fn two_real_logs_converge_over_a_socket() {
        // The end-to-end proof: a phone and a laptop, each with events the other
        // has never seen, exchange bundles over a real TCP socket on loopback
        // and both come away with everything. No folder, no cloud.
        use crate::sync::{make_bundle, merge, Log, What};

        let mut laptop = Log::new("laptop");
        laptop.append(What::Captured { id: "l1".into(), text: "rack parts list".into() }, 100);
        let mut phone = Log::new("phone");
        phone.append(What::Captured { id: "p1".into(), text: "a thought on the drive".into() }, 200);

        // The laptop's bundle, ready to hand back to whoever connects.
        let laptop_bundle = serde_json::to_vec(&make_bundle(&laptop, "laptop", 0, 300)).unwrap();

        let server = Server::bind_local_ephemeral().unwrap();
        let addr = server.local_addr().unwrap();
        let (got_tx, got_rx) = mpsc::channel();
        let srv = std::thread::spawn(move || loop {
            let served = server
                .poll(Duration::from_secs(2), |incoming| {
                    got_tx.send(incoming).unwrap();
                    laptop_bundle.clone()
                })
                .unwrap();
            if served {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        });

        // The phone connects, sends its bundle, and gets the laptop's back.
        let phone_bundle = serde_json::to_vec(&make_bundle(&phone, "phone", 0, 300)).unwrap();
        let from_laptop = exchange(&addr.ip().to_string(), addr.port(), &phone_bundle, Duration::from_secs(2)).unwrap();
        srv.join().unwrap();

        // The laptop takes in the phone's bundle.
        let phone_side: crate::sync::Bundle =
            serde_json::from_slice(&got_rx.recv().unwrap()).unwrap();
        let m_laptop = merge(&laptop.events, &phone_side.events, 400);
        // clean counts events from both sides — the merge replays the union.
        assert_eq!(m_laptop.clean, 2, "laptop's own event plus the phone's, none clashing");
        assert!(phone_side.events.iter().any(|e| e.what == (What::Captured { id: "p1".into(), text: "a thought on the drive".into() })), "the phone's event actually crossed the wire");

        // The phone takes in the laptop's bundle.
        let laptop_side: crate::sync::Bundle = serde_json::from_slice(&from_laptop).unwrap();
        let m_phone = merge(&phone.events, &laptop_side.events, 400);
        assert_eq!(m_phone.clean, 2, "phone's own event plus the laptop's, none clashing");
        assert!(laptop_side.events.iter().any(|e| e.what == (What::Captured { id: "l1".into(), text: "rack parts list".into() })));
    }
}
