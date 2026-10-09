//! Which Atlas answers on the hub's port, and trying again.
//!
//! Moved out of `server.rs` unchanged (audit Q6, 6 Oct 2026): one file of
//! thousands of lines was where every chat's edits collided.

use super::*;

impl Deadlined {
    pub(super) fn new(stream: TcpStream, until: std::time::Instant) -> Deadlined {
        Deadlined { stream, until }
    }

    /// Time for a body of `bytes` to arrive, at no less than
    /// `MIN_BODY_RATE` bytes a second on top of the head's deadline: a large
    /// upload from a phone, or a file over Tor, may take longer than a head
    /// does, but a trickle still runs out of time.
    pub(super) fn allow_body(&mut self, bytes: usize) {
        let secs = (bytes as u64).div_ceil(MIN_BODY_RATE);
        self.until = std::time::Instant::now() + std::time::Duration::from_secs(5 + secs);
    }
}

impl Read for Deadlined {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let left = self.until.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "the request took too long"));
        }
        self.stream.set_read_timeout(Some(left.min(std::time::Duration::from_secs(2))))?;
        self.stream.read(buf)
    }
}

/// A Talk page message given up on, kept rather than dropped.
pub(super) fn keep_if_talk(action: &Action, late: &LateTalk) {
    if let Some(talk) = talk_in(action) {
        if let Ok(mut v) = late.lock().or_else(crate::crash::unpoison) {
            v.push(talk);
        }
    }
}

/// The words and "said aloud" of a Talk page post, if this is one.
pub(super) fn talk_in(action: &Action) -> Option<(String, bool)> {
    let Action::HubPost { path, fields } = action else { return None };
    if path != "/hub/talk" {
        return None;
    }
    let field = |k: &str| fields.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    let text = field("text").unwrap_or_default();
    if text.trim().is_empty() {
        return None;
    }
    Some((text.trim().to_string(), field("spoken").as_deref() == Some("1")))
}

impl Waiting {
    /// Claim this request for answering. False when the connection has
    /// given up on it, or it has waited longer than any connection waits.
    pub(super) fn take(&self) -> bool {
        use std::sync::atomic::Ordering;
        if self.arrived.elapsed() >= self.wait {
            if self.state.compare_exchange(WAITING, ABANDONED, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                keep_if_talk(&self.action, &self.late);
            }
            return false;
        }
        self.state.compare_exchange(WAITING, TAKEN, Ordering::SeqCst, Ordering::SeqCst).is_ok()
    }
}

impl Clone for Serving {
    fn clone(&self) -> Self {
        Serving {
            server: self.server.clone(),
            tx: self.tx.clone(),
            open: self.open.clone(),
            per_address: self.per_address.clone(),
            port: self.port.clone(),
            news: self.news.clone(),
            shut: self.shut.clone(),
            late: self.late.clone(),
            snapshots: self.snapshots.clone(),
        }
    }
}

