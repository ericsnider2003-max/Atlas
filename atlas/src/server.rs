//! The local API.
//!
//! This is what makes "carry on from my phone" possible. Atlas listens on
//! loopback only; anything from another device reaches it through a VPN that
//! terminates on this machine, so the listener itself is never exposed.
//!
//! Three rules, none of them optional:
//!
//! 1. **Loopback only.** Binding to 0.0.0.0 would put a command endpoint for
//!    your workspace on whatever café network you're on.
//! 2. **Every request carries a token.** Generated on first run, stored with
//!    the rest of Atlas's state, compared in constant time.
//! 3. **Read and queue, never execute directly.** A phone can ask what's
//!    happening and add to the queue. It cannot make Atlas type into a window
//!    you can't see.

use crate::error::{AtlasError, Result};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub enabled: bool,
    pub port: u16,
    /// Body size ceiling. A phone sends sentences, not files.
    pub max_body: usize,
    /// The cap for `/hand/file` only.
    ///
    /// Separate from `max_body` on purpose. Every other endpoint takes a short
    /// JSON request, and raising the limit for all of them so a photo can get
    /// through would mean any request on the port could ask Atlas to hold
    /// twenty megabytes.
    #[serde(default = "default_max_upload")]
    pub max_upload: usize,
    /// One more address the hub may answer on, for reaching it from your
    /// phone.
    ///
    /// Empty, and empty means loopback only — which is what this listener has
    /// always done and what it should keep doing for anyone who does not
    /// deliberately change it.
    ///
    /// The case it exists for: the sync page has the buttons that fix sync,
    /// and the day sync is broken is a day you may be holding a phone rather
    /// than sitting at the machine. Loopback is unreachable from a phone even
    /// over a VPN, so "use a VPN" was never an answer on its own.
    ///
    /// What it will accept is narrow and checked: a private or overlay-network
    /// address — `10.`, `192.168.`, `172.16–31.`, Tailscale's `100.64–127.`,
    /// link-local, or a loopback address. Never `0.0.0.0`, never a public
    /// address. See [`bind_address`], which is where the rule lives and where
    /// it is tested.
    #[serde(default)]
    pub reachable_from: String,
}

/// Which address the hub may listen on.
///
/// The rule rather than the intention, so it can be tested. Anything outside
/// it is refused with a sentence naming what was wrong, because a listener
/// that silently falls back to loopback after you asked for something else is
/// a setting that does nothing — and a listener that silently does what you
/// asked when you asked for the whole internet is worse.
pub fn bind_address(reachable_from: &str) -> std::result::Result<std::net::IpAddr, String> {
    use std::net::IpAddr;
    let want = reachable_from.trim();
    if want.is_empty() {
        return Ok(IpAddr::from([127, 0, 0, 1]));
    }
    let addr: IpAddr = want
        .parse()
        .map_err(|_| format!("`{want}` isn't an address. Leave it empty for this machine only."))?;
    match addr {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            let private = o[0] == 10
                || (o[0] == 172 && (16..=31).contains(&o[1]))
                || (o[0] == 192 && o[1] == 168)
                // Tailscale and other CGNAT overlays.
                || (o[0] == 100 && (64..=127).contains(&o[1]))
                || (o[0] == 169 && o[1] == 254)
                || o[0] == 127;
            if v4.is_unspecified() {
                return Err(
                    "0.0.0.0 means every network this machine is on, including whatever wifi                      you are on next week. Give the one address you reach it by."
                        .into(),
                );
            }
            if !private {
                return Err(format!(
                    "`{want}` is a public address. This is a personal assistant with your                      notes in it; put it on your own network or a VPN and give me that                      address instead."
                ));
            }
            Ok(addr)
        }
        IpAddr::V6(v6) => {
            if v6.is_unspecified() {
                return Err("`::` means every network. Give the one address you reach it by.".into());
            }
            let s = v6.segments()[0];
            // Unique-local (fc00::/7), link-local (fe80::/10), or loopback.
            let private = (s & 0xfe00) == 0xfc00 || (s & 0xffc0) == 0xfe80 || v6.is_loopback();
            if !private {
                return Err(format!(
                    "`{want}` is a public address. Put it on your own network or a VPN and                      give me that address instead."
                ));
            }
            Ok(addr)
        }
    }
}

