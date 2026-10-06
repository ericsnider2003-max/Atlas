//! The door for friends and signals, inside the daemon's loop.
//!
//! Moved out of `server.rs` unchanged (audit Q6, 6 Oct 2026): one file of
//! thousands of lines was where every chat's edits collided.

use super::*;

impl SignalListener {
    /// Stop letting a peer in, on the door that is currently open.
    ///
    /// Returns whether one was there. See `kin::Door::forget` for what it
    /// cost not to have this: `Intent::ForgetPeer` said the peer could no
    /// longer reach you and the live door went on accepting them.
    pub fn forget_peer(&self, name: &str) -> bool {
        held(&self.door).forget(name)
    }

    /// The port the ordinary door listens on.
    pub fn port(&self) -> u16 {
        self.listeners.iter().find(|(_, s)| !s).and_then(|(l, _)| l.local_addr().ok()).map(|a| a.port()).unwrap_or(0)
    }

    /// The port of the sealed door, on this machine only: where Tor sends
    /// what arrives at your onion address.
    pub fn sealed_port(&self) -> u16 {
        self.listeners.iter().find(|(_, s)| *s).and_then(|(l, _)| l.local_addr().ok()).map(|a| a.port()).unwrap_or(0)
    }

    /// Hand out release files kept in this state folder to paired peers who
    /// ask for them by fingerprint.
    pub fn serve_releases(&self, state_root: std::path::PathBuf) {
        *held(&self.releases) = Some(state_root);
    }

    /// Note in `reached` each paired Atlas that comes to the door.
    pub fn note_reached(&self, reached: std::sync::Arc<crate::kin::Reached>) {
        *held(&self.reached) = Some(reached);
    }

    /// Open envelopes sealed to this key.
    pub fn serve_sealed(&self, me: crate::peerkey::Identity) {
        *held(&self.me) = Some(me);
    }

    /// Open the friend door on this listener: links made and kept in
    /// `invites` can be used to knock.
    pub fn accept_friends(&self, invites: crate::store::Store) {
        held(&self.door).accept_friends(invites);
    }

    /// Let a peer in, on the door that is currently open.
    pub fn admit_peer(&self, peer: crate::kin::Peer) -> bool {
        held(&self.door).admit(peer);
        true
    }

    /// On every address this machine has: IPv6 and IPv4 where the system
    /// allows both on one socket, and a second IPv4 socket where it doesn't.
    pub fn bind(port: u16, peers: Vec<crate::kin::Peer>) -> Result<SignalListener> {
        let mut listeners = Vec::new();
        if let Ok(l) = TcpListener::bind(("::", port)) {
            listeners.push(l);
        }
        // IPv4 on the same number. On port 0 that is the number the IPv6
        // socket was given: a door with two numbers is two doors. Windows'
        // IPv6 sockets don't take IPv4 (Linux's do), so skipping this for
        // port 0 left the door unreachable at 127.0.0.1 there (6 Oct 2026:
        // every friend-door test failed on Windows).
        let v4_port = match (port, listeners.first()) {
            (0, Some(l)) => l.local_addr().map(|a| a.port()).unwrap_or(0),
            _ => port,
        };
        if listeners.is_empty() || v4_port != 0 {
            match TcpListener::bind(("0.0.0.0", v4_port)) {
                Ok(l) => listeners.push(l),
                // Covered only if the IPv6 socket takes IPv4 too -- asked of
                // the socket, not assumed (6 Oct 2026). Linux's do; Windows'
                // don't, and there a port held on IPv4 by something else
                // left the door "open" on IPv6 only, with IPv4 visitors
                // reaching the other program.
                Err(e) if listeners.is_empty() || listeners.first().map(v6_only).unwrap_or(true) => {
                    return Err(AtlasError::Platform(format!("could not open the signal door on {v4_port}: {e}")))
                }
                Err(_) => {}
            }
        }
        let mut listeners: Vec<(TcpListener, bool)> = listeners.into_iter().map(|l| (l, false)).collect();
        let sealed = TcpListener::bind(("127.0.0.1", 0))
            .map_err(|e| AtlasError::Platform(format!("could not open the sealed door: {e}")))?;
        listeners.push((sealed, true));
        for (l, _) in &listeners {
            l.set_nonblocking(true).map_err(|e| AtlasError::Platform(e.to_string()))?;
        }
        Ok(SignalListener {
            listeners,
            door: std::sync::Mutex::new(crate::kin::Door::new(peers)),
            releases: std::sync::Mutex::new(None),
            me: std::sync::Mutex::new(None),
            seen: std::sync::Mutex::new(crate::wire::Seen::default()),
            kept: std::sync::Mutex::new(Vec::new()),
            reached: std::sync::Mutex::new(None),
        })
    }