impl Serving {
    /// Start answering on `server`'s sockets: one accepting thread for each
    /// (the one asked for, and loopback beside it when `reachable_from`
    /// named another address -- merge of 28 Sep 2026: the desktop window
    /// always uses 127.0.0.1).
    pub(super) fn serve(&self, server: Server) -> Result<()> {
        // `poll_once` may have made the listener non-blocking; the listener
        // thread wants to sleep in `accept`.
        server.listener.set_nonblocking(false).map_err(|e| AtlasError::Platform(e.to_string()))?;
        if let Some(also) = &server.also {
            also.set_nonblocking(false).map_err(|e| AtlasError::Platform(e.to_string()))?;
        }
        let port = server.port();
        let server = std::sync::Arc::new(server);
        let sockets = if server.also.is_some() { 2 } else { 1 };
        for which in 0..sockets {
            let (s, me) = (server.clone(), self.clone());
            std::thread::Builder::new()
                .name("atlas-hub".into())
                .spawn(move || {
                    let Some(listener) = (if which == 0 { Some(&s.listener) } else { s.also.as_ref() }) else { return };
                    me.accept_on(listener, &s);
                })
                .map_err(|e| AtlasError::Platform(format!("couldn't start the hub's listener: {e}")))?;
        }
        // unheard-ok: a OnceLock already set keeps its first value, which is the one wanted
        let _ = self.server.set(server);
        self.port.store(port, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    /// One more socket for the same server: the configured port, freed
    /// after the hub had to fall back to another (`open_hub`), so the
    /// bookmarked address works again beside the one it fell back to.
    pub(super) fn serve_also(&self, listener: TcpListener) -> bool {
        let Some(server) = self.server.get().cloned() else { return false };
        if listener.set_nonblocking(false).is_err() {
            return false;
        }
        let me = self.clone();
        std::thread::Builder::new()
            .name("atlas-hub".into())
            .spawn(move || me.accept_on(&listener, &server))
            .is_ok()
    }

    pub(super) fn accept_on(&self, listener: &TcpListener, s: &std::sync::Arc<Server>) {
        use std::sync::atomic::Ordering;
        for conn in listener.incoming() {
            let Ok(stream) = conn else {
                // A failed accept (the client gave up first) is not a
                // reason to stop listening; a pause keeps a broken
                // socket from spinning a core.
                std::thread::sleep(std::time::Duration::from_millis(20));
                continue;
            };
            if self.open.load(Ordering::SeqCst) >= MAX_OPEN {
                drop(stream);
                continue;
            }
            let from = stream.peer_addr().map(|a| a.ip()).ok();
            if !by_address_take(&self.per_address, from) {
                drop(stream);
                continue;
            }
            self.open.fetch_add(1, Ordering::SeqCst);
            let (s, tx, still_open, per, late, snapshots) = (s.clone(), self.tx.clone(), self.open.clone(), self.per_address.clone(), self.late.clone(), self.snapshots.clone());
            let spawned = std::thread::Builder::new().name("atlas-hub-conn".into()).spawn(move || {
                s.serve_on_its_own(stream, &tx, &late, &snapshots);
                still_open.fetch_sub(1, Ordering::SeqCst);
                by_address_give_back(&per, from);
            });
            if spawned.is_err() {
                // The thread never started, so neither will its
                // decrement. (The stream went with the closure and
                // is closed.)
                self.open.fetch_sub(1, Ordering::SeqCst);
                by_address_give_back(&self.per_address, from);
            }
        }
    }

    pub(super) fn say(&self, line: String) {
        if let Ok(mut n) = self.news.lock().or_else(crate::crash::unpoison) {
            n.push(line);
        }
    }
}

impl Default for Retry {
    fn default() -> Self {
        Retry {
            first: std::time::Duration::from_millis(500),
            longest: std::time::Duration::from_secs(10),
            fall_back_after: std::time::Duration::from_secs(30),
            look_again_every: std::time::Duration::from_secs(30),
        }
    }
}

/// Open the hub, and keep trying when its port is taken (28 Sep 2026).
///
/// ## Why
///
/// The hub was bound once at start. If anything held the port at that
/// moment -- the Atlas an update was replacing, still on its way out;
/// another program; a browser's leftover -- the bind failed, a line went to
/// a console nobody sees, and the hub was gone for the whole session: the
/// tray's "Open the hub" and every bookmark answered nothing, with nothing
/// saying why.
///
/// ## What
///
/// A switched-off hub, a short token or a refused address are still errors
/// at once: waiting doesn't change them. A port that's taken is waited for
/// on a thread of its own, with a doubling wait (`Retry`). If it's still
/// taken after `fall_back_after`, the hub opens on one of the next
/// `FALLBACK_PORTS` ports (or any free one), says so (`take_news`), and
/// `on_open` is told the real port -- so the state file, `atlas hub`, the
/// tray and the phone's link use it. The token is the same, so the address
/// differs only in its port. The configured port is still tried every
/// `look_again_every`; when it frees, the hub answers there too, and the
/// bookmark works again.
pub fn open_hub(
    cfg: &ServerConfig,
    token: &str,
    id: &str,
    peers: Vec<crate::kin::Peer>,
    retry: Retry,
    on_open: Box<dyn Fn(u16) + Send>,
) -> Result<HubDoor> {
    let dress = {
        let (id, peers) = (id.to_string(), peers);
        move |s: Server| {
            let s = s.with_id(&id);
            if peers.is_empty() {
                s
            } else {
                s.with_peers(peers.clone())
            }
        }
    };
    let first = Server::bind(cfg, token);
    let why = match first {
        Ok(s) => {
            let (door, serving) = HubDoor::waiting();
            serving.serve(dress(s))?;
            on_open(door.port());
            return Ok(door);
        }
        // Only a port that couldn't be had is worth waiting for.
        Err(AtlasError::Platform(why)) => why,
        Err(other) => return Err(other),
    };
    let (door, serving) = HubDoor::waiting();
    serving.say(format!(
        "The hub's port ({}) is taken right now ({why}), so I'm trying again in the background -- \
         the hub will open as soon as it can.",
        cfg.port
    ));
    let (cfg, token) = (cfg.clone(), token.to_string());
    std::thread::Builder::new()
        .name("atlas-hub-bind".into())
        .spawn(move || keep_trying(cfg, token, retry, serving, dress, on_open))
        .map_err(|e| AtlasError::Platform(format!("couldn't start trying the hub's port again: {e}")))?;
    Ok(door)
}

pub(super) fn keep_trying(
    cfg: ServerConfig,
    token: String,
    retry: Retry,
    serving: Serving,
    dress: impl Fn(Server) -> Server,
    on_open: Box<dyn Fn(u16) + Send>,
) {
    use std::sync::atomic::Ordering;
    let started = std::time::Instant::now();
    let mut wait = retry.first;
    let is_shut = |s: &Serving| s.shut.load(Ordering::SeqCst);
    // The configured port, while it's worth waiting for.
    while started.elapsed() < retry.fall_back_after {
        crate::goodbye::nap(wait.as_millis() as u64);
        if is_shut(&serving) {
            return;
        }
        if let Ok(s) = Server::bind(&cfg, &token) {
            if serving.serve(dress(s)).is_ok() {
                serving.say(format!("The hub is open now, on its usual port ({}).", cfg.port));
                on_open(cfg.port);
            }
            return;
        }
        wait = (wait * 2).min(retry.longest);
    }
    // Another port, near it if possible.
    let near = (1..=FALLBACK_PORTS).filter_map(|i| cfg.port.checked_add(i));
    let mut opened = None;
    for port in near.chain(std::iter::once(0)) {
        if let Ok(s) = Server::bind_on(&cfg, &token, port) {
            let got = s.port();
            if serving.serve(dress(s)).is_ok() {
                opened = Some(got);
            }
            break;
        }
    }
    let Some(port) = opened else {
        serving.say("I couldn't open the hub on any port. Everything else still works; restarting Atlas tries again.".into());
        return;
    };
    serving.say(format!(
        "The hub's usual port ({}) is still taken by another program, so the hub is on port {port} for now. \
         Your bookmark won't reach it until that port is free -- the icon by the clock opens the right address.",
        cfg.port
    ));
    on_open(port);
    // And the usual port again, whenever it frees.
    let where_to = match bind_address(&cfg.reachable_from) {
        Ok(a) => a,
        Err(_) => return,
    };
    loop {
        crate::goodbye::nap(retry.look_again_every.as_millis() as u64);
        if is_shut(&serving) {
            return;
        }
        if let Ok(l) = TcpListener::bind((where_to, cfg.port)) {
            if serving.serve_also(l) {
                if !where_to.is_loopback() {
                    if let Ok(l) = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, cfg.port)) {
                        // unheard-ok: returns `bool`, not a Result
                        let _ = serving.serve_also(l);
                    }
                }
                serving.port.store(cfg.port, Ordering::SeqCst);
                serving.say(format!("The hub's usual port ({}) is free again, so your bookmark works again.", cfg.port));
                on_open(cfg.port);
            }
            return;
        }
    }
}