/// Room for a 20 MB file once base64 has made it a third larger, plus the
/// rest of the request.
fn default_max_upload() -> usize {
    28 * 1024 * 1024
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            enabled: false,
            port: 8787,
            max_body: 16 * 1024,
            max_upload: default_max_upload(),
            reachable_from: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub method: String,
    /// The path alone. The query is in [`Request::query`] — keeping them
    /// together is what made `hub::route` miss `/hub?t=…`.
    pub path: String,
    /// Everything after the `?`, undecoded. Read through
    /// [`query_field`].
    pub query: String,
    pub token: Option<String>,
    /// The token arrived in the URL and nowhere else.
    ///
    /// Read by `handle_conn`, which answers such a request with a redirect
    /// that sets a `SameSite=Strict` cookie and drops the token from the
    /// address. Two reasons, and the second is the one that matters:
    ///
    /// 1. A token in a URL goes into browser history, into the `Referer` of
    ///    anything the page links to, and into whatever the person pasted
    ///    the address into. One visit, and it is out of the address bar.
    /// 2. **`SameSite=Strict` is what keeps CSRF shut.** A cookie the
    ///    browser sends automatically would otherwise let any page on any
    ///    other origin submit a form to this one; `Strict` means the browser
    ///    does not send it on a cross-site request at all. The header-only
    ///    scheme had that property by accident (no browser sends a custom
    ///    header unasked) and lost it the moment the hub became reachable,
    ///    so it is now held on purpose.
    pub token_from_url: bool,
    pub body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Body {
    Json,
    Html,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub status: u16,
    pub body: String,
    pub kind: Body,
    /// A `Set-Cookie` value to send with this reply, if any.
    ///
    /// Only ever set by [`Reply::cookie_then`]. A field rather than something
    /// smuggled inside `body`, because `render` has to emit it as a header
    /// and a header is not a body.
    pub set_cookie: Option<String>,
    /// A file to save rather than a page to show: (filename, media type).
    /// The hub's "download your calendar" and "your contacts" buttons.
    pub download: Option<(String, &'static str)>,
    /// Bytes to play or show in place, not text: (media type, bytes). A
    /// voice's sample on the Sound page (`voicepick`).
    pub bytes: Option<(&'static str, Vec<u8>)>,
}

impl Default for Reply {
    fn default() -> Reply {
        Reply { status: 200, body: String::new(), kind: Body::Json, set_cookie: None, download: None, bytes: None }
    }
}

impl Reply {
    pub fn ok(body: impl Into<String>) -> Reply {
        Reply { body: body.into(), ..Reply::default() }
    }
    pub fn html(body: impl Into<String>) -> Reply {
        Reply { body: body.into(), kind: Body::Html, ..Reply::default() }
    }
    /// A page that sends you somewhere else — used after saving a setting, so
    /// a refresh doesn't re-submit the form.
    /// A file for the browser to save, named.
    pub fn file(name: &str, mime: &'static str, body: impl Into<String>) -> Reply {
        Reply { body: body.into(), download: Some((name.to_string(), mime)), ..Reply::default() }
    }
    pub fn redirect(to: &str) -> Reply {
        Reply { status: 303, body: to.to_string(), kind: Body::Html, ..Reply::default() }
    }
    /// Take the token out of the address and put it in a cookie.
    ///
    /// The first thing a browser gets when it opens a printed hub address.
    /// See [`Request::token_from_url`] for why, and for why `SameSite=Strict`
    /// is doing real work rather than being belt and braces.
    ///
    /// `Path=/` because the hub spans `/hub`, `/hub/settings` and the form
    /// endpoints under `/hub/…`; `HttpOnly` because no script on these pages
    /// needs to read it and a stored-XSS would otherwise hand it over. No
    /// `Secure`, deliberately: the listener is plain HTTP on loopback, and
    /// `Secure` would mean the cookie was never sent at all.
    ///
    /// A page that moves itself on, not a 303 (27 Sep 2026): a phone opening
    /// the address from its camera arrives cross-site, and browsers withhold
    /// a `Strict` cookie from a redirect that such a visit started -- so the
    /// clean `/hub` came back "denied" on Eric's iPhone. A hop the page makes
    /// itself starts on this site, and the cookie goes with it.
    fn cookie_then(to: &str, token: &str) -> Reply {
        let to = crate::hub::esc(to);
        Reply {
            status: 200,
            body: format!(
                "<!doctype html><html lang=en><head><meta charset=utf-8><meta name=referrer content=no-referrer>\
                 <meta http-equiv=refresh content='0;url={to}'><meta name=viewport content='width=device-width,initial-scale=1'>\
                 <title>Atlas</title></head><body><p><a href='{to}'>Open Atlas</a></p>\
                 <script>location.replace('{to}')</script></body></html>"
            ),
            kind: Body::Html,
            set_cookie: Some(format!(
                "{COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict"
            )),
            download: None,
            bytes: None,
        }
    }
    /// Bytes the browser plays or shows in place (a voice's sample).
    pub fn media(mime: &'static str, bytes: Vec<u8>) -> Reply {
        Reply { bytes: Some((mime, bytes)), ..Reply::default() }
    }
    pub fn denied() -> Reply {
        // Deliberately says nothing about why. An endpoint that distinguishes
        // "wrong token" from "no token" tells a prober it found something.
        Reply { status: 401, body: "{\"error\":\"denied\"}".into(), ..Reply::default() }
    }
    /// A browser asking for a page, rather than the app or a peer asking for data.
    fn wants_a_page(method: &str, path: &str) -> bool {
        method == "GET" && (path == "/" || path == "/hub" || path.starts_with("/hub/")) && !path.contains('.')
    }
    /// "denied", for a person: what to do, never a bare error with no way on
    /// (27 Sep 2026: a phone showed `{"error":"denied"}` and nothing else).
    /// It says no more than the JSON does about why.
    fn denied_page() -> Reply {
        Reply {
            status: 401,
            body: "<!doctype html><html lang=en><head><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'>\
                   <title>Atlas</title><style>body{font:17px/1.5 system-ui,sans-serif;max-width:32em;margin:3em auto;padding:0 1em}</style></head>\
                   <body><h1>This device isn't signed in to Atlas</h1><p>On the computer Atlas runs on, open Atlas, go to \
                   <b>Your phone</b>, and scan the code again with this device. On a phone, Tailscale has to be on and \
                   signed in with the same account.</p></body></html>"
                .into(),
            kind: Body::Html,
            ..Reply::default()
        }
    }
    /// A page that doesn't exist, for a person: where they are, and the way back.
    fn not_found_page(path: &str) -> Reply {
        Reply {
            status: 404,
            body: format!(
                "<!doctype html><html lang=en><head><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'>\
                 <title>Atlas</title><style>body{{font:17px/1.5 system-ui,sans-serif;max-width:32em;margin:3em auto;padding:0 1em}}</style></head>\
                 <body><h1>That isn't a page in Atlas</h1><p>Nothing answers at <code>{}</code>. It's a fault in Atlas, not \
                 something you did.</p><p><a href='/hub'>Back to Atlas</a></p></body></html>",
                crate::hub::esc(path)
            ),
            kind: Body::Html,
            ..Reply::default()
        }
    }
    /// Atlas failed partway through answering a page or a form.
    fn failed_page() -> Reply {
        Reply {
            status: 500,
            body: "<!doctype html><html lang=en><head><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'>\
                   <title>Atlas</title><style>body{font:17px/1.5 system-ui,sans-serif;max-width:32em;margin:3em auto;padding:0 1em}</style></head>\
                   <body><h1>Something in Atlas went wrong there</h1><p>It's a fault in Atlas, not something you did, and \
                   it's been written down. Try again, or go back.</p><p><a href='/hub'>Back to Atlas</a></p></body></html>"
                .into(),
            kind: Body::Html,
            ..Reply::default()
        }
    }
    fn not_found() -> Reply {
        Reply { status: 404, body: "{\"error\":\"no such endpoint\"}".into(), ..Reply::default() }
    }
    fn too_big() -> Reply {
        Reply { status: 413, body: "{\"error\":\"body too large\"}".into(), ..Reply::default() }
    }
    /// Too much sent from a hub form, for a person: what happened and the way
    /// back to the page it came from (27 Sep 2026: a pasted page on Give got
    /// `{"error":"body too large"}` and nothing else).
    fn too_big_page(path: &str) -> Reply {
        let back = crate::hub::route(path).unwrap_or(crate::hub::Page::Dashboard);
        Reply {
            status: 413,
            body: format!(
                "<!doctype html><html lang=en><head><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'>\
                 <title>Atlas</title><style>body{{font:17px/1.5 system-ui,sans-serif;max-width:32em;margin:3em auto;padding:0 1em}}</style></head>\
                 <body><h1>That was more than Atlas takes in one go</h1><p>Nothing was kept. Send less at once — \
                 for something long, save it as a file and give Atlas the file instead.</p>\
                 <p><a href='{}'>Back to {}</a></p></body></html>",
                back.href(),
                crate::hub::esc(back.label())
            ),
            kind: Body::Html,
            ..Reply::default()
        }
    }
}

/// A lock, even one a panic elsewhere left poisoned: the data behind these
/// (a door's peer list, the count of wrong tokens) is still whole, and
/// refusing every request after one panic turned a single fault into a dead
/// hub (27 Sep 2026; the same rule as `localclock`).
fn held<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The cookie the browser carries once the URL has handed its token over.
pub const COOKIE: &str = "atlas_hub";

/// Parse an HTTP/1.1 request.
///
/// ## The query string, and the defect that made every printed address dead
///
/// This used to read the token from `X-Atlas-Token` and
/// `Authorization: Bearer` **and nothing else**, and to keep the raw target
/// as the path. [`hub_url`] prints `http://127.0.0.1:PORT/hub?t=TOKEN`.
///
/// A browser asked for that address sends neither header, so the token was
/// `None`; and the path was `"/hub?t=…"`, which `hub::route` exact-matches
/// against `"/hub"` and does not find. So **every address Atlas printed
/// answered 401** — the daemon's startup line, `atlas hub`, and both lines
/// `atlas settings` prints, which is ATLAS.bat's menu item 3, the one
/// labelled "works even when Atlas won't". The break-glass recovery path was
/// the loudest thing broken.
///
/// It was not caught because `tests/the_hub_is_reachable.rs` split `?t=` out
/// of the URL *string* and handed the pieces to `token_matches` directly. It
/// never went through this function, so it asserted nothing about a request —
/// and its own comment says it exists "so a change to how the token is
/// carried fails here rather than silently printing something refused."
///
/// Now: the query is split off the path, `?t=` is a token source, and so is
/// the cookie. See [`Request::token_from_url`] for why the URL token is
/// immediately traded for a cookie rather than left in every link.
pub fn parse_request(head: &str, body: &str) -> Option<Request> {
    let mut lines = head.lines();
    let first = lines.next()?;
    let mut parts = first.split_whitespace();
    let method = parts.next()?.to_string();
    let target = parts.next()?.to_string();

    // The path and the query are different things, and conflating them is
    // half of the defect above. An anchor never reaches a server, but a
    // hand-typed address can carry one, so it is dropped here too.
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), q.split('#').next().unwrap_or(q).to_string()),
        None => (target.split('#').next().unwrap_or(&target).to_string(), String::new()),
    };

    // Collected separately rather than assigned over each other. The old
    // loop assigned on every matching header, so `X-Atlas-Token: <valid>`
    // followed by any `Authorization:` that is not `Bearer …` set the token
    // back to `None` — a valid request refused by header order.
    let mut header_token = None;
    let mut bearer = None;
    let mut cookie_token = None;
    for line in lines {
        let Some((k, v)) = line.split_once(':') else { continue };
        let k = k.trim();
        let v = v.trim();
        if k.eq_ignore_ascii_case("x-atlas-token") {
            header_token = header_token.or_else(|| Some(v.to_string()));
        } else if k.eq_ignore_ascii_case("authorization") {
            bearer = bearer.or_else(|| v.strip_prefix("Bearer ").map(str::to_string));
        } else if k.eq_ignore_ascii_case("cookie") {
            for pair in v.split(';') {
                if let Some(val) = pair.trim().strip_prefix(&format!("{COOKIE}=")) {
                    cookie_token = cookie_token.or_else(|| Some(val.trim().to_string()));
                }
            }
        }
    }
    let url_token = query_field(&query, "t");

    // A header beats a cookie beats the URL. The order is not arbitrary: a
    // header is what the phone client and the peer door send deliberately, a
    // cookie is what this server itself set, and the URL is the one a person
    // may have pasted into a chat window.
    let token_from_url = header_token.is_none() && bearer.is_none() && cookie_token.is_none();
    let token = header_token.or(bearer).or(cookie_token).or(url_token);

    Some(Request {
        method,
        path,
        query,
        token,
        token_from_url,
        body: body.to_string(),
    })
}

/// One `name=value` out of a query string, percent-decoded.
///
/// Its own decoder rather than `hub::urldecode`, because that one maps each
/// decoded byte to a `char` — fine for a search box rendered back as text,
/// wrong for a credential, where `%C3%A9` must come back as the two bytes it
/// was and not as two codepoints. A token that round-trips differently to the
/// one that was generated is a token that never matches.
pub fn query_field(query: &str, name: &str) -> Option<String> {
    for pair in query.split('&') {
        let Some((k, v)) = pair.split_once('=') else { continue };
        if k != name {
            continue;
        }
        let bytes = v.as_bytes();
        let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'%' if i + 2 < bytes.len() => {
                    let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
                    match u8::from_str_radix(hex, 16) {
                        Ok(b) => {
                            out.push(b);
                            i += 3;
                        }
                        Err(_) => {
                            out.push(b'%');
                            i += 1;
                        }
                    }
                }
                b'+' => {
                    out.push(b' ');
                    i += 1;
                }
                b => {
                    out.push(b);
                    i += 1;
                }
            }
        }
        return String::from_utf8(out).ok();
    }
    None
}

pub fn content_length(head: &str) -> usize {
    head.lines()
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse().ok())
        .unwrap_or(0)
}

/// Compare without leaking length or position through timing.
pub fn token_matches(expected: &str, given: Option<&str>) -> bool {
    let Some(given) = given else { return false };
    if expected.is_empty() {
        return false;
    }
    let a = expected.as_bytes();
    let b = given.as_bytes();
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        diff |= (x ^ y) as usize;
    }
    diff == 0
}

/// Something typed that must not be repeated: a passphrase, a household key.
///
/// `Action` derives `Debug`, and actions are logged, cloned and compared. A
/// passphrase in a plain `String` field would print itself into the first
/// `{:?}` anyone wrote -- so the words are kept in a type whose `Debug` says
/// only that there is a secret here (27 Sep 2026, when the hub began taking
/// passphrases).
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Secret(String);

impl Secret {
    pub fn new(s: impl Into<String>) -> Secret {
        Secret(s.into())
    }
    /// The words, for the one call that checks them.
    pub fn reveal(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(\u{2026})")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        // Overwritten before the allocation is given back, as the vault does
        // with its key. Best effort: copies made on the way here are not ours.
        let mut bytes = std::mem::take(&mut self.0).into_bytes();
        for b in bytes.iter_mut() {
            *b = 0;
        }
        std::hint::black_box(&bytes);
    }
}

/// Is this address on a line only you are on: this machine, or your own
/// Tailscale network?
///
/// The hub needs its token everywhere, but a token is not a reason to send a
/// passphrase across a network you do not control -- a café's wifi, a
/// forwarded port. Loopback, Tailscale's own range (100.64.0.0/10 and
/// fd7a:115c:a1e0::/48), and the same written as IPv4-in-IPv6. Deliberately
/// narrower than `onion::is_local_origin`, which also admits the home LAN.
pub fn private_line(ip: std::net::IpAddr) -> bool {
    use std::net::IpAddr;
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_loopback() || (o[0] == 100 && (o[1] & 0xC0) == 64)
        }
        IpAddr::V6(v6) => {
            if v6.is_loopback() {
                return true;
            }
            if let Some(v4) = v6.to_ipv4_mapped() {
                return private_line(IpAddr::V4(v4));
            }
            let seg = v6.segments();
            seg[0] == 0xfd7a && seg[1] == 0x115c && seg[2] == 0xa1e0
        }
    }
}

