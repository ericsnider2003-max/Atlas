//! Requests and replies: parsing, query fields, tokens, the refusal pages.
//!
//! Moved out of `server.rs` unchanged (audit Q6, 6 Oct 2026): one file of
//! thousands of lines was where every chat's edits collided.

use super::*;

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
    pub(super) fn cookie_then(to: &str, token: &str) -> Reply {
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
    pub(super) fn wants_a_page(method: &str, path: &str) -> bool {
        method == "GET" && (path == "/" || path == "/hub" || path.starts_with("/hub/")) && !path.contains('.')
    }
    /// "denied", for a person: what to do, never a bare error with no way on
    /// (27 Sep 2026: a phone showed `{"error":"denied"}` and nothing else).
    /// It says no more than the JSON does about why.
    pub(super) fn denied_page() -> Reply {
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
    pub(super) fn not_found_page(path: &str) -> Reply {
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
    pub(super) fn failed_page() -> Reply {
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
    pub(super) fn not_found() -> Reply {
        Reply { status: 404, body: "{\"error\":\"no such endpoint\"}".into(), ..Reply::default() }
    }
    pub(super) fn too_big() -> Reply {
        Reply { status: 413, body: "{\"error\":\"body too large\"}".into(), ..Reply::default() }
    }
    /// Too much sent from the phone or another program, said so a person can
    /// act on it (30 Sep 2026: a file shared from the phone that was over the
    /// limit came back as `body too large`, with no limit named).
    pub(super) fn too_big_for(cap: usize) -> Reply {
        let mb = (cap as f64 / (1024.0 * 1024.0)).max(0.1);
        Reply {
            status: 413,
            body: serde_json::json!({
                "error": "body too large",
                "message": format!("That's more than Atlas takes at once (the limit is {mb:.0} MB). Nothing was kept -- send something smaller."),
                "limit_bytes": cap,
            })
            .to_string(),
            ..Reply::default()
        }
    }
    /// Too much sent from a hub form, for a person: what happened and the way
    /// back to the page it came from (27 Sep 2026: a pasted page on Give got
    /// `{"error":"body too large"}` and nothing else).
    pub(super) fn too_big_page(path: &str) -> Reply {
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
pub(super) fn held<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

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
pub(super) fn secret_refused_page() -> Reply {
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