/// Count one more connection from `from`; false when it already has
/// `MAX_OPEN_PER_ADDRESS`. An address that can't be read is let through
/// (`MAX_OPEN` still bounds it).
pub(super) fn by_address_take(per: &PerAddress, from: Option<std::net::IpAddr>) -> bool {
    let Some(ip) = from else { return true };
    let mut m = held(per);
    let n = m.entry(ip).or_insert(0);
    if *n >= MAX_OPEN_PER_ADDRESS {
        return false;
    }
    *n += 1;
    true
}

pub(super) fn by_address_give_back(per: &PerAddress, from: Option<std::net::IpAddr>) {
    let Some(ip) = from else { return };
    let mut m = held(per);
    if let Some(n) = m.get_mut(&ip) {
        *n = n.saturating_sub(1);
        if *n == 0 {
            m.remove(&ip);
        }
    }
}

impl Server {
    /// Serve from threads of its own; the daemon collects what needs it
    /// through the returned door. See [`HubDoor`].
    pub fn threaded(self) -> Result<HubDoor> {
        let (door, serving) = HubDoor::waiting();
        serving.serve(self)?;
        Ok(door)
    }

    /// One connection, on its own thread: read and check it, hand an
    /// authenticated action to the daemon, wait for the answer, send it.
    pub(super) fn serve_on_its_own(&self, stream: TcpStream, to_daemon: &crate::doorbell::Sender<Waiting>, late: &LateTalk, snapshots: &ReadOnlySnapshots) {
        let Ok(Some(mut asked)) = self.read_asked(stream) else {
            return;
        };
        // Authentication and origin checks above still run for every request.
        if let Action::Pause(on) = &asked.action {
            if let Some(reply) = fast_pause(snapshots, *on) {
                self.answer(&mut asked, reply);
                return;
            }
        }
        if let Action::HubPost { path, fields } = &asked.action {
            if path == "/hub/outstanding" && fields.iter().any(|(name, value)| name == "what" && value == "drop") {
                if let Some((_, key)) = fields.iter().find(|(name, _)| name == "key") {
                    if let Some(reply) = fast_cancel(snapshots, key) { self.answer(&mut asked, reply); return; }
                }
            }
        }
        // Only explicitly published, query-free observational pages bypass
        // the loop; actions and GETs with side effects stay on its own path.
        if let Action::Hub(page) = &asked.action {
            if let Some(reply) = cached_readonly(snapshots, *page) {
                self.answer(&mut asked, reply);
                return;
            }
        }
        let what = match &asked.page {
            Some((m, p)) => format!("{m} {p}"),
            None => "peer".to_string(),
        };
        let (reply_tx, reply_rx) = std::sync::mpsc::sync_channel::<Reply>(1);
        let state = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(WAITING));
        let waiting = Waiting {
            action: asked.action.clone(),
            what,
            arrived: std::time::Instant::now(),
            reply: reply_tx,
            state: state.clone(),
            wait: self.answer_wait,
            late: late.clone(),
        };
        let reply = if to_daemon.send(waiting).is_err() {
            None
        } else {
            match reply_rx.recv_timeout(self.answer_wait) {
                Ok(r) => Some(r),
                Err(_) => {
                    use std::sync::atomic::Ordering;
                    // Given up on -- unless the daemon took it just now, in
                    // which case it is being answered and its answer is the
                    // one to send.
                    match state.compare_exchange(WAITING, ABANDONED, Ordering::SeqCst, Ordering::SeqCst) {
                        Ok(_) => {
                            keep_if_talk(&asked.action, late);
                            None
                        }
                        Err(_) => reply_rx.recv_timeout(ANSWER_WAIT).ok(),
                    }
                }
            }
        };
        let reply = reply.unwrap_or_else(|| Reply::busy(asked.page.as_ref().map(|(m, p)| Reply::wants_a_page(m, p)).unwrap_or(false)));
        self.answer(&mut asked, reply);
    }
}