/// The page a passphrase form gets when it came from anywhere else.
fn secret_refused_page() -> Reply {
    Reply {
        status: 403,
        body: "<!doctype html><html lang=en><head><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'>\
               <title>Atlas</title><style>body{font:17px/1.5 system-ui,sans-serif;max-width:32em;margin:3em auto;padding:0 1em}</style></head>\
               <body><h1>Passphrases only over this machine or Tailscale.</h1><p>This came from a network I can't vouch for, so \
               I didn't read it and nothing changed. Open Atlas on this laptop, or on your phone with Tailscale switched on, \
               and try again there.</p><p><a href='/hub'>Back to Atlas</a></p></body></html>"
            .into(),
        kind: Body::Html,
        ..Reply::default()
    }
}

/// What the API can be asked to do.
///
/// Deliberately small. Everything here either reads state or adds to a queue
/// the daemon drains under its normal rules — the phone gets no shortcut past
/// the approval gate.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Status,
    Outstanding,
    Queued,
    Health,
    /// Add a command to the queue. Runs under the same policy as speech.
    Say(String),
    /// Approve something Atlas already asked about, by id.
    Approve(u64),
    Deny(u64),
    /// Show a hub page.
    Hub(crate::hub::Page),
    /// Change a setting from the hub.
    HubSet { key: String, value: String },
    /// Your calendar as an .ics file, and your client list as a .vcf — the
    /// hub's download buttons.
    ExportCalendar,
    ExportClients,
    /// A file brought in from the hub: an .ics goes into the calendar, a .vcf
    /// into the client list, anything else to the tray to be read.
    BringIn { name: String, base64: String },
    /// Implement a queued change, by its title — the hub's implement button.
    Implement(String),
    /// A button on the sync page: make a new household key, write the
    /// recovery card out again, or invite another device.
    SyncKey(String),
    /// The other half of inviting: the code typed on the device that is
    /// joining, and what to call it.
    SyncJoin { code: String, device: String },
    /// Take Atlas's access to one site away. Immediate, and it does not touch
    /// your password — Atlas simply stops having it.
    RevokeAccess(String),
    /// Take all of it away at once.
    RevokeAllAccess,
    /// A button on the Add-ons page: approve, revoke, off, on.
    AddOn { what: String, id: String, key: String, sha: String },
    /// "Back to the default" on the Your edits page.
    ForgetEdit { file: String, path: String },
    /// A button on the Groups page: new, add, remove, role, rename.
    GroupChange { what: String, group: String, who: String, role: String },
    /// A button on the Friends page.
    Friend { what: String, who: String, link: String },
    /// Rearrange the dashboard. Both the move buttons and a drop post this --
    /// one route, so the two ways of moving a card cannot disagree.
    DashMove(crate::dash::Move),
    /// Turn arranging on or off. Reading is the default, because a dashboard
    /// you rearrange by accident while reading is worse than one you cannot
    /// rearrange at all.
    DashArrange(bool),
    /// Pause Atlas (true) or have it carry on (false), from Now's button —
    /// the same as saying "pause" or "carry on".
    Pause(bool),
    /// A hub page that reads its address's query: which conversation, which
    /// business, which view — or, for Give, what a phone's share sheet sent.
    HubQ(crate::hub::Page, String),
    /// A hub form that arrived without what it needs (an empty site, a
    /// button with no card): back to its page, saying what was missing,
    /// rather than "that isn't a page in Atlas".
    HubBack(crate::hub::Page, String),
    /// What Atlas is doing and what's ready for you, as data: the phone app's
    /// live-activity card and notifications read it.
    LiveJson,
    /// What a phone widget shows (`glance`).
    GlanceJson,
    /// Has a live page (`now`, `talk`) changed since it was drawn? A few
    /// bytes a page polls instead of fetching itself whole every few seconds
    /// (28 Sep 2026).
    Changed(String),
    /// The phone's calendar, read on the phone and sent here to be merged;
    /// the answer carries Atlas's own events back (H7).
    PhoneCalendar(String),
    /// A voice's sample, to hear it before downloading it (`voicepick`).
    VoiceSample(String),
    /// The Accounts page's vault forms: set a first passphrase, change it,
    /// or make a recovery key (`what` = set | change | recovery). `nonce` is
    /// the form's one-time mark, so a refresh cannot send it twice.
    Vault { what: String, old: Secret, new: Secret, again: Secret, nonce: String },
    /// "Take it back" on the Accounts page, while handed over.
    TakeBack { phrase: Secret, nonce: String },
    /// "Use a key from another device" on the Sync page.
    SyncKeySet { phrase: Secret, replace: bool },
    /// "Start one here" on the Sync page: a household, what to call this
    /// device, and whether to make a household key with it.
    HouseholdInit { name: String, device: String, key: bool },
    /// A form posted to one of the hub pages the locked design added
    /// (messages, tasks, clients, sound, trusted, give, talk, help, a new
    /// project): the path, and its fields decoded.
    HubPost { path: String, fields: Vec<(String, String)> },
    /// A file handed over from another device: the bytes, base64-encoded.
    ///
    /// Base64 rather than a multipart upload because the thing sending this is
    /// usually a phone shortcut, and every shortcut tool on every platform can
    /// base64 a file and post JSON. Multipart would be smaller on the wire and
    /// harder to set up, and the setup is where this feature lives or dies.
    HandFile {
        name: String,
        base64: String,
        space: Option<String>,
        from: String,
        asked: Option<String>,
    },
    /// Something handed over from another device: a link, a path, or words.
    ///
    /// Deliberately on the hub's own door rather than `kin`'s. Your phone is
    /// you; another Atlas is not, and giving them the same entrance would mean
    /// one door to keep honest for two different kinds of trust.
    Hand {
        what: String,
        space: Option<String>,
        from: String,
        /// What you wanted done with it, if you said.
        asked: Option<String>,
    },
    /// Mark a handed-over thing finished with.
    TrayDone(u64),
    /// Search everything you can reach. `q` may be empty, which is the
    /// palette opened and not yet typed into.
    Find(String),
    /// One choice from the hub's appearance menu.
    Appearance { what: String, to: String },
    /// Change what Atlas knows about one of your accounts. Never a password —
    /// only what kind of protection the site has, which is what the audit
    /// reads and is not itself worth stealing.
    Account(crate::accounts::Change),
    /// A message from another, trusted Atlas. Reachable only through
    /// `route_signal`, never through `route` -- see the module doc on why
    /// that separation is structural rather than a convention.
    Signal(crate::kin::Incoming),
    /// A note another, trusted Atlas handed over. Reachable only through
    /// `route_handoff`, and its only destination is the waiting list a
    /// person then decides about -- never the tray, never an intent.
    Handed(crate::kin::Delivered),
    /// A chat message from another, trusted Atlas. Reachable only through
    /// `route_chat`, and its only destination is `chat::Chats::receive` in a
    /// room the receiver opens for the sender the token names -- never an
    /// intent, never the tray.
    Chatted(crate::kin::Chatted),
    /// A read receipt from another, trusted Atlas. Reachable only through
    /// `route_read`, and its only destination is `chat::Chats::mark_read` --
    /// never a room, a message, an intent, or the tray.
    ReadReceipt(crate::kin::ReadReceipt),
    /// A notice from another, trusted Atlas that they have left a group.
    /// Reachable only through `route_left`, and its only destination is
    /// dropping that peer from that group's membership.
    LeftGroup(crate::kin::LeftGroup),
    /// A paired Atlas introducing its key. Reachable only through
    /// `route_hello`; its only destination is pinning that key.
    PeerHello(crate::kin::Hello),
    /// A group's signed list. Reachable only through `route_group`; its only
    /// destination is `groups::Groups::take`.
    PeerGroup(crate::kin::GroupList),
    /// Someone used one of your friend links. Reachable only through
    /// `route_friend`; its only destination is recording a friend.
    Befriended(crate::kin::Befriended),
    /// Feedback from a paired Atlas. Reachable only through
    /// `route_feedback`; its only destination is filing it.
    PeerFeedback(crate::kin::FeedbackIn),
    /// An answer to feedback this Atlas sent. Reachable only through
    /// `route_feedback`; its only destination is `feedback::heard_answer`.
    PeerFeedbackAnswer(crate::kin::FeedbackIn),
}

/// The entire routing surface a peer credential can ever reach. Deliberately
/// does not call `route()` or share any code path with it -- a peer token
/// must be structurally incapable of producing `Action::Say`, `Approve`,
/// `Deny`, or `HubSet`, not merely prevented from it by which handler happens
/// to run first.
pub fn route_signal(
    r: &Request,
    door: &std::sync::Mutex<crate::kin::Door>,
    token: &str,
) -> Option<Action> {
    if r.method != "POST" || r.path != "/signal" {
        return None;
    }
    let what = field(&r.body, "what")?;
    let urgent = field(&r.body, "urgency").map(|u| u == "urgent").unwrap_or(false);
    let urgency = if urgent { crate::kin::Urgency::Urgent } else { crate::kin::Urgency::Info };
    let now = crate::store::now();
    let mut door = held(door);
    door.receive(token, &what, urgency, now).ok().map(Action::Signal)
}

