//! The hub's server: binding, serving and answering.
//!
//! Moved out of `server.rs` unchanged (audit Q6, 6 Oct 2026): one file of
//! thousands of lines was where every chat's edits collided.

use super::*;

impl Server {
    pub fn bind(cfg: &ServerConfig, token: &str) -> Result<Server> {
        Server::bind_on(cfg, token, cfg.port)
    }

    /// `bind`, on `port` rather than the configured one (the hub falling
    /// back when its own port is taken: `open_hub`). Port 0 is any free one.
    pub(super) fn bind_on(cfg: &ServerConfig, token: &str, port: u16) -> Result<Server> {
        // The setting, honoured.
        //
        // `enabled` was never read here, and the daemon's start-up path set
        // it to `true` on a copy before calling this -- so `server.enabled:
        // false` in your own config listened anyway. Loopback-only with a
        // token is a narrow door, but a switch that does nothing is the wrong
        // kind of narrow: you turned it off and it stayed on.
        //
        // `atlas settings` passes an explicitly enabled config, because there
        // you have typed the command that opens the page.
        if !cfg.enabled {
            return Err(AtlasError::Config(
                "the local API is switched off in your settings (server.enabled)".into(),
            ));
        }
        if token.len() < 16 {
            return Err(AtlasError::Config("the API token is too short to be safe".into()));
        }
        // Loopback unless you named one other address, and never 0.0.0.0 --
        // `bind_address` is the rule and refuses anything public. Reaching
        // this from another device used to be "the VPN's job", which was only
        // half true: a VPN gives your phone a route to the machine and
        // loopback still refuses it.
        let where_to = bind_address(&cfg.reachable_from)
            .map_err(|why| AtlasError::Config(format!("server.reachable_from: {why}")))?;
        let listener = TcpListener::bind((where_to, port))
            .map_err(|e| AtlasError::Platform(format!("could not listen on port {port}: {e}")))?;
        let also = if where_to.is_loopback() {
            None
        } else {
            let port = listener.local_addr().map(|a| a.port()).unwrap_or(port);
            TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).ok()
        };
        Ok(Server {
            listener,
            also,
            token: token.to_string(),
            max_body: cfg.max_body,
            max_upload: cfg.max_upload,
            signals: None,
            failures: std::sync::Mutex::new(Failures::default()),
            answer_wait: ANSWER_WAIT,
            id: String::new(),
        })
    }

    /// Say this to `/hub/ping` (see `id`).
    pub(super) fn with_id(mut self, id: &str) -> Self {
        self.id = id.to_string();
        self
    }

    /// Wait this long for the daemon's answer instead of `ANSWER_WAIT` --
    /// for the tests of what happens after it runs out.
    #[doc(hidden)]
    pub fn with_answer_wait(mut self, wait: std::time::Duration) -> Self {
        self.answer_wait = wait;
        self
    }

    /// Open the notify-only door to a fixed set of peers. Takes the whole
    /// list rather than one-at-a-time `add_peer` on purpose -- who Atlas
    /// trusts for this is a decision made once, deliberately, not a list that
    /// grows by accident while the server is running.
    pub(super) fn with_peers(mut self, peers: Vec<crate::kin::Peer>) -> Self {
        self.signals = Some(std::sync::Mutex::new(crate::kin::Door::new(peers)));
        self
    }

    /// A wrong token: counted, and the answer held back by `delay_ms`.
    /// Capped at two seconds, so a run of them can't hold the one
    /// connection open for long.
    pub(super) fn slow_down_a_guess(&self) {
        let wait = match self.failures.lock().or_else(crate::crash::unpoison) {
            Ok(mut f) => {
                f.failed(crate::store::now());
                f.delay_ms()
            }
            Err(_) => 0,
        };
        if wait > 0 {
            std::thread::sleep(std::time::Duration::from_millis(wait));
        }
    }

    /// Something to tell you when wrong tokens keep arriving — a run of them
    /// is something other than you trying.
    pub fn guesses_worth_mentioning(&self) -> Option<String> {
        self.failures.lock().or_else(crate::crash::unpoison).ok().and_then(|f| f.worth_mentioning())
    }

    pub fn port(&self) -> u16 {
        self.listener.local_addr().map(|a| a.port()).unwrap_or(0)
    }

    /// Serve one request, waiting until one arrives.
    ///
    /// Right for the settings-only hub, which has nothing else to do while it
    /// waits. Wrong inside the daemon loop — see `poll_once`.
    pub fn serve_once(&self, handle: &mut dyn FnMut(Action) -> Reply) -> Result<Option<Action>> {
        let Some(also) = &self.also else {
            let (stream, _) = self.listener.accept()?;
            return self.handle_conn(stream, handle);
        };
        // Two sockets: take whichever is asked first.
        let _ = self.listener.set_nonblocking(true);
        let _ = also.set_nonblocking(true);
        loop {
            for l in [&self.listener, also] {
                if let Ok((stream, _)) = l.accept() {
                    let _ = stream.set_nonblocking(false);
                    return self.handle_conn(stream, handle);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    /// Check once. Returns immediately whether or not a browser was waiting.
    ///
    /// This is what lets the hub be served by the running Atlas rather than by
    /// a separate settings-only mode — which is why most of its pages used to
    /// answer "needs the full Atlas running". They were reachable; there was
    /// simply nothing behind them.
    ///
    /// 27 Sep 2026: nothing in Atlas serves the hub this way any more -- a
    /// click is several connections, and one a pass kept each waiting behind
    /// the others (`threaded`, `HubDoor`). Kept as the shape the listener is
    /// measured against (`tests/the_hub_answers_every_click.rs`), and it now
    /// makes its own listener non-blocking, which `polling` used to.
    pub fn poll_once(&self, handle: &mut dyn FnMut(Action) -> Reply) -> Option<Action> {
        let _ = self.listener.set_nonblocking(true);
        if let Some(also) = &self.also {
            let _ = also.set_nonblocking(true);
        }
        let (stream, _) = self
            .listener
            .accept()
            .or_else(|e| self.also.as_ref().map(|l| l.accept()).unwrap_or(Err(e)))
            .ok()?;
        // The wait that must never happen is the wait for a browser that
        // isn't there. Once one has genuinely connected, reading its request
        // synchronously is fine, and the read timeout bounds even that.
        let _ = stream.set_nonblocking(false);
        self.handle_conn(stream, handle).ok().flatten()
    }

    pub(super) fn handle_conn(
        &self,
        stream: TcpStream,
        handle: &mut dyn FnMut(Action) -> Reply,
    ) -> Result<Option<Action>> {
        let Some(mut asked) = self.read_asked(stream)? else {
            return Ok(None);
        };
        let r = handle(asked.action.clone());
        self.answer(&mut asked, r);
        Ok(Some(asked.action))
    }

    /// Read one request, check its credential, and answer everything that
    /// needs nothing of Atlas's state: the public files, the manifest, every
    /// refusal, the cookie trade and every 404. What is left -- an
    /// authenticated action -- comes back with the socket to answer it on.
    ///
    /// Split from the answering (27 Sep 2026) so the reading can happen on a
    /// connection's own thread (`threaded`) and the daemon only ever sees
    /// requests that have already arrived in full and been allowed: a slow
    /// or silent client, a probe that connects and says nothing, or a wrong
    /// token's deliberate delay no longer cost the assistant's loop anything.
    pub(super) fn read_asked(&self, mut stream: TcpStream) -> Result<Option<Asked>> {
        // Short, and not the whole budget. The old value was five seconds and
        // it was the *only* limit, which made it a per-read timeout rather
        // than a request deadline: a client that sent one byte before each
        // expiry held the daemon's tick indefinitely, because every byte
        // restarted the clock. `DEADLINE` below is what actually bounds it.
        stream.set_read_timeout(Some(std::time::Duration::from_secs(2)))?;
        let started = std::time::Instant::now();

        // How long one request may occupy the tick, in total.
        //
        // `poll_once` runs inline in `Daemon::run`, so this is time in which
        // Atlas does not listen, does not answer, does not serve the hub and
        // does not collect a message from another Atlas. Five seconds is
        // generous for a loopback request whose body is capped at 16KB, and
        // it is a ceiling rather than a target — a browser on the same
        // machine finishes in single-digit milliseconds.
        const DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);
        const DRAIN_DEADLINE: std::time::Duration = std::time::Duration::from_secs(30);
        // The header block's ceiling, enforced DURING the read.
        //
        // The old code checked `head.len() > 8192` after `read_line`
        // returned, which is after the whole line is already in memory. One
        // request of `GET / HTTP/1.1\r\nX: ` followed by an endless stream
        // with no newline grew the process without bound and answered 413
        // only once the client stopped — measured at 1.5GB of resident
        // memory from a single unauthenticated connection. `take` is the
        // fix: the reader itself cannot yield more than this.
        const MAX_HEAD: u64 = 8 * 1024;

        // ONE reader for the whole request.
        //
        // The first attempt at this wrapped the socket in `take(MAX_HEAD)`
        // and then opened a second `BufReader` for the body — which lost
        // every form POST, because reading the head had already pulled the
        // body's bytes into the first reader's buffer and the second reader
        // was looking at an empty socket. `by_ref().take(..)` bounds the
        // head phase without consuming the reader, so the body is read from
        // the same buffered stream that holds it.
        //
        // And every read of it stops at `DEADLINE` (28 Sep 2026). The
        // deadline used to be checked only between header lines, while
        // `read_line` waited up to two seconds per byte: a client sending one
        // byte every 1.9 seconds held a connection for about four hours, and
        // 48 of them filled `MAX_OPEN` so nobody else was answered.
        let mut reader = BufReader::new(Deadlined::new(stream.try_clone()?, started + DEADLINE));
        let mut head = String::new();
        loop {
            if started.elapsed() > DEADLINE {
                // Nothing written back: a client that never finished its
                // request has not asked a question to answer.
                return Ok(None);
            }
            let room = MAX_HEAD.saturating_sub(head.len() as u64);
            if room == 0 {
                let _ = stream.write_all(render(&Reply::too_big()).as_bytes());
                return Ok(None);
            }
            let mut line = String::new();
            // The cap is per read, so no single line can exceed what is left
            // of the header budget. That is the difference from the old
            // check, which ran after `read_line` had already taken the whole
            // line — however long — into memory.
            let read = reader.by_ref().take(room).read_line(&mut line)?;
            if read == 0 {
                break;
            }
            if line == "\r\n" || line == "\n" {
                break;
            }
            head.push_str(&line);
            if !line.ends_with('\n') {
                // The budget ran out mid-line. Refuse rather than treat a
                // truncated header as a complete one.
                let _ = stream.write_all(render(&Reply::too_big()).as_bytes());
                return Ok(None);
            }
        }

        // PARSE AND AUTHENTICATE BEFORE ALLOCATING THE BODY.
        //
        // The body used to be read first, with `vec![0u8; len]` sized from
        // the request's own `Content-Length` and capped at `max_upload` —
        // 28MB by default — and only then was the token checked. So an
        // unauthenticated `POST /hand/file` with `Content-Length: 29360128`
        // cost 28MB and a five-second hold, for free, from any local
        // process. The token is in the head; there is no reason to read a
        // byte of body before deciding whether this request may ask
        // anything at all.
        let Some(mut req) = parse_request(&head, "") else {
            let _ = stream.write_all(render(&Reply::not_found()).as_bytes());
            return Ok(None);
        };
        // The phone app's public files: the service worker and the icons.
        // Answered before the token check because a browser fetches them
        // without the cookie, and safe to because they are the same fixed
        // bytes on every install. Nothing of the request is read beyond its
        // first line, so this is no wider a door than a 401 is.
        if let Some(file) = crate::hub::public_file(&req.method, &req.path) {
            let _ = stream.write_all(&render_file(file.content_type, file.headers, file.bytes));
            let _ = stream.flush();
            return Ok(None);
        }
        // Which Atlas answers here (`id`), for "is Atlas running?" asked
        // from outside: nothing but the start's own id, which opens nothing.
        if req.method == "GET" && req.path == PING_PATH {
            let body = serde_json::json!({ "atlas": self.id }).to_string();
            let _ = stream.write_all(&render_file("application/json", "Cache-Control: no-store\r\n", body.as_bytes()));
            let _ = stream.flush();
            return Ok(None);
        }

        let given = req.token.clone();

        // The manifest, which carries the token (see `hub::manifest`). Checked
        // on its own terms and answered here, before the general rules below,
        // for two reasons:
        //
        // - `?t=` is taken directly, with no trade for a cookie: a browser
        //   fetches a manifest without the cookie and does not follow the
        //   redirect into a page, so the 303 below would install an app
        //   that opens on nothing.
        // - Either credential will do. A stale cookie beside a correct `?t=`
        //   would otherwise shadow it, since a cookie outranks the URL.
        //
        // Only the hub token opens it. A peer's token never does: the thing
        // it hands back is the hub token itself.
        if req.method == "GET" && req.path == crate::hub::MANIFEST_PATH {
            let url_token = query_field(&req.query, "t");
            let ok = token_matches(&self.token, given.as_deref())
                || token_matches(&self.token, url_token.as_deref());
            if ok {
                let body = crate::hub::manifest(&self.token);
                let _ = stream.write_all(&render_file(
                    "application/manifest+json",
                    "Cache-Control: no-store\r\n",
                    body.as_bytes(),
                ));
            } else {
                self.slow_down_a_guess();
                let _ = stream.write_all(render(&Reply::denied()).as_bytes());
            }
            let _ = stream.flush();
            return Ok(None);
        }

        // Another page on this machine riding the hub's cookie: turned away
        // before any token is looked at.
        // A link carrying the token itself (`?t=`) was made by Atlas, so
        // following it from another page -- a note, a chat -- still opens.
        let carries_its_token = req.method == "GET" && token_matches(&self.token, query_field(&req.query, "t").as_deref());
        if from_another_page(&head) && !carries_its_token {
            let _ = stream.write_all(render(&Reply::denied()).as_bytes());
            let _ = stream.flush();
            return Ok(None);
        }
        let is_hub = token_matches(&self.token, given.as_deref());
        // A peer credential is checked against its own space. `knows` only
        // answers whether this token belongs to a paired peer; what such a
        // request may *do* is still decided solely by `route_signal` and
        // `route_handoff`, which is the property that keeps a peer token
        // incapable of reaching a hub page.
        let is_peer = match (&self.signals, given.as_deref()) {
            (Some(door), Some(t)) => door.lock().or_else(crate::crash::unpoison).map(|d| d.knows(t)).unwrap_or(false),
            _ => false,
        };
        if !is_hub && !is_peer {
            self.slow_down_a_guess();
            let r = if Reply::wants_a_page(&req.method, &req.path) { Reply::denied_page() } else { Reply::denied() };
            let _ = stream.write_all(render(&r).as_bytes());
            let _ = stream.flush();
            return Ok(None);
        }
        if is_hub {
            if let Ok(mut f) = self.failures.lock().or_else(crate::crash::unpoison) {
                f.succeeded();
            }
        }

        // Authenticated. Now the body, with the cap its endpoint earns.
        let len = content_length(&head);
        let cap = body_cap(&req, self.max_body, self.max_upload);
        if len > cap {
            let too_big = if req.path.starts_with("/hub") { Reply::too_big_page(&req.path) } else { Reply::too_big_for(cap) };
            if req.path.starts_with("/hub") {
                // 30 Sep 2026: the drain had the request's own five seconds,
                // so a big file over Wi-Fi was still arriving when it stopped
                // and the browser showed a reset instead of this page. The
                // drain gets its own half-minute, still bounded.
                let drain_until = std::time::Instant::now() + DRAIN_DEADLINE;
                // Read what the browser is still sending before answering, up
                // to the upload cap and the deadline: closing on unread bytes
                // resets the connection, and the browser then shows its own
                // error instead of this page.
                let mut left = len.min(self.max_upload) as u64;
                let mut sink = [0u8; 16 * 1024];
                reader.get_mut().allow_body(0);
                while left > 0 && std::time::Instant::now() < drain_until {
                    match reader.by_ref().take(left.min(sink.len() as u64)).read(&mut sink) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => left -= n as u64,
                    }
                }
            }
            let _ = stream.write_all(render(&too_big).as_bytes());
            let _ = stream.flush();
            return Ok(None);
        }
        if len > 0 {
            if started.elapsed() > DEADLINE {
                return Ok(None);
            }
            let mut body = vec![0u8; len];
            // The same reader the head came from — see the note above it.
            reader.get_mut().allow_body(len);
            reader.read_exact(&mut body)?;
            req.body = String::from_utf8_lossy(&body).to_string();
        }

        // The narrow door first, and it is checked against its own
        // credential space entirely -- a peer token that happens to also
        // equal the phone token (it never will; they are generated
        // independently, but "never by construction" is not the same
        // guarantee as "never by an if-statement") still could not reach
        // anything past route_signal, because route_signal is the only
        // routing function ever called for a peer-authenticated request.
        if let (Some(door), Some(t)) = (&self.signals, given.as_deref()) {
            if let Some(action) = route_signal(&req, door, t) {
                return Ok(Some(Asked { stream, action, page: None }));
            }
        }

        if !is_hub {
            // A paired peer that asked for something other than its own two
            // endpoints. Refused, not routed.
            let _ = stream.write_all(render(&Reply::denied()).as_bytes());
            let _ = stream.flush();
            return Ok(None);
        }

        // The token came in the address and nowhere else, so trade it for a
        // cookie and send the browser to the clean path. Only for a GET: a
        // 303 on a POST would drop the body, and a form is never the first
        // thing a browser does with a printed address.
        //
        // Done after `route` has confirmed the path exists, so a typo in a
        // pasted URL still answers 404 rather than redirecting to itself.
        if req.token_from_url && req.method == "GET" && route(&req).is_some() {
            let r = Reply::cookie_then(&req.path, &self.token);
            let _ = stream.write_all(render(&r).as_bytes());
            let _ = stream.flush();
            return Ok(None);
        }

        let reply = match route(&req) {
            // ---- passphrases only on a private line (pages, 27 Sep 2026) ----
            // Where the peer's address and the parsed action first meet. A
            // form carrying a secret from anywhere but this machine or
            // Tailscale is answered without being handed to Atlas at all.
            // Kept as one small hunk so it can move with connection handling.
            Some(action)
                if action.carries_a_secret()
                    && !stream.peer_addr().map(|a| private_line(a.ip())).unwrap_or(false) =>
            {
                secret_refused_page()
            }
            // ---- end of the private-line check ----
            Some(action) => {
                return Ok(Some(Asked { stream, action, page: Some((req.method.clone(), req.path.clone())) }));
            }
            None if req.method == "GET" && req.path.starts_with("/hub") && !req.path.contains('.') => Reply::not_found_page(&req.path),
            None => {
                // A form that posts somewhere nothing answers: the fault is
                // Atlas's, so say so in words, with the way back.
                if req.method == "POST" && req.path.starts_with("/hub") {
                    crate::errln!("atlas: nothing answers a form at {}", req.path);
                    Reply::not_found_page(&req.path)
                } else {
                    Reply::not_found()
                }
            }
        };
        let _ = stream.write_all(render(&reply).as_bytes());
        let _ = stream.flush();
        Ok(None)
    }

    /// Send Atlas's answer to an action `read_asked` let through.
    pub(super) fn answer(&self, asked: &mut Asked, reply: Reply) {
        let stream = &mut asked.stream;
        let Some((method, path)) = &asked.page else {
            // The narrow door: the answer as it is.
            let _ = stream.write_all(render(&reply).as_bytes());
            let _ = stream.flush();
            return;
        };
        let mut r = reply;
        // Something in Atlas failed while answering a page or a form:
        // said in words with the way back, never a bare JSON error.
        if r.status >= 500 && r.kind == Body::Json && path.starts_with("/hub") && !path.contains('.') {
            crate::errln!("atlas: {} {} failed: {}", method, path, r.body);
            r = Reply::failed_page();
        }
        // Every page, from the one place that knows the token — the
        // daemon's hub and settings-only mode both answer through
        // here. See `hub::with_app_head`.
        if r.kind == Body::Html && r.status == 200 {
            r.body = crate::hub::with_app_head(r.body, &self.token);
        }
        match &r.bytes {
            // Kept a day in the browser: the same sample is the same bytes.
            Some((mime, b)) => {
                let _ = stream.write_all(&render_file(mime, "Cache-Control: private, max-age=86400\r\n", b));
            }
            None => {
                let _ = stream.write_all(render(&r).as_bytes());
            }
        }
        let _ = stream.flush();
    }
}