impl Reply {
    /// Atlas didn't get to this request in time (it was busy for a long
    /// while, or it is stopping).
    pub(super) fn busy(page: bool) -> Reply {
        if page {
            Reply {
                status: 503,
                body: "<!doctype html><html lang=en><head><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'><meta http-equiv=refresh content=5>\
                       <title>Atlas</title><style>body{font:17px/1.5 system-ui,sans-serif;max-width:32em;margin:3em auto;padding:0 1em}</style></head>\
                       <body><h1>Atlas is busy for a moment</h1><p>It's in the middle of something and didn't get to this page. \
                       This page tries again by itself in a few seconds.</p><p><a href='/hub'>Back to Atlas</a></p></body></html>"
                    .into(),
                kind: Body::Html,
                ..Reply::default()
            }
        } else {
            Reply { status: 503, body: "{\"error\":\"busy\"}".into(), ..Reply::default() }
        }
    }
}

impl HubDoor {
    /// Publish only observational pages. Rendering never runs on a socket
    /// thread; the last completed state remains readable during slow work.
    pub(crate) fn publish_readonly(&self, pages: Vec<(crate::hub::Page, Reply)>, install_state: std::path::PathBuf, owner_state: std::path::PathBuf) {
        let Some(permissions) = permission_records(&install_state) else { self.invalidate_readonly(); return };
        if crate::profiles::active_dir(&install_state).unwrap_or_else(|| install_state.clone()) != owner_state {
            self.invalidate_readonly();
            return;
        }
        let mut cache = self.snapshots.lock().unwrap_or_else(|p| p.into_inner());
        *cache = Some(ReadOnlySnapshot { made: std::time::Instant::now(), install_state, owner_state, permissions, pages, pause: None, pause_requested: self.pause_requested.clone(), cancellations: Vec::new(), cancel_requested: self.cancel_requested.clone() });
    }

    pub(crate) fn publish_pause(&self, pause: std::sync::Arc<dyn Fn(bool) + Send + Sync>) {
        if let Some(snapshot) = self.snapshots.lock().unwrap_or_else(|p| p.into_inner()).as_mut() { snapshot.pause = Some(pause); }
    }

    pub(crate) fn take_pause_requested(&self) -> Option<bool> {
        match self.pause_requested.swap(0, std::sync::atomic::Ordering::SeqCst) { 1 => Some(false), 2 => Some(true), _ => None }
    }
    pub(crate) fn publish_cancellations(&self, controls: Vec<(FastCancel, std::sync::Arc<dyn Fn() + Send + Sync>)>) {
        if let Some(snapshot) = self.snapshots.lock().unwrap_or_else(|p| p.into_inner()).as_mut() { snapshot.cancellations = controls; }
    }
    pub(crate) fn take_cancel_requested(&self) -> Vec<FastCancel> {
        std::mem::take(&mut *self.cancel_requested.lock().unwrap_or_else(|p| p.into_inner()))
    }

    pub(crate) fn readonly_due(&self) -> bool {
        self.snapshots.lock().unwrap_or_else(|p| p.into_inner()).as_ref().is_none_or(|s| s.made.elapsed() >= std::time::Duration::from_secs(1))
    }

    pub(crate) fn invalidate_readonly(&self) {
        *self.snapshots.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }

    /// A door with nothing listening yet, and its serving end.
    pub(super) fn waiting() -> (HubDoor, Serving) {
        let (tx, asks) = crate::doorbell::channel::<Waiting>();
        let server = std::sync::Arc::new(std::sync::OnceLock::new());
        let port = std::sync::Arc::new(std::sync::atomic::AtomicU16::new(0));
        let news = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let shut = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let late: LateTalk = Default::default();
        let snapshots: ReadOnlySnapshots = Default::default();
        let serving = Serving {
            server: server.clone(),
            tx,
            open: Default::default(),
            per_address: Default::default(),
            port: port.clone(),
            news: news.clone(),
            shut: shut.clone(),
            late: late.clone(),
            snapshots: snapshots.clone(),
        };
        (HubDoor { server, asks, port, news, shut, late, snapshots, pause_requested: Default::default(), cancel_requested: Default::default() }, serving)
    }