/// The routing surface for handed-over content, and the whole of it.
///
/// A separate function from `route_signal` for the same structural reason
/// `route_signal` is separate from `route`: a peer credential must be
/// incapable of producing anything but the one Action its endpoint is for,
/// not merely prevented from it by which handler happens to run first.
pub fn route_handoff(
    r: &Request,
    door: &std::sync::Mutex<crate::kin::Door>,
    token: &str,
) -> Option<Action> {
    if r.method != "POST" || r.path != "/handoff" {
        return None;
    }
    let what = field(&r.body, "what").unwrap_or_default();
    let now = crate::store::now();
    // The `from` field in the body is deliberately not read. The token says
    // who this is; a name in a body is a claim anyone holding the token
    // could write, and recording it would put a sender-chosen string in
    // front of you as though your own pairing had vouched for it.
    //
    // `name` and `data` *are* read, because a file has to be called
    // something and has to come from somewhere. `name` is sanitised inside
    // `receive_handoff_file` rather than here — one door, one place that
    // turns a peer's string into something path-shaped.
    let file = match (field(&r.body, "name"), field(&r.body, "data")) {
        (Some(n), Some(d)) => {
            // A body that says it has a file and then does not decode is
            // refused outright rather than quietly delivered as a bare note.
            // Half a file looks exactly like a whole one on a list.
            Some((n, crate::tray::from_base64(&d).ok()?))
        }
        // Only one of the two is a malformed request, not a note.
        (Some(_), None) | (None, Some(_)) => return None,
        (None, None) => None,
    };
    let mut door = held(door);
    match file {
        None => door.receive_handoff(token, &what, now).ok().map(Action::Handed),
        Some((name, bytes)) => door
            .receive_handoff_file(token, &what, &name, bytes, now)
            .ok()
            .map(Action::Handed),
    }
}

/// The chat door. A separate function for the same structural reason
/// `route_signal` and `route_handoff` are separate from `route`: a peer token
/// must be *incapable* of producing any Action but `Chatted`, not merely
/// prevented from it by handler order. It checks its own method and path first
/// and builds exactly one variant.
///
/// The sender is taken from the token by the `Door`; a `from` in the body, if
/// one were ever put there, is never read. The `business` label is passed
/// through as the sender's claim — whether the sender may actually see that
/// business is decided later, by *this* machine's roster, when the message is
/// filed. The door only says the token is a peer it knows.
pub fn route_chat(
    r: &Request,
    door: &std::sync::Mutex<crate::kin::Door>,
    token: &str,
) -> Option<Action> {
    if r.method != "POST" || r.path != "/chat" {
        return None;
    }
    let body = field(&r.body, "body")?;
    let sent_at = field(&r.body, "sent_at")?.parse::<u64>().ok()?;
    let offset = field(&r.body, "offset")?.parse::<i16>().ok()?;
    let after = field(&r.body, "after")?.parse::<u64>().ok()?;
    let id = field(&r.body, "id")?;
    // `null` on the wire (a personal message) comes back from `field` as the
    // string "null"; a real business is any other value.
    let business = match field(&r.body, "business") {
        Some(b) if b != "null" => Some(b),
        _ => None,
    };
    // The group fields are absent for a one-to-one. Present together for a
    // group: the shared id, the name, and the sender's member list.
    let group_id = field(&r.body, "group_id").filter(|s| s != "null");
    let group_name = field(&r.body, "group_name").filter(|s| s != "null");
    let members: Vec<String> = serde_json::from_str::<serde_json::Value>(&r.body)
        .ok()
        .and_then(|v| v.get("members").cloned())
        .and_then(|m| serde_json::from_value(m).ok())
        .unwrap_or_default();
    // Who a relayed group message is really from, as a key. Only a group's
    // owner is believed when it says this -- decided where it's filed.
    let on_behalf_of = field(&r.body, "on_behalf_of").filter(|s| s != "null" && crate::peerkey::is_public_key(s));
    let now = crate::store::now();
    let mut door = held(door);
    door.receive_chat(
        token, business, &body, sent_at, offset, after, &id, group_id, group_name, members, now,
    )
    .ok()
    .map(|mut c| {
        c.on_behalf_of = on_behalf_of;
        Action::Chatted(c)
    })
}

/// The read-receipt door. A separate function for the same structural reason
/// `route_chat` is separate from `route`: a peer token must be incapable of
/// producing any Action but `ReadReceipt`, not merely prevented from it by
/// handler order. It checks its own method and path first and builds exactly
/// one variant.
///
/// The reader is taken from the token by the `Door`; the body carries only the
/// ids of the messages that were read. A body that is not a JSON object with a
/// string array at `ids` is not a receipt and is refused rather than guessed
/// at.
pub fn route_read(
    r: &Request,
    door: &std::sync::Mutex<crate::kin::Door>,
    token: &str,
) -> Option<Action> {
    if r.method != "POST" || r.path != "/read" {
        return None;
    }
    let ids: Vec<String> = serde_json::from_str::<serde_json::Value>(&r.body)
        .ok()
        .and_then(|v| v.get("ids").cloned())
        .and_then(|m| serde_json::from_value(m).ok())
        .unwrap_or_default();
    let now = crate::store::now();
    let mut door = held(door);
    door.receive_read(token, ids, now).ok().map(Action::ReadReceipt)
}

/// The introduction door: a paired Atlas's public key, and nothing else. A
/// separate function for the same structural reason as every door here.
pub fn route_hello(r: &Request, door: &std::sync::Mutex<crate::kin::Door>, token: &str) -> Option<Action> {
    if r.method != "POST" || r.path != "/hello" {
        return None;
    }
    let key = field(&r.body, "key")?;
    let now = crate::store::now();
    let mut door = held(door);
    door.receive_hello(token, &key, now).ok().map(Action::PeerHello)
}

/// The group-list door: a signed list and its signature, nothing else. Who
/// carried it is the token's; whether it is true is the owner's signature's.
pub fn route_group(r: &Request, door: &std::sync::Mutex<crate::kin::Door>, token: &str) -> Option<Action> {
    if r.method != "POST" || r.path != "/group" {
        return None;
    }
    let state = field(&r.body, "state")?;
    let signature = field(&r.body, "signature")?;
    let signer = field(&r.body, "signer").unwrap_or_default();
    let now = crate::store::now();
    let mut door = held(door);
    door.receive_group(token, &state, &signature, &signer, now).ok().map(Action::PeerGroup)
}

/// The feedback doors: a friend's feedback (`/feedback`), or the answer to
/// feedback you sent (`/feedback-answer`). Either way, from the peer the token
/// names, size-capped, and filed -- nothing else.
pub fn route_feedback(r: &Request, door: &std::sync::Mutex<crate::kin::Door>, token: &str) -> Option<Action> {
    if r.method != "POST" || (r.path != "/feedback" && r.path != "/feedback-answer") {
        return None;
    }
    let body = field(&r.body, "body")?;
    let now = crate::store::now();
    let got = held(door).receive_feedback(token, &body, now).ok()?;
    Some(if r.path == "/feedback" { Action::PeerFeedback(got) } else { Action::PeerFeedbackAnswer(got) })
}

/// The friend door: the one door with no token, because whoever knocks
/// isn't a friend yet. What lets them in is the one-time secret from a link
/// you made, spent by `kin::Door::receive_friend` before anything happens.
/// It builds exactly one variant, like every door here.
fn route_friend(r: &Request, door: &std::sync::Mutex<crate::kin::Door>, sealed_by: Option<&str>) -> Option<Action> {
    if r.method != "POST" || r.path != "/friend" {
        return None;
    }
    let hello: crate::friends::Hello = serde_json::from_str(&r.body).ok()?;
    // Sealed, the knock proves which key made it: it must be the key it
    // introduces, or someone is introducing a key they don't hold.
    if sealed_by.is_some_and(|k| k != hello.key) {
        return None;
    }
    let now = crate::store::now();
    let mut door = held(door);
    door.receive_friend(hello, now).ok().map(Action::Befriended)
}

/// The leave-group door. A separate function for the same structural reason
/// the others are: a peer token must be incapable of producing any Action but
/// `LeftGroup`, and its only destination is dropping that peer from the group.
/// It checks its own method and path first and builds exactly one variant. The
/// peer who left is taken from the token; the body carries only the group id.
pub fn route_left(
    r: &Request,
    door: &std::sync::Mutex<crate::kin::Door>,
    token: &str,
) -> Option<Action> {
    if r.method != "POST" || r.path != "/left" {
        return None;
    }
    let group_id = field(&r.body, "group_id")?;
    let now = crate::store::now();
    let mut door = held(door);
    door.receive_left(token, &group_id, now).ok().map(Action::LeftGroup)
}

impl Action {
    /// Does this carry something typed that must only travel on a private
    /// line (`private_line`)? The one list the connection handler asks.
    pub fn carries_a_secret(&self) -> bool {
        match self {
            Action::Vault { .. } | Action::TakeBack { .. } | Action::SyncKeySet { .. } => true,
            // The Updates page's release-key forms take the vault passphrase
            // as an ordinary field.
            // The Social page's key and sign-in forms carry a `secret`.
            Action::HubPost { fields, .. } => fields.iter().any(|(k, _)| k == "passphrase" || k == "again" || k == "secret"),
            _ => false,
        }
    }
}