    /// How many friends' connections are being kept open right now.
    pub fn kept_open_for_test(&self) -> usize {
        held(&self.kept).len()
    }

    /// A kept connection with a request waiting on it, if any. Closed and
    /// idle ones are let go on the way.
    pub(super) fn next_kept(&self) -> Option<(TcpStream, std::net::SocketAddr)> {
        let mut kept = held(&self.kept);
        let mut i = 0;
        while i < kept.len() {
            let (s, _, at) = &kept[i];
            if at.elapsed().as_secs() >= crate::kin::KEPT_IDLE_SECS + 30 {
                kept.remove(i);
                continue;
            }
            let _ = s.set_nonblocking(true);
            let mut b = [0u8; 1];
            match s.peek(&mut b) {
                // Closed by the other end.
                Ok(0) => {
                    kept.remove(i);
                }
                Ok(_) => {
                    let (s, remote, _) = kept.remove(i);
                    return Some((s, remote));
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => i += 1,
                Err(_) => {
                    kept.remove(i);
                }
            }
        }
        None
    }

    /// Check once. Returns immediately, with or without a message — this is
    /// what makes it safe to call every tick of a loop that must never stall.
    pub fn poll_once(&self, now: u64) -> Option<crate::kin::Arrived> {
        // A friend's next request on a connection kept open for it.
        if let Some((stream, remote)) = self.next_kept() {
            let _ = stream.set_nonblocking(false);
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
            return self.handle_one(stream, remote, true, now);
        }
        for (l, sealed_only) in &self.listeners {
            let Ok((stream, remote)) = l.accept() else { continue };
            // Reading and replying still happen synchronously, but only after
            // a connection has genuinely arrived -- the wait that must never
            // happen is the wait for a peer that isn't there. A short
            // timeout bounds even that.
            let _ = stream.set_nonblocking(false);
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
            return self.handle_one(stream, remote, *sealed_only, now);
        }
        None
    }

    /// Opened against the wall clock, not the caller's: whether an envelope
    /// is fresh is a fact about real time.
    pub(super) fn open(&self, env: &crate::wire::Envelope, _now: u64) -> Option<(String, crate::wire::Inner, crate::wire::ReplyKey)> {
        let now = crate::store::now();
        let me = held(&self.me);
        let me = me.as_ref()?;
        let mut seen = held(&self.seen);
        crate::wire::open(me, env, now, &mut seen).ok()
    }

    /// The token names a paired peer, and -- when the request came sealed --
    /// that peer's pinned key is the one that sealed it. A token copied onto
    /// someone else's envelope is refused.
    pub(super) fn token_fits(&self, token: &str, sealed_by: Option<&str>) -> bool {
        let door = held(&self.door);
        match (door.key_for_token(token), sealed_by) {
            (None, _) => false,
            (Some(_), None) => true,
            (Some(Some(pinned)), Some(by)) => pinned == by,
            // Not introduced yet: the first sealed request pins nothing, but
            // the token still has to be right.
            (Some(None), Some(_)) => true,
        }
    }

    pub(super) fn handle_one(&self, mut stream: TcpStream, remote: std::net::SocketAddr, sealed_only: bool, now: u64) -> Option<crate::kin::Arrived> {
        // Bounded in total, not per read, and no line longer than the head's
        // cap (28 Sep 2026): this runs on the daemon's own loop, and a peer
        // trickling a byte at a time used to hold it for hours.
        let until = std::time::Instant::now() + PEER_DEADLINE;
        let mut reader = BufReader::new(Deadlined::new(stream.try_clone().ok()?, until));
        let mut head = String::new();
        loop {
            let mut line = String::new();
            let room = 8192u64.saturating_sub(head.len() as u64) + 1;
            if reader.by_ref().take(room).read_line(&mut line).ok()? == 0 {
                break;
            }
            if line == "\r\n" || line == "\n" {
                break;
            }
            head.push_str(&line);
            if head.len() > 8192 {
                return None;
            }
        }
        // How much body this endpoint is allowed to send, decided from the
        // request line before a single byte of it is read.
        //
        // It was a flat 4096 for everything, which is right for a signal —
        // a sentence — and made a file handoff impossible: the body was
        // truncated to 4KB and then failed to parse, silently. Raising it
        // for everything would be the wrong fix: a peer-reachable endpoint
        // with a large body cap is a memory-exhaustion surface, and
        // `/signal` has no reason to want one. So the cap is per endpoint,
        // and the bound is computed from the file cap rather than guessed:
        // base64 is 4 bytes per 3, plus room for the JSON around it.
        let cap = if head.starts_with("POST /sealed ") {
            // Anything below, sealed: the largest is a handed-over file.
            (crate::kin::MAX_HANDOFF_FILE_BYTES / 3 * 4 + 8192) / 3 * 4 + 4096
        } else if head.starts_with("POST /friend ") {
            // Who you are and one secret: small.
            2048
        } else if head.starts_with("POST /group ") {
            // A signed list, escaped into JSON: bounded by the list cap, with
            // room for the escaping and the signature around it.
            crate::groups::MAX_STATE_BYTES * 2 + 1024
        } else if head.starts_with("POST /handoff ") {
            crate::kin::MAX_HANDOFF_FILE_BYTES / 3 * 4 + 8192
        } else if head.starts_with("POST /read ") {
            // A read receipt is only ids, but a backlog read at once can
            // acknowledge many. Bounded from the receipt cap so a large but
            // legitimate catch-up is not silently truncated (which would make
            // the receipt fail to parse and be re-sent forever), while still
            // far short of a memory-exhaustion surface: ids are short and the
            // door caps how many receipts a peer may send per window anyway.
            128 * 1024
        } else {
            4096
        };
        let len = content_length(&head).min(cap);
        let mut body = vec![0u8; len];
        reader.get_mut().allow_body(len);
        if len > 0 && reader.read_exact(&mut body).is_err() {
            return None;
        }
        let body = String::from_utf8_lossy(&body).to_string();

        // Kept open for the next request only where friends arrive through
        // Tor, only when asked, and only while there's room.
        let keep = sealed_only
            && head.to_ascii_lowercase().contains("\r\nconnection: keep-alive")
            && held(&self.kept).len() < crate::kin::MAX_KEPT;
        let (reply, arrived) = match parse_request(&head, &body) {
            Some(req) if req.method == "POST" && req.path == crate::wire::PATH => self.sealed(&req.body, now),
            // In the clear: only on the ordinary door, and only from this
            // machine, your home network, or your own private network. From
            // anywhere else a request must be sealed -- refused here before
            // its token is even read.
            Some(req) if !sealed_only && crate::onion::is_local_origin(remote.ip()) => self.answer(&req, None),
            _ => (Reply::denied(), None),
        };
        let keep = keep && reply.status == 200;
        let text = if keep { render(&reply).replacen("Connection: close\r\n", "Connection: keep-alive\r\n", 1) } else { render(&reply) };
        let written = stream.write_all(text.as_bytes()).and_then(|_| stream.flush()).is_ok();
        if keep && written {
            {
                let mut k = held(&self.kept);
                k.push((stream, remote, std::time::Instant::now()));
            }
        }
        arrived
    }

    /// A sealed envelope: opened, answered like any request, the answer
    /// sealed back.
    pub(super) fn sealed(&self, body: &str, now: u64) -> Answered {
        let Ok(env) = serde_json::from_str::<crate::wire::Envelope>(body) else { return (Reply::denied(), None) };
        let Some((from, inner, key)) = self.open(&env, now) else { return (Reply::denied(), None) };
        let req = inner_request(&inner);
        let (reply, arrived) = self.answer(&req, Some(&from));
        (Reply::ok(crate::wire::seal_reply(&key, reply.status, &reply.body)), arrived)
    }

    /// Every door, for a request that is in the clear from somewhere local or
    /// was opened from an envelope (`sealed_by` is then who sealed it).
    pub(super) fn answer(&self, req: &Request, sealed_by: Option<&str>) -> Answered {
        let denied = || (Reply::denied(), None);
        if req.method != "POST" {
            return denied();
        }
        // The friend door is checked first and alone: it is the only one
        // without a token, and nothing else may be reached without one.
        if req.path == "/friend" {
            return match route_friend(req, &self.door, sealed_by) {
                Some(Action::Befriended(b)) => (Reply::ok("{\"received\":true}"), Some(crate::kin::Arrived::Friend(b))),
                _ => denied(),
            };
        }
        let Some(token) = req.token.clone() else { return denied() };
        if !self.token_fits(&token, sealed_by) {
            return denied();
        }
        // A paired Atlas at the door is a paired Atlas that's up.
        {
            let (r, door) = (held(&self.reached), held(&self.door));
            if let (Some(r), Some(name)) = (r.as_ref(), door.name_for_token(&token)) {
                r.note(&name, crate::store::now());
            }
        }
        if req.path == "/release" {
            return self.release_piece(req, &token);
        }

        // Routed through `route_signal` and `route_handoff` rather than
        // reimplemented here.
        //
        // This function used to parse the body and call the door itself,
        // which meant two implementations of the same routing: these
        // functions, tested directly, and this copy, which is the one a
        // real peer actually reaches. Two copies of a security boundary
        // is one copy too many -- a rule added to one and forgotten in
        // the other looks exactly like a rule that is enforced. The
        // functions are the boundary now, and this is the socket.
        //
        // The lock is taken inside each of them, held for exactly as
        // long as the door needs and no longer -- unlike an early
        // version of this function, which took the door apart, handed it
        // to a throwaway Mutex, and never wrote the mutated rate-limit
        // state back. That would have reset every peer's standing on
        // every single message.
        let action = route_signal(req, &self.door, &token)
            .or_else(|| route_handoff(req, &self.door, &token))
            .or_else(|| route_chat(req, &self.door, &token))
            .or_else(|| route_read(req, &self.door, &token))
            .or_else(|| route_left(req, &self.door, &token))
            .or_else(|| route_hello(req, &self.door, &token))
            .or_else(|| route_group(req, &self.door, &token))
            .or_else(|| route_feedback(req, &self.door, &token));
        let arrived = match action {
            Some(Action::Signal(i)) => crate::kin::Arrived::Signal(i),
            Some(Action::Handed(d)) => crate::kin::Arrived::Handoff(d),
            Some(Action::Chatted(c)) => crate::kin::Arrived::Chat(c),
            Some(Action::ReadReceipt(r)) => crate::kin::Arrived::Read(r),
            Some(Action::LeftGroup(l)) => crate::kin::Arrived::Left(l),
            Some(Action::PeerHello(h)) => crate::kin::Arrived::Hello(h),
            Some(Action::PeerGroup(g)) => crate::kin::Arrived::Group(g),
            Some(Action::PeerFeedback(f)) => crate::kin::Arrived::Feedback(f),
            Some(Action::PeerFeedbackAnswer(f)) => crate::kin::Arrived::FeedbackAnswer(f),
            // No router can return anything else -- each checks its own
            // method and path first and builds exactly one variant.
            // Refused rather than assumed, so that if one ever grows a
            // second variant this fails shut instead of delivering it.
            _ => return denied(),
        };
        (Reply::ok("{\"received\":true}"), Some(arrived))
    }

    /// A piece of a release file, for a paired peer. It never becomes an
    /// `Arrived` -- nothing about it reaches the daemon.
    pub(super) fn release_piece(&self, req: &Request, token: &str) -> Answered {
        let now = crate::store::now();
        let allowed = held(&self.door).may_fetch(token, now).is_ok();
        if !allowed {
            return (Reply::denied(), None);
        }
        let piece = (|| {
            let root = held(&self.releases).clone()?;
            let sha = field(&req.body, "sha256")?;
            let offset = serde_json::from_str::<serde_json::Value>(&req.body).ok()?.get("offset")?.as_u64()?;
            crate::update_courier::chunk(&root, &sha, offset)
        })();
        match piece {
            Some((bytes, total)) => {
                (Reply::ok(format!("{{\"data\":\"{}\",\"total\":{total}}}", crate::b64::encode(&bytes))), None)
            }
            None => (Reply::not_found(), None),
        }
    }
}

/// The request inside an envelope, as the doors read it.
pub(super) fn inner_request(inner: &crate::wire::Inner) -> Request {
    Request {
        method: "POST".into(),
        path: inner.path.clone(),
        query: String::new(),
        token: (!inner.token.is_empty()).then(|| inner.token.clone()),
        token_from_url: false,
        body: inner.body.clone(),
    }
}

/// Whether a bound IPv6 listener refuses IPv4 too (Windows' default; Linux
/// is usually dual-stack). Asked of the socket rather than assumed.
/// `only_v6` is deprecated because *setting* it after bind does nothing;
/// reading it back is exactly right.
#[allow(deprecated)]
fn v6_only(l: &std::net::TcpListener) -> bool {
    l.only_v6().unwrap_or(true)
}