    /// Talk page messages given up on since last asked, oldest first.
    pub fn take_late_talk(&self) -> Vec<(String, bool)> {
        self.late.lock().or_else(crate::crash::unpoison).map(|mut v| std::mem::take(&mut *v)).unwrap_or_default()
    }

    /// Is anything listening yet?
    pub fn is_open(&self) -> bool {
        self.server.get().is_some()
    }

    /// What there is to say about the door since last asked: its port
    /// taken, where it opened instead. For the log.
    pub fn take_news(&self) -> Vec<String> {
        self.news.lock().or_else(crate::crash::unpoison).map(|mut n| std::mem::take(&mut *n)).unwrap_or_default()
    }

    /// Answer every request waiting now, without waiting for more.
    pub fn answer_waiting(&self, handle: &mut dyn FnMut(Action) -> Reply) -> Vec<HubCost> {
        let mut done = Vec::new();
        while let Ok(w) = self.asks.try_recv() {
            done.extend(Self::answer_one(w, handle));
        }
        done
    }

    /// Wait up to `ms` for a request, then answer it and every other one
    /// waiting. Returns at once when one arrives, so a click is answered in
    /// the time it takes to build the page rather than after a nap.
    pub fn wait_and_answer(&self, ms: u64, handle: &mut dyn FnMut(Action) -> Reply) -> Vec<HubCost> {
        match self.asks.recv_timeout(std::time::Duration::from_millis(ms)) {
            Ok(w) => {
                let mut done: Vec<HubCost> = Self::answer_one(w, handle).into_iter().collect();
                done.extend(self.answer_waiting(handle));
                done
            }
            Err(_) => Vec::new(),
        }
    }

    /// Answer one request -- unless its connection has already been told
    /// Atlas was busy, in which case it is not run at all.
    pub(super) fn answer_one(w: Waiting, handle: &mut dyn FnMut(Action) -> Reply) -> Option<HubCost> {
        if !w.take() {
            return None;
        }
        let waited_ms = w.arrived.elapsed().as_millis() as u64;
        let started = std::time::Instant::now();
        let r = handle(w.action);
        let took_ms = started.elapsed().as_millis() as u64;
        // Taken, so the connection waits for this answer (`serve_on_its_own`).
        let _ = w.reply.send(r);
        Some(HubCost { what: w.what, waited_ms, took_ms })
    }

    pub fn guesses_worth_mentioning(&self) -> Option<String> {
        self.server.get().and_then(|s| s.guesses_worth_mentioning())
    }

    /// The port it answers on now (0 while it's still waiting for one).
    pub fn port(&self) -> u16 {
        self.port.load(std::sync::atomic::Ordering::SeqCst)
    }
}

// These small non-secret records govern handover/profile visibility. Compare
// their contents on each request, not timestamps which can miss rapid edits.
// Unreadable or oversized records disable the cache instead of guessing.
fn permission_records(root: &std::path::Path) -> Option<Vec<Option<Vec<u8>>>> {
    use std::io::Read;
    ["handover.json", "profiles.json"].into_iter().map(|name| {
        let mut file = match std::fs::File::open(root.join(name)) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Some(None),
            Err(_) => return None,
        };
        let mut bytes = Vec::new();
        Read::by_ref(&mut file).take(65_537).read_to_end(&mut bytes).ok()?;
        if bytes.len() > 65_536 { return None; }
        let record: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
        // Match Store's current envelope and its legacy plain-record reader.
        // Unsupported schemas disable snapshots without mutating state.
        let data = if let Some(schema) = record.get("schema") {
            if schema.as_u64() != Some(u64::from(crate::store::SCHEMA)) { return None; }
            record.get("data")?.clone()
        } else { record };
        if name == "handover.json" {
            let permission: crate::handover::Handover = serde_json::from_value(data).ok()?;
            if permission.stance.handed_over() { return None; }
        } else {
            let _: crate::profiles::Profiles = serde_json::from_value(data).ok()?;
        }
        Some(Some(bytes))
    }).collect()
}

fn cached_readonly(cache: &ReadOnlySnapshots, page: crate::hub::Page) -> Option<Reply> {
    if !matches!(page, crate::hub::Page::Now | crate::hub::Page::Outstanding) { return None; }
    let mut cache = cache.lock().unwrap_or_else(|p| p.into_inner());
    let snapshot = cache.as_ref()?;
    if permission_records(&snapshot.install_state).as_ref() != Some(&snapshot.permissions) {
        *cache = None;
        return None;
    }
    if crate::profiles::active_dir(&snapshot.install_state).unwrap_or_else(|| snapshot.install_state.clone()) != snapshot.owner_state {
        *cache = None;
        return None;
    }
    let mut reply = snapshot.pages.iter().find(|(p, _)| *p == page)?.1.clone();
    let age = snapshot.made.elapsed().as_secs();
    let note = if age > 2 {
        format!("Status has not refreshed for {age} seconds. Atlas may be busy or unresponsive; this view may have changed. {}", if snapshot.pause.is_some() { "Pause requests reach the active workers now; other buttons still wait for Atlas." } else { "Buttons still wait for Atlas to respond." })
    } else {
        if snapshot.pause.is_some() { "This is the latest completed status. Pause requests reach active workers; other buttons wait for Atlas to handle them.".to_string() }
        else { "This is the latest completed status. Buttons wait for Atlas to handle them.".to_string() }
    };
    reply.body = crate::hub::with_said(reply.body, Some(&note));
    Some(reply)
}