pub fn route(r: &Request) -> Option<Action> {
    use crate::hub::Page::*;
    let back = |page: crate::hub::Page, said: &str| Action::HubBack(page, said.to_string());
    match (r.method.as_str(), r.path.as_str()) {
        ("GET", "/status") => Some(Action::Status),
        ("GET", "/outstanding") => Some(Action::Outstanding),
        ("GET", "/queued") => Some(Action::Queued),
        ("GET", "/health") => Some(Action::Health),
        ("POST", "/say") => Some(Action::Say(field(&r.body, "text")?)),
        ("POST", "/approve") => field(&r.body, "id")?.parse().ok().map(Action::Approve),
        ("POST", "/deny") => field(&r.body, "id")?.parse().ok().map(Action::Deny),
        ("POST", "/hub/set") => Some(match crate::hub::form_field(&r.body, "key") {
            Some(key) => Action::HubSet { key, value: crate::hub::form_field(&r.body, "value").unwrap_or_default() },
            None => back(Settings, "That didn't say which setting, so nothing changed."),
        }),
        ("GET", "/hub/calendar.ics") => Some(Action::ExportCalendar),
        ("GET", "/hub/clients.vcf") => Some(Action::ExportClients),
        ("POST", "/hub/bring-in") => Some(Action::BringIn {
            name: field(&r.body, "name").unwrap_or_else(|| "file".into()),
            base64: field(&r.body, "data")?,
        }),
        ("POST", "/hand/file") => Some(Action::HandFile {
            name: field(&r.body, "name").unwrap_or_else(|| "file".into()),
            base64: field(&r.body, "data")?,
            space: field(&r.body, "space"),
            from: field(&r.body, "from").unwrap_or_else(|| "somewhere".into()),
            asked: field(&r.body, "asked"),
        }),
        ("POST", "/hand") => Some(Action::Hand {
            what: field(&r.body, "what")?,
            space: field(&r.body, "space"),
            from: field(&r.body, "from").unwrap_or_else(|| "somewhere".into()),
            asked: field(&r.body, "asked"),
        }),
        ("POST", "/hub/sync") => {
            let what = crate::hub::form_field(&r.body, "what").unwrap_or_default();
            match what.as_str() {
                "join" => Some(Action::SyncJoin {
                    code: crate::hub::form_field(&r.body, "code").unwrap_or_default(),
                    device: crate::hub::form_field(&r.body, "device").unwrap_or_default(),
                }),
                "new" | "card" | "pair" => Some(Action::SyncKey(what)),
                "set-key" => Some(Action::SyncKeySet {
                    phrase: Secret::new(crate::hub::form_field(&r.body, "phrase").unwrap_or_default()),
                    replace: crate::hub::form_field(&r.body, "replace").is_some(),
                }),
                "init" => Some(Action::HouseholdInit {
                    name: crate::hub::form_field(&r.body, "name").unwrap_or_default(),
                    device: crate::hub::form_field(&r.body, "device").unwrap_or_default(),
                    key: crate::hub::form_field(&r.body, "key").is_some(),
                }),
                _ => Some(back(Sync, "That button isn't wired to anything, so nothing changed.")),
            }
        }
        ("POST", "/hub/vault") => {
            let get = |k: &str| crate::hub::form_field(&r.body, k).unwrap_or_default();
            let what = get("what");
            match what.as_str() {
                "back" => Some(Action::TakeBack { phrase: Secret::new(get("phrase")), nonce: get("nonce") }),
                "set" | "change" | "recovery" => Some(Action::Vault {
                    old: Secret::new(get("old")),
                    new: Secret::new(get("new")),
                    again: Secret::new(get("again")),
                    nonce: get("nonce"),
                    what,
                }),
                _ => None,
            }
        }
        ("POST", "/hub/tray") => Some(match crate::hub::form_field(&r.body, "id").and_then(|i| i.parse().ok()) {
            Some(id) => Action::TrayDone(id),
            None => back(Dashboard, "That didn't say which thing, so nothing was marked done."),
        }),
        ("POST", "/hub/implement") => {
            let title = crate::hub::form_field(&r.body, "title").unwrap_or_default();
            Some(Action::Implement(title))
        }
        (
            "POST",
            "/hub/messages" | "/hub/tasks" | "/hub/clients" | "/hub/sound" | "/hub/trusted" | "/hub/give"
            | "/hub/talk" | "/hub/help" | "/hub/workshop" | "/hub/updates" | "/hub/feedback" | "/hub/phonemodel" | "/hub/documents" | "/hub/phone" | "/hub/mcp" | "/hub/draftmodel"
            | "/hub/recommendations/go" | "/hub/reclaim" | "/hub/sync-setup" | "/hub/social" | "/hub/opportunities",
        ) => Some(Action::HubPost { path: r.path.clone(), fields: crate::hub::form_fields(&r.body) }),
        ("POST", "/hub/pause") => match crate::hub::form_field(&r.body, "what").as_deref() {
            Some("pause") => Some(Action::Pause(true)),
            Some("resume") => Some(Action::Pause(false)),
            _ => Some(back(Now, "That button isn't wired to anything, so nothing changed.")),
        },
        ("POST", "/hub/dash") => {
            let what = crate::hub::form_field(&r.body, "what").unwrap_or_default();
            match what.as_str() {
                "arrange" => Some(Action::DashArrange(true)),
                "done" => Some(Action::DashArrange(false)),
                _ => crate::dash::Move::parse(
                    &what,
                    crate::hub::form_field(&r.body, "card").as_deref(),
                    crate::hub::form_field(&r.body, "to").as_deref(),
                )
                .map(Action::DashMove)
                .or_else(|| Some(back(Dashboard, "That didn't say which card or where to, so nothing moved."))),
            }
        }
        ("POST", "/hub/accounts") => {
            let site = crate::hub::form_field(&r.body, "site").unwrap_or_default();
            Some(
                crate::accounts::Change::parse(
                    crate::hub::form_field(&r.body, "what").as_deref(),
                    Some(&site),
                    crate::hub::form_field(&r.body, "to").as_deref(),
                )
                .map(Action::Account)
                .unwrap_or_else(|| {
                    if site.trim().is_empty() {
                        back(Accounts, "Type the site's name first, then press the button.")
                    } else {
                        back(Accounts, "That isn't something the Accounts page does, so nothing changed.")
                    }
                }),
            )
        }
        // The search is in the query, not the path. This read it from the
        // path, which `parse_request` had already stripped of its query — so
        // every search that reached the server (a form submitted before the
        // palette's script filtered it, or the command deck's box) arrived
        // empty and answered with nothing. Found 23 Sep 2026 wiring the deck.
        ("GET", "/hub/find") => Some(Action::Find(
            r.query
                .split('&')
                .find_map(|pair| pair.strip_prefix("q="))
                .map(crate::hub::urldecode)
                .unwrap_or_default(),
        )),
        // The "Aa" menu: theme, text size, contrast, motion. Links, not a
        // script — the hub's appearance needs none (`the_hub_works_offline`).
        ("GET", "/hub/appearance") => {
            let get = |k: &str| {
                r.query.split('&').find_map(|p| p.strip_prefix(&format!("{k}="))).map(crate::hub::urldecode)
            };
            Some(Action::Appearance { what: get("set")?, to: get("to")? })
        }
        // Taking access away. The buttons for these have been rendered on
        // the access page since it was written, posting to routes that did
        // not exist -- a revoke button that does nothing is worse than no
        // button, because you press it and believe it worked.
        ("POST", "/hub/access/revoke") => {
            // `form_field`, not `field`: this comes from an HTML form on the
            // access page, not from the JSON API. `field` parses JSON and
            // would have returned `None` for every real press of the button.
            Some(
                crate::hub::form_field(&r.body, "domain")
                    .filter(|d| !d.trim().is_empty())
                    .map(Action::RevokeAccess)
                    .unwrap_or_else(|| back(Access, "That didn't say which site, so nothing was taken away.")),
            )
        }
        ("POST", "/hub/access/revoke-all") => Some(Action::RevokeAllAccess),
        ("POST", "/hub/addons") => Some(
            match (crate::hub::form_field(&r.body, "what"), crate::hub::form_field(&r.body, "id")) {
                (Some(what), Some(id)) => Action::AddOn {
                    what,
                    id,
                    key: crate::hub::form_field(&r.body, "key").unwrap_or_default(),
                    sha: crate::hub::form_field(&r.body, "sha").unwrap_or_default(),
                },
                _ => back(AddOns, "That button didn't say which add-on, so nothing changed."),
            },
        ),
        ("POST", "/hub/friends") => Some(match crate::hub::form_field(&r.body, "what") {
            Some(what) => Action::Friend {
                what,
                who: crate::hub::form_field(&r.body, "who").unwrap_or_default(),
                link: crate::hub::form_field(&r.body, "link").unwrap_or_default(),
            },
            None => back(Friends, "That button isn't wired to anything, so nothing changed."),
        }),
        ("POST", "/hub/groups") => Some(match crate::hub::form_field(&r.body, "what") {
            Some(what) => Action::GroupChange {
                what,
                group: crate::hub::form_field(&r.body, "group").unwrap_or_default(),
                who: crate::hub::form_field(&r.body, "who").unwrap_or_default(),
                role: crate::hub::form_field(&r.body, "role").unwrap_or_default(),
            },
            None => back(Groups, "That button isn't wired to anything, so nothing changed."),
        }),
        ("POST", "/hub/edits") => Some(
            match (crate::hub::form_field(&r.body, "file"), crate::hub::form_field(&r.body, "path")) {
                (Some(file), Some(path)) => Action::ForgetEdit { file, path },
                _ => back(Edits, "That didn't say which edit, so nothing was put back."),
            },
        ),
        ("GET", "/hub/live.json") => Some(Action::LiveJson),
        ("GET", "/hub/glance.json") => Some(Action::GlanceJson),
        ("GET", "/hub/changed.json") => Some(Action::Changed(query_field(&r.query, "p").unwrap_or_default())),
        ("POST", "/hub/calendar/phone") => Some(Action::PhoneCalendar(r.body.clone())),
        ("GET", "/hub/voice-sample") => Some(Action::VoiceSample(query_field(&r.query, "id")?)),
        ("GET", path) => crate::hub::route(path).map(|p| {
            // Only the pages that read their query get it; everything else
            // stays exactly the address it was, token and all.
            //
            // And every page gets a query saying what a button just did
            // (`said=`) or which job it is waiting on (`job=`): until 27 Sep
            // 2026 those reached only the pages above, so a button on any
            // other came back to a page that looked the same either way.
            let q = r.query.split('&').filter(|kv| !kv.starts_with("t=") && !kv.is_empty()).collect::<Vec<_>>().join("&");
            let tells = q.split('&').any(|kv| kv.starts_with("said=") || kv.starts_with("job="));
            if (p.reads_query() || tells) && !q.is_empty() {
                Action::HubQ(p, q)
            } else {
                Action::Hub(p)
            }
        }),
        _ => None,
    }
}

/// The most a request's body may be, by what it is for. Uploads take files;
/// a phone's calendar sync sends six weeks of events, which a 16 KB form
/// limit refused from about sixty events on (27 Sep 2026); everything else
/// is a form or a small JSON ask.
pub fn body_cap(req: &Request, max_body: usize, max_upload: usize) -> usize {
    match (req.method.as_str(), req.path.as_str()) {
        ("POST", "/hand/file" | "/hub/bring-in") => max_upload,
        ("POST", "/hub/calendar/phone") => max_body.max(CALENDAR_BODY),
        _ => max_body,
    }
}

/// Room for a few thousand calendar events: far more than six weeks of
/// anyone's diary, and still small.
pub const CALENDAR_BODY: usize = 2 * 1024 * 1024;

fn field(body: &str, key: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    match v.get(key)? {
        serde_json::Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

pub fn render(reply: &Reply) -> String {
    // One place that turns `set_cookie` into a header, so no reply can carry
    // a cookie that never reaches the browser.
    let cookie = match &reply.set_cookie {
        Some(c) => format!("Set-Cookie: {c}\r\n"),
        None => String::new(),
    };
    if reply.status == 303 {
        // After saving, send the browser back with a GET so a refresh doesn't
        // apply the change twice.
        return format!(
            "HTTP/1.1 303 See Other\r\nLocation: {}\r\n{cookie}Content-Length: 0\r\n\
             Cache-Control: no-store\r\nConnection: close\r\n\r\n",
            reply.body
        );
    }
    let (content_type, disposition) = match (&reply.download, reply.kind) {
        (Some((name, mime)), _) => (
            *mime,
            // Only the characters a filename needs; nothing that could end the header.
            format!(
                "Content-Disposition: attachment; filename=\"{}\"\r\n",
                name.chars().filter(|c| c.is_ascii_alphanumeric() || "._-".contains(*c)).collect::<String>()
            ),
        ),
        (None, Body::Json) => ("application/json", String::new()),
        (None, Body::Html) => ("text/html; charset=utf-8", String::new()),
    };
    format!(
        "HTTP/1.1 {} {}\r\n\
         Content-Type: {content_type}\r\n\
         {disposition}Content-Length: {}\r\n\
         {cookie}Cache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\n\
         Connection: close\r\n\r\n{}",
        reply.status,
        match reply.status {
            200 => "OK",
            401 => "Unauthorized",
            404 => "Not Found",
            413 => "Payload Too Large",
            503 => "Service Unavailable",
            _ => "Error",
        },
        reply.body.len(),
        reply.body
    )
}

/// A 200 carrying bytes rather than a page: the phone app's manifest, service
/// worker and icons.
///
/// Separate from [`render`] because `Reply` holds a `String` and an icon is
/// not one. `headers` is extra response headers, each ending `\r\n`.
fn render_file(content_type: &str, headers: &str, bytes: &[u8]) -> Vec<u8> {
    let mut out = format!(
        "HTTP/1.1 200 OK\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         {headers}\
         X-Content-Type-Options: nosniff\r\n\
         Connection: close\r\n\r\n",
        bytes.len()
    )
    .into_bytes();
    out.extend_from_slice(bytes);
    out
}

/// A token you can read out loud once and never again.
/// Random bytes from the operating system.
///
/// Not from the clock. An outside review found this and was right: the token
/// used to come from an xorshift sequence seeded with a nanosecond timestamp,
/// which looks like 118 bits of entropy and isn't — the whole sequence is
/// determined by the seed, and process start time is knowable to within a
/// narrow window.
///
/// Every other control in the network path rests on this one value being
/// unguessable, so it has to come from somewhere that actually is.
fn os_random(n: usize) -> Result<Vec<u8>> {
    #[cfg(windows)]
    {
        // BCryptGenRandom with the system-preferred RNG. The platform layer
        // already carries Win32 bindings.
        crate::platform::win::random_bytes(n)
    }
    #[cfg(not(windows))]
    {
        use std::io::Read;
        let mut f = std::fs::File::open("/dev/urandom")
            .map_err(|e| AtlasError::Config(format!("no system randomness: {e}")))?;
        let mut buf = vec![0u8; n];
        f.read_exact(&mut buf)
            .map_err(|e| AtlasError::Config(format!("couldn't read randomness: {e}")))?;
        Ok(buf)
    }
}

/// A token, from real randomness.
///
/// Fails rather than falling back. A token quietly generated from the clock
/// because the entropy source was unavailable is worse than not starting —
/// you'd never know, and everything downstream assumes it's strong.
pub fn new_token() -> Result<String> {
    const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    let bytes = os_random(24)?;
    let mut out = String::with_capacity(28);
    for (i, b) in bytes.iter().enumerate() {
        // Rejection isn't needed at this size: 30 into 256 leaves a bias of
        // about 2%, which costs a fraction of a bit against 24 characters of
        // a 30-character alphabet — roughly 117 bits either way.
        out.push(ALPHABET[*b as usize % ALPHABET.len()] as char);
        if i % 6 == 5 && i != 23 {
            out.push('-');
        }
    }
    Ok(out)
}

/// What the token is stored in, between runs.
#[derive(Debug, Default, Serialize, Deserialize)]
struct HeldToken {
    token: String,
}

/// The hub token for this install: generated once, then reused.
///
/// **Rule 2 at the top of this file says "generated on first run, stored with
/// the rest of Atlas's state". Nothing stored it.** Both callers built a
/// fresh one with `new_token()` on every start, which has three consequences
/// and they are the reason the hub is hard to get to:
///
/// 1. **The dashboard URL changed every launch**, so it could not be
///    bookmarked, pinned as a tab, or saved on a phone. The only way to
///    reach the hub was to find the console window Atlas was printing into
///    and copy the address out of it — which means the way in to the web
///    interface was the terminal, defeating the point of having one.
/// 2. **`carry on from my phone` broke on every restart.** That is what this
///    whole module exists for: the phone holds a URL, and the URL stopped
///    working whenever the desktop rebooted.
/// 3. A second Atlas process — `atlas settings` beside a running daemon —
///    minted a *different* token for the same port, so whichever bound
///    first made the other's printed address wrong as well as ephemeral.
///
/// Stored via `Store`, so it lands in `data/state` with everything else and
/// is covered by the same backup and `atlas update` preservation as the rest.
/// Still `os_random` on first run, still fails rather than falling back.
///
/// Rotating it is deleting `data/state/hub_token.json`: the next start makes
/// a new one. That is deliberate — there is no rotate command, because a
/// token that rotates on a schedule is a bookmark that breaks on a schedule,
/// which is the defect this fixes.
pub fn token_for(store: &crate::store::Store) -> Result<String> {
    const FILE: &str = "hub_token";
    let held: HeldToken = store.load(FILE);
    if !held.token.trim().is_empty() {
        return Ok(held.token);
    }
    let token = new_token()?;
    store.save(FILE, &HeldToken { token: token.clone() })?;
    Ok(token)
}

/// Where the hub is, said the same way everywhere.
///
/// One function so the address cannot be printed two ways by two call sites,
/// which is how `atlas settings` came to print one that was missing the
/// token and therefore answered `Denied` to every request.
pub fn hub_url(port: u16, token: &str, path: &str) -> String {
    format!("http://127.0.0.1:{port}{path}?t={token}")
}

/// Failed attempts, and the delay they earn.
///
/// On its own this is hygiene: a genuinely random 24-character token isn't
/// brute-forceable. It matters because it's the mechanism that would make a
/// weak token exploitable, and a control that only works while another control
/// holds is one worth having anyway.
///
/// Delay rather than lockout: locking out means something else on the machine
/// can deny you your own hub by failing twice.
#[derive(Debug, Clone, Default)]
pub struct Failures {
    count: u32,
    last_at: u64,
}

impl Failures {
    /// How long to wait before answering, in milliseconds.
    pub fn delay_ms(&self) -> u64 {
        match self.count {
            0 => 0,
            1..=2 => 100,
            3..=5 => 500,
            // Caps, because an unbounded delay is a way to hold the single
            // connection open indefinitely — which is the other finding.
            _ => 2000,
        }
    }

    pub fn failed(&mut self, now: u64) {
        // A gap of five minutes with no failures forgets them. Otherwise one
        // fat-fingered attempt this morning slows you down all day.
        if now.saturating_sub(self.last_at) > 300 {
            self.count = 0;
        }
        self.count += 1;
        self.last_at = now;
    }

    pub fn succeeded(&mut self) {
        self.count = 0;
    }

    /// Worth telling you about.
    ///
    /// Not every failure — the first few are you, or a stale tab. A run of
    /// them is something else on the machine trying.
    pub fn worth_mentioning(&self) -> Option<String> {
        if self.count < 5 {
            return None;
        }
        Some(format!(
            "{} failed attempts to reach the hub. That's not you mistyping — something on this \
             machine is trying.",
            self.count
        ))
    }
}

/// The listener.
///
/// `handle` receives an authenticated action and returns what to send back.
/// Serving one connection at a time is deliberate: this is a phone talking to
/// your own laptop, and a threadpool would be more moving parts than the job
/// needs.
pub struct Server {
    listener: TcpListener,
    /// Loopback as well, when `reachable_from` named another address: Atlas's
    /// own window and this machine's browser open `127.0.0.1`, and a server
    /// bound only to the network address refused them (27 Sep 2026).
    also: Option<TcpListener>,
    token: String,
    max_body: usize,
    max_upload: usize,
    /// Peers on the narrow, notify-only door. `None` means the door does not
    /// exist for this server at all -- most Atlas installs will never have
    /// another Atlas to talk to, and an empty-but-present door is a
    /// different, worse thing than no door.
    signals: Option<std::sync::Mutex<crate::kin::Door>>,
    /// Wrong tokens lately (Eric, B2): each answer to a wrong one is slowed,
    /// so guessing the token from the phone link's address is slow too.
    failures: std::sync::Mutex<Failures>,
    /// How long a connection waits for the daemon (`ANSWER_WAIT`).
    answer_wait: std::time::Duration,
    /// Which Atlas this is, said to `/hub/ping` (28 Sep 2026): how Setup,
    /// "Open Atlas" and `atlas hub` tell Atlas's own hub from another
    /// program that happens to answer on the port. Not a secret -- it opens
    /// nothing -- and new at every start.
    id: String,
}

impl Server {
    pub fn bind(cfg: &ServerConfig, token: &str) -> Result<Server> {
        Server::bind_on(cfg, token, cfg.port)
    }

    /// `bind`, on `port` rather than the configured one (the hub falling
    /// back when its own port is taken: `open_hub`). Port 0 is any free one.
    fn bind_on(cfg: &ServerConfig, token: &str, port: u16) -> Result<Server> {
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
    fn with_id(mut self, id: &str) -> Self {
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
    fn with_peers(mut self, peers: Vec<crate::kin::Peer>) -> Self {
        self.signals = Some(std::sync::Mutex::new(crate::kin::Door::new(peers)));
        self
    }

    /// A wrong token: counted, and the answer held back by `delay_ms`.
    /// Capped at two seconds, so a run of them can't hold the one
    /// connection open for long.
    fn slow_down_a_guess(&self) {
        let wait = match self.failures.lock() {
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
        self.failures.lock().ok().and_then(|f| f.worth_mentioning())
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

    fn handle_conn(
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
    fn read_asked(&self, mut stream: TcpStream) -> Result<Option<Asked>> {
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

        let is_hub = token_matches(&self.token, given.as_deref());
        // A peer credential is checked against its own space. `knows` only
        // answers whether this token belongs to a paired peer; what such a
        // request may *do* is still decided solely by `route_signal` and
        // `route_handoff`, which is the property that keeps a peer token
        // incapable of reaching a hub page.
        let is_peer = match (&self.signals, given.as_deref()) {
            (Some(door), Some(t)) => door.lock().map(|d| d.knows(t)).unwrap_or(false),
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
            if let Ok(mut f) = self.failures.lock() {
                f.succeeded();
            }
        }

        // Authenticated. Now the body, with the cap its endpoint earns.
        let len = content_length(&head);
        let cap = body_cap(&req, self.max_body, self.max_upload);
        if len > cap {
            let too_big = if req.path.starts_with("/hub") { Reply::too_big_page(&req.path) } else { Reply::too_big() };
            if req.path.starts_with("/hub") {
                // Read what the browser is still sending before answering, up
                // to the upload cap and the deadline: closing on unread bytes
                // resets the connection, and the browser then shows its own
                // error instead of this page.
                let mut left = len.min(self.max_upload) as u64;
                let mut sink = [0u8; 16 * 1024];
                reader.get_mut().allow_body(0);
                while left > 0 && started.elapsed() < DEADLINE {
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
                    eprintln!("atlas: nothing answers a form at {}", req.path);
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
    fn answer(&self, asked: &mut Asked, reply: Reply) {
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
            eprintln!("atlas: {} {} failed: {}", method, path, r.body);
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

/// A socket read that never waits past `until`, however slowly the other
/// end sends (28 Sep 2026). Each read's timeout is set to the time left, so
/// a client trickling a byte at a time runs out of time, not the reader out
/// of patience one byte at a time.
struct Deadlined {
    stream: TcpStream,
    until: std::time::Instant,
}

impl Deadlined {
    fn new(stream: TcpStream, until: std::time::Instant) -> Deadlined {
        Deadlined { stream, until }
    }

    /// Time for a body of `bytes` to arrive, at no less than
    /// `MIN_BODY_RATE` bytes a second on top of the head's deadline: a large
    /// upload from a phone, or a file over Tor, may take longer than a head
    /// does, but a trickle still runs out of time.
    fn allow_body(&mut self, bytes: usize) {
        let secs = (bytes as u64).div_ceil(MIN_BODY_RATE);
        self.until = std::time::Instant::now() + std::time::Duration::from_secs(5 + secs);
    }
}

/// The slowest a request body may arrive, in bytes a second.
const MIN_BODY_RATE: u64 = 16 * 1024;

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

/// How long a peer's request may take to arrive in full.
const PEER_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);

/// A request `Server::read_asked` has read in full and let through, with the
/// socket its answer goes back on. `page` is the method and path when it
/// came through the hub's routes (the reply gets the app head and the
/// failed-page treatment), `None` for the narrow peer door.
struct Asked {
    stream: TcpStream,
    action: Action,
    page: Option<(String, String)>,
}

/// One authenticated hub request waiting for the daemon to answer it.
pub struct Waiting {
    pub action: Action,
    /// `GET /hub/notes` -- method and path only, never the query (the token
    /// can travel there) and never a body.
    pub what: String,
    arrived: std::time::Instant,
    reply: std::sync::mpsc::SyncSender<Reply>,
    /// `WAITING`, then `TAKEN` by the daemon or `ABANDONED` by the
    /// connection that gave up and answered "busy" -- whichever comes first.
    /// A request answered "busy" used to stay queued and run anyway when
    /// the daemon got to it (28 Sep 2026): a POST applied after the browser
    /// was told it wasn't, and a one-time flash (a recovery key, a friend
    /// link) spent on a page nobody saw.
    state: std::sync::Arc<std::sync::atomic::AtomicU8>,
    /// How long its connection waits (`Server::answer_wait`).
    wait: std::time::Duration,
}

const WAITING: u8 = 0;
const TAKEN: u8 = 1;
const ABANDONED: u8 = 2;

impl Waiting {
    /// Claim this request for answering. False when the connection has
    /// given up on it, or it has waited longer than any connection waits.
    fn take(&self) -> bool {
        use std::sync::atomic::Ordering;
        if self.arrived.elapsed() >= self.wait {
            let _ = self.state.compare_exchange(WAITING, ABANDONED, Ordering::SeqCst, Ordering::SeqCst);
            return false;
        }
        self.state.compare_exchange(WAITING, TAKEN, Ordering::SeqCst, Ordering::SeqCst).is_ok()
    }
}

/// What answering one waiting request cost, for the log.
#[derive(Debug, Clone)]
pub struct HubCost {
    pub what: String,
    /// From the request arriving in full to Atlas starting on it.
    pub waited_ms: u64,
    /// Atlas working out the answer.
    pub took_ms: u64,
}

/// The hub on its own threads (27 Sep 2026).
///
/// ## Why
///
/// Eric: "Atlas felt slow when clicking around and trying to talk to it and
/// that is not acceptable." The cause was here. The daemon's loop called
/// `poll_once` once a pass, and `poll_once` takes ONE connection. A pass
/// also waits on the keyboard (half a second), the wake word (seconds of
/// recording) and a nap of up to two seconds. One click in a browser is
/// four or more requests (the page, the manifest, icons, the service
/// worker), the open page refetches itself every few seconds, and the setup
/// window probed the port with empty connections every two seconds -- each
/// of those used up a whole pass. So a click waited seconds in a queue
/// behind other connections, with the daemon asleep between them.
///
/// ## What
///
/// A listener thread accepts, and each connection gets a short-lived thread
/// of its own that reads and checks it with exactly the code `poll_once`
/// uses (`read_asked`): the token, the cookie trade, the caps, the deadline
/// and the delay for a wrong guess are unchanged, they just happen off the
/// daemon's thread. The public files, the manifest, every refusal and every
/// 404 are answered right there. Only an authenticated action reaches the
/// daemon, through a channel, and the daemon answers every one waiting each
/// time it looks (`answer_waiting`), which it now does every 50ms while idle
/// (`wait_and_answer`) instead of napping.
///
/// The daemon still owns every answer: `&mut Daemon` never crosses a thread,
/// which is the reason the loop was single-threaded in the first place.
pub struct HubDoor {
    /// The server, once it is listening (`open_hub` may still be waiting
    /// for its port).
    server: std::sync::Arc<std::sync::OnceLock<std::sync::Arc<Server>>>,
    asks: std::sync::mpsc::Receiver<Waiting>,
    /// Where it answers now, 0 while it has no port yet.
    port: std::sync::Arc<std::sync::atomic::AtomicU16>,
    /// What there is to say about the door (its port taken, a fallback),
    /// for the log; emptied as it's read (`take_news`).
    news: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    /// Set when the door is dropped: the retrying thread stops.
    shut: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// The hub's end of a `HubDoor`: what a listener, bound now or later, feeds.
struct Serving {
    server: std::sync::Arc<std::sync::OnceLock<std::sync::Arc<Server>>>,
    tx: std::sync::mpsc::Sender<Waiting>,
    open: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    per_address: PerAddress,
    port: std::sync::Arc<std::sync::atomic::AtomicU16>,
    news: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    shut: std::sync::Arc<std::sync::atomic::AtomicBool>,
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
        }
    }
}

impl Serving {
    /// Start answering on `server`'s sockets: one accepting thread for each
    /// (the one asked for, and loopback beside it when `reachable_from`
    /// named another address -- merge of 28 Sep 2026: the desktop window
    /// always uses 127.0.0.1).
    fn serve(&self, server: Server) -> Result<()> {
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
                    let listener = if which == 0 { &s.listener } else { s.also.as_ref().expect("counted above") };
                    me.accept_on(listener, &s);
                })
                .map_err(|e| AtlasError::Platform(format!("couldn't start the hub's listener: {e}")))?;
        }
        let _ = self.server.set(server);
        self.port.store(port, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    /// One more socket for the same server: the configured port, freed
    /// after the hub had to fall back to another (`open_hub`), so the
    /// bookmarked address works again beside the one it fell back to.
    fn serve_also(&self, listener: TcpListener) -> bool {
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

    fn accept_on(&self, listener: &TcpListener, s: &std::sync::Arc<Server>) {
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
            let (s, tx, still_open, per) = (s.clone(), self.tx.clone(), self.open.clone(), self.per_address.clone());
            let spawned = std::thread::Builder::new().name("atlas-hub-conn".into()).spawn(move || {
                s.serve_on_its_own(stream, &tx);
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

    fn say(&self, line: String) {
        if let Ok(mut n) = self.news.lock() {
            n.push(line);
        }
    }
}

/// How `open_hub` waits for a port that's taken. Production values in
/// `Default`; the tests shorten them.
#[derive(Debug, Clone, Copy)]
pub struct Retry {
    /// The first wait before trying the port again; doubled each time...
    pub first: std::time::Duration,
    /// ...up to this.
    pub longest: std::time::Duration,
    /// How long the configured port is waited for before the hub opens on
    /// another one.
    pub fall_back_after: std::time::Duration,
    /// Once on another port, how often the configured one is tried again.
    pub look_again_every: std::time::Duration,
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

/// The ports tried after the configured one, before any free port at all:
/// the next few, so the address is still easy to find.
pub const FALLBACK_PORTS: u16 = 10;

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

fn keep_trying(
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

/// How many connections may be being read at once. A browser opens six or
/// so per origin; a phone and a laptop together, a few more. Past this a new
/// connection is closed unread rather than given a thread -- something on
/// the machine opening hundreds is not a person clicking.
const MAX_OPEN: usize = 48;

/// How many of those may come from one address (28 Sep 2026), so one
/// machine on the network can't take every place and lock the phone and the
/// desktop out. A browser opens about six at once.
const MAX_OPEN_PER_ADDRESS: usize = 16;

type PerAddress = std::sync::Arc<std::sync::Mutex<std::collections::HashMap<std::net::IpAddr, usize>>>;

/// Count one more connection from `from`; false when it already has
/// `MAX_OPEN_PER_ADDRESS`. An address that can't be read is let through
/// (`MAX_OPEN` still bounds it).
fn by_address_take(per: &PerAddress, from: Option<std::net::IpAddr>) -> bool {
    let Some(ip) = from else { return true };
    let mut m = held(per);
    let n = m.entry(ip).or_insert(0);
    if *n >= MAX_OPEN_PER_ADDRESS {
        return false;
    }
    *n += 1;
    true
}

fn by_address_give_back(per: &PerAddress, from: Option<std::net::IpAddr>) {
    let Some(ip) = from else { return };
    let mut m = held(per);
    if let Some(n) = m.get_mut(&ip) {
        *n = n.saturating_sub(1);
        if *n == 0 {
            m.remove(&ip);
        }
    }
}

/// How long a connection waits for the daemon's answer before it is told
/// Atlas is busy. Long, because a page asked for while Atlas is in the
/// middle of answering you (a model reply can take tens of seconds on a
/// laptop) is answered as soon as it's free, and that is better than an
/// error; but bounded, so a connection never waits forever on a daemon that
/// has stopped.
const ANSWER_WAIT: std::time::Duration = std::time::Duration::from_secs(90);

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
    fn serve_on_its_own(&self, stream: TcpStream, to_daemon: &std::sync::mpsc::Sender<Waiting>) {
        let Ok(Some(mut asked)) = self.read_asked(stream) else {
            return;
        };
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
                        Ok(_) => None,
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
    fn busy(page: bool) -> Reply {
        if page {
            Reply {
                status: 503,
                body: "<!doctype html><html lang=en><head><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'>\
                       <title>Atlas</title><style>body{font:17px/1.5 system-ui,sans-serif;max-width:32em;margin:3em auto;padding:0 1em}</style></head>\
                       <body><h1>Atlas is busy for a moment</h1><p>It's in the middle of something and didn't get to this page. \
                       Try again in a few seconds.</p><p><a href='/hub'>Back to Atlas</a></p></body></html>"
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
    /// A door with nothing listening yet, and its serving end.
    fn waiting() -> (HubDoor, Serving) {
        let (tx, asks) = std::sync::mpsc::channel::<Waiting>();
        let server = std::sync::Arc::new(std::sync::OnceLock::new());
        let port = std::sync::Arc::new(std::sync::atomic::AtomicU16::new(0));
        let news = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let shut = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let serving = Serving {
            server: server.clone(),
            tx,
            open: Default::default(),
            per_address: Default::default(),
            port: port.clone(),
            news: news.clone(),
            shut: shut.clone(),
        };
        (HubDoor { server, asks, port, news, shut }, serving)
    }

    /// Is anything listening yet?
    pub fn is_open(&self) -> bool {
        self.server.get().is_some()
    }

    /// What there is to say about the door since last asked: its port
    /// taken, where it opened instead. For the log.
    pub fn take_news(&self) -> Vec<String> {
        self.news.lock().map(|mut n| std::mem::take(&mut *n)).unwrap_or_default()
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
    fn answer_one(w: Waiting, handle: &mut dyn FnMut(Action) -> Reply) -> Option<HubCost> {
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

impl Drop for HubDoor {
    fn drop(&mut self) {
        self.shut.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

// ---------------------------------------------------------------------------
// Which Atlas answers on the hub's port (28 Sep 2026)

/// Where `Server` answers "which Atlas is this?".
pub const PING_PATH: &str = "/hub/ping";

/// The file in `data/state` that says where the running Atlas's hub is and
/// which Atlas it is (`open_hub`'s `on_open` writes it).
pub const DOOR_FILE: &str = "hub_door.json";

/// What `DOOR_FILE` holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Door {
    pub port: u16,
    pub id: String,
}

/// Write where the hub is. Written whole to a new file and moved over the
/// old one, so a reader never sees half of it.
pub fn record_door(state_dir: &std::path::Path, door: &Door) -> std::io::Result<()> {
    std::fs::create_dir_all(state_dir)?;
    let text = serde_json::to_string(door).map_err(|e| std::io::Error::other(e.to_string()))?;
    let tmp = state_dir.join(format!("{DOOR_FILE}.new"));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, state_dir.join(DOOR_FILE))
}

/// What `record_door` last wrote, if anything.
fn recorded_door(state_dir: &std::path::Path) -> Option<Door> {
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
    let _ = s.take(4096).read_to_end(&mut got);
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

// ---------------------------------------------------------------------------
// A door that fits inside the daemon's own loop.
//
// `Server` blocks on `accept()` — right for `run_hub`, which has nothing
// else to do while it waits. `Daemon::run` is different: it already polls
// everything once per tick (typing, the wake word, scheduled work) rather
// than blocking on any one of them, and bolting a second, blocking listener
// onto that would mean either freezing the whole assistant while it waits
// for a peer that may never call, or standing up a second thread sharing
// `&mut Daemon` across threads — which `Daemon<'a>`'s borrowed lifetimes and
// `dyn Platform`/`dyn Llm` trait objects are not set up for, and forcing that
// through under time pressure is exactly the kind of thing that ships a race
// condition nobody notices until it's a nudge that fires twice or a message
// that's lost. So: same shape as everything else in the loop instead.
//
// A non-blocking accept, checked once per tick, handled inline, on the same
// thread as everything else. `Daemon::run`'s loop already tolerates
// `keyboard.poll()` costing nothing when nothing was typed; this costs the
// same — nothing — when nobody has signalled.
// ---------------------------------------------------------------------------

/// The kin door, opened in a way `Daemon::run`'s loop can check without
/// blocking on it.
///
/// Two kinds of socket, since 25 Sep:
///
/// * **The ordinary door**, on every address this machine has, for your own
///   devices -- on this machine, your home network, or your own private
///   network. From anywhere else a request in the clear is refused before its
///   token is looked at (`onion::is_local_origin`).
/// * **The sealed door**, on this machine only, which Tor forwards your onion
///   address to (`onion`). Everything from a friend arrives there, and there
///   **only sealed envelopes (`wire`) are read**: what Tor delivers looks as if
///   it came from this machine, so it gets its own socket rather than a rule
///   about where it came from.
pub struct SignalListener {
    /// Each socket, and whether it reads sealed envelopes only.
    listeners: Vec<(TcpListener, bool)>,
    door: std::sync::Mutex<crate::kin::Door>,
    /// The state folder release files are handed out from
    /// (`update_courier::FILES` inside it), once set.
    releases: std::sync::Mutex<Option<std::path::PathBuf>>,
    /// This Atlas's key, to open envelopes sealed to it.
    me: std::sync::Mutex<Option<crate::peerkey::Identity>>,
    /// One-time keys already accepted.
    seen: std::sync::Mutex<crate::wire::Seen>,
    /// Connections through Tor that a friend's Atlas asked to keep open for
    /// its next request (gap AN): each with where it came from and when it
    /// was last used. Sealed door only, and bounded (`kin::MAX_KEPT`).
    kept: std::sync::Mutex<Vec<(TcpStream, std::net::SocketAddr, std::time::Instant)>>,
    /// Where to note which paired Atlas came to the door (`kin::Reached`).
    reached: std::sync::Mutex<Option<std::sync::Arc<crate::kin::Reached>>>,
}

/// What a door said, and what (if anything) goes on to the daemon.
type Answered = (Reply, Option<crate::kin::Arrived>);

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
        // A second socket only for a fixed port: port 0 would pick a
        // different number for it, and a door with two numbers is two doors.
        if listeners.is_empty() || port != 0 {
            match TcpListener::bind(("0.0.0.0", port)) {
                Ok(l) => listeners.push(l),
                Err(e) if listeners.is_empty() => {
                    return Err(AtlasError::Platform(format!("could not open the signal door on {port}: {e}")))
                }
                // Already covered: this system's IPv6 socket takes IPv4 too.
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
    fn next_kept(&self) -> Option<(TcpStream, std::net::SocketAddr)> {
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
    fn open(&self, env: &crate::wire::Envelope, _now: u64) -> Option<(String, crate::wire::Inner, crate::wire::ReplyKey)> {
        let now = crate::store::now();
        let me = held(&self.me);
        let me = me.as_ref()?;
        let mut seen = held(&self.seen);
        crate::wire::open(me, env, now, &mut seen).ok()
    }

    /// The token names a paired peer, and -- when the request came sealed --
    /// that peer's pinned key is the one that sealed it. A token copied onto
    /// someone else's envelope is refused.
    fn token_fits(&self, token: &str, sealed_by: Option<&str>) -> bool {
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

    fn handle_one(&self, mut stream: TcpStream, remote: std::net::SocketAddr, sealed_only: bool, now: u64) -> Option<crate::kin::Arrived> {
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
    fn sealed(&self, body: &str, now: u64) -> Answered {
        let Ok(env) = serde_json::from_str::<crate::wire::Envelope>(body) else { return (Reply::denied(), None) };
        let Some((from, inner, key)) = self.open(&env, now) else { return (Reply::denied(), None) };
        let req = inner_request(&inner);
        let (reply, arrived) = self.answer(&req, Some(&from));
        (Reply::ok(crate::wire::seal_reply(&key, reply.status, &reply.body)), arrived)
    }

    /// Every door, for a request that is in the clear from somewhere local or
    /// was opened from an envelope (`sealed_by` is then who sealed it).
    fn answer(&self, req: &Request, sealed_by: Option<&str>) -> Answered {
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
    fn release_piece(&self, req: &Request, token: &str) -> Answered {
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
fn inner_request(inner: &crate::wire::Inner) -> Request {
    Request {
        method: "POST".into(),
        path: inner.path.clone(),
        query: String::new(),
        token: (!inner.token.is_empty()).then(|| inner.token.clone()),
        token_from_url: false,
        body: inner.body.clone(),
    }
}