fn fast_pause(cache: &ReadOnlySnapshots, on: bool) -> Option<Reply> {
    let cache = cache.lock().unwrap_or_else(|p| p.into_inner());
    let snapshot = cache.as_ref()?;
    if permission_records(&snapshot.install_state).as_ref() != Some(&snapshot.permissions)
        || crate::profiles::active_dir(&snapshot.install_state).unwrap_or_else(|| snapshot.install_state.clone()) != snapshot.owner_state {
        return Some(Reply { status: 503, ..Reply::html("<p>The active owner changed. No pause or resume was applied to the old profile. Refresh after Atlas has switched profiles.</p>") });
    }
    let pause = snapshot.pause.clone()?;
    let pending = snapshot.pause_requested.clone();
    pending.store(if on { 2 } else { 1 }, std::sync::atomic::Ordering::SeqCst);
    pause(on);
    drop(cache);
    crate::doorbell::ring();
    let notice = if on { "Pause requested. Active workers will hold at their next safe point. Atlas will pause its microphone and update its status when the control loop handles this request." } else { "Resume requested. Active workers can continue; Atlas will update its microphone and status when the control loop handles this request." };
    let (title, next, button) = if on { ("Pause requested", "resume", "Resume") } else { ("Resume requested", "pause", "Pause") };
    Some(Reply { status: 202, ..Reply::html(format!("<!doctype html><html><head><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'><title>{title} — Atlas</title><style>body{{font:16px system-ui,sans-serif;max-width:32em;margin:3em auto;padding:0 1em}}button{{font:inherit;min-height:44px;padding:.5em 1em}}</style></head><body><h1>{title}</h1><p role=status>{notice}</p><form method=post action='/hub/pause'><input type=hidden name=what value={next}><button type=submit>{button}</button></form><p><a href='/hub/now'>Back to Now</a></p></body></html>")) })
}

fn fast_cancel(cache: &ReadOnlySnapshots, key: &str) -> Option<Reply> {
    let cache = cache.lock().unwrap_or_else(|p| p.into_inner());
    let snapshot = cache.as_ref()?;
    if permission_records(&snapshot.install_state).as_ref() != Some(&snapshot.permissions)
        || crate::profiles::active_dir(&snapshot.install_state).unwrap_or_else(|| snapshot.install_state.clone()) != snapshot.owner_state {
        return Some(Reply { status: 503, ..Reply::html("<p>The active owner changed. This old worker was not asked to stop. Refresh Outstanding after Atlas has switched profiles.</p>") });
    }
    let (request, stop) = snapshot.cancellations.iter().find(|(request, _)| request.key == key)?;
    let mut pending = snapshot.cancel_requested.lock().unwrap_or_else(|p| p.into_inner());
    if !pending.contains(request) {
        if pending.len() >= 32 { return Some(Reply { status: 503, ..Reply::html("<p>Too many control requests are waiting. This worker was not asked to stop; try again shortly.</p>") }); }
        pending.push(request.clone());
    }
    stop(); crate::doorbell::ring();
    Some(Reply { status: 202, ..Reply::html("<p role=status>Stop requested for that worker. Its result is not confirmed yet; Atlas will record its final outcome when the control loop responds.</p><p><a href='/hub/outstanding'>Back to Outstanding</a></p>") })
}

#[cfg(test)]
mod readonly_tests {
    use super::*;
    use std::io::{Read, Write};
    use std::sync::atomic::Ordering;
    const TOKEN: &str = "snapshot-test-token-is-long-enough";

    #[test]
    fn authenticated_pause_reaches_control_flags_without_daemon_and_owner_switch_blocks_it() {
        let cfg = ServerConfig { enabled: true, port: 0, ..Default::default() };
        let mut server = Server::bind(&cfg, TOKEN).unwrap();
        server.answer_wait = std::time::Duration::from_millis(80);
        let port = server.port(); let door = server.threaded().unwrap();
        let root = std::env::temp_dir().join(format!("atlas-fast-pause-{}-{port}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(&root);
        crate::handover::Handover::default().save(&store).unwrap();
        crate::profiles::Profiles::default().save(&root).unwrap();
        door.publish_readonly(vec![(crate::hub::Page::Now, Reply::html("<h1>Status</h1>"))], root.clone(), root.clone());
        let paused = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let control = crate::crew::Control::for_fixture();
        let flags = control.clone();
        let flag = paused.clone(); door.publish_pause(std::sync::Arc::new(move |on| { flag.store(on, Ordering::SeqCst); flags.set_paused(on); }));
        let post = |on: bool, authorized: bool| {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            stream.set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
            let body = if on { "what=pause" } else { "what=resume" };
            let auth = if authorized { format!("X-Atlas-Token: {TOKEN}\r\n") } else { String::new() };
            write!(stream, "POST /hub/pause HTTP/1.1\r\nHost: 127.0.0.1\r\n{auth}Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
            let mut reply = String::new(); stream.read_to_string(&mut reply).unwrap(); reply
        };
        assert!(post(true, false).starts_with("HTTP/1.1 401")); assert!(!paused.load(Ordering::SeqCst));
        let start = std::time::Instant::now(); let reply = post(true, true);
        assert!(reply.starts_with("HTTP/1.1 202"), "{reply}"); assert!(reply.contains("next safe point"));
        assert!(start.elapsed() < std::time::Duration::from_secs(1)); assert!(paused.load(Ordering::SeqCst)); assert_eq!(door.take_pause_requested(), Some(true));
        let (done, finished) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || { done.send(control.checkpoint()).unwrap(); });
        assert!(matches!(finished.recv_timeout(std::time::Duration::from_millis(40)), Err(std::sync::mpsc::RecvTimeoutError::Timeout)), "the worker passed its safe point while paused");
        assert!(post(false, true).starts_with("HTTP/1.1 202")); assert!(!paused.load(Ordering::SeqCst)); assert_eq!(door.take_pause_requested(), Some(false));
        assert!(!finished.recv_timeout(std::time::Duration::from_secs(1)).unwrap()); worker.join().unwrap();
        let cancel_control = crate::crew::Control::for_fixture();
        let exact = FastCancel { key: "t:7".into(), worker: Some(17), files: None };
        let flag = cancel_control.clone();
        door.publish_cancellations(vec![(exact.clone(), std::sync::Arc::new(move || flag.request_stop()))]);
        let cancel = |authorized: bool| {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            stream.set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
            let body = "what=drop&key=t%3A7";
            let auth = if authorized { format!("X-Atlas-Token: {TOKEN}\r\n") } else { String::new() };
            write!(stream, "POST /hub/outstanding HTTP/1.1\r\nHost: 127.0.0.1\r\n{auth}Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
            let mut reply = String::new(); stream.read_to_string(&mut reply).unwrap(); reply
        };
        assert!(cancel(false).starts_with("HTTP/1.1 401"));
        assert!(!cancel_control.checkpoint());
        assert!(door.take_cancel_requested().is_empty());
        assert!(cancel(true).starts_with("HTTP/1.1 202"));
        assert!(cancel_control.checkpoint(), "authorized cancellation must reach the owned worker before daemon dispatch");
        assert_eq!(door.take_cancel_requested(), vec![exact]);
        let mut profiles = crate::profiles::Profiles::default(); profiles.add("Owner", crate::profiles::Role::Owner).unwrap(); profiles.add("Guest", crate::profiles::Role::Guest).unwrap(); profiles.switch("guest", 1).unwrap(); profiles.save(&root).unwrap();
        let rejected_pause = post(true, true);
        assert!(rejected_pause.starts_with("HTTP/1.1 503"), "{rejected_pause}"); assert!(!paused.load(Ordering::SeqCst)); assert_eq!(door.take_pause_requested(), None);
        let rejected = cancel(true);
        assert!(rejected.starts_with("HTTP/1.1 503"), "{rejected}"); assert!(door.take_cancel_requested().is_empty());
        drop(door); let _ = std::fs::remove_dir_all(root);
    }

    fn request(port: u16, path: &str, authorized: bool, method: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.set_read_timeout(Some(std::time::Duration::from_secs(3))).unwrap();
        let auth = if authorized { format!("X-Atlas-Token: {TOKEN}\r\n") } else { String::new() };
        stream.write_all(format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{auth}Content-Length: 0\r\n\r\n").as_bytes()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }

    #[test]
    fn readonly_pages_answer_without_loop_but_auth_actions_queries_and_permissions_still_gate() {
        let cfg = ServerConfig { enabled: true, port: 0, ..Default::default() };
        let mut server = Server::bind(&cfg, TOKEN).unwrap();
        server.answer_wait = std::time::Duration::from_millis(80);
        let port = server.port();
        let door = server.threaded().unwrap();
        let root = std::env::temp_dir().join(format!("atlas-readonly-{}-{}", std::process::id(), port));
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(root.clone());
        crate::handover::Handover::default().save(&store).unwrap();
        crate::profiles::Profiles::default().save(&root).unwrap();
        door.publish_readonly(vec![(crate::hub::Page::Now, Reply::html("<html><body><h1>First status</h1></body></html>"))], root.clone(), root.clone());
        // No daemon answers requests during this entire fixture.
        let status = request(port, "/hub/now", true, "GET");
        assert!(status.starts_with("HTTP/1.1 200"), "{status}");
        assert!(status.contains("First status"));
        assert!(status.contains("latest completed status"));
        assert!(request(port, "/hub/now", false, "GET").starts_with("HTTP/1.1 401"));
        assert!(request(port, "/hub/now?said=test", true, "GET").starts_with("HTTP/1.1 503"));
        assert!(request(port, "/hub/messages", true, "GET").starts_with("HTTP/1.1 503"));
        assert!(!request(port, "/hub/outstanding", true, "POST").starts_with("HTTP/1.1 200"));
        // A changed permission record prevents serving the owner's old view.
        let mut handover = crate::handover::Handover::default();
        handover.hand_over("test guest", 1);
        handover.save(&store).unwrap();
        let changed = request(port, "/hub/now", true, "GET");
        assert!(changed.starts_with("HTTP/1.1 503"));
        assert!(!changed.contains("First status"));
        drop(door);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn snapshot_age_is_visible_and_publish_replaces_the_previous_status() {
        let (door, _) = HubDoor::waiting();
        let root = std::env::temp_dir().join(format!("atlas-snapshot-age-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(root.clone());
        crate::handover::Handover::default().save(&store).unwrap();
        let mut profiles = crate::profiles::Profiles::default();
        profiles.save(&root).unwrap();
        let page = crate::hub::Page::Now;
        door.publish_readonly(vec![(page, Reply::html("<html><body><h1>Old</h1></body></html>"))], root.clone(), root.clone());
        door.snapshots.lock().unwrap().as_mut().unwrap().made -= std::time::Duration::from_secs(10);
        let old = cached_readonly(&door.snapshots, page).unwrap();
        assert!(old.body.contains("Status has not refreshed for"));
        assert!(old.body.contains("busy or unresponsive"));
        door.publish_readonly(vec![(page, Reply::html("<html><body><h1>New</h1></body></html>"))], root.clone(), root.clone());
        let new = cached_readonly(&door.snapshots, page).unwrap();
        assert!(new.body.contains("New"));
        assert!(!new.body.contains("Old"));
        profiles.add("Owner", crate::profiles::Role::Owner).unwrap();
        profiles.add("Guest", crate::profiles::Role::Guest).unwrap();
        profiles.switch("guest", 1).unwrap();
        profiles.save(&root).unwrap();
        assert!(cached_readonly(&door.snapshots, page).is_none());
        // Old-owner status cannot be republished after a profile switch.
        door.publish_readonly(vec![(page, Reply::html("<h1>Old owner</h1>"))], root.clone(), root.clone());
        assert!(cached_readonly(&door.snapshots, page).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}

impl Drop for HubDoor {
    fn drop(&mut self) {
        self.shut.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Write where the hub is. Written whole to a new file and moved over the
/// old one, so a reader never sees half of it.
pub fn record_door(state_dir: &std::path::Path, door: &Door) -> std::io::Result<()> {
    let _state = crate::store::state_transaction(state_dir)?;
    std::fs::create_dir_all(state_dir)?;
    let text = serde_json::to_string(door).map_err(|e| std::io::Error::other(e.to_string()))?;
    let tmp = state_dir.join(format!("{DOOR_FILE}.new"));
    std::fs::write(&tmp, text)?;
    crate::store::rename_patiently(&tmp, &state_dir.join(DOOR_FILE))
}

/// What `record_door` last wrote, if anything.
pub(super) fn recorded_door(state_dir: &std::path::Path) -> Option<Door> {
    let text = std::fs::read_to_string(state_dir.join(DOOR_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Ask whatever answers on `port` on this machine which Atlas it is: its
/// id, or `None` when nothing answers or it isn't an Atlas hub.
pub fn ping(port: u16, wait: std::time::Duration) -> Option<String> {
    use std::io::Read;
    if port == 0 {
        return None;
    }
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut s = TcpStream::connect_timeout(&addr, wait).ok()?;
    let _ = s.set_read_timeout(Some(wait));
    let _ = s.set_write_timeout(Some(wait));
    s.write_all(format!("GET {PING_PATH} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").as_bytes()).ok()?;
    let mut got = Vec::new();
    crate::heard!(s.take(4096).read_to_end(&mut got));
    let text = String::from_utf8_lossy(&got);
    let (head, body) = text.split_once("\r\n\r\n")?;
    if !head.starts_with("HTTP/1.1 200") && !head.starts_with("HTTP/1.0 200") {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(body.trim()).ok()?;
    v.get("atlas").and_then(|a| a.as_str()).map(|a| a.to_string())
}

/// The port the running Atlas's hub really answers on: the one recorded in
/// `state_dir`, when the hub there says it is that Atlas. `None` when no
/// Atlas hub answers (it's stopped, switched off, or something else holds
/// the port).
pub fn atlas_hub_port(state_dir: &std::path::Path) -> Option<u16> {
    let door = recorded_door(state_dir)?;
    let said = ping(door.port, std::time::Duration::from_millis(400))?;
    (!door.id.is_empty() && said == door.id).then_some(door.port)
}

/// The port to give out for the hub: the running Atlas's real one when it
/// answers (`atlas_hub_port`), else the configured one.
pub fn hub_port(state_dir: &std::path::Path, configured: u16) -> u16 {
    atlas_hub_port(state_dir).unwrap_or(configured)
}
